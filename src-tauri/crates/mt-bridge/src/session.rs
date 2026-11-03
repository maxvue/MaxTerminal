//! Registro de sessões WebSocket ativas (`SessionRegistry`) e mapa de espera de ACKs (`PendingAckMap`).

use std::collections::HashMap;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use crate::error::{BridgeError, BridgeErrorCode};
use crate::protocol::{
    BridgeConnectionState, BridgeStatusSnapshot, RpcEnvelope, RpcPayload, current_unix_ms,
};

/// Tempo limite padrão para aguardar um ACK pela sessão loopback (3 000 ms).
pub const DEFAULT_ACK_TIMEOUT: Duration = Duration::from_millis(3000);

/// Metadados de um peer `MaxCode` autenticado.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Identificador da instância do `MaxCode`.
    pub instance_id: String,
    /// PID do processo da extensão.
    pub pid: u32,
    /// Versão da extensão.
    pub version: String,
    /// Capabilities anunciadas.
    pub capabilities: Vec<String>,
}

struct ActivePeerSlot {
    info: PeerInfo,
    tx: mpsc::Sender<RpcEnvelope>,
}

/// Gerenciador concorrente da sessão ativa com o `MaxCode` e das promessas de ACK pendentes.
#[derive(Default)]
pub struct SessionRegistry {
    active_peer: RwLock<Option<ActivePeerSlot>>,
    server_port: RwLock<Option<u16>>,
    state: RwLock<BridgeConnectionState>,
    last_rtt_ms: RwLock<Option<u64>>,
    last_seen_ms: RwLock<Option<i64>>,
    last_error: RwLock<Option<String>>,
    pending_acks: Mutex<HashMap<Uuid, oneshot::Sender<RpcPayload>>>,
}

impl SessionRegistry {
    /// Cria um novo `SessionRegistry`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Atualiza a porta do servidor loopback local.
    pub fn set_server_port(&self, port: Option<u16>) {
        *self.server_port.write() = port;
    }

    /// Atualiza o estado da conexão.
    pub fn set_state(&self, state: BridgeConnectionState) {
        *self.state.write() = state;
    }

    /// Registra o último erro observado.
    pub fn set_last_error(&self, err: Option<String>) {
        *self.last_error.write() = err;
    }

    /// Registra a última latência de ping/pong.
    pub fn record_rtt(&self, rtt_ms: u64) {
        *self.last_rtt_ms.write() = Some(rtt_ms);
        *self.last_seen_ms.write() = Some(current_unix_ms());
    }

    /// Marca atividade recente no canal.
    pub fn touch_last_seen(&self) {
        *self.last_seen_ms.write() = Some(current_unix_ms());
    }

    /// Registra um peer conectado com canal de envio WebSocket.
    pub fn register_peer(&self, info: PeerInfo, tx: mpsc::Sender<RpcEnvelope>) {
        *self.active_peer.write() = Some(ActivePeerSlot { info, tx });
        *self.state.write() = BridgeConnectionState::Connected;
        *self.last_seen_ms.write() = Some(current_unix_ms());
        *self.last_error.write() = None;
    }

    /// Registra metadados de um peer conectado via HTTP RPC (sem canal WebSocket persistente).
    pub fn update_peer_metadata(&self, info: PeerInfo) {
        let mut guard = self.active_peer.write();
        if let Some(slot) = guard.as_mut() {
            slot.info = info;
        } else {
            let (tx, _rx) = mpsc::channel(1);
            *guard = Some(ActivePeerSlot { info, tx });
        }
        *self.state.write() = BridgeConnectionState::Connected;
        *self.last_seen_ms.write() = Some(current_unix_ms());
    }

    /// Remove o peer ativo quando a conexão cai.
    pub fn clear_peer(&self) {
        *self.active_peer.write() = None;
        let mut st = self.state.write();
        if *st != BridgeConnectionState::Disabled {
            *st = BridgeConnectionState::Disconnected;
        }
    }

    /// Retorna as capabilities do peer conectado, se houver.
    #[must_use]
    pub fn peer_capabilities(&self) -> Vec<String> {
        self.active_peer
            .read()
            .as_ref()
            .map(|p| p.info.capabilities.clone())
            .unwrap_or_default()
    }

    /// Tenta enviar um `RpcEnvelope` pelo canal WebSocket ativo (não bloqueante).
    pub async fn try_send_ws(&self, envelope: RpcEnvelope) -> Result<(), BridgeError> {
        let tx_opt = self.active_peer.read().as_ref().map(|slot| slot.tx.clone());
        let Some(tx) = tx_opt else {
            return Err(BridgeError::new(
                BridgeErrorCode::PeerDisconnected,
                "Nenhuma sessão WebSocket ativa com o MaxCode.",
            ));
        };
        tx.send(envelope).await.map_err(|_| {
            self.clear_peer();
            BridgeError::new(
                BridgeErrorCode::PeerDisconnected,
                "Canal WebSocket com o MaxCode foi encerrado.",
            )
        })
    }

    /// Registra um `oneshot::Receiver` para aguardar o ACK correlacionado a `request_id`.
    pub fn register_pending_ack(&self, request_id: Uuid) -> oneshot::Receiver<RpcPayload> {
        let (tx, rx) = oneshot::channel();
        self.pending_acks.lock().insert(request_id, tx);
        rx
    }

    /// Resolve um ACK pendente se o `RpcPayload` corresponder a um `request_id` aguardado.
    pub fn resolve_if_ack(&self, envelope: &RpcEnvelope) -> bool {
        let target_id = match &envelope.message {
            RpcPayload::InspectAck(ack) => Some(ack.request_id),
            RpcPayload::TerminalOpenedAck(ack) => Some(ack.request_id),
            RpcPayload::IssueCommandAck(ack) => Some(ack.request_id),
            RpcPayload::BugsCycleAck(ack) => Some(ack.request_id),
            RpcPayload::Welcome(_) | RpcPayload::Pong(_) => Some(envelope.request_id),
            RpcPayload::Error(err) => err.request_id.or(Some(envelope.request_id)),
            _ => None,
        };

        if let Some(req_id) = target_id
            && let Some(tx) = self.pending_acks.lock().remove(&req_id)
        {
            let _ = tx.send(envelope.message.clone());
            return true;
        }
        false
    }

    /// Cancela uma espera de ACK após timeout.
    pub fn remove_pending_ack(&self, request_id: &Uuid) {
        self.pending_acks.lock().remove(request_id);
    }

    /// Constrói um `BridgeStatusSnapshot` consistente.
    #[must_use]
    pub fn snapshot(&self) -> BridgeStatusSnapshot {
        let peer = self.active_peer.read();
        BridgeStatusSnapshot {
            state: *self.state.read(),
            server_port: *self.server_port.read(),
            peer_instance_id: peer.as_ref().map(|p| p.info.instance_id.clone()),
            peer_pid: peer.as_ref().map(|p| p.info.pid),
            peer_version: peer.as_ref().map(|p| p.info.version.clone()),
            peer_capabilities: peer
                .as_ref()
                .map(|p| p.info.capabilities.clone())
                .unwrap_or_default(),
            last_rtt_ms: *self.last_rtt_ms.read(),
            last_seen_ms: *self.last_seen_ms.read(),
            last_error: self.last_error.read().clone(),
        }
    }
}
