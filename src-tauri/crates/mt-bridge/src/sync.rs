//! Máquina de sincronização bidirecional de projeto ativo (`ProjectSyncDecider`) com prevenção de eco (`origin_id`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::config::BridgeSettings;
use crate::error::BridgeError;
use crate::handler::{BridgeHost, BridgeProjectSummary};
use crate::paths::normalize_existing_project_path;
use crate::protocol::ProjectOpenedPayload;

/// Origin ID emitido pelo `MaxTerminal` em todas as notificações `project.opened` de saída.
pub const ORIGIN_MAXTERMINAL: &str = "maxterminal";
/// Origin ID esperado das notificações iniciadas genuinamente pelo `MaxCode`.
pub const ORIGIN_MAXCODE: &str = "maxcode";
/// Janela de supressão anti-loop após aplicar uma mudança remota de projeto (750 ms).
pub const ECHO_SUPPRESSION_WINDOW: Duration = Duration::from_millis(750);

#[derive(Debug, Default)]
#[allow(clippy::struct_field_names)]
struct SyncState {
    last_inbound_path: Option<PathBuf>,
    last_inbound_at: Option<Instant>,
    last_outbound_path: Option<PathBuf>,
}

/// Coordenador de prevenção de loops infinitos A → B → A na sincronização de projeto ativo.
#[derive(Debug, Default)]
pub struct ProjectSyncDecider {
    state: Mutex<SyncState>,
}

/// Decisão ao avaliar se uma seleção local de projeto no `MaxTerminal` deve ser enviada ao `MaxCode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundSyncDecision {
    /// Deve enviar `project.opened` ao `MaxCode`.
    Send(ProjectOpenedPayload),
    /// Suprimido (desabilitado nas configurações, eco de mudança recém-recebida ou mesmo caminho já anunciado).
    Suppress(&'static str),
}

/// Resultado da aplicação de um `project.opened` recebido do `MaxCode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundSyncOutcome {
    /// Projeto já existente foi selecionado/focado.
    FocusedExisting(BridgeProjectSummary),
    /// Novo projeto foi cadastrado automaticamente e selecionado.
    CreatedAndFocused(BridgeProjectSummary),
    /// Ignorado (eco próprio, sincronização desabilitada ou projeto desconhecido com `auto_register_unknown_project = false`).
    Ignored(&'static str),
}

impl ProjectSyncDecider {
    /// Cria um novo `ProjectSyncDecider`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra que o `MaxTerminal` acabou de aplicar um projeto recebido do `MaxCode`.
    pub fn record_inbound_applied(&self, canonical_path: PathBuf, now: Instant) {
        let mut guard = self.state.lock();
        guard.last_inbound_path = Some(canonical_path.clone());
        guard.last_inbound_at = Some(now);
        guard.last_outbound_path = Some(canonical_path);
    }

    /// Decide se a seleção local de `project_path` no `MaxTerminal` deve disparar `project.opened` para o `MaxCode`.
    pub fn decide_outbound(
        &self,
        settings: &BridgeSettings,
        project_path: &str,
        project_name: Option<String>,
        git_branch: Option<String>,
        now: Instant,
    ) -> Result<OutboundSyncDecision, BridgeError> {
        if !settings.enabled || !settings.auto_sync_active_project {
            return Ok(OutboundSyncDecision::Suppress("sync_disabled"));
        }

        let canonical = normalize_existing_project_path(project_path)?;
        let mut guard = self.state.lock();

        if let (Some(in_path), Some(in_at)) = (&guard.last_inbound_path, guard.last_inbound_at)
            && *in_path == canonical
            && now
                .checked_duration_since(in_at)
                .is_some_and(|d| d <= ECHO_SUPPRESSION_WINDOW)
        {
            return Ok(OutboundSyncDecision::Suppress("inbound_echo_window"));
        }

        if guard.last_outbound_path.as_ref() == Some(&canonical) {
            return Ok(OutboundSyncDecision::Suppress("duplicate_outbound"));
        }

        guard.last_outbound_path = Some(canonical.clone());
        Ok(OutboundSyncDecision::Send(ProjectOpenedPayload {
            project_path: canonical.to_string_lossy().into_owned(),
            project_name,
            git_branch,
            origin_id: ORIGIN_MAXTERMINAL.to_string(),
        }))
    }
}

/// Processa uma mensagem `project.opened` recebida do `MaxCode` aplicando as regras de `BridgeSettings` e `ProjectSyncDecider`.
pub async fn apply_inbound_project_opened(
    decider: &ProjectSyncDecider,
    settings: &BridgeSettings,
    host: &dyn BridgeHost,
    payload: &ProjectOpenedPayload,
) -> Result<InboundSyncOutcome, BridgeError> {
    if !settings.enabled || !settings.auto_sync_active_project {
        return Ok(InboundSyncOutcome::Ignored("sync_disabled"));
    }

    if payload.origin_id.eq_ignore_ascii_case(ORIGIN_MAXTERMINAL) {
        return Ok(InboundSyncOutcome::Ignored("self_origin_echo"));
    }

    let canonical = normalize_existing_project_path(&payload.project_path)?;

    if let Some(existing) = host.find_project_by_path(&canonical).await? {
        decider.record_inbound_applied(canonical, Instant::now());
        host.focus_project(&existing.id).await?;
        return Ok(InboundSyncOutcome::FocusedExisting(existing));
    }

    if !settings.auto_register_unknown_project {
        return Ok(InboundSyncOutcome::Ignored(
            "unknown_project_auto_register_off",
        ));
    }

    let created = host
        .create_project_for_path(&canonical, payload.project_name.as_deref())
        .await?;
    decider.record_inbound_applied(canonical, Instant::now());
    host.focus_project(&created.id).await?;
    Ok(InboundSyncOutcome::CreatedAndFocused(created))
}
