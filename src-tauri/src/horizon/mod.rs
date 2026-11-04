//! Serviço nativo de gerenciamento do ciclo de vida do daemon Laravel Horizon.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use mt_commands::horizon::{
    HorizonClearPreview, HorizonClearResult, HorizonDomainService, HorizonSessionDto,
    HorizonSnapshot, HorizonStatus,
};
use mt_core::ids::{PaneId, ProjectId, PtyId};
use mt_core::{ErrorCode, MtError, MtResult};
use mt_projects::ProjectRemovalHook;
use mt_pty::{KillMode, PtyExitEvent, PtyManager, PtyOwner, PtySize, SpawnSpec};
use nix::sys::signal::Signal;
use parking_lot::RwLock;

/// Estado de uma sessão supervisionada do daemon Horizon.
#[derive(Debug, Clone)]
pub struct HorizonSessionState {
    pub pty_id: PtyId,
    pub pane_id: PaneId,
    pub generation: u64,
    pub status: HorizonStatus,
    pub scope_id: String,
    pub started_at_ms: i64,
}

/// Serviço de coordenação nativa do runtime do Horizon com o PtyManager.
pub struct HorizonRuntimeService {
    pty: Arc<PtyManager>,
    domain: Arc<HorizonDomainService>,
    sessions: Arc<RwLock<HashMap<ProjectId, HorizonSessionState>>>,
    generation_counter: Arc<RwLock<HashMap<ProjectId, u64>>>,
}

impl HorizonRuntimeService {
    /// Constrói um novo [`HorizonRuntimeService`].
    #[must_use]
    pub fn new(pty: Arc<PtyManager>, domain: Arc<HorizonDomainService>) -> Arc<Self> {
        Arc::new(Self {
            pty,
            domain,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            generation_counter: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Retorna o [`HorizonDomainService`] associado.
    #[must_use]
    pub fn domain(&self) -> &Arc<HorizonDomainService> {
        &self.domain
    }

    /// Obtém o snapshot atual com status sincronizado do PTY supervisionado.
    pub async fn snapshot(&self, project_id: ProjectId) -> MtResult<HorizonSnapshot> {
        let (sup_status, session_dto) = {
            let sessions = self.sessions.read();
            if let Some(s) = sessions.get(&project_id) {
                // Sincroniza estado com o PtyManager caso o processo tenha encerrado
                let actual_status = match self.pty.get_instance(s.pty_id) {
                    Some(inst) => match inst.status() {
                        mt_pty::PtyStatus::Running => s.status,
                        mt_pty::PtyStatus::Exiting { .. } => HorizonStatus::Stopping,
                        mt_pty::PtyStatus::Exited { .. } => HorizonStatus::Stopped,
                        mt_pty::PtyStatus::CleanupFailed { .. } => HorizonStatus::Error,
                    },
                    None => HorizonStatus::Stopped,
                };
                (
                    Some(actual_status),
                    Some(HorizonSessionDto {
                        pty_id: s.pty_id,
                        pane_id: s.pane_id,
                        generation: s.generation,
                    }),
                )
            } else {
                (None, None)
            }
        };

        self.domain
            .snapshot(project_id, sup_status, session_dto, false)
            .await
    }

    /// Inicia o daemon `php artisan horizon` em um PTY supervisionado e exclusivo.
    pub async fn start(
        &self,
        project_id: ProjectId,
        expected_context_revision: &str,
        _request_id: &str,
    ) -> MtResult<HorizonSnapshot> {
        let current_snap = self.domain.snapshot(project_id, None, None, true).await?;

        if !expected_context_revision.is_empty()
            && current_snap.context.context_revision != expected_context_revision
        {
            return Err(MtError::new(
                ErrorCode("horizon.context_mismatch"),
                "A revisão de contexto foi alterada; atualize as informações antes de iniciar.",
            ));
        }

        // Verifica conflito de destino (mesmo Redis/prefixo já em execução em outro projeto)
        let scope_id = current_snap.context.scope_id.clone();
        let already_running = {
            let sessions = self.sessions.read();
            for (p_id, s) in sessions.iter() {
                if *p_id != project_id
                    && s.scope_id == scope_id
                    && matches!(s.status, HorizonStatus::Running | HorizonStatus::Starting)
                {
                    return Err(MtError::new(
                        ErrorCode("horizon.destination_conflict"),
                        "Já existe um daemon do Horizon em execução para este mesmo destino Redis.",
                    ));
                }
            }
            if let Some(s) = sessions.get(&project_id) {
                matches!(s.status, HorizonStatus::Running | HorizonStatus::Starting)
            } else {
                false
            }
        };

        if already_running {
            return self.snapshot(project_id).await;
        }

        let root = self.domain.resolve_project_root(project_id)?;

        let generation = {
            let mut gens = self.generation_counter.write();
            let g = gens.entry(project_id).or_insert(0);
            *g = g.saturating_add(1);
            *g
        };

        let pane_id = PaneId::new();
        let spec = SpawnSpec {
            shell: Some("php".to_string()),
            args: vec!["artisan".to_string(), "horizon".to_string()],
            login: false,
            cwd: Some(root.display().to_string()),
            owner: PtyOwner {
                tab_id: None,
                pane_id: Some(pane_id),
            },
            size: PtySize {
                cols: 120,
                rows: 32,
                pixel_width: 0,
                pixel_height: 0,
            },
            ..Default::default()
        };

        let summary = self.pty.spawn(spec).map_err(|e| {
            MtError::new(
                ErrorCode("horizon.spawn_failed"),
                format!("Falha ao iniciar processo PTY do Horizon: {e}"),
            )
        })?;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(0))
            .unwrap_or(0);

        let session_state = HorizonSessionState {
            pty_id: summary.id,
            pane_id,
            generation,
            status: HorizonStatus::Running,
            scope_id,
            started_at_ms: now_ms,
        };

        {
            let mut sessions = self.sessions.write();
            sessions.insert(project_id, session_state);
        }

        // Força novo snapshot refletindo a sessão iniciada
        self.snapshot(project_id).await
    }

    /// Encerra graciosamente o processo master do Horizon gerenciado.
    pub async fn stop(
        &self,
        project_id: ProjectId,
        expected_generation: u64,
        _request_id: &str,
    ) -> MtResult<HorizonSnapshot> {
        let (pty_id, target_inst) = {
            let mut sessions = self.sessions.write();
            if let Some(session) = sessions.get_mut(&project_id) {
                if expected_generation > 0 && session.generation != expected_generation {
                    return Err(MtError::new(
                        ErrorCode("horizon.generation_mismatch"),
                        "A geração do processo ativo mudou; parada cancelada para evitar corrida.",
                    ));
                }

                session.status = HorizonStatus::Stopping;
                let pty_id = session.pty_id;
                let inst = self.pty.get_instance(pty_id);
                (Some(pty_id), inst)
            } else {
                (None, None)
            }
        };

        if let Some(pty_id) = pty_id {
            // Envia SIGTERM cirurgicamente apenas ao líder do PTY (o processo master do Horizon)
            if let Some(inst) = target_inst {
                let leader = inst.leader_identity();
                let _ = self.pty.signal_process(pty_id, &leader, Signal::SIGTERM);
            } else {
                let _ = self.pty.kill(pty_id, KillMode::Graceful);
            }
        }

        self.snapshot(project_id).await
    }

    /// Solicita prévia de limpeza exclusiva de pendentes.
    pub async fn clear_pending_preview(
        &self,
        project_id: ProjectId,
    ) -> MtResult<HorizonClearPreview> {
        self.domain.clear_pending_preview(project_id).await
    }

    /// Executa a limpeza confirmada de tarefas pendentes.
    pub async fn clear_pending(
        &self,
        project_id: ProjectId,
        token: &str,
        _request_id: &str,
    ) -> MtResult<HorizonClearResult> {
        self.domain.clear_pending(project_id, token).await
    }

    /// Verifica se há algum daemon do Horizon em execução ativa (`Running` ou `Starting`).
    #[must_use]
    pub fn has_active_daemon(&self) -> bool {
        let sessions = self.sessions.read();
        sessions.values().any(|s| {
            if matches!(s.status, HorizonStatus::Running | HorizonStatus::Starting) {
                if let Some(inst) = self.pty.get_instance(s.pty_id) {
                    matches!(inst.status(), mt_pty::PtyStatus::Running)
                } else {
                    false
                }
            } else {
                false
            }
        })
    }

    /// Hook de evento de saída de PTY emitido pelo PtyManager.
    pub fn on_pty_exit(&self, event: &PtyExitEvent) {
        let mut sessions = self.sessions.write();
        for s in sessions.values_mut() {
            if s.pty_id == event.pty_id {
                s.status = HorizonStatus::Stopped;
            }
        }
    }

    /// Encerra todos os runtimes supervisionados do Horizon no desligamento do MaxTerminal.
    pub fn shutdown(&self) {
        let sessions = self.sessions.read();
        for s in sessions.values() {
            if matches!(s.status, HorizonStatus::Running | HorizonStatus::Starting) {
                if let Some(inst) = self.pty.get_instance(s.pty_id) {
                    let leader = inst.leader_identity();
                    let _ = self.pty.signal_process(s.pty_id, &leader, Signal::SIGTERM);
                }
                let _ = self.pty.kill(s.pty_id, KillMode::Graceful);
            }
        }
    }

    /// Desliga o daemon associado a um projeto removido.
    pub async fn on_project_deleting(&self, project_id: &ProjectId) {
        let pty_to_kill = {
            let mut sessions = self.sessions.write();
            sessions.remove(project_id).map(|s| s.pty_id)
        };

        if let Some(pty_id) = pty_to_kill {
            let _ = self.pty.kill(pty_id, KillMode::Graceful);
        }
        self.domain.invalidate_cache(*project_id);
    }
}

/// Adaptador para o hook de remoção de projeto do `ProjectService`.
pub struct HorizonProjectRemovalHook {
    service: Arc<HorizonRuntimeService>,
}

impl HorizonProjectRemovalHook {
    /// Cria um novo [`HorizonProjectRemovalHook`].
    #[must_use]
    pub fn new(service: Arc<HorizonRuntimeService>) -> Self {
        Self { service }
    }
}

#[async_trait]
impl ProjectRemovalHook for HorizonProjectRemovalHook {
    async fn before_remove(&self, project_id: ProjectId) -> MtResult<()> {
        self.service.on_project_deleting(&project_id).await;
        Ok(())
    }
}
