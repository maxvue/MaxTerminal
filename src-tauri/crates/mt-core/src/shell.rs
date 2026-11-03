//! Contratos e DTOs compartilhados de Shell Integration OSC 133/633, blocos de comando e
//! máquina de estados de abas/painéis (Fase 05).

use serde::{Deserialize, Serialize};

use crate::ids::{CommandBlockId, PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId};

/// Família de shell identificada para seleção de injeção de scripts OSC 133.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ShellKind {
    /// GNU Bash (`bash`).
    Bash,
    /// Z Shell (`zsh`).
    Zsh,
    /// Friendly Interactive Shell (`fish`).
    Fish,
    /// Shell POSIX genérico (`sh`, `dash`, `ash`).
    Sh,
    /// Executável customizado ou TUI sem injeção automática de scripts.
    Custom(String),
}

impl ShellKind {
    /// Representação textual canônica persistida na coluna `sessions.shell_kind`.
    #[must_use]
    pub fn as_db_str(&self) -> &str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Sh => "sh",
            Self::Custom(name) => {
                if name.is_empty() {
                    "custom"
                } else {
                    name.as_str()
                }
            }
        }
    }

    /// Reconstrói [`ShellKind`] a partir da string persistida no SQLite.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Self {
        match raw {
            "bash" => Self::Bash,
            "zsh" => Self::Zsh,
            "fish" => Self::Fish,
            "sh" => Self::Sh,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// Política de classificação de execução contínua (`Continuous`) de uma aba ou painel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ContinuousPolicy {
    /// Detecta automaticamente por catálogo de padrões de comando e limiar temporal (15 s).
    #[default]
    Auto,
    /// Qualquer comando em execução classifica imediatamente como `Continuous` (templates de daemon).
    Always,
    /// Nunca promove automaticamente para `Continuous` (permanece em `Running`).
    Never,
}

/// Parâmetros de integração de shell solicitados no `pty_spawn`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShellIntegrationRequest {
    /// Habilita injeção não intrusiva de hooks OSC 133 e rastreamento de blocos (`default: true`).
    pub enabled: bool,
    /// Política inicial de classificação de execução contínua (`default: Auto`).
    pub continuous_policy: ContinuousPolicy,
    /// Projeto associado à sessão (preenchido a partir da Fase 06).
    pub project_id: Option<ProjectId>,
    /// Sessão de aba do workspace (preenchida a partir da Fase 07).
    pub tab_session_id: Option<TabSessionId>,
}

impl Default for ShellIntegrationRequest {
    fn default() -> Self {
        Self {
            enabled: true,
            continuous_policy: ContinuousPolicy::Auto,
            project_id: None,
            tab_session_id: None,
        }
    }
}

/// Modo operacional efetivo da integração de shell em um PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationMode {
    /// Aguardando primeiro marcador OSC 133/633 dentro da janela de ativação.
    #[default]
    Pending,
    /// Marcadores OSC 133/633 ativos e validados.
    Osc133,
    /// Fallback ativo via supervisão de grupo de processos em foreground (`/proc`).
    FallbackProc,
    /// Integração explicitamente desabilitada no `SpawnSpec`.
    Disabled,
}

impl IntegrationMode {
    /// Representação textual persistida em `sessions.integration_mode`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Osc133 => "osc133",
            Self::FallbackProc => "fallback_proc",
            Self::Disabled => "disabled",
        }
    }

    /// Reconstrói [`IntegrationMode`] a partir do valor salvo no banco.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "pending" => Some(Self::Pending),
            "osc133" => Some(Self::Osc133),
            "fallback_proc" => Some(Self::FallbackProc),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// Fonte de observação que originou o bloco de comando.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    /// Marcadores semânticos `FinalTerm` / `WezTerm` / Kitty (`OSC 133`).
    Osc133,
    /// Marcadores semânticos VS Code (`OSC 633`).
    Osc633,
    /// Detecção por transição de `foreground_pgid` em `/proc`.
    ProcFallback,
}

impl ObservationSource {
    /// Representação textual persistida em `command_blocks.source`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Osc133 => "osc133",
            Self::Osc633 => "osc633",
            Self::ProcFallback => "proc_fallback",
        }
    }

    /// Reconstrói [`ObservationSource`] a partir do banco.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "osc133" => Some(Self::Osc133),
            "osc633" => Some(Self::Osc633),
            "proc_fallback" => Some(Self::ProcFallback),
            _ => None,
        }
    }
}

/// Grau de confiança semântica nos limites e código de saída do bloco.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ObservationConfidence {
    /// Marcadores completos (`A -> B -> C -> D`) com nonce válido.
    Exact,
    /// Transição sintética, fechamento implícito ou fallback `/proc`.
    Degraded,
}

impl ObservationConfidence {
    /// Representação textual persistida em `command_blocks.confidence`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Degraded => "degraded",
        }
    }

    /// Reconstrói [`ObservationConfidence`] a partir do banco.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "exact" => Some(Self::Exact),
            "degraded" => Some(Self::Degraded),
            _ => None,
        }
    }
}

/// Qualidade do texto de comando capturado para o bloco.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CommandTextQuality {
    /// Linha de comando literal reportada pelo hook (`cmd_b64` / `OSC 633 ; E`).
    Exact,
    /// Linha reconstruída a partir de `/proc/<pid>/cmdline` (sem pipes/redirecionamentos do shell).
    ArgvReconstructed,
    /// Comando não pôde ser obtido.
    Unavailable,
}

impl CommandTextQuality {
    /// Representação textual persistida em `command_blocks.command_text_quality`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::ArgvReconstructed => "argv_reconstructed",
            Self::Unavailable => "unavailable",
        }
    }

    /// Reconstrói [`CommandTextQuality`] a partir do banco.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "exact" => Some(Self::Exact),
            "argv_reconstructed" => Some(Self::ArgvReconstructed),
            "unavailable" => Some(Self::Unavailable),
            _ => None,
        }
    }
}

/// Estado de persistência assíncrona do bloco no SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceState {
    /// Enfileirado para gravação ou aguardando commit.
    Queued,
    /// Persistido com sucesso no SQLite.
    Persisted,
    /// Falha ao persistir no SQLite (mantido apenas no cache em memória da sessão ativa).
    Failed,
}

/// Status do ciclo de vida de um bloco de comando.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockStatus {
    /// Comando atualmente em execução.
    Running,
    /// Finalizado com sucesso (`exit_code == 0`).
    Succeeded,
    /// Finalizado com erro (`exit_code != 0`).
    Failed,
    /// Interrompido pelo usuário (`SIGINT` / `exit_code == 130`).
    Cancelled,
    /// Encerrado por sinal POSIX diferente de `SIGINT`.
    TerminatedBySignal,
    /// PTY encerrou enquanto o bloco estava em execução.
    PtyExited,
    /// Sessão anterior encerrada abruptamente (recuperado no startup seguinte).
    InterruptedByRestart,
}

impl BlockStatus {
    /// Representação textual persistida em `command_blocks.status`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TerminatedBySignal => "terminated_by_signal",
            Self::PtyExited => "pty_exited",
            Self::InterruptedByRestart => "interrupted_by_restart",
        }
    }

    /// Reconstrói [`BlockStatus`] a partir do banco.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "running" => Some(Self::Running),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            "terminated_by_signal" => Some(Self::TerminatedBySignal),
            "pty_exited" => Some(Self::PtyExited),
            "interrupted_by_restart" => Some(Self::InterruptedByRestart),
            _ => None,
        }
    }
}

/// Motivo detalhado do encerramento de um bloco de comando.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockEndReason {
    /// Retorno normal com código `0`.
    ExitZero,
    /// Retorno com código diferente de `0` (e não classificado como `CtrlC`/`KilledBySignal`).
    ExitNonZero,
    /// Interrupção via `SIGINT` (`Ctrl+C`, código `130`).
    CtrlC,
    /// Processo finalizado por sinal (`129..=192` exceto `130`, ou sinal explícito).
    KilledBySignal,
    /// Novo `PromptStart (A)` recebido sem `CommandFinished (D)` anterior.
    ImplicitNextPrompt,
    /// Novo `CommandExecuted (C)` recebido enquanto outro bloco ainda estava ativo.
    ImplicitNextCommand,
    /// O processo do PTY encerrou antes do retorno ao prompt.
    PtyExited,
    /// O aplicativo encerrou graciosamente enquanto o comando rodava.
    AppShutdown,
    /// Bloco órfão fechado na inicialização seguinte (`close_dangling`).
    RecoveredFromCrash,
}

impl BlockEndReason {
    /// Representação textual persistida em `command_blocks.end_reason`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::ExitZero => "exit_zero",
            Self::ExitNonZero => "exit_non_zero",
            Self::CtrlC => "ctrl_c",
            Self::KilledBySignal => "killed_by_signal",
            Self::ImplicitNextPrompt => "implicit_next_prompt",
            Self::ImplicitNextCommand => "implicit_next_command",
            Self::PtyExited => "pty_exited",
            Self::AppShutdown => "app_shutdown",
            Self::RecoveredFromCrash => "recovered_from_crash",
        }
    }

    /// Reconstrói [`BlockEndReason`] a partir do banco.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "exit_zero" => Some(Self::ExitZero),
            "exit_non_zero" => Some(Self::ExitNonZero),
            "ctrl_c" => Some(Self::CtrlC),
            "killed_by_signal" => Some(Self::KilledBySignal),
            "implicit_next_prompt" => Some(Self::ImplicitNextPrompt),
            "implicit_next_command" => Some(Self::ImplicitNextCommand),
            "pty_exited" => Some(Self::PtyExited),
            "app_shutdown" => Some(Self::AppShutdown),
            "recovered_from_crash" | "recovered_from_restart" => Some(Self::RecoveredFromCrash),
            _ => None,
        }
    }
}

/// Vínculo de execução entre PTY, sessão SQLite, aba e painel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBinding {
    /// Identificador do PTY.
    pub pty_id: PtyId,
    /// Identificador da sessão persistida no SQLite.
    pub session_id: SessionId,
    /// Identificador opcional da aba proprietária.
    pub tab_id: Option<TabId>,
    /// Identificador opcional do painel proprietário.
    pub pane_id: Option<PaneId>,
}

/// Referência leve a uma execução de comando em andamento.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRef {
    /// Identificador único do bloco.
    pub block_id: CommandBlockId,
    /// Sequência monotônica do comando dentro da sessão (`1..=N`).
    pub command_seq: u32,
    /// Timestamp UNIX em milissegundos do início da execução.
    pub started_at: i64,
    /// Prévia sanitizada da primeira linha do comando (até 256 bytes).
    pub command_preview: Option<String>,
    /// Fonte de observação que iniciou o bloco.
    pub source: ObservationSource,
}

/// Coordenada exata dentro do stream de bytes do PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamPosition {
    /// Número de sequência do frame `Data` do PTY.
    pub pty_seq: u64,
    /// Offset acumulado de bytes no stream do PTY.
    pub byte_offset: u64,
}

/// Os 5 estados canônicos de uma aba ou painel no `MaxTerminal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TabStatus {
    /// 🟢 Executando (`--max-success-500`): comando temporário em execução.
    Running,
    /// 🔵 Execução Contínua (`--max-info-500`): daemon/watcher ou comando `>= 15 s`.
    Continuous,
    /// 🟠 Finalizado (`--max-warning-500`): comando concluiu com `exit 0` em aba não visualizada.
    CompletedUnseen,
    /// 🔴 Com Erro (`--max-danger-500`): comando falhou (`exit != 0`) ou agente requer atenção em aba não visualizada.
    ErrorUnseen,
    /// ⚪ Parado: ocioso no prompt aguardando comando ou sem conclusão pendente de visualização.
    #[default]
    Idle,
}

impl TabStatus {
    /// Retorna a precedência estrita para agregação multi-painel (`ErrorUnseen > Running > Continuous > CompletedUnseen > Idle`).
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            Self::ErrorUnseen => 5,
            Self::Running => 4,
            Self::Continuous => 3,
            Self::CompletedUnseen => 2,
            Self::Idle => 1,
        }
    }
}

/// Desfecho semântico da última execução de um painel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CompletionOutcome {
    /// Concluído com código de saída `0`.
    Success,
    /// Concluído com erro (`exit != 0`, sinal fatal ou queda do PTY).
    Error,
    /// Interrompido intencionalmente pelo usuário (`Ctrl+C` / `SIGINT`).
    Cancelled,
}

/// Fotografia do último comando finalizado em um painel ou aba.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CompletionSnapshot {
    /// Identificador do bloco finalizado.
    pub block_id: CommandBlockId,
    /// Identificador do PTY onde rodou.
    pub pty_id: PtyId,
    /// Identificador opcional do painel.
    pub pane_id: Option<PaneId>,
    /// Resultado semântico (`Success`, `Error`, `Cancelled`).
    pub outcome: CompletionOutcome,
    /// Código de saída numérico, quando aplicável.
    pub exit_code: Option<i32>,
    /// Sinal de término, quando aplicável.
    pub signal: Option<String>,
    /// Razão detalhada do fechamento do bloco.
    pub end_reason: BlockEndReason,
    /// Timestamp UNIX em ms do término.
    pub ended_at: i64,
    /// Duração total em milissegundos.
    pub duration_ms: i64,
    /// Indica se o usuário já visualizou este desfecho.
    pub seen: bool,
}

/// Alerta de intervenção/permissão levantado por um Agente CLI (Fase 12) ou subsistema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename = "ShellAttentionSnapshot", rename_all = "camelCase")]
pub struct AttentionSnapshot {
    /// Origem do alerta (ex.: `"claude"`, `"codex"`, `"agy"`, `"opencode"`).
    pub source: String,
    /// Mensagem sanitizada descrevendo a intervenção solicitada.
    pub message: String,
    /// Timestamp UNIX em ms em que o alerta foi emitido.
    pub raised_at: i64,
    /// Indica se o alerta já foi visualizado pelo usuário.
    pub seen: bool,
}

/// Fotografia do estado de um painel individual dentro de uma aba.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PaneStateDto {
    /// Identificador do painel (`None` quando a aba possui painel único sem `PaneId` explícito).
    pub pane_id: Option<PaneId>,
    /// Identificador do PTY ativo neste painel.
    pub pty_id: PtyId,
    /// Identificador da sessão SQLite vinculada.
    pub session_id: SessionId,
    /// Estado semântico atual deste painel.
    pub status: TabStatus,
    /// Modo de integração ativo no shell deste painel.
    pub integration_mode: IntegrationMode,
    /// Política de execução contínua configurada para este painel.
    pub continuous_policy: ContinuousPolicy,
    /// Último diretório de trabalho (`cwd`) sanitizado reportado.
    pub cwd: Option<String>,
    /// Execução em andamento, se houver.
    pub active_execution: Option<ExecutionRef>,
    /// Última execução concluída neste painel, se houver.
    pub last_completion: Option<CompletionSnapshot>,
    /// Timestamp UNIX em ms da última transição deste painel.
    pub updated_at: i64,
}

/// Fotografia consolidada e versionada do estado de uma aba (`tab://state-changed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TabStateSnapshot {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Revisão monotônica estritamente crescente por `tab_id` (`>= 1`).
    pub revision: u64,
    /// Estado agregado da aba segundo a prioridade dos painéis e alertas de atenção.
    pub status: TabStatus,
    /// Indica se a aba está efetivamente focada e visível (`window_focused && active_tab_id == Some(tab_id)`).
    pub focused: bool,
    /// Lista ordenada de estados dos painéis pertencentes à aba.
    pub panes: Vec<PaneStateDto>,
    /// Última conclusão relevante da aba.
    pub last_completion: Option<CompletionSnapshot>,
    /// Alerta de atenção pendente na aba, se houver.
    pub attention: Option<AttentionSnapshot>,
    /// Timestamp UNIX em ms da última atualização.
    pub updated_at: i64,
}

/// Requisição para marcar conclusões e alertas de uma aba/painel como visualizados (`tab_mark_seen`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SeenRequest {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Se `Some`, marca apenas o painel informado; se `None`, marca todos os painéis da aba.
    pub pane_id: Option<PaneId>,
    /// Se informado, só aplica se a revisão atual for `<= up_to_revision` (ou reconcilia de forma idempotente).
    pub up_to_revision: Option<u64>,
}

/// Estado global de foco da janela e da aba/painel ativo (`tab_set_focus`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FocusSnapshot {
    /// Indica se a janela principal do aplicativo está focada e visível (não minimizada/oculta no tray).
    pub window_focused: bool,
    /// Aba atualmente selecionada na coluna direita.
    pub active_tab_id: Option<TabId>,
    /// Painel atualmente focado dentro da aba ativa.
    pub active_pane_id: Option<PaneId>,
}

/// Formato do texto de saída armazenado em `command_blocks.output_kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    /// Nenhuma saída textual produzida pelo comando.
    #[default]
    None,
    /// Saída integral preservada (`<= 64 KiB`).
    Full,
    /// Saída truncada preservando `head` (16 KiB) + marcador + `tail` (48 KiB).
    HeadTailTruncated,
    /// Apenas a cauda final foi preservada.
    TailOnly,
}

impl OutputKind {
    /// Representação textual persistida em `command_blocks.output_kind`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Full => "full",
            Self::HeadTailTruncated => "head_tail_truncated",
            Self::TailOnly => "tail_only",
        }
    }

    /// Reconstrói [`OutputKind`] a partir do banco.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "none" => Some(Self::None),
            "full" => Some(Self::Full),
            "head_tail_truncated" => Some(Self::HeadTailTruncated),
            "tail_only" => Some(Self::TailOnly),
            _ => None,
        }
    }
}

/// Evento emitido quando um comando inicia sua execução (`shell://command-started`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandStarted {
    /// Vínculo de runtime (`pty_id`, `session_id`, `tab_id`, `pane_id`).
    pub binding: RuntimeBinding,
    /// Referência resumida da execução.
    pub execution: ExecutionRef,
    /// Texto sanitizado do comando, quando disponível.
    pub command_text: Option<String>,
    /// Qualidade do texto do comando.
    pub command_text_quality: CommandTextQuality,
    /// Diretório de trabalho sanitizado no início do comando.
    pub cwd: Option<String>,
    /// Posição no stream onde o bloco começou.
    pub start_pos: StreamPosition,
    /// Posição no stream onde a saída (`C`) começou.
    pub output_start_pos: Option<StreamPosition>,
    /// Grau de confiança na observação inicial.
    pub confidence: ObservationConfidence,
    /// Estado de persistência no SQLite.
    pub persistence: PersistenceState,
}

/// Evento emitido quando um comando termina sua execução (`shell://command-finished`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandFinished {
    /// Vínculo de runtime (`pty_id`, `session_id`, `tab_id`, `pane_id`).
    pub binding: RuntimeBinding,
    /// Identificador único do bloco finalizado.
    pub block_id: CommandBlockId,
    /// Ordem sequencial do comando na sessão.
    pub command_seq: u32,
    /// Texto sanitizado do comando.
    pub command_text: Option<String>,
    /// Qualidade do texto do comando.
    pub command_text_quality: CommandTextQuality,
    /// Diretório de trabalho sanitizado.
    pub cwd: Option<String>,
    /// Timestamp UNIX em ms do início.
    pub started_at: i64,
    /// Timestamp UNIX em ms do término.
    pub ended_at: i64,
    /// Duração total em ms (`ended_at - started_at`).
    pub duration_ms: i64,
    /// Status final do bloco.
    pub status: BlockStatus,
    /// Razão detalhada do encerramento.
    pub end_reason: BlockEndReason,
    /// Código de saída numérico, quando disponível.
    pub exit_code: Option<i32>,
    /// Nome do sinal POSIX (`ex.: "SIGINT"`, `"SIGKILL"`), quando aplicável.
    pub signal: Option<String>,
    /// Posição inicial do bloco no stream do PTY.
    pub start_pos: StreamPosition,
    /// Posição inicial da saída do comando no stream do PTY.
    pub output_start_pos: Option<StreamPosition>,
    /// Posição final do bloco no stream do PTY.
    pub end_pos: StreamPosition,
    /// Total de bytes de saída produzidos pelo comando.
    pub output_bytes: i64,
    /// Total de linhas de saída produzidas pelo comando.
    pub output_lines: i64,
    /// Prévia curta sanitizada da saída (até 512 bytes) para exibição imediata sem roundtrip adicional.
    pub output_preview: Option<String>,
    /// Excerto sanitizado do erro (até 16 KiB) quando `inspectable == true` ou `status == Failed`.
    pub error_excerpt: Option<String>,
    /// Indica se o bloco deve exibir a ação "Inspecionar" (`exit_code != 0` e não foi `Ctrl+C`).
    pub inspectable: bool,
    /// Fonte de observação.
    pub source: ObservationSource,
    /// Confiança na observação.
    pub confidence: ObservationConfidence,
    /// Estado de persistência no SQLite.
    pub persistence: PersistenceState,
}

/// Contexto completo, sanitizado e truncado de um bloco com falha para envio ao `MaxCode` (Fase 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ErrorContext {
    /// Identificador do bloco.
    pub block_id: CommandBlockId,
    /// Identificador da sessão.
    pub session_id: SessionId,
    /// Identificador opcional da aba.
    pub tab_id: Option<TabId>,
    /// Identificador opcional do painel.
    pub pane_id: Option<PaneId>,
    /// Comando sanitizado (ou placeholder legível se indisponível).
    pub command: String,
    /// Qualidade da captura do comando.
    pub command_text_quality: CommandTextQuality,
    /// Saída de erro sanitizada e truncada via `prepare_external_payload` (máx. 16 KiB).
    pub error: String,
    /// Código de saída numérico.
    pub exit_code: Option<i32>,
    /// Sinal POSIX, se aplicável.
    pub signal: Option<String>,
    /// Diretório de trabalho sanitizado.
    pub cwd: Option<String>,
    /// Duração da execução em ms.
    pub duration_ms: i64,
    /// Timestamp UNIX em ms do início.
    pub started_at: i64,
    /// Timestamp UNIX em ms do término.
    pub ended_at: i64,
    /// Indica se o comando ou erro sofreu truncamento para caber no orçamento externo.
    pub truncated: bool,
    /// Prompt canônico pronto para Plan Mode no `MaxCode`:
    /// `Ao executar o comando "${comando}" tive o seguinte erro: "${erro}"`.
    pub plan_mode_prompt: String,
}

/// DTO completo de um bloco de comando retornado por `command_block_get` e `command_block_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandBlockDto {
    /// Identificador único do bloco (UUID v7).
    pub id: CommandBlockId,
    /// Sessão proprietária.
    pub session_id: SessionId,
    /// Aba associada, quando houver.
    pub tab_id: Option<TabId>,
    /// Painel associado, quando houver.
    pub pane_id: Option<PaneId>,
    /// PTY onde o comando foi executado.
    pub pty_id: PtyId,
    /// Número sequencial do comando na sessão (`1..=N`).
    pub command_seq: u32,
    /// Texto sanitizado do comando.
    pub command_text: Option<String>,
    /// Qualidade da captura do comando.
    pub command_text_quality: CommandTextQuality,
    /// Diretório de trabalho sanitizado.
    pub cwd: Option<String>,
    /// Timestamp UNIX em ms do início.
    pub started_at: i64,
    /// Timestamp UNIX em ms do término (`None` enquanto `Running`).
    pub ended_at: Option<i64>,
    /// Duração em ms (`None` enquanto `Running`).
    pub duration_ms: Option<i64>,
    /// Código de saída (`None` enquanto `Running` ou se encerrado sem código).
    pub exit_code: Option<i32>,
    /// Sinal POSIX, se aplicável.
    pub signal: Option<String>,
    /// Status do bloco.
    pub status: BlockStatus,
    /// Razão do término (`None` enquanto `Running`).
    pub end_reason: Option<BlockEndReason>,
    /// Indica se o comando atingiu a classificação de execução contínua.
    pub is_continuous: bool,
    /// Fonte de observação.
    pub source: ObservationSource,
    /// Grau de confiança.
    pub confidence: ObservationConfidence,
    /// Texto sanitizado da saída capturada (até 64 KiB).
    pub output_text: Option<String>,
    /// Formato da saída armazenada.
    pub output_kind: OutputKind,
    /// Total de bytes de saída produzidos pelo comando.
    pub output_bytes: i64,
    /// Total de linhas de saída produzidas pelo comando.
    pub output_lines: i64,
    /// Excerto sanitizado do erro (até 16 KiB).
    pub error_excerpt: Option<String>,
    /// Indica se o bloco é elegível para o botão "Inspecionar".
    pub inspectable: bool,
    /// Sequência de frame do PTY no início do bloco.
    pub start_pty_seq: u64,
    /// Offset de bytes no PTY no início do bloco.
    pub start_byte_offset: u64,
    /// Sequência de frame do PTY no início do output.
    pub output_start_pty_seq: Option<u64>,
    /// Offset de bytes no PTY no início do output.
    pub output_start_byte_offset: Option<u64>,
    /// Sequência de frame do PTY no fim do bloco.
    pub end_pty_seq: Option<u64>,
    /// Offset de bytes no PTY no fim do bloco.
    pub end_byte_offset: Option<u64>,
    /// Estado de persistência do registro.
    pub persistence: PersistenceState,
}

/// Parâmetros de filtragem e paginação keyset para `command_block_list`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandBlockListQuery {
    /// Filtra por sessão específica.
    pub session_id: Option<SessionId>,
    /// Filtra por aba específica.
    pub tab_id: Option<TabId>,
    /// Filtra por PTY específico.
    pub pty_id: Option<PtyId>,
    /// Filtra por conjunto de status.
    pub statuses: Option<Vec<BlockStatus>>,
    /// Limite de itens por página (`default: 50`, `max: 200`).
    pub limit: Option<u32>,
    /// Cursor opaco de paginação keyset (`started_at`, `id`).
    pub cursor: Option<String>,
}

/// Parâmetros de busca textual FTS5 para `command_block_search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandBlockSearchQuery {
    /// Consulta do usuário higienizada por `build_safe_fts_query`.
    pub query: String,
    /// Filtra opcionalmente por sessão.
    pub session_id: Option<SessionId>,
    /// Filtra opcionalmente por aba.
    pub tab_id: Option<TabId>,
    /// Se `Some(true)`, restringe a blocos com falha (`Failed` / `TerminatedBySignal` / `PtyExited`).
    pub only_errors: Option<bool>,
    /// Limite de resultados (`default: 25`, `max: 100`).
    pub limit: Option<u32>,
    /// Cursor de paginação da busca.
    pub cursor: Option<String>,
}

/// Resultado individual da busca textual FTS5 em `command_blocks_fts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandBlockHit {
    /// Bloco de comando correspondente.
    pub block: CommandBlockDto,
    /// Snippet destacado do texto do comando, se houve match nessa coluna.
    pub command_snippet: Option<String>,
    /// Snippet destacado da saída/erro, se houve match nessas colunas.
    pub output_snippet: Option<String>,
    /// Score de relevância BM25 (valores menores/mais negativos indicam maior relevância no FTS5).
    pub rank: f64,
}

/// Página genérica com cursor opaco para próxima consulta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(inline)]
pub struct Page<T> {
    /// Registros retornados nesta página.
    pub items: Vec<T>,
    /// Cursor opaco para buscar a próxima página (`None` quando chegou ao fim).
    pub next_cursor: Option<String>,
}

/// Diagnóstico da integração de shell de um PTY (`shell_integration_info`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationInfo {
    /// Identificador do PTY.
    pub pty_id: PtyId,
    /// Identificador da sessão SQLite associada.
    pub session_id: SessionId,
    /// Aba associada, se houver.
    pub tab_id: Option<TabId>,
    /// Painel associado, se houver.
    pub pane_id: Option<PaneId>,
    /// Família do shell detectada.
    pub shell_kind: ShellKind,
    /// Modo de integração atual (`Pending`, `Osc133`, `FallbackProc`, `Disabled`).
    pub mode: IntegrationMode,
    /// Política de execução contínua ativa.
    pub continuous_policy: ContinuousPolicy,
    /// Diretório de trabalho atual sanitizado.
    pub cwd: Option<String>,
    /// Total de eventos OSC 133/633 válidos processados.
    pub osc_events_received: u64,
    /// Total de ativações de blocos via fallback `/proc`.
    pub fallback_activations: u64,
}

/// Evento emitido quando uma aba é encerrada e removida do registro (`tab://state-removed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TabStateRemovedEvent {
    /// Identificador da aba removida.
    pub tab_id: TabId,
    /// Última revisão monotônica antes da remoção.
    pub revision: u64,
}

/// Evento emitido quando o diretório de trabalho de um painel muda (`shell://cwd-changed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShellCwdChangedEvent {
    /// Vínculo de runtime do painel.
    pub binding: RuntimeBinding,
    /// Novo caminho de diretório sanitizado.
    pub cwd: String,
    /// Timestamp UNIX em ms da mudança.
    pub at: i64,
}

/// Evento emitido quando o modo de integração do shell muda (`shell://integration-mode`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShellIntegrationModeEvent {
    /// Vínculo de runtime do painel.
    pub binding: RuntimeBinding,
    /// Novo modo de integração.
    pub mode: IntegrationMode,
    /// Família do shell.
    pub shell_kind: ShellKind,
    /// Motivo opcional da transição (ex.: `"first_osc133_received"`, `"activation_window_expired"`).
    pub reason: Option<String>,
}

/// Constrói o prompt canônico para abertura em Plan Mode no `MaxCode` a partir de comando e erro já sanitizados.
#[must_use]
pub fn format_plan_mode_prompt(command: &str, error: &str) -> String {
    format!("Ao executar o comando \"{command}\" tive o seguinte erro: \"{error}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_status_priority_strict_order() {
        assert!(TabStatus::ErrorUnseen.priority() > TabStatus::Running.priority());
        assert!(TabStatus::Running.priority() > TabStatus::Continuous.priority());
        assert!(TabStatus::Continuous.priority() > TabStatus::CompletedUnseen.priority());
        assert!(TabStatus::CompletedUnseen.priority() > TabStatus::Idle.priority());
    }

    #[test]
    fn db_str_roundtrips() {
        for mode in [
            IntegrationMode::Pending,
            IntegrationMode::Osc133,
            IntegrationMode::FallbackProc,
            IntegrationMode::Disabled,
        ] {
            assert_eq!(IntegrationMode::from_db_str(mode.as_db_str()), Some(mode));
        }

        for status in [
            BlockStatus::Running,
            BlockStatus::Succeeded,
            BlockStatus::Failed,
            BlockStatus::Cancelled,
            BlockStatus::TerminatedBySignal,
            BlockStatus::PtyExited,
            BlockStatus::InterruptedByRestart,
        ] {
            assert_eq!(BlockStatus::from_db_str(status.as_db_str()), Some(status));
        }
    }
}
