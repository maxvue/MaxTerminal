//! Comandos IPC Tauri para o detector de portas ativas (Fase 08).

use mt_core::MtResult;
use mt_core::ids::{PaneId, TabId};
use mt_ports::{PollMode, PortKillResult, PortSnapshot, PortsService};
use tauri::State;

/// Lista as portas TCP em estado `LISTEN` confirmadas para os painéis da aba `tab_id`.
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn ports_list_for_tab(ports: State<'_, PortsService>, tab_id: TabId) -> MtResult<PortSnapshot> {
    Ok(ports.list_for_tab(tab_id))
}

/// Abre a porta local (`http://127.0.0.1:<port>/`) no navegador padrão após validar pertencimento ao painel.
#[tauri::command]
#[specta::specta]
pub async fn ports_open_browser(
    ports: State<'_, PortsService>,
    pane_id: PaneId,
    port: u16,
) -> MtResult<()> {
    let _url = ports.open_browser(pane_id, port).await?;
    Ok(())
}

/// Encerra cirurgicamente o processo proprietário da porta `(pane_id, port)` com verificação de `(pid, start_time_ticks)`.
#[tauri::command]
#[specta::specta]
pub async fn ports_kill(
    ports: State<'_, PortsService>,
    pane_id: PaneId,
    port: u16,
    expected_pid: Option<u32>,
) -> MtResult<PortKillResult> {
    Ok(ports.kill_port(pane_id, port, expected_pid).await?)
}

/// Altera a cadência adaptativa de polling de `/proc/net/tcp{,6}` (`Active`, `Background`, `Paused`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn ports_set_poll_mode(ports: State<'_, PortsService>, mode: PollMode) -> MtResult<()> {
    ports.set_poll_mode(mode);
    Ok(())
}
