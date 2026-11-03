//! Comandos IPC Tauri do subsistema `mt-agents` (Fase 13 — Agentes CLI, Isolamento, Telemetria e Atenção).

use mt_agents::attention::{AckReason, AttentionSnapshot};
use mt_agents::detect::{AgentInfo, current_unix_ms};
use mt_agents::isolation::{
    AgentCloseInspection, AgentTabLink, AgentTabOpenRequest, WorktreeProposal, WorktreeResolution,
    get_template_options_in_db, set_template_options_in_db,
};
use mt_agents::launch::AgentTemplateOptions;
use mt_agents::registry::AgentKind;
use mt_agents::settings::{AgentsSettings, default_reserved_shortcuts, validate_settings};
use mt_agents::telemetry::AgentTelemetry;
use mt_core::error::MtError;
use mt_core::ids::{ProjectId, TabId, TemplateId};
use tauri::State;
use tauri_specta::Event;

use crate::agents::{AgentTabOpened, AppAgentState};
use crate::events::agents::{
    AgentInstallationsChangedTauriEvent, AgentLinkChangedTauriEvent, AgentSettingsChangedTauriEvent,
};

/// Lista os 4 agentes CLI suportados com seus metadados, instalação e capabilities (`agent_list`).
#[tauri::command]
#[specta::specta]
pub async fn agent_list(state: State<'_, AppAgentState>) -> Result<Vec<AgentInfo>, MtError> {
    let settings = state.service.settings.read().clone();
    Ok(state
        .service
        .detector
        .list(&state.service.registry, &settings)
        .await)
}

/// Reexecuta a detecção de instalação/versão ignorando o cache e emite `agent://installations-changed` se mudar (`agent_detect_refresh`).
#[tauri::command]
#[specta::specta]
pub async fn agent_detect_refresh(
    state: State<'_, AppAgentState>,
) -> Result<Vec<AgentInfo>, MtError> {
    let settings = state.service.settings.read().clone();
    let previous = state
        .service
        .detector
        .list(&state.service.registry, &settings)
        .await;
    let refreshed = state
        .service
        .detector
        .refresh(&state.service.registry, &settings)
        .await;
    if refreshed != previous {
        let _ = AgentInstallationsChangedTauriEvent(refreshed.clone()).emit(&state.app);
    }
    Ok(refreshed)
}

/// Retorna as configurações globais de agentes CLI (`agent_settings_get`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn agent_settings_get(
    state: State<'_, AppAgentState>,
) -> Result<AgentsSettings, MtError> {
    Ok(state.service.settings.read().clone())
}

/// Valida, persiste e publica as configurações de agentes CLI (`agent_settings_update`).
#[tauri::command]
#[specta::specta]
pub async fn agent_settings_update(
    state: State<'_, AppAgentState>,
    settings: AgentsSettings,
) -> Result<AgentsSettings, MtError> {
    let reserved = default_reserved_shortcuts();
    let validated = validate_settings(&settings, &state.service.registry, &reserved)?;
    validated.save_to_db(&state.db).await?;
    *state.service.settings.write() = validated.clone();
    let _ = AgentSettingsChangedTauriEvent(validated.clone()).emit(&state.app);
    Ok(validated)
}

/// Recarrega os descritores de agentes do diretório de configuração do usuário (`agent_descriptors_reload`).
#[tauri::command]
#[specta::specta]
pub async fn agent_descriptors_reload(
    state: State<'_, AppAgentState>,
) -> Result<Vec<AgentInfo>, MtError> {
    state.service.registry.reload(&state.paths)?;
    let settings = state.service.settings.read().clone();
    let refreshed = state
        .service
        .detector
        .refresh(&state.service.registry, &settings)
        .await;
    let _ = AgentInstallationsChangedTauriEvent(refreshed.clone()).emit(&state.app);
    Ok(refreshed)
}

/// Semeia de forma idempotente os 4 templates dedicados de agentes para o projeto (`agent_templates_ensure`).
#[tauri::command]
#[specta::specta]
pub async fn agent_templates_ensure(
    state: State<'_, AppAgentState>,
    project_id: ProjectId,
) -> Result<u32, MtError> {
    state.service.isolation.ensure_templates(project_id).await
}

/// Obtém as opções específicas de agente configuradas para um template (`agent_template_options_get`).
#[tauri::command]
#[specta::specta]
pub async fn agent_template_options_get(
    state: State<'_, AppAgentState>,
    template_id: TemplateId,
) -> Result<AgentTemplateOptions, MtError> {
    let tpl = state.workspace.template_get(template_id).await?;
    if tpl.config.environment.kind != mt_core::workspace::EnvironmentKind::Agent {
        return Err(mt_agents::error::tab_not_agent(template_id.to_string()));
    }
    get_template_options_in_db(&state.db, template_id).await
}

/// Atualiza as opções específicas de agente de um template (`agent_template_options_set`).
#[tauri::command]
#[specta::specta]
pub async fn agent_template_options_set(
    state: State<'_, AppAgentState>,
    template_id: TemplateId,
    options: AgentTemplateOptions,
) -> Result<AgentTemplateOptions, MtError> {
    let tpl = state.workspace.template_get(template_id).await?;
    if tpl.config.environment.kind != mt_core::workspace::EnvironmentKind::Agent {
        return Err(mt_agents::error::tab_not_agent(template_id.to_string()));
    }
    set_template_options_in_db(
        &state.db,
        &state.service.registry,
        template_id,
        &options,
        current_unix_ms(),
    )
    .await
}

/// Calcula uma proposta somente-leitura de criação de worktree isolada para o agente (`agent_worktree_propose`).
#[tauri::command]
#[specta::specta]
pub async fn agent_worktree_propose(
    state: State<'_, AppAgentState>,
    project_id: ProjectId,
    agent: AgentKind,
    hint: Option<String>,
) -> Result<WorktreeProposal, MtError> {
    let proj = state.service.isolation.project_record(project_id).await?;
    state
        .service
        .isolation
        .propose(&proj, agent, hint.as_deref())
        .await
}

/// Abre uma nova aba de Agente CLI no projeto com suporte a isolamento opcional em Git Worktree (`agent_tab_open`).
#[tauri::command]
#[specta::specta]
pub async fn agent_tab_open(
    state: State<'_, AppAgentState>,
    request: AgentTabOpenRequest,
) -> Result<AgentTabOpened, MtError> {
    state.open_agent_tab(request).await
}

/// Obtém o vínculo `agent_tab_links` associado a uma aba (`agent_tab_link_get`).
#[tauri::command]
#[specta::specta]
pub async fn agent_tab_link_get(
    state: State<'_, AppAgentState>,
    tab_id: TabId,
) -> Result<Option<AgentTabLink>, MtError> {
    state.service.isolation.get_link_by_tab(tab_id).await
}

/// Executa inspeção Git e de processos para a worktree vinculada à aba (`agent_tab_close_check`).
#[tauri::command]
#[specta::specta]
pub async fn agent_tab_close_check(
    state: State<'_, AppAgentState>,
    tab_id: TabId,
) -> Result<Option<AgentCloseInspection>, MtError> {
    state.service.isolation.inspect_by_tab(tab_id).await
}

/// Resolve uma worktree pendente pós-fechamento da aba (`Keep`, `Remove`, `RemoveAndDeleteBranch`) validando `inspection_token` (`agent_worktree_resolve`).
#[tauri::command]
#[specta::specta]
pub async fn agent_worktree_resolve(
    state: State<'_, AppAgentState>,
    link_id: i64,
    action: WorktreeResolution,
    inspection_token: String,
) -> Result<AgentTabLink, MtError> {
    let resolved = state
        .service
        .isolation
        .resolve(link_id, action, &inspection_token)
        .await?;
    let _ = AgentLinkChangedTauriEvent(resolved.clone()).emit(&state.app);
    Ok(resolved)
}

/// Lista todas as worktrees de agentes fechadas e ainda pendentes de resolução (`agent_worktree_pending_list`).
#[tauri::command]
#[specta::specta]
pub async fn agent_worktree_pending_list(
    state: State<'_, AppAgentState>,
    project_id: Option<ProjectId>,
) -> Result<Vec<AgentCloseInspection>, MtError> {
    state.service.isolation.list_pending(project_id).await
}

/// Retorna o snapshot atual de telemetria (`StatusLine`) de uma aba (`agent_telemetry_get`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn agent_telemetry_get(
    state: State<'_, AppAgentState>,
    tab_id: TabId,
) -> Result<Option<AgentTelemetry>, MtError> {
    Ok(state.service.telemetry.snapshot(tab_id, current_unix_ms()))
}

/// Lista todos os snapshots ativos de telemetria (`agent_telemetry_list`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn agent_telemetry_list(
    state: State<'_, AppAgentState>,
) -> Result<Vec<AgentTelemetry>, MtError> {
    Ok(state.service.telemetry.list_snapshots(current_unix_ms()))
}

/// Retorna o snapshot atual de atenção correlacionada de uma aba (`agent_attention_get`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn agent_attention_get(
    state: State<'_, AppAgentState>,
    tab_id: TabId,
) -> Result<Option<AttentionSnapshot>, MtError> {
    Ok(state.service.attention.snapshot(tab_id))
}

/// Reconhece visualmente um alerta de atenção sem resolver a espera real no agente (`agent_attention_ack`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn agent_attention_ack(
    state: State<'_, AppAgentState>,
    tab_id: TabId,
    run_id: String,
    episode: u64,
    reason: AckReason,
) -> Result<Option<AttentionSnapshot>, MtError> {
    Ok(state
        .service
        .attention
        .ack(tab_id, &run_id, episode, reason))
}
