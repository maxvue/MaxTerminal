//! Registro concorrente de requisições de IA com suporte a cancelamento precoce (tombstones)
//! e guarda RAII (`RequestGuard`).

use crate::error::{ai_error, codes};
use crate::ids::AiRequestId;
use mt_core::MtResult;
use mt_core::ids::{PaneId, PtyId, SessionId};
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const STATE_RUNNING: u8 = 0;
const STATE_COMPLETED: u8 = 1;
const STATE_FAILED: u8 = 2;
const STATE_CANCELLED: u8 = 3;

const MAX_TOMBSTONES: usize = 256;
const TOMBSTONE_TTL: Duration = Duration::from_secs(120);

/// Estado terminal de uma requisição de IA.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalState {
    /// Concluída com sucesso.
    Completed,
    /// Encerrada com falha.
    Failed,
    /// Cancelada pelo usuário ou pelo encerramento do painel/sessão.
    Cancelled,
}

/// Metadados contextuais de uma requisição ativa para invalidação seletiva.
#[derive(Clone, Debug)]
pub struct RequestMeta {
    /// Painel associado à requisição (`None` em chamadas globais).
    pub pane_id: Option<PaneId>,
    /// Sessão de painel ativa no momento da criação.
    pub session_id: Option<SessionId>,
    /// PTY ativo no momento da criação.
    pub pty_id: Option<PtyId>,
    /// Revisão das configurações de IA no momento da criação.
    pub config_revision: u32,
}

#[derive(Clone)]
struct ActiveEntry {
    meta: RequestMeta,
    cancel: CancellationToken,
    state: Arc<AtomicU8>,
}

/// Registro concorrente de requisições de IA ativas, com suporte a tombstones de cancelamento precoce
/// e guarda RAII (`RequestGuard`) para garantir exatamente um estado terminal por `AiRequestId` (CA-09, CA-33).
pub struct AiRequestRegistry {
    active: Arc<RwLock<HashMap<AiRequestId, ActiveEntry>>>,
    recent_finished: Arc<Mutex<VecDeque<(AiRequestId, Instant)>>>,
    early_cancelled: Arc<Mutex<VecDeque<(AiRequestId, Instant)>>>,
}

impl Default for AiRequestRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AiRequestRegistry {
    /// Cria um novo registro vazio de requisições.
    #[must_use]
    pub fn new() -> Self {
        Self {
            active: Arc::new(RwLock::new(HashMap::new())),
            recent_finished: Arc::new(Mutex::new(VecDeque::new())),
            early_cancelled: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Registra o início de uma requisição.
    /// - Se o `id` já estiver ativo ou tiver sido concluído recentemente, retorna `ai.request_conflict`.
    /// - Se um cancelamento chegou antes do `begin` (tombstone), consome o tombstone e retorna `ai.cancelled`.
    pub fn begin(&self, id: AiRequestId, meta: RequestMeta) -> MtResult<RequestGuard> {
        let now = Instant::now();
        {
            let mut early = self.early_cancelled.lock();
            early.retain(|(_, ts)| now.duration_since(*ts) < TOMBSTONE_TTL);
            if let Some(pos) = early.iter().position(|(eid, _)| *eid == id) {
                early.remove(pos);
                return Err(ai_error(
                    codes::AI_CANCELLED,
                    "Requisição cancelada antes de iniciar.",
                ));
            }
        }
        {
            let mut recent = self.recent_finished.lock();
            recent.retain(|(_, ts)| now.duration_since(*ts) < TOMBSTONE_TTL);
            if recent.iter().any(|(rid, _)| *rid == id) {
                return Err(ai_error(
                    codes::AI_REQUEST_CONFLICT,
                    "Esta solicitação já foi registrada anteriormente.",
                ));
            }
        }

        let mut active = self.active.write();
        if active.contains_key(&id) {
            return Err(ai_error(
                codes::AI_REQUEST_CONFLICT,
                "Esta solicitação já está em execução.",
            ));
        }

        let cancel = CancellationToken::new();
        let state = Arc::new(AtomicU8::new(STATE_RUNNING));
        active.insert(
            id,
            ActiveEntry {
                meta,
                cancel: cancel.clone(),
                state: state.clone(),
            },
        );
        drop(active);

        Ok(RequestGuard {
            id,
            cancel,
            state,
            active: self.active.clone(),
            recent_finished: self.recent_finished.clone(),
        })
    }

    /// Cancela uma requisição específica. Se ela ainda não entrou em `begin`, grava um tombstone.
    #[must_use]
    pub fn cancel(&self, id: AiRequestId) -> bool {
        if let Some(entry) = self.active.read().get(&id).cloned() {
            if entry
                .state
                .compare_exchange(
                    STATE_RUNNING,
                    STATE_CANCELLED,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
            {
                entry.cancel.cancel();
                return true;
            }
            return false;
        }

        let now = Instant::now();
        {
            let recent = self.recent_finished.lock();
            if recent
                .iter()
                .any(|(rid, ts)| *rid == id && now.duration_since(*ts) < TOMBSTONE_TTL)
            {
                return false;
            }
        }

        let mut early = self.early_cancelled.lock();
        early.retain(|(_, ts)| now.duration_since(*ts) < TOMBSTONE_TTL);
        if !early.iter().any(|(eid, _)| *eid == id) {
            if early.len() >= MAX_TOMBSTONES {
                early.pop_front();
            }
            early.push_back((id, now));
        }
        true
    }

    /// Cancela todas as requisições associadas a um painel (chamado ao fechar painel/aba ou trocar PTY/sessão).
    #[must_use]
    pub fn cancel_pane(&self, pane: PaneId) -> usize {
        let entries: Vec<ActiveEntry> = self
            .active
            .read()
            .values()
            .filter(|e| e.meta.pane_id == Some(pane))
            .cloned()
            .collect();
        let mut count = 0;
        for entry in entries {
            if entry
                .state
                .compare_exchange(
                    STATE_RUNNING,
                    STATE_CANCELLED,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
            {
                entry.cancel.cancel();
                count += 1;
            }
        }
        count
    }

    /// Cancela todas as requisições ativas (chamado ao alterar URL/dialeto ou desabilitar IA).
    #[must_use]
    pub fn cancel_all(&self) -> usize {
        let entries: Vec<ActiveEntry> = self.active.read().values().cloned().collect();
        let mut count = 0;
        for entry in entries {
            if entry
                .state
                .compare_exchange(
                    STATE_RUNNING,
                    STATE_CANCELLED,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
            {
                entry.cancel.cancel();
                count += 1;
            }
        }
        count
    }
}

/// Guarda RAII de uma requisição em andamento no `AiRequestRegistry`.
pub struct RequestGuard {
    id: AiRequestId,
    cancel: CancellationToken,
    state: Arc<AtomicU8>,
    active: Arc<RwLock<HashMap<AiRequestId, ActiveEntry>>>,
    recent_finished: Arc<Mutex<VecDeque<(AiRequestId, Instant)>>>,
}

impl RequestGuard {
    /// Retorna o identificador da requisição.
    #[must_use]
    pub fn id(&self) -> AiRequestId {
        self.id
    }

    /// Retorna o token de cancelamento cooperativo.
    #[must_use]
    pub fn token(&self) -> &CancellationToken {
        &self.cancel
    }

    /// Indica se a requisição já foi cancelada.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::SeqCst) == STATE_CANCELLED || self.cancel.is_cancelled()
    }

    /// Tenta finalizar atomicamente a requisição como `Completed` ou `Failed`.
    /// Retorna `true` se esta transição venceu, ou `false` se o cancelamento já havia vencido.
    #[must_use]
    pub fn try_finish(&self, terminal: TerminalState) -> bool {
        let target = match terminal {
            TerminalState::Completed => STATE_COMPLETED,
            TerminalState::Failed => STATE_FAILED,
            TerminalState::Cancelled => STATE_CANCELLED,
        };
        let ok = self
            .state
            .compare_exchange(STATE_RUNNING, target, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if target == STATE_CANCELLED && ok {
            self.cancel.cancel();
        }
        ok
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        let _ = self.state.compare_exchange(
            STATE_RUNNING,
            STATE_CANCELLED,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        self.active.write().remove(&self.id);
        let mut recent = self.recent_finished.lock();
        if recent.len() >= MAX_TOMBSTONES {
            recent.pop_front();
        }
        recent.push_back((self.id, Instant::now()));
    }
}
