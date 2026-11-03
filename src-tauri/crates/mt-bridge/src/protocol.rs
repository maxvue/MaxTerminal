//! Tipos do protocolo bidirecional `MaxTerminal` ⇄ `MaxCode` (`protocol_version = 1`),
//! incluindo capabilities, envelopes RPC e snapshots de status.

use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::issues::{
    BugCycleProgressPayload, BugsCycleAckPayload, BugsCycleStartPayload, IssueCommandAckPayload,
    IssueCommandPayload,
};

/// Versão atual do protocolo da ponte (`1`).
pub const PROTOCOL_VERSION: u32 = 1;

///Tamanho máximo permitido para um único envelope JSON (256 KiB).
pub const MAX_ENVELOPE_BYTES: usize = 256 * 1024;

/// Capability para inspeção de erros de comando em Plan Mode (`inspect_v1`).
pub const CAP_INSPECT_V1: &str = "inspect_v1";
/// Capability para sincronização bidirecional de projeto ativo (`sync_project_v1`).
pub const CAP_SYNC_PROJECT_V1: &str = "sync_project_v1";
/// Capability para abertura de arquivo em linha/coluna (`open_file_v1`).
pub const CAP_OPEN_FILE_V1: &str = "open_file_v1";
/// Capability de ping/pong (`ping_v1`).
pub const CAP_PING_V1: &str = "ping_v1";
/// Capability de ações unitárias de Issues (`issues_protocol_v1` — Fase 11/06 & Fase 14).
pub const CAP_ISSUES_PROTOCOL_V1: &str = "issues_protocol_v1";
/// Capability do ciclo automatizado em lote de Bugs (`bugs_cycle_v1` — Fase 11/06 & Fase 14).
pub const CAP_BUGS_CYCLE_V1: &str = "bugs_cycle_v1";

/// Lista padrão de capabilities anunciadas pelo `MaxTerminal`.
#[must_use]
pub fn default_maxterminal_capabilities() -> Vec<String> {
    vec![
        CAP_INSPECT_V1.to_string(),
        CAP_SYNC_PROJECT_V1.to_string(),
        CAP_OPEN_FILE_V1.to_string(),
        CAP_PING_V1.to_string(),
        CAP_ISSUES_PROTOCOL_V1.to_string(),
        CAP_BUGS_CYCLE_V1.to_string(),
    ]
}

/// Papel da ponta na comunicação bidirecional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PeerRole {
    /// Aplicativo desktop `MaxTerminal`.
    MaxTerminal,
    /// Extensão VS Code `MaxCode`.
    MaxCode,
}

/// Agente CLI opcional solicitado ao abrir aba para uma issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename = "BridgeAgentKind", rename_all = "snake_case")]
pub enum AgentKind {
    /// Claude Code (`claude`).
    Claude,
    /// `OpenAI` Codex (`codex`).
    Codex,
    /// Antigravity (`agy`).
    Agy,
    /// `OpenCode` (`opencode`).
    OpenCode,
}

/// Payload de apresentação inicial (`hello`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HelloPayload {
    /// Versão do protocolo.
    pub protocol_version: u32,
    /// Papel do remetente.
    pub role: PeerRole,
    /// Identificador único da instância remetente.
    pub instance_id: String,
    /// PID do processo remetente.
    pub pid: u32,
    /// Versão semântica do software remetente.
    pub version: String,
    /// Caminho do projeto atualmente focado no remetente, se houver.
    pub active_project_path: Option<String>,
    /// Lista de capabilities suportadas.
    pub capabilities: Vec<String>,
}

/// Resposta de confirmação do handshake (`welcome`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WelcomePayload {
    /// Versão do protocolo aceita.
    pub protocol_version: u32,
    /// Papel do servidor que aceitou.
    pub role: PeerRole,
    /// Identificador da instância que aceitou.
    pub instance_id: String,
    /// PID da instância que aceitou.
    pub pid: u32,
    /// Versão do software.
    pub version: String,
    /// Projeto ativo atual no receptor, se houver.
    pub active_project_path: Option<String>,
    /// Capabilities suportadas pelo receptor.
    pub capabilities: Vec<String>,
}

/// Payload de `ping`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PingPayload {
    /// Número de sequência monotônico do ping.
    pub seq: u64,
}

/// Payload de `pong`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PongPayload {
    /// Número de sequência ecoado do `ping`.
    pub seq: u64,
}

/// Notificação bidirecional de abertura/foco de projeto (`project.opened`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOpenedPayload {
    /// Caminho absoluto da pasta do projeto.
    pub project_path: String,
    /// Nome amigável opcional do projeto.
    pub project_name: Option<String>,
    /// Branch Git atual opcional.
    pub git_branch: Option<String>,
    /// Identificador de origem para prevenção de eco (`"maxterminal"` ou `"maxcode"`).
    pub origin_id: String,
}

/// Requisição `MaxTerminal` → `MaxCode` para abrir arquivo em linha/coluna (`editor.open_file`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenFilePayload {
    /// Caminho absoluto do arquivo.
    pub file_path: String,
    /// Linha (1-indexed).
    pub line: Option<u32>,
    /// Coluna (1-indexed).
    pub column: Option<u32>,
    /// Caminho do projeto associado, se houver.
    pub project_path: Option<String>,
    /// Se `true`, solicita foco na janela do editor.
    pub focus_editor: bool,
}

/// Tipo de código de saída do comando com falha.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InspectErrorCode {
    /// Código numérico de saída (`exit != 0`).
    ExitCode {
        /// Código de saída retornado pelo processo.
        code: i32,
    },
    /// Processo encerrado por sinal POSIX.
    Signal {
        /// Número do sinal.
        signal: i32,
        /// Nome simbólico do sinal (ex.: `"SIGSEGV"`).
        name: String,
    },
}

/// Requisição `MaxTerminal` → `MaxCode` para abrir conversa em Plan Mode (`inspect.command_error`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectCommandErrorPayload {
    /// Identificador do bloco de comando no `MaxTerminal`.
    pub block_id: String,
    /// Identificador da sessão.
    pub session_id: String,
    /// Identificador da aba.
    pub tab_id: String,
    /// Caminho absoluto do projeto.
    pub project_path: String,
    /// Diretório de trabalho em que o comando foi executado.
    pub cwd: String,
    /// Comando sanitizado.
    pub command: String,
    /// Trecho de erro sanitizado.
    pub error_output: String,
    /// Código de saída ou sinal.
    pub exit_status: InspectErrorCode,
    /// Duração da execução em milissegundos.
    pub duration_ms: Option<u64>,
    /// Modo de conversa exigido (`"plan"`).
    pub mode: String,
    /// Prompt canônico obrigatório: `Ao executar o comando "${comando}" tive o seguinte erro: "${erro}"`.
    pub formatted_prompt: String,
}

/// Resposta de confirmação da abertura de inspeção (`inspect.ack`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InspectAckPayload {
    /// ID da requisição original.
    pub request_id: Uuid,
    /// Indica se o `MaxCode` aceitou e abriu a conversa.
    pub accepted: bool,
    /// Identificador opcional da conversa criada no `MaxCode`.
    pub conversation_id: Option<String>,
    /// Motivo de falha caso `accepted == false`.
    pub error: Option<String>,
}

/// Requisição `MaxCode` → `MaxTerminal` para abrir aba posicionada em uma issue (`terminal.open_for_issue`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenTerminalForIssuePayload {
    /// Caminho absoluto da raiz do projeto.
    pub project_path: String,
    /// Caminho opcional da worktree dedicada da issue.
    pub worktree_path: Option<String>,
    /// Branch opcional da issue.
    pub branch: Option<String>,
    /// Número da issue no GitHub.
    pub issue_number: u64,
    /// Título opcional da issue.
    pub issue_title: Option<String>,
    /// Agente CLI opcional a ser iniciado na aba (`None` = shell padrão).
    pub agent: Option<AgentKind>,
    /// Comando inicial opcional.
    pub initial_command: Option<String>,
}

/// Confirmação de abertura da aba para issue (`terminal.opened_ack`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOpenedAckPayload {
    /// ID da requisição original.
    pub request_id: Uuid,
    /// Indica se a aba foi criada/focada com sucesso.
    pub accepted: bool,
    /// ID do projeto no `MaxTerminal`.
    pub project_id: Option<String>,
    /// ID da aba aberta no `MaxTerminal`.
    pub tab_id: Option<String>,
    /// Mensagem de erro caso `accepted == false`.
    pub error: Option<String>,
}

/// Payload de erro de protocolo (`error`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    /// ID da requisição que causou o erro, se conhecido.
    pub request_id: Option<Uuid>,
    /// Código textual do erro.
    pub code: String,
    /// Mensagem descritiva em `pt-BR`.
    pub message: String,
}

/// Variações tipadas de mensagens RPC trafegadas em `/v1/ws` e `/v1/rpc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", content = "payload")]
pub enum RpcPayload {
    /// Handshake inicial do cliente.
    #[serde(rename = "hello")]
    Hello(HelloPayload),
    /// Resposta do servidor ao `hello`.
    #[serde(rename = "welcome")]
    Welcome(WelcomePayload),
    /// Heartbeat ping.
    #[serde(rename = "ping")]
    Ping(PingPayload),
    /// Heartbeat pong.
    #[serde(rename = "pong")]
    Pong(PongPayload),
    /// Sincronização bidirecional de projeto ativo.
    #[serde(rename = "project.opened")]
    ProjectOpened(ProjectOpenedPayload),
    /// Abertura de arquivo no editor em linha/coluna.
    #[serde(rename = "editor.open_file")]
    OpenFile(OpenFilePayload),
    /// Envio de erro de comando para Plan Mode no `MaxCode`.
    #[serde(rename = "inspect.command_error")]
    InspectCommandError(InspectCommandErrorPayload),
    /// Confirmação de recebimento do `inspect.command_error`.
    #[serde(rename = "inspect.ack")]
    InspectAck(InspectAckPayload),
    /// Abertura de aba no `MaxTerminal` posicionada na worktree/branch de uma issue.
    #[serde(rename = "terminal.open_for_issue")]
    OpenTerminalForIssue(OpenTerminalForIssuePayload),
    /// Confirmação de abertura de aba para issue.
    #[serde(rename = "terminal.opened_ack")]
    TerminalOpenedAck(TerminalOpenedAckPayload),
    /// Comando unitário de issue (`issue.open`, `issue.plan`, `issue.fix` — Subetapa 11/06).
    #[serde(rename = "issue.command")]
    IssueCommand(IssueCommandPayload),
    /// Confirmação de comando unitário de issue (`issue.command_ack`).
    #[serde(rename = "issue.command_ack")]
    IssueCommandAck(IssueCommandAckPayload),
    /// Disparo ou controle do ciclo automatizado de bugs em lote (`bugs.cycle_start`).
    #[serde(rename = "bugs.cycle_start")]
    BugsCycleStart(BugsCycleStartPayload),
    /// Confirmação do disparo do ciclo de bugs (`bugs.cycle_ack`).
    #[serde(rename = "bugs.cycle_ack")]
    BugsCycleAck(BugsCycleAckPayload),
    /// Evento de progresso em tempo real do ciclo de bugs (`bugs.cycle_progress`).
    #[serde(rename = "bugs.cycle_progress")]
    BugCycleProgress(BugCycleProgressPayload),
    /// Erro de protocolo.
    #[serde(rename = "error")]
    Error(ErrorPayload),
}

/// Envelope raiz de todas as mensagens trafegadas na ponte `MaxTerminal` ⇄ `MaxCode`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RpcEnvelope {
    /// Versão do protocolo (`1`).
    pub v: u32,
    /// Identificador único de idempotência/correlação da mensagem (`UUID v4`).
    pub request_id: Uuid,
    /// Timestamp UNIX em milissegundos de emissão.
    pub timestamp_ms: i64,
    /// Mensagem tipada (`type` + `payload`).
    #[serde(flatten)]
    pub message: RpcPayload,
}

impl RpcEnvelope {
    /// Cria um novo envelope `v = 1` gerando `Uuid::new_v4()` e timestamp atual.
    #[must_use]
    pub fn new(message: RpcPayload) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            request_id: Uuid::new_v4(),
            timestamp_ms: current_unix_ms(),
            message,
        }
    }

    /// Cria um envelope com `request_id` explícito.
    #[must_use]
    pub fn with_id(request_id: Uuid, message: RpcPayload) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            request_id,
            timestamp_ms: current_unix_ms(),
            message,
        }
    }
}

/// Estado de conexão da ponte com a extensão `MaxCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum BridgeConnectionState {
    /// Ponte desabilitada pelo usuário nas preferências.
    Disabled,
    /// Servidor loopback do `MaxTerminal` ouvindo, mas sem sessão ativa com o `MaxCode`.
    #[default]
    Disconnected,
    /// Tentando estabelecer conexão com o endpoint descoberto do `MaxCode`.
    Connecting,
    /// Conectado em tempo real via WebSocket ou HTTP RPC validado.
    Connected,
}

/// Fotografia de diagnóstico e estado da ponte (`bridge_status`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatusSnapshot {
    /// Estado atual da conexão.
    pub state: BridgeConnectionState,
    /// Porta local em que o servidor loopback do `MaxTerminal` está ouvindo (`127.0.0.1:<port>`).
    pub server_port: Option<u16>,
    /// Instância conectada do `MaxCode`, se houver.
    pub peer_instance_id: Option<String>,
    /// PID do processo da extensão `MaxCode`, se conectado.
    pub peer_pid: Option<u32>,
    /// Versão reportada pelo `MaxCode`, se conectado.
    pub peer_version: Option<String>,
    /// Capabilities anunciadas pelo `MaxCode` conectado.
    pub peer_capabilities: Vec<String>,
    /// Última latência de ping/pong observada em milissegundos.
    pub last_rtt_ms: Option<u64>,
    /// Timestamp UNIX em milissegundos do último evento recebido/enviado.
    pub last_seen_ms: Option<i64>,
    /// Último erro registrado na ponte, se houver.
    pub last_error: Option<String>,
}

impl Default for BridgeStatusSnapshot {
    fn default() -> Self {
        Self {
            state: BridgeConnectionState::Disconnected,
            server_port: None,
            peer_instance_id: None,
            peer_pid: None,
            peer_version: None,
            peer_capabilities: Vec::new(),
            last_rtt_ms: None,
            last_seen_ms: None,
            last_error: None,
        }
    }
}

/// Retorna o timestamp UNIX atual em milissegundos.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn current_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
