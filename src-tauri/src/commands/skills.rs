//! Comandos IPC Tauri do subsistema `MaxSkills` (Fase 13 — Catálogo, Alvos, Plano Dry-Run e Sincronização).

use std::collections::BTreeMap;

use mt_agents::skills::{ConflictResolution, SkillCatalog, SkillSyncTarget, SyncPlan, SyncResult};
use mt_core::error::MtError;
use mt_core::ids::ProjectId;
use tauri::State;
use tauri_specta::Event;

use crate::agents::AppAgentState;
use crate::events::agents::SkillsSyncCompletedTauriEvent;

/// Escaneia o repositório local `MaxSkills` e calcula relevância para o projeto opcional (`skills_scan`).
#[tauri::command]
#[specta::specta]
pub async fn skills_scan(
    state: State<'_, AppAgentState>,
    project_id: Option<ProjectId>,
) -> Result<SkillCatalog, MtError> {
    state.service.skills.scan(project_id).await
}

/// Lista os alvos de sincronização de skills cadastrados (`skills_targets_list`).
#[tauri::command]
#[specta::specta]
pub async fn skills_targets_list(
    state: State<'_, AppAgentState>,
    project_id: Option<ProjectId>,
) -> Result<Vec<SkillSyncTarget>, MtError> {
    state.service.skills.list_targets(project_id).await
}

/// Cria ou atualiza um alvo de sincronização de skills validando o diretório de destino (`skills_target_upsert`).
#[tauri::command]
#[specta::specta]
pub async fn skills_target_upsert(
    state: State<'_, AppAgentState>,
    target: SkillSyncTarget,
) -> Result<SkillSyncTarget, MtError> {
    state.service.skills.upsert_target(target).await
}

/// Remove um alvo de sincronização e opcionalmente limpa apenas as entradas gerenciadas intactas (`skills_target_remove`).
#[tauri::command]
#[specta::specta]
pub async fn skills_target_remove(
    state: State<'_, AppAgentState>,
    target_id: i64,
    remove_managed: bool,
) -> Result<u32, MtError> {
    state
        .service
        .skills
        .remove_target(target_id, remove_managed)
        .await
}

/// Calcula um plano *dry-run* de sincronização sem modificar o disco (`skills_plan`).
#[tauri::command]
#[specta::specta]
pub async fn skills_plan(
    state: State<'_, AppAgentState>,
    target_id: i64,
) -> Result<SyncPlan, MtError> {
    state.service.skills.plan(target_id).await
}

/// Aplica um plano de sincronização revalidando o fingerprint do disco e emite `skills://sync-completed` (`skills_apply`).
#[tauri::command]
#[specta::specta]
pub async fn skills_apply(
    state: State<'_, AppAgentState>,
    plan_id: String,
    resolutions: BTreeMap<String, ConflictResolution>,
) -> Result<SyncResult, MtError> {
    let result = state.service.skills.apply(&plan_id, resolutions).await?;
    let _ = SkillsSyncCompletedTauriEvent(result.clone()).emit(&state.app);
    Ok(result)
}
