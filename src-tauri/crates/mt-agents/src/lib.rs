//! Crate `mt-agents` — Agentes CLI (`Claude Code`, `Codex`, `Antigravity (agy)`, `OpenCode`),
//! Isolamento em Git Worktree, Relay IPC, Telemetria `StatusLine`, Detector de Atenção,
//! Sincronizador `MaxSkills` e Memória Contextual de Comandos.

#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![allow(
    missing_docs,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::expect_used,
    clippy::large_enum_variant,
    clippy::match_like_matches_macro,
    clippy::needless_borrow,
    clippy::unnecessary_lazy_evaluations,
    clippy::unnecessary_literal_unwrap
)]

pub mod attention;
pub mod detect;
pub mod error;
pub mod isolation;
pub mod launch;
pub mod memory;
pub mod ports;
pub mod registry;
pub mod relay;
pub mod settings;
pub mod skills;
pub mod telemetry;

use std::collections::HashMap;
use std::sync::Arc;

use mt_core::{PaneId, PtyId, TabId};
use parking_lot::RwLock;

use crate::attention::AttentionDetector;
use crate::detect::Detector;
use crate::isolation::IsolationService;
use crate::launch::LaunchPlanner;
use crate::memory::CommandMemory;
use crate::ports::RunBinding;
use crate::registry::AgentRegistry;
use crate::relay::RelayServer;
use crate::settings::AgentsSettings;
use crate::skills::SkillsService;
use crate::telemetry::aggregator::TelemetryAggregator;

/// Índice em memória das execuções de agentes ativas (`TabId` / `PaneId` / `PtyId` -> `RunBinding`).
#[derive(Default)]
pub struct AgentRuntimeIndex {
    by_tab: RwLock<HashMap<TabId, RunBinding>>,
    by_pane: RwLock<HashMap<PaneId, RunBinding>>,
    by_pty: RwLock<HashMap<PtyId, RunBinding>>,
}

impl AgentRuntimeIndex {
    /// Cria um novo índice vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra uma execução ativa.
    pub fn insert(&self, binding: RunBinding) {
        self.by_tab.write().insert(binding.tab_id, binding.clone());
        self.by_pane
            .write()
            .insert(binding.pane_id, binding.clone());
        self.by_pty.write().insert(binding.pty_id, binding);
    }

    /// Obtém o `RunBinding` ativo de uma aba.
    #[must_use]
    pub fn by_tab(&self, tab_id: TabId) -> Option<RunBinding> {
        self.by_tab.read().get(&tab_id).cloned()
    }

    /// Obtém o `RunBinding` ativo de um painel.
    #[must_use]
    pub fn by_pane(&self, pane_id: PaneId) -> Option<RunBinding> {
        self.by_pane.read().get(&pane_id).cloned()
    }

    /// Obtém o `RunBinding` ativo de um PTY.
    #[must_use]
    pub fn by_pty(&self, pty_id: PtyId) -> Option<RunBinding> {
        self.by_pty.read().get(&pty_id).cloned()
    }

    /// Remove uma execução pelo `TabId`.
    pub fn remove_by_tab(&self, tab_id: TabId) -> Option<RunBinding> {
        let removed = self.by_tab.write().remove(&tab_id);
        if let Some(ref b) = removed {
            self.by_pane.write().remove(&b.pane_id);
            self.by_pty.write().remove(&b.pty_id);
        }
        removed
    }

    /// Remove uma execução pelo `PaneId`.
    pub fn remove_by_pane(&self, pane_id: PaneId) -> Option<RunBinding> {
        let removed = self.by_pane.write().remove(&pane_id);
        if let Some(ref b) = removed {
            self.by_tab.write().remove(&b.tab_id);
            self.by_pty.write().remove(&b.pty_id);
        }
        removed
    }
}

/// Fachada central do crate `mt-agents` (`AgentService`).
pub struct AgentService {
    /// Registro de descritores dos 4 agentes.
    pub registry: Arc<AgentRegistry>,
    /// Detector de instalação e versão.
    pub detector: Arc<Detector>,
    /// Planejador de lançamento no shell.
    pub planner: Arc<LaunchPlanner>,
    /// Serviço de isolamento em Git worktrees.
    pub isolation: Arc<IsolationService>,
    /// Servidor Unix socket para sidecar `mt-agent-relay`.
    pub relay: Option<Arc<RelayServer>>,
    /// Agregador multi-fonte de telemetria.
    pub telemetry: Arc<TelemetryAggregator>,
    /// Detector de atenção correlacionada.
    pub attention: Arc<AttentionDetector>,
    /// Serviço de sincronização de `MaxSkills`.
    pub skills: Arc<SkillsService>,
    /// Motor de memória contextual de comandos.
    pub memory: Arc<CommandMemory>,
    /// Configurações de agentes em memória.
    pub settings: Arc<RwLock<AgentsSettings>>,
    /// Índice de execuções ativas.
    pub runtime: Arc<AgentRuntimeIndex>,
}
