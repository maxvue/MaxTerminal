//! Contexto de execução por PTY (`PtyShellRuntime`) e cache em memória limitado de blocos recentes
//! (`RecentBlocksCache`) para acesso imediato mesmo quando o SQLite estiver ocupado ou degradado.

use std::collections::{BTreeMap, VecDeque};

use mt_core::ids::{CommandBlockId, PtyId, SessionId};
use mt_core::shell::{
    CommandBlockDto, ContinuousPolicy, IntegrationInfo, IntegrationMode, RuntimeBinding,
};
use mt_db::sanitizer::SanitizedText;

use crate::fallback::FallbackTracker;
use crate::osc::OscScanner;
use crate::tracker::CommandTracker;

/// Capacidade máxima de blocos finalizados retidos em memória por sessão ativa (64 blocos).
pub const MAX_CACHED_BLOCKS_PER_SESSION: usize = 64;

/// Contexto de runtime por PTY reunindo scanner OSC, tracker de comandos e fallback `/proc`.
#[derive(Debug)]
pub struct PtyShellRuntime {
    /// Scanner incremental de sequências `OSC 133`, `OSC 633` e `OSC 7`.
    pub scanner: OscScanner,
    /// Rastreador de ciclos `A -> B -> C -> D` e captura de saída.
    pub tracker: CommandTracker,
    /// Rastreador de fallback via `/proc` quando hooks OSC 133 não estão ativos.
    pub fallback: Option<FallbackTracker>,
    /// Versão dos scripts de integração embarcados.
    pub scripts_version: String,
    /// Último `foreground_pgid` observado para este PTY.
    pub last_fg_pgid: Option<u32>,
}

impl PtyShellRuntime {
    /// Constrói o DTO de diagnóstico [`IntegrationInfo`] para este PTY.
    #[must_use]
    pub fn integration_info(&self) -> IntegrationInfo {
        let binding = self.tracker.binding();
        IntegrationInfo {
            pty_id: binding.pty_id,
            session_id: binding.session_id,
            tab_id: binding.tab_id,
            pane_id: binding.pane_id,
            shell_kind: self.tracker.shell_kind().clone(),
            mode: self.tracker.mode(),
            continuous_policy: self.tracker.continuous_policy(),
            cwd: self.tracker.current_cwd().map(|c| c.as_str().to_string()),
            osc_events_received: self.tracker.osc_events_received(),
            fallback_activations: self.tracker.fallback_activations(),
        }
    }
}

/// Estado preparado antes do nascimento do PID durante `SpawnLifecycle::prepare`.
#[derive(Debug, Clone)]
pub struct PendingSpawnContext {
    /// Vínculo de runtime alocado (`pty_id`, `session_id`, `tab_id`, `pane_id`).
    pub binding: RuntimeBinding,
    /// Família de shell detectada.
    pub shell_kind: mt_core::shell::ShellKind,
    /// Nonce esperado para validação de marcadores OSC 133.
    pub nonce: Option<String>,
    /// Modo de integração inicial (`Pending`, `FallbackProc` ou `Disabled`).
    pub initial_mode: IntegrationMode,
    /// Política de classificação contínua solicitada.
    pub continuous_policy: ContinuousPolicy,
    /// Diretório de trabalho inicial sanitizado.
    pub initial_cwd: SanitizedText,
    /// Versão dos scripts embarcados.
    pub scripts_version: String,
    /// Indica se deve ativar fallback `/proc` imediatamente (ex.: `sh` ou opt-out de hooks).
    pub immediate_fallback: bool,
}

/// Cache limitado em memória de blocos de comando recentes (até 64 por sessão).
#[derive(Debug, Default)]
pub struct RecentBlocksCache {
    by_id: BTreeMap<CommandBlockId, CommandBlockDto>,
    by_session: BTreeMap<SessionId, VecDeque<CommandBlockId>>,
}

impl RecentBlocksCache {
    /// Cria um novo [`RecentBlocksCache`] vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insere ou atualiza um [`CommandBlockDto`] no cache, respeitando o limite de 64 blocos por sessão.
    pub fn upsert(&mut self, block: CommandBlockDto) {
        let session_id = block.session_id;
        let block_id = block.id;
        self.by_id.insert(block_id, block);

        let queue = self.by_session.entry(session_id).or_default();
        if !queue.contains(&block_id) {
            queue.push_back(block_id);
            while queue.len() > MAX_CACHED_BLOCKS_PER_SESSION {
                if let Some(evicted) = queue.pop_front() {
                    self.by_id.remove(&evicted);
                }
            }
        }
    }

    /// Busca um bloco pelo seu [`CommandBlockId`].
    #[must_use]
    pub fn get(&self, id: CommandBlockId) -> Option<CommandBlockDto> {
        self.by_id.get(&id).cloned()
    }

    /// Lista os blocos em memória pertencentes a uma sessão na ordem reversa (`started_at DESC`).
    #[must_use]
    pub fn list_for_session(&self, session_id: SessionId) -> Vec<CommandBlockDto> {
        let Some(ids) = self.by_session.get(&session_id) else {
            return Vec::new();
        };
        let mut items: Vec<CommandBlockDto> = ids
            .iter()
            .rev()
            .filter_map(|id| self.by_id.get(id).cloned())
            .collect();
        items.sort_by(|a, b| {
            b.started_at
                .cmp(&a.started_at)
                .then_with(|| b.command_seq.cmp(&a.command_seq))
        });
        items
    }

    /// Remove todos os blocos de uma sessão específica quando necessário.
    pub fn remove_session(&mut self, session_id: SessionId) {
        if let Some(ids) = self.by_session.remove(&session_id) {
            for id in ids {
                self.by_id.remove(&id);
            }
        }
    }

    /// Retorna o mapa de PTYs ativos apenas para diagnóstico.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Indica se o cache está vazio.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// Mapa concorrente de contextos de runtime indexados por [`PtyId`].
pub type RuntimeMap = BTreeMap<PtyId, PtyShellRuntime>;
