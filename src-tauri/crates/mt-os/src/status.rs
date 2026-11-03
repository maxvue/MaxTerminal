//! Agregação de estados de abas (`StatusCounters`, `TabStatusProvider` e `ActivitySnapshot`).

use mt_core::ids::{ProjectId, TabId};
use mt_core::shell::TabStatus;
use serde::{Deserialize, Serialize};

use crate::attention::AttentionRegistry;

/// Entrada individual de estado de uma aba aberta fornecida pelo adaptador do workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabStatusEntry {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Bucket de status da aba (Fase 05: `badge_bucket`).
    pub status: TabStatus,
    /// Se o estado terminal da aba já foi visualizado pelo usuário.
    pub seen: bool,
    /// Título sanitizado da aba.
    pub title: String,
}

/// Fotografia de atividade real de processos para a decisão de fechamento da janela.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActivitySnapshot {
    /// Quantidade de painéis vivos em estado `Running` ou `Continuous`.
    pub active_panes: u32,
    /// Quantidade de operações de spawn de painel/aba em andamento.
    pub pending_spawns: u32,
    /// `true` se o gerenciador de PTYs possui filhos ocupados fora do prompt ocioso.
    pub supervised_busy: bool,
    /// `true` se o workspace ainda não concluiu a restauração inicial.
    pub incomplete: bool,
}

impl ActivitySnapshot {
    /// Retorna `true` se houver qualquer atividade viva ou pendente que impeça saída silenciosa.
    #[must_use]
    pub const fn has_active_work(&self) -> bool {
        self.active_panes > 0 || self.pending_spawns > 0 || self.supervised_busy || self.incomplete
    }
}

/// Provedor de estados de abas e atividade de processos implementado no crate binário.
pub trait TabStatusProvider: Send + Sync + 'static {
    /// Retorna a lista atual de estados das abas abertas.
    fn snapshot(&self) -> Vec<TabStatusEntry>;

    /// Retorna a fotografia instantânea de atividade para decisão de fechamento.
    fn activity(&self) -> ActivitySnapshot;

    /// Resolve o `ProjectId` ao qual pertence uma aba aberta.
    fn project_of(&self, tab_id: &TabId) -> Option<ProjectId>;
}

/// Contadores globais agregados para exibição na bandeja do sistema e na barra de título.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatusCounters {
    /// Abas com comando temporário em execução.
    pub running: u32,
    /// Abas em execução contínua (servidores/daemons).
    pub continuous: u32,
    /// Abas cujo comando terminou com sucesso (`exit 0`) e ainda não foram vistas.
    pub finished_unseen: u32,
    /// Abas cujo último comando falhou (`exit != 0`) e ainda não foram vistas.
    pub error_unseen: u32,
    /// Abas ociosas no prompt.
    pub idle: u32,
    /// Abas com agente aguardando input ou aprovação de ferramenta.
    pub awaiting_input: u32,
}

impl StatusCounters {
    /// Agrega os contadores a partir do snapshot de abas e do registro transversal de atenção.
    #[must_use]
    pub fn aggregate(entries: &[TabStatusEntry], attention: &AttentionRegistry) -> Self {
        let mut counters = Self::default();
        for entry in entries {
            match entry.status {
                TabStatus::Running => counters.running = counters.running.saturating_add(1),
                TabStatus::Continuous => {
                    counters.continuous = counters.continuous.saturating_add(1);
                }
                TabStatus::CompletedUnseen => {
                    if entry.seen {
                        counters.idle = counters.idle.saturating_add(1);
                    } else {
                        counters.finished_unseen = counters.finished_unseen.saturating_add(1);
                    }
                }
                TabStatus::ErrorUnseen => {
                    if entry.seen {
                        counters.idle = counters.idle.saturating_add(1);
                    } else {
                        counters.error_unseen = counters.error_unseen.saturating_add(1);
                    }
                }
                TabStatus::Idle => counters.idle = counters.idle.saturating_add(1),
            }
        }
        counters.awaiting_input = attention.count();
        counters
    }

    /// Retorna `true` se houver abas em execução ou execução contínua (uso visual no tray).
    #[must_use]
    pub const fn has_active(&self) -> bool {
        self.running.saturating_add(self.continuous) > 0
    }

    /// Retorna `true` se houver abas com erro não visto ou aguardando intervenção.
    #[must_use]
    pub const fn needs_attention(&self) -> bool {
        self.error_unseen.saturating_add(self.awaiting_input) > 0
    }
}
