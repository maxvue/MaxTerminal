//! Tipos e contratos do Detector de Atenção Correlacionada (`StatusLine` e alertas).

pub mod detector;
pub mod patterns;

use mt_core::{PaneId, ProjectId, SessionId, TabId};
use serde::{Deserialize, Serialize};

use crate::ports::AttentionKind;
use crate::registry::AgentKind;

pub use detector::{AttentionDetector, AttentionSink, TailBuffer, parse_osc_and_bel};
pub use patterns::{CompiledPattern, PatternRule, PatternSet, filter_quoted_lines};

/// Fonte que originou a transição de atenção.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AttentionSource {
    /// Hook nativo estruturado do Claude Code (`Notification`).
    NativeHook,
    /// Callback `notify` do Codex CLI.
    CodexNotify,
    /// Sequência OSC 9 / OSC 777 ou BEL emitida no PTY.
    OscNotification,
    /// Regra regex de duas âncoras avaliada após silêncio.
    Pattern,
}

/// Estado da máquina de atenção de uma execução de agente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum AttentionState {
    /// Agente fora do foreground ou inativo.
    Inactive,
    /// Agente em execução normal no foreground.
    Working,
    /// Indício heurístico (OSC/BEL/padrão de 2 âncoras + silêncio) de possível espera.
    #[serde(rename_all = "camelCase")]
    Suspected {
        /// Tipo de espera suspeita.
        kind: AttentionKind,
        /// Timestamp em ms desde o início da suspeita.
        since_ms: i64,
        /// Fonte do indício.
        source: AttentionSource,
        /// Identificador do padrão regex quando `source == Pattern`.
        #[serde(rename = "patternId")]
        pattern_id: Option<String>,
    },
    /// Espera confirmada por fonte nativa correlacionada.
    #[serde(rename_all = "camelCase")]
    Awaiting {
        /// Tipo de espera confirmada (`AwaitingApproval` ou `AwaitingInput`).
        kind: AttentionKind,
        /// Número monotônico do episódio dentro da execução.
        episode: u64,
        /// Timestamp em ms desde o início da espera.
        since_ms: i64,
        /// Fonte da confirmação.
        source: AttentionSource,
        /// Mensagem local sanitizada opcional.
        message: Option<String>,
    },
}

/// Snapshot completo do estado de atenção de uma aba de agente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttentionSnapshot {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Identificador do painel.
    pub pane_id: PaneId,
    /// Identificador do projeto.
    pub project_id: ProjectId,
    /// Identificador da sessão SQLite.
    pub session_id: SessionId,
    /// Identificador único da execução do agente.
    pub run_id: String,
    /// Agente CLI associado.
    pub agent: AgentKind,
    /// Revisão monotônica das transições de atenção dentro de `run_id`.
    pub revision: u64,
    /// Indica se o usuário já visualizou/reconheceu o episódio atual.
    pub acknowledged: bool,
    /// Estado corrente de atenção.
    pub state: AttentionState,
}

/// Motivo do reconhecimento visual de um alerta de atenção.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AckReason {
    /// O usuário digitou no terminal (xterm).
    UserInput,
    /// O usuário clicou em "Dispensar aviso" na UI.
    Dismissed,
}

/// Sinais consumidos pela máquina de estados `AttentionDetector`.
#[derive(Debug, Clone)]
pub enum AttentionSignal {
    /// Hook `Notification` autenticado do Claude Code.
    HookNotification {
        /// Mensagem opcional reportada pelo hook.
        message: Option<String>,
        /// Tipo da notificação (`permission_prompt`, `idle_prompt`, `auth_success`, etc.).
        notification_type: Option<String>,
        /// Identificador opcional de subagente; quando `Some`, não altera a conversa raiz.
        subagent_id: Option<String>,
    },
    /// Hook `Stop` do Claude Code (fim de turno).
    HookStop {
        /// Quando `true`, veio de um subagente e não deve encerrar atenção da conversa raiz.
        is_subagent: bool,
    },
    /// Hook `UserPromptSubmit` do Claude Code.
    HookUserPromptSubmit,
    /// Hook `PreToolUse` do Claude Code (precede pedido de permissão; não resolve aprovação).
    HookPreToolUse,
    /// Evento pós-ferramenta ou atividade de execução confirmada após aprovação.
    ToolActivityConfirmed,
    /// Notificação `agent-turn-complete` do Codex CLI.
    CodexTurnComplete {
        /// Última mensagem opcional do turno.
        last_message: Option<String>,
    },
    /// Sequência OSC 9 ou OSC 777;notify detectada no output do PTY.
    OscNotify {
        /// Texto da notificação OSC.
        text: String,
    },
    /// Caractere BEL (`\x07`) fora de sequência OSC detectado no PTY.
    Bell,
    /// Casamento de padrão regex de duas âncoras no tail sem ANSI.
    PatternMatch {
        /// Identificador da regra casada.
        pattern_id: String,
        /// Tipo de atenção associado à regra.
        kind: AttentionKind,
    },
    /// Saída bruta do PTY para alimentar o `TailBuffer` e atualizar `last_output_ms`.
    OutputBytes {
        /// Bytes recebidos do PTY.
        bytes: Vec<u8>,
    },
    /// Tecla pressionada pelo usuário no xterm (apenas reconhece visualização).
    UserInput,
    /// Resolução explícita correlacionada por `(run_id, episode)`.
    Resolved {
        /// Identificador da execução.
        run_id: String,
        /// Episódio resolvido.
        episode: u64,
    },
    /// Mudança de processo em foreground no PTY.
    Foreground {
        /// `true` quando o binário do agente está em foreground.
        is_agent: bool,
    },
    /// Fim do comando no shell (`CommandFinished`) ou saída do PTY (`PtyExit`).
    ExecutionEnded,
}
