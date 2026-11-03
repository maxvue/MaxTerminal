//! Comandos IPC Tauri para gerenciamento de pseudo-terminais (`pty_*`).

use std::sync::Arc;

use mt_core::{ErrorCode, MtError, ids::PtyId, shell::ShellIntegrationRequest};
use mt_pty::{
    AttachInfo, AttachmentId, KillMode, OutputCursor, PtyError, PtyInput, PtyManager,
    PtyProcessReport, PtySize, PtySummary, SpawnSpec,
};
use mt_shell::ShellService;
use serde_json::json;
use tauri::State;
use tauri::ipc::Channel;

use crate::events::pty::{BinaryPtyFrame, ChannelSink};

/// Cria e inicia um novo pseudo-terminal a partir de um [`SpawnSpec`] e configuração opcional de [`ShellIntegrationRequest`].
#[tauri::command]
#[specta::specta]
pub async fn pty_spawn(
    state: State<'_, Arc<ShellService>>,
    spec: SpawnSpec,
    integration: Option<ShellIntegrationRequest>,
) -> Result<PtySummary, MtError> {
    let shell = Arc::clone(&*state);
    tauri::async_runtime::spawn_blocking(move || {
        shell.spawn_pty(spec, integration).map_err(MtError::from)
    })
    .await
    .map_err(|e| {
        MtError::new(
            ErrorCode("pty.spawn_failed"),
            format!("Falha na tarefa de spawn: {e}"),
        )
        .with_details(json!({ "stage": "spawn_blocking_join" }))
    })?
}

/// Escreve texto UTF-8 ou bytes arbitrários na entrada do PTY.
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn pty_write(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    input: PtyInput,
) -> Result<(), MtError> {
    state
        .write_input(pty_id, input)
        .map(|_| ())
        .map_err(MtError::from)
}

/// Redimensiona a janela do PTY indicado (`rows` e `cols` em `1..=1000`).
#[tauri::command]
#[specta::specta]
pub async fn pty_resize(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    size: PtySize,
) -> Result<(), MtError> {
    let manager = Arc::clone(&*state);
    tauri::async_runtime::spawn_blocking(move || {
        manager.resize(pty_id, size).map_err(MtError::from)
    })
    .await
    .map_err(|e| {
        MtError::new(
            ErrorCode("pty.io"),
            format!("Falha na tarefa de resize: {e}"),
        )
        .with_details(json!({ "stage": "spawn_blocking_join" }))
    })?
}

/// Solicita o encerramento do PTY com modo opcional (`Graceful` por padrão ou `Force`).
#[tauri::command]
#[specta::specta]
pub async fn pty_kill(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    mode: Option<KillMode>,
) -> Result<(), MtError> {
    let manager = Arc::clone(&*state);
    let kill_mode = mode.unwrap_or(KillMode::Graceful);
    tauri::async_runtime::spawn_blocking(move || {
        manager
            .kill(pty_id, kill_mode)
            .map(|_| ())
            .map_err(MtError::from)
    })
    .await
    .map_err(|e| {
        MtError::new(
            ErrorCode("pty.cleanup_failed"),
            format!("Falha na tarefa de kill: {e}"),
        )
        .with_details(json!({ "stage": "spawn_blocking_join" }))
    })?
}

/// Lista todos os PTYs gerenciados (ativos e tombstones dentro do TTL).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn pty_list(state: State<'_, Arc<PtyManager>>) -> Result<Vec<PtySummary>, MtError> {
    Ok(state.list())
}

/// Inspeciona a árvore de processos e o processo em foreground do PTY via `/proc`.
#[tauri::command]
#[specta::specta]
pub async fn pty_process_info(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
) -> Result<PtyProcessReport, MtError> {
    let manager = Arc::clone(&*state);
    tauri::async_runtime::spawn_blocking(move || {
        manager.process_report(pty_id).map_err(MtError::from)
    })
    .await
    .map_err(|e| {
        MtError::new(
            ErrorCode("pty.proc_unavailable"),
            format!("Falha na tarefa de inspeção: {e}"),
        )
        .with_details(json!({ "stage": "spawn_blocking_join" }))
    })?
}

/// Registra confirmação cumulativa de bytes consumidos (`consumed_bytes`) na geração `attachment_id`.
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn pty_ack(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    attachment_id: AttachmentId,
    consumed_bytes: String,
) -> Result<(), MtError> {
    let parsed = consumed_bytes.parse::<u64>().map_err(|_| {
        MtError::from(PtyError::AckInvalid(format!(
            "Valor consumedBytes inválido: {consumed_bytes}"
        )))
    })?;
    state
        .ack(pty_id, attachment_id, parsed)
        .map_err(MtError::from)
}

/// Anexa um canal binário (`tauri::ipc::Channel<BinaryPtyFrame>`) como consumidor de saída do PTY.
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn pty_attach(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    cursor: Option<OutputCursor>,
    on_output: Channel<BinaryPtyFrame>,
) -> Result<AttachInfo, MtError> {
    let sink = Arc::new(ChannelSink::new(on_output));
    state.attach(pty_id, sink, cursor).map_err(MtError::from)
}

/// Desanexa explicitamente uma geração de consumidor de saída do PTY.
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn pty_detach(
    state: State<'_, Arc<PtyManager>>,
    pty_id: PtyId,
    attachment_id: AttachmentId,
) -> Result<(), MtError> {
    state.detach(pty_id, attachment_id).map_err(MtError::from)
}
