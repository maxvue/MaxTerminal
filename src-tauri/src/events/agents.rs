//! Eventos Tauri emitidos pelos subsistemas `mt-agents`, `skills` e `memory` (Fase 13).

use mt_agents::attention::AttentionSnapshot;
use mt_agents::detect::AgentInfo;
use mt_agents::isolation::{AgentCloseInspection, AgentTabLink};
use mt_agents::settings::AgentsSettings;
use mt_agents::skills::SyncResult;
use mt_agents::skills::watch::SkillsChangedPayload;
use mt_agents::telemetry::AgentTelemetry;
use mt_core::ProjectId;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

/// Payload do evento `memory://updated`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUpdatedPayload {
    /// Identificador do projeto cuja memória contextual foi atualizada.
    pub project_id: ProjectId,
}

/// Evento `agent://installations-changed` emitido quando a detecção dos Agentes CLI muda.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "agent://installations-changed")]
pub struct AgentInstallationsChangedTauriEvent(pub Vec<AgentInfo>);

/// Evento `agent://settings-changed` emitido quando as configurações de agentes são atualizadas.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "agent://settings-changed")]
pub struct AgentSettingsChangedTauriEvent(pub AgentsSettings);

/// Evento `agent://telemetry` emitido quando um snapshot de telemetria `StatusLine` muda (<= 1 Hz por aba).
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "agent://telemetry")]
pub struct AgentTelemetryTauriEvent(pub AgentTelemetry);

/// Evento `agent://attention` emitido em toda transição do estado de atenção de uma aba de agente.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "agent://attention")]
pub struct AgentAttentionTauriEvent(pub AttentionSnapshot);

/// Evento `agent://link-changed` emitido na criação, fechamento ou resolução de um vínculo aba ↔ worktree.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "agent://link-changed")]
pub struct AgentLinkChangedTauriEvent(pub AgentTabLink);

/// Evento `agent://worktree-close-pending` emitido após o fechamento de aba com worktree não resolvida.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "agent://worktree-close-pending")]
pub struct AgentWorktreeClosePendingTauriEvent(pub AgentCloseInspection);

/// Evento `skills://changed` emitido pelo watcher da fonte `MaxSkills` (debounce 500 ms).
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "skills://changed")]
pub struct SkillsChangedTauriEvent(pub SkillsChangedPayload);

/// Evento `skills://sync-completed` emitido após `skills_apply` ou sincronização automática.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "skills://sync-completed")]
pub struct SkillsSyncCompletedTauriEvent(pub SyncResult);

/// Evento `memory://updated` emitido quando a memória contextual de comandos de um projeto muda.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "memory://updated")]
pub struct MemoryUpdatedTauriEvent(pub MemoryUpdatedPayload);
