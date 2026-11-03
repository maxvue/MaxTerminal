//! Contratos de reserva pré-spawn (`WorkspaceSpawnHook`, `WorkspaceSpawnGuard`, `SpawnHookChain`) para extensões futuras (Fase 12).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mt_core::error::{MtError, MtResult};
use mt_core::ids::{PaneId, ProjectId, TabId};
use mt_pty::PtyOwner;
use parking_lot::RwLock;

/// Dados da reserva solicitada antes de disparar o processo no PTY.
#[derive(Debug, Clone)]
pub struct SpawnReservationRequest {
    /// Identificador do projeto.
    pub project_id: ProjectId,
    /// Identificador completo do proprietário (`tab_id`, `pane_id`).
    pub owner: PtyOwner,
    /// Diretório de trabalho resolvido onde o processo será iniciado.
    pub cwd: PathBuf,
}

/// Guard RAII liberado somente quando o `WorkspaceManager` descarta a execução após compensação ou arquivamento.
pub trait WorkspaceSpawnGuard: Send + Sync {
    /// Libera os recursos reservados para o painel/sessão.
    fn release(self: Box<Self>);
}

/// Hook pré-spawn registrado opcionalmente no [`super::WorkspaceManager`].
pub trait WorkspaceSpawnHook: Send + Sync {
    /// Reserva recursos antes da criação do PTY e pode ajustar o mapa de ambiente em memória.
    /// Retornar `Err` aborta o spawn.
    fn reserve(
        &self,
        req: &SpawnReservationRequest,
        env: &mut BTreeMap<String, String>,
    ) -> Result<Box<dyn WorkspaceSpawnGuard>, MtError>;
}

/// Cadeia thread-safe de hooks pré-spawn.
#[derive(Clone, Default)]
pub struct SpawnHookChain {
    hooks: Arc<RwLock<Vec<Arc<dyn WorkspaceSpawnHook>>>>,
}

impl std::fmt::Debug for SpawnHookChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpawnHookChain")
            .field("count", &self.hooks.read().len())
            .finish()
    }
}

impl SpawnHookChain {
    /// Cria uma cadeia vazia de hooks pré-spawn.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra um novo hook na cadeia pré-spawn.
    pub fn register(&self, hook: Arc<dyn WorkspaceSpawnHook>) {
        self.hooks.write().push(hook);
    }

    /// Aplica todos os hooks registrados; em caso de falha em qualquer um, libera os guards já reservados.
    pub fn apply(
        &self,
        project_id: ProjectId,
        tab_id: TabId,
        pane_id: PaneId,
        cwd: &Path,
        env: &mut BTreeMap<String, String>,
    ) -> MtResult<Vec<Box<dyn WorkspaceSpawnGuard>>> {
        let req = SpawnReservationRequest {
            project_id,
            owner: PtyOwner {
                tab_id: Some(tab_id),
                pane_id: Some(pane_id),
            },
            cwd: cwd.to_path_buf(),
        };
        let hooks = self.hooks.read().clone();
        let mut guards = Vec::with_capacity(hooks.len());
        for hook in hooks {
            match hook.reserve(&req, env) {
                Ok(g) => guards.push(g),
                Err(err) => {
                    for g in guards {
                        g.release();
                    }
                    return Err(err);
                }
            }
        }
        Ok(guards)
    }
}
