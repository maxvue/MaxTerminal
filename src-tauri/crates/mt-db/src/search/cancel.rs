//! Registro de cancelamento determinístico (`CancelRegistry`) para consultas FTS5 em andamento.
//!
//! Protege contra:
//! 1. `search_cancel` chegando **antes** de `search_global` iniciar a leitura SQLite (via tombstones com TTL).
//! 2. `search_cancel(A)` chegando **depois** que a consulta `A` terminou e devolveu a conexão ao pool,
//!    onde uma consulta `B` já reutiliza a mesma conexão física (desvinculando o `InterruptHandle` sob mutex
//!    antes de liberar a conexão).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rusqlite::InterruptHandle;

use crate::search::model::CancelOutcome;
use crate::search::snapshot::SearchOwner;

const TOMBSTONE_TTL: Duration = Duration::from_secs(30);

/// Chave composta de uma requisição de busca por janela.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestKey {
    /// Janela dona da requisição.
    pub window_label: String,
    /// ID único da requisição.
    pub request_id: String,
}

impl RequestKey {
    /// Cria uma nova `RequestKey`.
    #[must_use]
    pub fn new(owner: &SearchOwner, request_id: impl Into<String>) -> Self {
        Self {
            window_label: owner.window_label.clone(),
            request_id: request_id.into(),
        }
    }
}

/// Estado do ciclo de vida de uma requisição no `CancelRegistry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestState {
    /// Na fila aguardando semáforo/conexão.
    Queued,
    /// Executando consulta SQLite com `InterruptHandle` vinculado.
    Running,
    /// Concluída normalmente.
    Completed,
    /// Cancelada explicitamente.
    Cancelled,
}

struct ActiveEntry {
    generation: u32,
    state: RequestState,
    cancelled_flag: Arc<AtomicBool>,
    interrupt_handle: Option<InterruptHandle>,
    registered_at: Instant,
}

#[derive(Default)]
struct RegistryInner {
    active: HashMap<RequestKey, ActiveEntry>,
    tombstones: HashMap<RequestKey, Instant>,
    latest_generation_by_owner: HashMap<String, u32>,
}

/// Token RAII de uma requisição registrada no `CancelRegistry`.
pub struct RegisteredSearchGuard {
    registry: Arc<CancelRegistry>,
    key: RequestKey,
    cancelled_flag: Arc<AtomicBool>,
}

impl RegisteredSearchGuard {
    /// Retorna `true` se a requisição foi marcada como cancelada.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled_flag.load(Ordering::Acquire)
    }

    /// Retorna clone do `Arc<AtomicBool>` para inspeção rápida dentro do `progress_handler` do SQLite.
    #[must_use]
    pub fn cancelled_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled_flag)
    }

    /// Vincula o `InterruptHandle` da conexão SQLite atual à requisição sob o mutex do registro.
    /// Se a requisição já tiver sido cancelada enquanto aguardava a conexão, dispara `interrupt()`
    /// imediatamente e retorna `false`.
    #[must_use]
    pub fn attach_interrupt_handle(&self, handle: InterruptHandle) -> bool {
        let mut guard = self.registry.inner.lock();
        if let Some(entry) = guard.active.get_mut(&self.key) {
            if entry.cancelled_flag.load(Ordering::Acquire)
                || entry.state == RequestState::Cancelled
            {
                entry.state = RequestState::Cancelled;
                return false;
            }
            entry.state = RequestState::Running;
            entry.interrupt_handle = Some(handle);
            true
        } else {
            false
        }
    }

    /// Desvincula o `InterruptHandle` sob o mutex **antes** de devolver a conexão ao pool,
    /// impedindo que um `cancel()` tardio interrompa uma consulta futura na mesma conexão.
    pub fn detach_interrupt_handle(&self) {
        let mut guard = self.registry.inner.lock();
        if let Some(entry) = guard.active.get_mut(&self.key) {
            entry.interrupt_handle = None;
            if entry.state == RequestState::Running {
                entry.state = RequestState::Completed;
            }
        }
    }
}

impl Drop for RegisteredSearchGuard {
    fn drop(&mut self) {
        let mut guard = self.registry.inner.lock();
        if let Some(mut entry) = guard.active.remove(&self.key) {
            entry.interrupt_handle = None;
        }
    }
}

/// Coordenador thread-safe de cancelamento de consultas de busca global.
#[derive(Default)]
pub struct CancelRegistry {
    inner: Mutex<RegistryInner>,
}

impl std::fmt::Debug for CancelRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancelRegistry").finish_non_exhaustive()
    }
}

impl CancelRegistry {
    /// Cria um novo `CancelRegistry`.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Registra o início de uma requisição `(owner, request_id, generation)`.
    ///
    /// - Qualquer requisição anterior da mesma janela com `generation < new_generation` é automaticamente
    ///   cancelada e interrompida.
    /// - Se já existir um tombstone prévio para `(owner, request_id)`, o guard já nasce com
    ///   `is_cancelled() == true`.
    #[must_use]
    pub fn register(
        self: &Arc<Self>,
        owner: &SearchOwner,
        request_id: &str,
        generation: u32,
    ) -> RegisteredSearchGuard {
        let now = Instant::now();
        let key = RequestKey::new(owner, request_id);
        let flag = Arc::new(AtomicBool::new(false));

        let mut guard = self.inner.lock();
        Self::gc_locked(&mut guard, now);

        // Se uma geração mais nova já foi vista para o mesmo owner, ou se há tombstone para esta key, nasce cancelada
        let pre_cancelled = guard.tombstones.remove(&key).is_some()
            || guard
                .latest_generation_by_owner
                .get(&owner.window_label)
                .is_some_and(|&latest| generation < latest);

        if let Some(latest) = guard
            .latest_generation_by_owner
            .get_mut(&owner.window_label)
        {
            if generation > *latest {
                *latest = generation;
            }
        } else {
            guard
                .latest_generation_by_owner
                .insert(owner.window_label.clone(), generation);
        }

        // Cancela consultas ativas de geração anterior da mesma janela
        for (other_key, entry) in &mut guard.active {
            if other_key.window_label == owner.window_label && entry.generation < generation {
                entry.cancelled_flag.store(true, Ordering::Release);
                entry.state = RequestState::Cancelled;
                if let Some(ref handle) = entry.interrupt_handle {
                    handle.interrupt();
                }
            }
        }

        if pre_cancelled {
            flag.store(true, Ordering::Release);
        }

        guard.active.insert(
            key.clone(),
            ActiveEntry {
                generation,
                state: if pre_cancelled {
                    RequestState::Cancelled
                } else {
                    RequestState::Queued
                },
                cancelled_flag: Arc::clone(&flag),
                interrupt_handle: None,
                registered_at: now,
            },
        );

        RegisteredSearchGuard {
            registry: Arc::clone(self),
            key,
            cancelled_flag: flag,
        }
    }

    /// Solicita o cancelamento de `(owner, request_id)`.
    ///
    /// Se a requisição ainda não chegou ao `register()`, grava um tombstone para que seja abortada
    /// assim que chegar. Se já terminou, retorna `CancelOutcome { cancelled: false }` sem tocar em nenhuma conexão.
    #[must_use]
    pub fn cancel(&self, owner: &SearchOwner, request_id: &str) -> CancelOutcome {
        let now = Instant::now();
        let key = RequestKey::new(owner, request_id);
        let mut guard = self.inner.lock();
        Self::gc_locked(&mut guard, now);

        if let Some(entry) = guard.active.get_mut(&key) {
            if entry.state == RequestState::Completed {
                return CancelOutcome { cancelled: false };
            }
            entry.cancelled_flag.store(true, Ordering::Release);
            entry.state = RequestState::Cancelled;
            if let Some(ref handle) = entry.interrupt_handle {
                handle.interrupt();
            }
            return CancelOutcome { cancelled: true };
        }

        // Ainda não registrada (ou já finalizada): registra tombstone temporário
        guard.tombstones.insert(key, now);
        CancelOutcome { cancelled: true }
    }

    fn gc_locked(inner: &mut RegistryInner, now: Instant) {
        inner
            .tombstones
            .retain(|_, ts| now.saturating_duration_since(*ts) < TOMBSTONE_TTL);
        inner.active.retain(|_, e| {
            now.saturating_duration_since(e.registered_at) < Duration::from_secs(120)
        });
    }
}
