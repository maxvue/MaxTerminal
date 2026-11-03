//! Eventos de ciclo de vida de baixo volume (`PtyEvent`) e hub de distribuição para listeners e subscribers.

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender, TrySendError};
use mt_core::ids::PtyId;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::types::{ExitInfo, ProcessIdentity, ProcessInfo, PtyOwner, PtySummary, U64Decimal};

/// Capacidade padrão da fila individual de cada subscriber de eventos.
pub const SUBSCRIBER_QUEUE_CAPACITY: usize = 256;

/// Payload do evento `pty://spawned`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySpawnedEvent {
    /// Fotografia inicial do PTY criado.
    pub summary: PtySummary,
    /// Revisão monotônica do evento.
    pub revision: U64Decimal,
}

/// Payload do evento `pty://foreground-changed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyForegroundChangedEvent {
    /// Identificador do PTY.
    pub pty_id: PtyId,
    /// Novo PGID em foreground.
    pub pgid: Option<u32>,
    /// Informações do processo representativo do foreground.
    pub process: Option<ProcessInfo>,
    /// Revisão monotônica do evento.
    pub revision: U64Decimal,
}

/// Payload do evento `pty://exit`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyExitEvent {
    /// Identificador do PTY finalizado.
    pub pty_id: PtyId,
    /// Vínculo de aba/painel do PTY.
    pub owner: PtyOwner,
    /// Detalhes do término.
    pub exit: ExitInfo,
    /// Último número de sequência de frame `Data` emitido antes do `End`.
    pub final_seq: U64Decimal,
    /// Revisão monotônica do evento.
    pub revision: U64Decimal,
}

/// Payload do evento `pty://cleanup-failed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyCleanupFailedEvent {
    /// Identificador do PTY cuja limpeza falhou.
    pub pty_id: PtyId,
    /// Identidades dos processos sobreviventes no cgroup.
    pub survivors: Vec<ProcessIdentity>,
    /// Revisão monotônica do evento.
    pub revision: U64Decimal,
}

/// Evento interno de ciclo de vida de baixo volume emitido pelo `PtyManager`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PtyEvent {
    /// Emitido quando um novo PTY é iniciado.
    #[serde(rename_all = "camelCase")]
    Spawned {
        /// Resumo do PTY criado.
        summary: PtySummary,
        /// Revisão monotônica.
        revision: U64Decimal,
    },
    /// Emitido quando o grupo de processos em foreground muda.
    #[serde(rename_all = "camelCase")]
    ForegroundChanged {
        /// Identificador do PTY.
        pty_id: PtyId,
        /// PGID em foreground.
        pgid: Option<u32>,
        /// Processo representativo do foreground.
        process: Option<ProcessInfo>,
        /// Revisão monotônica.
        revision: U64Decimal,
    },
    /// Emitido quando o PTY e seu cgroup encerram completamente.
    #[serde(rename_all = "camelCase")]
    Exited {
        /// Identificador do PTY.
        pty_id: PtyId,
        /// Dono opaco do PTY.
        owner: PtyOwner,
        /// Dados de saída.
        exit: ExitInfo,
        /// Último número de sequência de dados.
        final_seq: U64Decimal,
        /// Revisão monotônica.
        revision: U64Decimal,
    },
    /// Emitido quando processos sobrevivem ao deadline de encerramento.
    #[serde(rename_all = "camelCase")]
    CleanupFailed {
        /// Identificador do PTY.
        pty_id: PtyId,
        /// Processos sobreviventes.
        survivors: Vec<ProcessIdentity>,
        /// Revisão monotônica.
        revision: U64Decimal,
    },
}

/// Listener para eventos de ciclo de vida do PTY.
pub trait PtyEventListener: Send + Sync + 'static {
    /// Invocado de forma síncrona e não bloqueante a cada evento emitido.
    fn on_event(&self, event: &PtyEvent);
}

/// Hub de distribuição (fan-out) de `PtyEvent` para listeners e canais assinantes limitados.
#[derive(Default)]
pub struct EventHub {
    listeners: RwLock<Vec<Arc<dyn PtyEventListener>>>,
    subscribers: RwLock<Vec<Sender<PtyEvent>>>,
}

impl EventHub {
    /// Cria um novo hub de eventos vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra um listener de eventos.
    pub fn add_listener(&self, listener: Arc<dyn PtyEventListener>) {
        self.listeners.write().push(listener);
    }

    /// Cria uma nova assinatura dedicada com fila limitada (`SUBSCRIBER_QUEUE_CAPACITY`).
    #[must_use]
    pub fn subscribe(&self) -> Receiver<PtyEvent> {
        let (tx, rx) = crossbeam_channel::bounded(SUBSCRIBER_QUEUE_CAPACITY);
        self.subscribers.write().push(tx);
        rx
    }

    /// Publica um evento para todos os listeners e assinantes ativos sem bloquear.
    pub fn publish(&self, event: &PtyEvent) {
        let listeners = self.listeners.read().clone();
        for listener in &listeners {
            listener.on_event(event);
        }

        let mut subs = self.subscribers.write();
        subs.retain(|tx| match tx.try_send(event.clone()) {
            Ok(()) | Err(TrySendError::Full(_)) => true,
            Err(TrySendError::Disconnected(_)) => false,
        });
    }
}
