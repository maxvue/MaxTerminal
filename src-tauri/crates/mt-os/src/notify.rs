//! Notificações nativas (`Notifier`, `RateLimiter`, `NotificationKind` e `NotifyOutcome`).

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mt_core::error::MtResult;
use mt_core::ids::{PaneId, TabId};
use mt_db::sanitizer::sanitize;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::environment::OsEnvironment;
use crate::error::{codes, os_error};
use crate::settings::OsSettings;

/// Categoria semântica da notificação nativa.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// Comando longo concluído com sucesso (`exit 0`).
    CommandFinished,
    /// Comando longo concluído com falha (`exit != 0`).
    CommandFailed,
    /// Agente CLI aguardando resposta ou aprovação de ferramenta.
    AgentAwaitingInput,
    /// Notificação informativa geral ou de teste.
    Info,
}

impl NotificationKind {
    /// Categoria XDG (`hint category`) correspondente.
    #[must_use]
    pub const fn category(self) -> &'static str {
        match self {
            Self::CommandFinished => "transfer.complete",
            Self::CommandFailed => "transfer.error",
            Self::AgentAwaitingInput => "im.received",
            Self::Info => "device",
        }
    }

    /// Nível de urgência XDG (`0 = Low`, `1 = Normal`, `2 = Critical`).
    #[must_use]
    pub const fn urgency_level(self, tab_visible: bool) -> u8 {
        match self {
            Self::CommandFailed | Self::AgentAwaitingInput if !tab_visible => 2,
            _ => 1,
        }
    }
}

/// Solicitação de envio de notificação (`os_notify`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotifyRequest {
    /// Tipo da notificação.
    pub kind: NotificationKind,
    /// Título bruto (será sanitizado e truncado em 80 caracteres).
    pub title: String,
    /// Corpo bruto (será sanitizado e truncado em 200 caracteres).
    pub body: String,
    /// Aba associada ao clique na notificação, se houver.
    #[specta(optional)]
    pub tab_id: Option<TabId>,
    /// Painel associado ao clique na notificação, se houver.
    #[specta(optional)]
    pub pane_id: Option<PaneId>,
}

/// Desfecho do processamento de uma notificação nativa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NotifyOutcome {
    /// Notificação enviada como nova entrada.
    Shown,
    /// Notificação atualizou/substituiu uma entrada existente no servidor (`replaces_id`).
    Replaced,
    /// Notificação suprimida por deduplicação ou rate limit.
    Suppressed,
    /// Notificações estão desabilitadas nas preferências.
    Disabled,
    /// Falha ao comunicar com o servidor de notificações.
    Failed,
}

/// Decisão pura do `RateLimiter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitDecision {
    /// Permite envio como nova notificação.
    Allow,
    /// Permite substituindo a notificação anterior da mesma chave.
    Replace(u64),
    /// Suprime envio por excesso de frequência ou duplicação idêntica.
    Suppress,
}

/// Limitador de taxa e deduplicador puro:
/// - Deduplica a mesma chave `(tab_id, kind, hash(title+body))` dentro de 10 s;
/// - Limita o volume global a no máximo 5 notificações a cada 30 s (incluindo `AgentAwaitingInput`).
#[derive(Debug, Default)]
pub struct RateLimiter {
    recent_global: VecDeque<Instant>,
    dedupe_map: HashMap<u64, Instant>,
    slot_map: HashMap<(Option<TabId>, NotificationKind), (u64, Instant)>,
}

impl RateLimiter {
    /// Janela de deduplicação para conteúdo idêntico (10 s).
    pub const DEDUPE_WINDOW: Duration = Duration::from_secs(10);
    /// Janela do teto global de notificações (30 s).
    pub const GLOBAL_WINDOW: Duration = Duration::from_secs(30);
    /// Quantidade máxima de notificações permitidas na janela global.
    pub const GLOBAL_MAX: usize = 5;
    /// Limite máximo de chaves retidas no mapa de deduplicação.
    pub const MAX_TRACKED_KEYS: usize = 32;

    /// Cria um novo `RateLimiter`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Calcula a chave de deduplicação de 64 bits para `(tab_id, kind, title, body)`.
    #[must_use]
    pub fn compute_key(
        tab_id: Option<TabId>,
        kind: NotificationKind,
        title: &str,
        body: &str,
    ) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        tab_id.hash(&mut hasher);
        kind.hash(&mut hasher);
        title.hash(&mut hasher);
        body.hash(&mut hasher);
        hasher.finish()
    }

    /// Avalia se uma notificação com a `key` informada deve ser enviada, substituir anterior ou ser suprimida.
    pub fn check(&mut self, key: u64, kind: NotificationKind, now: Instant) -> LimitDecision {
        self.check_with_tab(key, None, kind, now)
    }

    /// Variante de `check` que associa o slot `(tab_id, kind)` para substituição de notificações da mesma aba.
    pub fn check_with_tab(
        &mut self,
        key: u64,
        tab_id: Option<TabId>,
        kind: NotificationKind,
        now: Instant,
    ) -> LimitDecision {
        self.dedupe_map.retain(|_, ts| {
            now.checked_duration_since(*ts)
                .is_some_and(|d| d < Self::DEDUPE_WINDOW)
        });
        self.slot_map.retain(|_, (_, ts)| {
            now.checked_duration_since(*ts)
                .is_some_and(|d| d < Duration::from_secs(300))
        });

        while let Some(&front) = self.recent_global.front() {
            if now.saturating_duration_since(front) >= Self::GLOBAL_WINDOW {
                self.recent_global.pop_front();
            } else {
                break;
            }
        }

        // 1. Mesma chave exata em menos de 10 s → suprime
        if self.dedupe_map.contains_key(&key) {
            return LimitDecision::Suppress;
        }

        // 2. Teto global de 5 notificações em 30 s → suprime (inclusive AgentAwaitingInput)
        if self.recent_global.len() >= Self::GLOBAL_MAX {
            return LimitDecision::Suppress;
        }

        if self.dedupe_map.len() >= Self::MAX_TRACKED_KEYS
            && let Some((&oldest_k, _)) = self.dedupe_map.iter().min_by_key(|(_, ts)| *ts)
        {
            self.dedupe_map.remove(&oldest_k);
        }

        self.dedupe_map.insert(key, now);
        self.recent_global.push_back(now);

        if tab_id.is_some() {
            let slot_key = (tab_id, kind);
            if let Some((prev_key, _)) = self.slot_map.insert(slot_key, (key, now)) {
                return LimitDecision::Replace(prev_key);
            }
        }

        LimitDecision::Allow
    }
}

/// Payload sanitizado pronto para envio ao servidor de notificações D-Bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedNotification {
    /// Categoria semântica.
    pub kind: NotificationKind,
    /// Título sanitizado, sem markup e truncado em até 80 caracteres.
    pub summary: String,
    /// Corpo sanitizado, sem markup e truncado em até 200 caracteres.
    pub body: String,
    /// Nível de urgência (`0..=2`).
    pub urgency: u8,
    /// ID de substituição D-Bus (`0` se nova).
    pub replaces_id: u32,
    /// Se deve incluir a ação `"default"` (`"Abrir aba"`).
    pub include_default_action: bool,
}

/// Sanitiza, escapa caracteres de markup (`<`, `&`) e trunca título (80 chars) e corpo (200 chars).
#[must_use]
pub fn sanitize_and_truncate_notification(raw_title: &str, raw_body: &str) -> (String, String) {
    let clean_title = escape_plain_text(&sanitize(raw_title));
    let clean_body = escape_plain_text(&sanitize(raw_body));
    (
        truncate_chars(&clean_title, 80),
        truncate_chars(&clean_body, 200),
    )
}

fn escape_plain_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' | '\n' | '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.trim().to_owned()
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_owned();
    }
    let keep = max_chars.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

/// Transporte de envio de notificações nativas (permite mock em testes e D-Bus real em produção).
#[async_trait]
pub trait NotificationTransport: Send + Sync + 'static {
    /// Envia a notificação ao servidor D-Bus e retorna o `notification_id` atribuído.
    async fn send(&self, notification: &SanitizedNotification) -> MtResult<u32>;
}

/// Transporte D-Bus via `gdbus call --session --dest org.freedesktop.Notifications`.
#[derive(Debug, Default)]
pub struct DbusNotificationTransport;

#[async_trait]
impl NotificationTransport for DbusNotificationTransport {
    async fn send(&self, n: &SanitizedNotification) -> MtResult<u32> {
        let actions_arg = if n.include_default_action {
            "['default', 'Abrir aba']"
        } else {
            "[]"
        };
        let hints_arg = format!(
            "{{'urgency': <byte {}>, 'category': <'{}'>}}",
            n.urgency,
            n.kind.category()
        );
        let replaces_str = n.replaces_id.to_string();

        let mut cmd = Command::new("gdbus");
        cmd.args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.Notifications",
            "--object-path",
            "/org/freedesktop/Notifications",
            "--method",
            "org.freedesktop.Notifications.Notify",
            "MaxTerminal",
            &replaces_str,
            "maxterminal",
            &n.summary,
            &n.body,
            actions_arg,
            &hints_arg,
            "5000",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

        let output = tokio::time::timeout(Duration::from_millis(1500), cmd.output())
            .await
            .map_err(|_| {
                os_error(
                    codes::NOTIFY_FAILED,
                    "Tempo limite esgotado ao enviar notificação para o servidor D-Bus.",
                )
            })?
            .map_err(|e| {
                os_error(
                    codes::NOTIFY_FAILED,
                    format!("Falha ao invocar transporte de notificações: {e}"),
                )
            })?;

        if !output.status.success() {
            return Err(os_error(
                codes::NOTIFY_FAILED,
                "Servidor de notificações indisponível ou recusou a mensagem.",
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Ex.: "(uint32 42,)\n"
        let parsed_id = stdout
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .nth(1)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1);

        Ok(parsed_id)
    }
}

type ActiveNotificationMap = HashMap<(Option<TabId>, NotificationKind), (u32, Instant)>;

/// Despachante de notificações nativas com sanitização, rate limiting e substituição de IDs D-Bus.
pub struct Notifier {
    transport: Arc<dyn NotificationTransport>,
    limiter: Mutex<RateLimiter>,
    active_ids: Mutex<ActiveNotificationMap>,
    settings: Arc<RwLock<OsSettings>>,
    env: Arc<RwLock<OsEnvironment>>,
}

impl Notifier {
    /// Cria um novo `Notifier`.
    #[must_use]
    pub fn new(
        transport: Arc<dyn NotificationTransport>,
        settings: Arc<RwLock<OsSettings>>,
        env: Arc<RwLock<OsEnvironment>>,
    ) -> Self {
        Self {
            transport,
            limiter: Mutex::new(RateLimiter::new()),
            active_ids: Mutex::new(HashMap::new()),
            settings,
            env,
        }
    }

    /// API canônica consumida pela Fase 09 (`long_command`) e pela Fase 13 (agentes).
    pub async fn notify_user(
        &self,
        kind: NotificationKind,
        title: &str,
        body: &str,
        tab_id: Option<TabId>,
    ) -> NotifyOutcome {
        match self
            .dispatch_internal(kind, title, body, tab_id, false)
            .await
        {
            Ok(outcome) => outcome,
            Err(err) => {
                tracing::warn!(target: "mt_os", error = %err, "Falha ao enviar notificação nativa");
                NotifyOutcome::Failed
            }
        }
    }

    /// Envia uma notificação explícita solicitada via IPC (`os_notify`), propagando erro caso o servidor falhe.
    pub async fn notify_explicit(&self, req: &NotifyRequest) -> MtResult<NotifyOutcome> {
        self.dispatch_internal(req.kind, &req.title, &req.body, req.tab_id, true)
            .await
    }

    async fn dispatch_internal(
        &self,
        kind: NotificationKind,
        title: &str,
        body: &str,
        tab_id: Option<TabId>,
        fail_on_transport_error: bool,
    ) -> MtResult<NotifyOutcome> {
        if !self.settings.read().notifications.enabled {
            return Ok(NotifyOutcome::Disabled);
        }

        let (summary, clean_body) = sanitize_and_truncate_notification(title, body);
        let now = Instant::now();
        let key = RateLimiter::compute_key(tab_id, kind, &summary, &clean_body);

        let decision = self.limiter.lock().check_with_tab(key, tab_id, kind, now);

        if decision == LimitDecision::Suppress {
            return Ok(NotifyOutcome::Suppressed);
        }

        let slot = (tab_id, kind);
        let replaces_id = {
            let mut active = self.active_ids.lock();
            active.retain(|_, (_, ts)| {
                now.checked_duration_since(*ts)
                    .is_some_and(|d| d < Duration::from_secs(300))
            });
            if matches!(decision, LimitDecision::Replace(_)) {
                active.get(&slot).map_or(0, |(id, _)| *id)
            } else {
                0
            }
        };

        let include_actions = self.env.read().notification_actions && tab_id.is_some();
        let payload = SanitizedNotification {
            kind,
            summary,
            body: clean_body,
            urgency: kind.urgency_level(false),
            replaces_id,
            include_default_action: include_actions,
        };

        match self.transport.send(&payload).await {
            Ok(assigned_id) => {
                let mut active = self.active_ids.lock();
                if active.len() >= 32
                    && !active.contains_key(&slot)
                    && let Some((oldest_slot, _)) = active
                        .iter()
                        .min_by_key(|(_, (_, ts))| *ts)
                        .map(|(k, v)| (*k, *v))
                {
                    active.remove(&oldest_slot);
                }
                active.insert(slot, (assigned_id, now));
                if replaces_id != 0 {
                    Ok(NotifyOutcome::Replaced)
                } else {
                    Ok(NotifyOutcome::Shown)
                }
            }
            Err(err) => {
                if fail_on_transport_error {
                    Err(err)
                } else {
                    Ok(NotifyOutcome::Failed)
                }
            }
        }
    }
}
