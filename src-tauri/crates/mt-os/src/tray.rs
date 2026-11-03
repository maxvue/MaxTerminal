//! Controlador e máquina de estados visual da bandeja do sistema (`TrayVisualState` e `TrayController`).

use std::sync::Arc;
use std::time::Duration;

use mt_core::error::MtResult;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::status::StatusCounters;

/// Estado visual do ícone da bandeja do sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrayVisualState {
    /// Nenhuma aba em execução nem aguardando atenção.
    #[default]
    Idle,
    /// Pelo menos uma aba em execução (`running` ou `continuous`) e nenhuma com erro/intervenção.
    Active,
    /// Pelo menos uma aba com erro não visto (`error_unseen`) ou aguardando intervenção (`awaiting_input`).
    Attention,
}

impl TrayVisualState {
    /// Calcula o estado visual da bandeja a partir dos contadores agregados.
    #[must_use]
    pub const fn from_counters(c: &StatusCounters) -> Self {
        if c.needs_attention() {
            Self::Attention
        } else if c.has_active() {
            Self::Active
        } else {
            Self::Idle
        }
    }
}

/// Textos formatados em `pt-BR` para atualização incremental dos itens do menu da bandeja.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayMenuTexts {
    /// Rótulo do item `tray.toggle` ("Mostrar `MaxTerminal`" ou "Ocultar `MaxTerminal`").
    pub toggle_label: String,
    /// Rótulo curto de título do tray (ex.: `"2 ativas"`).
    pub title: String,
    /// Linha `tray.count.running` (`"● Executando: N"`).
    pub running_label: String,
    /// Linha `tray.count.continuous` (`"● Execução contínua: N"`).
    pub continuous_label: String,
    /// Linha `tray.count.finished` (`"● Finalizado: N"`).
    pub finished_label: String,
    /// Linha `tray.count.error` (`"● Com erro: N"`).
    pub error_label: String,
    /// Linha `tray.count.awaiting` (`"● Aguardando intervenção: N"`).
    pub awaiting_label: String,
}

impl TrayMenuTexts {
    /// Constrói os textos canônicos em `pt-BR` a partir dos contadores e da visibilidade da janela.
    #[must_use]
    pub fn from_state(counters: &StatusCounters, window_visible: bool) -> Self {
        let toggle_label = if window_visible {
            String::from("Ocultar MaxTerminal")
        } else {
            String::from("Mostrar MaxTerminal")
        };
        let active_total = counters.running.saturating_add(counters.continuous);
        let title = format!("{active_total} ativas");

        Self {
            toggle_label,
            title,
            running_label: format!("● Executando: {}", counters.running),
            continuous_label: format!("● Execução contínua: {}", counters.continuous),
            finished_label: format!("● Finalizado: {}", counters.finished_unseen),
            error_label: format!("● Com erro: {}", counters.error_unseen),
            awaiting_label: format!("● Aguardando intervenção: {}", counters.awaiting_input),
        }
    }
}

/// Abstração da bandeja nativa implementada pelo adaptador Tauri no crate binário.
pub trait TrayHost: Send + Sync + 'static {
    /// Atualiza os textos dos itens do menu sem reconstruir o menu.
    fn update_texts(&self, texts: &TrayMenuTexts) -> MtResult<()>;

    /// Atualiza o ícone da bandeja apenas quando `TrayVisualState` muda.
    fn update_icon(&self, visual: TrayVisualState) -> MtResult<()>;
}

/// Controlador da bandeja do sistema com coalescência de 250 ms e diff de ícone/textos.
pub struct TrayController {
    host: Arc<dyn TrayHost>,
    last: Mutex<Option<(StatusCounters, TrayVisualState, bool)>>,
    refresh_tx: mpsc::Sender<()>,
}

impl TrayController {
    /// Intervalo de coalescência das atualizações da bandeja (250 ms).
    pub const COALESCE_WINDOW: Duration = Duration::from_millis(250);

    /// Cria um novo `TrayController` e inicia o worker assíncrono de coalescência.
    #[must_use]
    pub fn new(
        host: Arc<dyn TrayHost>,
        provider: Arc<dyn Fn() -> (StatusCounters, bool) + Send + Sync>,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel::<()>(16);
        let last = Arc::new(Mutex::new(None));
        let last_bg = Arc::clone(&last);
        let host_bg = Arc::clone(&host);

        let controller = Self {
            host,
            last: Mutex::new(None),
            refresh_tx: tx,
        };

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                while rx.recv().await.is_some() {
                    tokio::time::sleep(Self::COALESCE_WINDOW).await;
                    while rx.try_recv().is_ok() {}
                    let (counters, visible) = provider();
                    let _ = apply_update(&host_bg, &last_bg, counters, visible);
                }
            });
        }

        controller
    }

    /// Agenda uma atualização coalescida da bandeja (não bloqueante).
    pub fn request_refresh(&self) {
        let _ = self.refresh_tx.try_send(());
    }

    /// Aplica imediatamente o estado na bandeja (usado na inicialização e em testes).
    pub fn apply_now(&self, counters: StatusCounters, window_visible: bool) -> MtResult<()> {
        apply_update(&self.host, &self.last, counters, window_visible)
    }
}

fn apply_update(
    host: &Arc<dyn TrayHost>,
    last: &Mutex<Option<(StatusCounters, TrayVisualState, bool)>>,
    counters: StatusCounters,
    window_visible: bool,
) -> MtResult<()> {
    let next_visual = TrayVisualState::from_counters(&counters);
    let mut guard = last.lock();

    let (counters_changed, visual_changed, visible_changed) = match *guard {
        Some((prev_c, prev_v, prev_vis)) => (
            prev_c != counters,
            prev_v != next_visual,
            prev_vis != window_visible,
        ),
        None => (true, true, true),
    };

    if counters_changed || visible_changed {
        let texts = TrayMenuTexts::from_state(&counters, window_visible);
        host.update_texts(&texts)?;
    }

    if visual_changed {
        host.update_icon(next_visual)?;
    }

    *guard = Some((counters, next_visual, window_visible));
    Ok(())
}
