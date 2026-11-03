//! Persistência assíncrona e em lote de sessões e blocos de comando (`persist.rs`)
//! acoplada aos repositórios [`SessionsRepo`] e [`CommandBlocksRepo`] do `mt-db`.

use std::sync::Arc;

use mt_core::ids::SessionId;
use mt_core::shell::{
    CommandFinished, CommandStarted, IntegrationMode, PersistenceState, ShellCwdChangedEvent,
    ShellIntegrationModeEvent, TabStateRemovedEvent, TabStateSnapshot,
};
use mt_db::Db;
use mt_db::repos::{
    CommandBlocksRepo, FinishCommandBlock, NewCommandBlock, SessionEnd, SessionsRepo,
};
use mt_db::sanitizer::SanitizedText;

/// Sink de eventos emitidos pelo `mt-shell` para a camada superior (ex.: eventos Tauri no binário).
pub trait ShellEventSink: Send + Sync + 'static {
    /// Emitido quando o snapshot de estado de uma aba muda (`tab://state-changed`).
    fn on_tab_state_changed(&self, snapshot: &TabStateSnapshot) {
        let _ = snapshot;
    }

    /// Emitido quando uma aba é removida do registro (`tab://state-removed`).
    fn on_tab_state_removed(&self, event: &TabStateRemovedEvent) {
        let _ = event;
    }

    /// Emitido quando um bloco de comando inicia (`shell://command-started`).
    fn on_command_started(&self, event: &CommandStarted) {
        let _ = event;
    }

    /// Emitido quando um bloco de comando termina (`shell://command-finished`).
    fn on_command_finished(&self, event: &CommandFinished) {
        let _ = event;
    }

    /// Emitido quando o diretório de trabalho de um painel muda (`shell://cwd-changed`).
    fn on_cwd_changed(&self, event: &ShellCwdChangedEvent) {
        let _ = event;
    }

    /// Emitido quando o modo de integração de um painel muda (`shell://integration-mode`).
    fn on_integration_mode_changed(&self, event: &ShellIntegrationModeEvent) {
        let _ = event;
    }
}

/// Implementação nula de [`ShellEventSink`] para testes sem Tauri.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullShellEventSink;

impl ShellEventSink for NullShellEventSink {}

/// Coordenador de persistência de sessões e blocos de comando.
#[derive(Clone, Debug)]
pub struct ShellPersister {
    sessions: SessionsRepo,
    blocks: CommandBlocksRepo,
}

impl ShellPersister {
    /// Cria um novo [`ShellPersister`] apoiado pela instância [`Db`].
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn new(db: Db) -> Self {
        Self {
            sessions: db.sessions(),
            blocks: db.command_blocks(),
        }
    }

    /// Retorna referência ao [`SessionsRepo`].
    #[must_use]
    pub const fn sessions(&self) -> &SessionsRepo {
        &self.sessions
    }

    /// Retorna referência ao [`CommandBlocksRepo`].
    #[must_use]
    pub const fn blocks(&self) -> &CommandBlocksRepo {
        &self.blocks
    }

    /// Persiste o início de um bloco de comando e retorna o [`PersistenceState`] resultante.
    pub fn persist_block_start_blocking(&self, block: NewCommandBlock) -> PersistenceState {
        match self.blocks.start_blocking(block) {
            Ok(()) => PersistenceState::Persisted,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "Falha ao persistir início de bloco de comando no SQLite"
                );
                PersistenceState::Failed
            }
        }
    }

    /// Persiste o término de um bloco de comando, emite `shell://command-finished` com o
    /// [`PersistenceState`] efetivo (`Persisted` ou `Failed`) e retorna o estado de persistência.
    pub fn persist_block_finish_blocking(
        &self,
        finish: FinishCommandBlock,
        mut event: CommandFinished,
        sink: &Arc<dyn ShellEventSink>,
    ) -> PersistenceState {
        let state = match self.blocks.finish_blocking(finish) {
            Ok(()) => PersistenceState::Persisted,
            Err(err) => {
                tracing::warn!(
                    block_id = %event.block_id,
                    error = %err,
                    "Falha ao persistir término de bloco de comando no SQLite; mantendo no cache em memória"
                );
                PersistenceState::Failed
            }
        };
        event.persistence = state;
        sink.on_command_finished(&event);
        state
    }

    /// Atualiza `last_cwd` e `integration_mode` da sessão de forma assíncrona via `Db::write_blocking`.
    pub fn update_session_runtime_blocking(
        &self,
        session_id: SessionId,
        last_cwd: Option<SanitizedText>,
        mode: IntegrationMode,
    ) {
        let sessions = self.sessions.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = sessions
                    .update_runtime_state(session_id, last_cwd, mode)
                    .await;
            });
        }
    }

    /// Fecha a sessão no SQLite ao encerrar o PTY.
    pub fn close_session_blocking(&self, session_id: SessionId, end: SessionEnd) {
        if let Err(err) = self.sessions.close_blocking(session_id, end) {
            tracing::warn!(
                session_id = %session_id,
                error = %err,
                "Falha ao fechar sessão no SQLite após encerramento do PTY"
            );
        }
    }
}
