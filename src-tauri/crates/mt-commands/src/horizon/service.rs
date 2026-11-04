//! Serviço de domínio para orquestração de medições e limpeza do Laravel Horizon.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use mt_core::{MtError, MtResult, ProjectId};
use parking_lot::RwLock;
use uuid::Uuid;

use crate::horizon::cleaner::execute_horizon_clear;
use crate::horizon::model::{
    HorizonAvailability, HorizonClearPreview, HorizonClearResult, HorizonContext,
    HorizonSessionDto, HorizonSnapshot, HorizonStatus, QueueScope,
};
use crate::horizon::probe::{
    build_snapshot_from_probe, compute_capabilities, has_static_horizon_manifest,
    run_horizon_probe,
};
use crate::ports::{Clock, ProjectLookup};

const SNAPSHOT_CACHE_TTL_MS: i64 = 4_000;
const CLEAR_TOKEN_TTL_MS: i64 = 30_000;

#[derive(Clone)]
struct CachedProbe {
    sampled_at: i64,
    snapshot: HorizonSnapshot,
}

#[derive(Clone)]
struct PendingConfirmation {
    project_id: ProjectId,
    expires_at_ms: i64,
    context_revision: String,
    scope_id: String,
    queues: Vec<QueueScope>,
}

/// Serviço de domínio para descoberta, métricas e limpeza exclusiva do Horizon.
pub struct HorizonDomainService {
    lookup: Arc<dyn ProjectLookup>,
    clock: Arc<dyn Clock>,
    cache: DashMap<ProjectId, CachedProbe>,
    confirmations: Arc<RwLock<HashMap<String, PendingConfirmation>>>,
    revisions: DashMap<ProjectId, u64>,
}

impl HorizonDomainService {
    /// Cria uma nova instância do [`HorizonDomainService`].
    #[must_use]
    pub fn new(lookup: Arc<dyn ProjectLookup>, clock: Arc<dyn Clock>) -> Self {
        Self {
            lookup,
            clock,
            cache: DashMap::new(),
            confirmations: Arc::new(RwLock::new(HashMap::new())),
            revisions: DashMap::new(),
        }
    }

    /// Resolve e valida a raiz cadastrada do projeto autorizado.
    pub fn resolve_project_root(&self, project_id: ProjectId) -> MtResult<PathBuf> {
        let Some(path) = self.lookup.project_by_id(project_id) else {
            return Err(MtError::new(
                mt_core::ErrorCode("horizon.project_not_found"),
                format!("Projeto com ID '{}' não encontrado.", project_id),
            ));
        };

        let canon = std::fs::canonicalize(&path).unwrap_or(path);
        Ok(canon)
    }

    /// Obtém o snapshot mais recente do Horizon para o projeto informado.
    pub async fn snapshot(
        &self,
        project_id: ProjectId,
        supervised_status: Option<HorizonStatus>,
        session: Option<HorizonSessionDto>,
        force_refresh: bool,
    ) -> MtResult<HorizonSnapshot> {
        let now = self.clock.now_ms();

        // 1. Checagem de cache recente em memória
        if !force_refresh {
            if let Some(entry) = self.cache.get(&project_id) {
                if now.saturating_sub(entry.sampled_at) <= SNAPSHOT_CACHE_TTL_MS {
                    let mut current = entry.snapshot.clone();
                    if let Some(st) = supervised_status {
                        current.status = st;
                        current.session = session;
                        let (can_start, can_stop, can_clear) = compute_capabilities(
                            current.availability,
                            current.status,
                            !current.queues.is_empty(),
                            current.pending,
                        );
                        current.can_start = can_start;
                        current.can_stop = can_stop;
                        current.can_clear_pending = can_clear;
                    }
                    return Ok(current);
                }
            }
        }

        let root = self.resolve_project_root(project_id)?;

        // 2. Gate estático barato: se não houver manifest com horizon, retorna indisponível
        if !has_static_horizon_manifest(&root) {
            let snap = HorizonSnapshot {
                context: HorizonContext {
                    project_id,
                    root: root.display().to_string(),
                    context_revision: String::new(),
                    scope_id: String::new(),
                },
                availability: HorizonAvailability::Unavailable,
                horizon_version: None,
                queues: Vec::new(),
                environment: None,
                status: HorizonStatus::Stopped,
                pending: None,
                running: None,
                failed: None,
                sampled_at_ms: now,
                revision: 1,
                diagnostic: None,
                can_start: false,
                can_stop: false,
                can_clear_pending: false,
                session: None,
            };
            self.cache.insert(
                project_id,
                CachedProbe {
                    sampled_at: now,
                    snapshot: snap.clone(),
                },
            );
            return Ok(snap);
        }

        // 3. Execução do probe dinâmico via helper PHP
        let revision = self
            .revisions
            .entry(project_id)
            .and_modify(|r| *r = r.saturating_add(1))
            .or_insert(1);

        let probe_res = run_horizon_probe(&root).await;
        let snap = match probe_res {
            Ok(probe) => build_snapshot_from_probe(
                project_id,
                &root,
                Some(&probe),
                supervised_status,
                session,
                *revision,
                now,
            ),
            Err(err) => {
                let mut fallback = build_snapshot_from_probe(
                    project_id,
                    &root,
                    None,
                    supervised_status,
                    session,
                    *revision,
                    now,
                );
                fallback.availability = HorizonAvailability::Error;
                fallback.diagnostic = Some(err.message.to_string());
                fallback
            }
        };

        self.cache.insert(
            project_id,
            CachedProbe {
                sampled_at: now,
                snapshot: snap.clone(),
            },
        );

        Ok(snap)
    }

    /// Gera uma prévia e token seguro temporário para limpeza de tarefas pendentes.
    pub async fn clear_pending_preview(
        &self,
        project_id: ProjectId,
    ) -> MtResult<HorizonClearPreview> {
        let snap = self.snapshot(project_id, None, None, true).await?;

        if snap.availability != HorizonAvailability::Available {
            return Err(MtError::new(
                mt_core::ErrorCode("horizon.unavailable"),
                "Horizon não está disponível para este projeto.",
            ));
        }

        if snap.queues.is_empty() {
            return Err(MtError::new(
                mt_core::ErrorCode("horizon.no_queues"),
                "Nenhuma fila configurada encontrada para limpeza.",
            ));
        }

        let now = self.clock.now_ms();
        let expires_at_ms = now + CLEAR_TOKEN_TTL_MS;
        let token = Uuid::new_v4().to_string();

        let pending_conf = PendingConfirmation {
            project_id,
            expires_at_ms,
            context_revision: snap.context.context_revision.clone(),
            scope_id: snap.context.scope_id.clone(),
            queues: snap.queues.clone(),
        };

        {
            let mut confs = self.confirmations.write();
            // Limpa tokens expirados
            confs.retain(|_, v| v.expires_at_ms > now);
            confs.insert(token.clone(), pending_conf);
        }

        Ok(HorizonClearPreview {
            token,
            context_revision: snap.context.context_revision,
            scope_id: snap.context.scope_id,
            queues: snap.queues,
            estimated_pending: snap.pending.unwrap_or(0),
            expires_at_ms,
        })
    }

    /// Executa a exclusão de pendentes após validação do token opaco de confirmação.
    pub async fn clear_pending(
        &self,
        project_id: ProjectId,
        token: &str,
    ) -> MtResult<HorizonClearResult> {
        let now = self.clock.now_ms();

        // 1. Consome o token de forma estritamente idempotente (single-use)
        let conf = {
            let mut confs = self.confirmations.write();
            confs.remove(token)
        };

        let Some(conf) = conf else {
            return Err(MtError::new(
                mt_core::ErrorCode("horizon.token_invalid_or_expired"),
                "Token de confirmação inválido ou já utilizado.",
            ));
        };

        // Validação estrita de escopo: o token deve pertencer ao mesmo projeto
        if conf.project_id != project_id {
            return Err(MtError::new(
                mt_core::ErrorCode("horizon.token_project_mismatch"),
                "O token de confirmação não pertence ao projeto selecionado.",
            ));
        }

        if conf.expires_at_ms <= now {
            return Err(MtError::new(
                mt_core::ErrorCode("horizon.token_expired"),
                "O tempo para confirmar a limpeza expirou (máximo de 30 segundos).",
            ));
        }

        // Revalida o contexto atual antes de executar exclusão
        let current_snap = self.snapshot(project_id, None, None, true).await?;
        if current_snap.availability != HorizonAvailability::Available {
            return Err(MtError::new(
                mt_core::ErrorCode("horizon.unavailable"),
                "Horizon tornou-se indisponível antes da execução da limpeza.",
            ));
        }
        if current_snap.context.context_revision != conf.context_revision
            || current_snap.context.scope_id != conf.scope_id
        {
            return Err(MtError::new(
                mt_core::ErrorCode("horizon.context_mismatch"),
                "O contexto das filas foi alterado; confirme novamente a limpeza.",
            ));
        }

        let root = self.resolve_project_root(project_id)?;

        // 2. Executa a limpeza exclusiva preservando tarefas reservadas
        let result = execute_horizon_clear(&root, &conf.queues, 10_000).await?;

        // 3. Invalida o cache de snapshot para forçar atualização no próximo polling
        self.cache.remove(&project_id);

        Ok(result)
    }

    /// Invalida o cache e as confirmações temporárias pendentes do projeto informado.
    pub fn invalidate_cache(&self, project_id: ProjectId) {
        self.cache.remove(&project_id);
        let mut confs = self.confirmations.write();
        confs.retain(|_, v| v.project_id != project_id);
    }
}
