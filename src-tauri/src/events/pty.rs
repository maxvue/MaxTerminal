//! Integração de eventos de ciclo de vida do PTY e envelope binário de saída para `tauri::ipc::Channel`.

use bytes::Bytes;
use mt_pty::{
    OutputSink, PtyCleanupFailedEvent, PtyEvent, PtyEventListener, PtyExitEvent,
    PtyForegroundChangedEvent, PtySpawnedEvent, SinkClosed,
};
use tauri::ipc::{Channel, InvokeResponseBody, IpcResponse};
use tauri::{AppHandle, Emitter, Runtime};

/// Envelope binário enviado pelo `tauri::ipc::Channel` diretamente como `InvokeResponseBody::Raw`
/// e entregue ao JavaScript como `ArrayBuffer`.
#[derive(Clone, Debug, specta::Type)]
#[specta(transparent)]
pub struct BinaryPtyFrame(#[specta(type = Vec<u8>)] pub Bytes);

impl IpcResponse for BinaryPtyFrame {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        Ok(InvokeResponseBody::Raw(self.0.to_vec()))
    }
}

/// Implementação de [`OutputSink`] que encaminha frames binários brutos para um `tauri::ipc::Channel`.
#[derive(Clone)]
pub struct ChannelSink {
    channel: Channel<BinaryPtyFrame>,
}

impl std::fmt::Debug for ChannelSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelSink")
            .field("channel_id", &self.channel.id())
            .finish()
    }
}

impl ChannelSink {
    /// Cria um novo [`ChannelSink`] acoplado ao `Channel` informado.
    #[must_use]
    pub const fn new(channel: Channel<BinaryPtyFrame>) -> Self {
        Self { channel }
    }
}

impl OutputSink for ChannelSink {
    fn send(&self, frame: Bytes) -> Result<(), SinkClosed> {
        self.channel
            .send(BinaryPtyFrame(frame))
            .map_err(|_| SinkClosed)
    }
}

/// Listener que converte [`PtyEvent`] nos quatro eventos globais Tauri (`pty://spawned`,
/// `pty://foreground-changed`, `pty://exit`, `pty://cleanup-failed`).
pub struct TauriEventListener<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
}

impl<R: Runtime> std::fmt::Debug for TauriEventListener<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TauriEventListener").finish_non_exhaustive()
    }
}

impl<R: Runtime> TauriEventListener<R> {
    /// Cria um novo listener vinculado ao [`AppHandle`] informado.
    #[must_use]
    pub const fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> PtyEventListener for TauriEventListener<R> {
    fn on_event(&self, event: &PtyEvent) {
        match event.clone() {
            PtyEvent::Spawned { summary, revision } => {
                let _ = self
                    .app
                    .emit("pty://spawned", PtySpawnedEvent { summary, revision });
            }
            PtyEvent::ForegroundChanged {
                pty_id,
                pgid,
                process,
                revision,
            } => {
                let _ = self.app.emit(
                    "pty://foreground-changed",
                    PtyForegroundChangedEvent {
                        pty_id,
                        pgid,
                        process,
                        revision,
                    },
                );
            }
            PtyEvent::Exited {
                pty_id,
                owner,
                exit,
                final_seq,
                revision,
            } => {
                let _ = self.app.emit(
                    "pty://exit",
                    PtyExitEvent {
                        pty_id,
                        owner,
                        exit,
                        final_seq,
                        revision,
                    },
                );
            }
            PtyEvent::CleanupFailed {
                pty_id,
                survivors,
                revision,
            } => {
                let _ = self.app.emit(
                    "pty://cleanup-failed",
                    PtyCleanupFailedEvent {
                        pty_id,
                        survivors,
                        revision,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn binary_pty_frame_ipc_response_body_is_raw_bytes() {
        let payload = Bytes::from_static(&[0x01, 0x00, 0xff, 0x42]);
        let frame = BinaryPtyFrame(payload.clone());
        let body = frame.body().expect("IpcResponse::body deve ter sucesso");
        match body {
            InvokeResponseBody::Raw(vec) => assert_eq!(vec, payload.as_ref()),
            InvokeResponseBody::Json(_) => {
                panic!("BinaryPtyFrame nunca deve serializar como InvokeResponseBody::Json")
            }
        }
    }
}
