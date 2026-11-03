//! Modelos de domínio, identificadores UUID v7, DTOs serializáveis e códigos de erro da Fase 15.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mt_core::{
    CommandBlockId, CommandGroupId, MtError, PaneId, ProjectId, PtyId, SessionId, TabId, TemplateId,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

/// Identificador único de um item da definição de um grupo de comandos (UUID v7).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct GroupItemId(pub String);

impl GroupItemId {
    /// Cria um novo `GroupItemId` UUID v7.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    /// Retorna o identificador como `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for GroupItemId {
    fn default() -> Self {
        Self::new()
    }
}

/// Identificador único de uma execução de grupo de comandos (UUID v7).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct GroupRunId(pub String);

impl GroupRunId {
    /// Cria um novo `GroupRunId` UUID v7.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    /// Retorna o identificador como `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for GroupRunId {
    fn default() -> Self {
        Self::new()
    }
}

/// Identificador único de um item instanciado em uma execução de grupo (UUID v7).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct GroupRunItemId(pub String);

impl GroupRunItemId {
    /// Cria um novo `GroupRunItemId` UUID v7.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    /// Retorna o identificador como `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for GroupRunItemId {
    fn default() -> Self {
        Self::new()
    }
}

/// Identificador de idempotência enviado pelo cliente ao solicitar o lançamento de um grupo (UUID v7).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct GroupRequestId(pub String);

impl GroupRequestId {
    /// Cria um novo `GroupRequestId` UUID v7.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    /// Retorna o identificador como `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for GroupRequestId {
    fn default() -> Self {
        Self::new()
    }
}

/// Identificador de assinatura de stream de saída (`Channel<GroupOutputFrame>`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct OutputSubscriptionId(pub String);

impl OutputSubscriptionId {
    /// Cria um novo `OutputSubscriptionId` UUID v7.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    /// Retorna o identificador como `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for OutputSubscriptionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Escopo de visibilidade do grupo de comandos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GroupScope {
    /// Visível e executável apenas no projeto proprietário.
    Local,
    /// Visível e executável em qualquer projeto.
    Global,
}

impl GroupScope {
    /// Representação textual em `snake_case` persistida no SQLite.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Global => "global",
        }
    }

    /// Converte uma string persistida no SQLite para `GroupScope`.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "local" => Some(Self::Local),
            "global" => Some(Self::Global),
            _ => None,
        }
    }
}

/// Modo de execução do grupo de comandos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    /// Fila sequencial no PTY do painel ativo (`&&` ou `;`).
    Sequential,
    /// Execução concorrente em background (PTYs headless supervisionados).
    Parallel,
    /// Uma aba dedicada por item (`WorkspaceManager::open_tab`).
    MultiTabs,
}

impl ExecutionMode {
    /// Representação textual em `snake_case` persistida no SQLite.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::Parallel => "parallel",
            Self::MultiTabs => "multi_tabs",
        }
    }

    /// Converte uma string persistida no SQLite para `ExecutionMode`.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "sequential" => Some(Self::Sequential),
            "parallel" => Some(Self::Parallel),
            "multi_tabs" => Some(Self::MultiTabs),
            _ => None,
        }
    }
}

/// Política de continuação entre itens no modo Sequencial na mesma aba.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SequentialSeparator {
    /// Parar na primeira falha (`&&`).
    And,
    /// Continuar mesmo após falha (`;`).
    Semicolon,
}

impl SequentialSeparator {
    /// Representação textual em `snake_case` persistida no SQLite.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Semicolon => "semicolon",
        }
    }

    /// Converte uma string persistida no SQLite para `SequentialSeparator`.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "and" => Some(Self::And),
            "semicolon" => Some(Self::Semicolon),
            _ => None,
        }
    }
}

/// Dialeto real do shell onde o item será executado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ShellDialect {
    /// GNU Bash.
    Bash,
    /// Zsh.
    Zsh,
    /// Fish shell.
    Fish,
}

/// Estado individual de um item dentro de uma execução (`GroupRunItem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    /// Aguardando vez na fila ou semáforo.
    Pending,
    /// Em processo de spawn/abertura de aba ou submissão.
    Starting,
    /// Em execução ativa.
    Running,
    /// Cancelamento solicitado; aguardando confirmação de encerramento e limpeza da árvore.
    Cancelling,
    /// Concluído com sucesso (`exit_code == 0`).
    Succeeded,
    /// Concluído com erro (`exit_code != 0` ou falha de lançamento).
    Failed,
    /// Pulado devido a falha de item anterior com política `&&`.
    Skipped,
    /// Cancelado pelo usuário antes ou durante a execução.
    Cancelled,
    /// Interrompido por perda de supervisão/tracking ou crash anterior.
    Interrupted,
}

impl ItemStatus {
    /// Indica se o estado é terminal e imutável.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Skipped | Self::Cancelled | Self::Interrupted
        )
    }

    /// Representação textual em `snake_case` persistida no SQLite.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    /// Converte uma string persistida no SQLite para `ItemStatus`.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "pending" => Some(Self::Pending),
            "starting" => Some(Self::Starting),
            "running" => Some(Self::Running),
            "cancelling" => Some(Self::Cancelling),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "skipped" => Some(Self::Skipped),
            "cancelled" => Some(Self::Cancelled),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

/// Estado agregado determinístico de uma execução de grupo (`GroupRun`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Aceito e aguardando início do primeiro item.
    Queued,
    /// Execução em andamento.
    Running,
    /// Cancelamento em andamento com pelo menos um item ainda não terminal.
    Cancelling,
    /// Todos os itens terminaram com sucesso.
    Succeeded,
    /// Todos os itens terminaram sem nenhum sucesso e com pelo menos uma falha.
    Failed,
    /// Todos os itens terminaram com mistura de sucesso e falha.
    Partial,
    /// Todos os itens terminaram e pelo menos um foi cancelado (sem interrupção).
    Cancelled,
    /// Todos os itens terminaram e pelo menos um foi interrompido.
    Interrupted,
}

impl RunStatus {
    /// Indica se o estado agregado da execução é terminal.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Partial | Self::Cancelled | Self::Interrupted
        )
    }

    /// Representação textual em `snake_case` persistida no SQLite.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Partial => "partial",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    /// Converte uma string persistida no SQLite para `RunStatus`.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "queued" => Some(Self::Queued),
            "running" => Some(Self::Running),
            "cancelling" => Some(Self::Cancelling),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "partial" => Some(Self::Partial),
            "cancelled" => Some(Self::Cancelled),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

/// Entrada de criação/atualização para um item de grupo de comandos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandGroupItemInput {
    /// Identificador existente do item (quando editando um item já pertencente ao grupo).
    pub id: Option<GroupItemId>,
    /// Título curto do item (1–80 caracteres).
    pub title: String,
    /// Comando shell a executar (1–8192 bytes, sem `\r`, `\n` ou NUL).
    pub command: String,
    /// Diretório de trabalho relativo à raiz do projeto.
    pub cwd: Option<String>,
    /// Variáveis de ambiente adicionais do item.
    pub env: BTreeMap<String, String>,
    /// Indica se o item é um processo de execução contínua (daemon/watcher).
    pub continuous: bool,
}

/// Entrada de criação/atualização para um grupo de comandos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandGroupInput {
    /// Escopo do grupo (`local` ou `global`).
    pub scope: GroupScope,
    /// Projeto proprietário (obrigatório quando `scope == Local`, `None` quando `Global`).
    pub project_id: Option<ProjectId>,
    /// Nome do grupo (1–80 caracteres).
    pub name: String,
    /// Descrição opcional (até 2048 caracteres).
    pub description: String,
    /// Etiquetas de categorização/busca (até 20 labels de 1–32 caracteres).
    pub labels: Vec<String>,
    /// Modo de execução padrão do grupo.
    pub mode: ExecutionMode,
    /// Política de separador para o modo Sequencial (`and` ou `semicolon`).
    pub separator: SequentialSeparator,
    /// Diretório de trabalho relativo padrão do grupo.
    pub cwd: Option<String>,
    /// Variáveis de ambiente compartilhadas pelo grupo.
    pub env: BTreeMap<String, String>,
    /// Lista ordenada de itens do grupo (1–50 itens).
    pub items: Vec<CommandGroupItemInput>,
}

/// Item persistido de uma definição de grupo de comandos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandGroupItem {
    /// Identificador único do item.
    pub id: GroupItemId,
    /// Identificador do grupo ao qual pertence.
    pub group_id: CommandGroupId,
    /// Posição 0-based dentro do grupo.
    pub position: u32,
    /// Título sanitizado do item.
    pub title: String,
    /// Comando shell.
    pub command: String,
    /// Diretório relativo opcional.
    pub cwd: Option<String>,
    /// Variáveis de ambiente do item.
    pub env: BTreeMap<String, String>,
    /// Flag de execução contínua.
    pub continuous: bool,
}

/// Definição completa persistida de um grupo de comandos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandGroup {
    /// Identificador único do grupo.
    pub id: CommandGroupId,
    /// Escopo (`local` ou `global`).
    pub scope: GroupScope,
    /// Projeto proprietário quando `local`.
    pub project_id: Option<ProjectId>,
    /// Nome exibido do grupo.
    pub name: String,
    /// Descrição do grupo.
    pub description: String,
    /// Labels ordenadas associadas ao grupo.
    pub labels: Vec<String>,
    /// Modo de execução padrão.
    pub mode: ExecutionMode,
    /// Política de continuação no modo Sequencial.
    pub separator: SequentialSeparator,
    /// Diretório de trabalho relativo padrão.
    pub cwd: Option<String>,
    /// Variáveis de ambiente no nível do grupo.
    pub env: BTreeMap<String, String>,
    /// Ordem de exibição dentro do escopo/projeto.
    pub sort_order: u32,
    /// Revisão monotônica otimista (`>= 1`).
    pub revision: u64,
    /// Timestamp de criação (epoch ms).
    pub created_at: i64,
    /// Timestamp da última atualização (epoch ms).
    pub updated_at: i64,
    /// Itens ordenados por `position`.
    pub items: Vec<CommandGroupItem>,
}

/// Catálogo retornado por `command_group_list` e `command_group_reorder`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandGroupCatalog {
    /// Projeto consultado (quando informado).
    pub project_id: Option<ProjectId>,
    /// Grupos locais do projeto ordenados por `sort_order`.
    pub local_groups: Vec<CommandGroup>,
    /// Grupos globais ordenados por `sort_order`.
    pub global_groups: Vec<CommandGroup>,
}

/// Entrada de reordenação com verificação otimista de revisão por grupo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReorderGroupEntry {
    /// Identificador do grupo.
    pub id: CommandGroupId,
    /// Revisão esperada pelo cliente.
    pub revision: u64,
}

/// Recibo de exclusão de grupo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeleteGroupReceipt {
    /// Identificador do grupo removido.
    pub id: CommandGroupId,
    /// Revisão tombstone subsequente à remoção (`revision_anterior + 1`).
    pub revision: u64,
}

/// Requisição IPC para iniciar a execução de um grupo salvo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupRunRequest {
    /// Chave de idempotência do pedido de execução.
    pub request_id: GroupRequestId,
    /// Grupo salvo a executar.
    pub group_id: CommandGroupId,
    /// Revisão esperada da definição do grupo.
    pub expected_revision: u64,
    /// Projeto alvo no qual o grupo será executado.
    pub project_id: ProjectId,
    /// Painel de origem (obrigatório no modo Sequencial; opcional nos demais).
    pub origin_pane_id: Option<PaneId>,
    /// Sobrescrita opcional do modo de execução para este lançamento.
    pub mode_override: Option<ExecutionMode>,
    /// Mapa explícito opcional de `GroupItemId -> TemplateId` para o modo Multi-Abas.
    pub template_by_item: BTreeMap<GroupItemId, TemplateId>,
}

/// Contexto resolvido para o lançamento de uma execução.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunContext {
    /// Projeto alvo.
    pub project_id: ProjectId,
    /// Raiz canonicalizada do projeto.
    pub root: PathBuf,
    /// Vínculo de origem `(TabId, PaneId, PtyId, SessionId)` quando aplicável.
    pub origin: Option<(TabId, PaneId, PtyId, SessionId)>,
    /// Caminho do executável do shell.
    pub shell: PathBuf,
    /// Dialeto detectado do shell.
    pub dialect: ShellDialect,
    /// Geração do painel/PTY no momento do lançamento.
    pub generation: u64,
}

/// Snapshot imutável do grupo persistido em `command_group_runs.snapshot_json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunSnapshot {
    /// ID do grupo original.
    pub group_id: CommandGroupId,
    /// Revisão do grupo capturada no lançamento.
    pub group_revision: u64,
    /// Escopo do grupo.
    pub scope: GroupScope,
    /// Nome do grupo no momento do lançamento.
    pub name: String,
    /// Descrição do grupo no momento do lançamento.
    pub description: String,
    /// Labels do grupo no momento do lançamento.
    pub labels: Vec<String>,
    /// Modo definido no grupo.
    pub defined_mode: ExecutionMode,
    /// Modo efetivamente utilizado na execução.
    pub effective_mode: ExecutionMode,
    /// Separador sequencial configurado.
    pub separator: SequentialSeparator,
    /// Diretório relativo no nível do grupo.
    pub cwd: Option<String>,
    /// Ambiente no nível do grupo.
    pub env: BTreeMap<String, String>,
    /// Raiz canônica do projeto onde foi executado.
    pub project_root: String,
    /// Caminho do shell utilizado.
    pub shell_path: String,
    /// Dialeto do shell utilizado.
    pub shell_dialect: ShellDialect,
    /// Aba de origem (quando houver).
    pub origin_tab_id: Option<TabId>,
    /// Painel de origem (quando houver).
    pub origin_pane_id: Option<PaneId>,
    /// PTY de origem (quando houver).
    pub origin_pty_id: Option<PtyId>,
    /// Sessão de origem (quando houver).
    pub origin_session_id: Option<SessionId>,
    /// Geração do painel de origem (quando houver).
    pub origin_generation: Option<u64>,
}

/// Snapshot imutável de um item persistido em `command_group_run_items.snapshot_json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunItemSnapshot {
    /// ID do item na definição do grupo.
    pub source_item_id: GroupItemId,
    /// Posição 0-based.
    pub position: u32,
    /// Título do item.
    pub title: String,
    /// Comando shell original.
    pub command: String,
    /// Diretório relativo configurado no item/grupo.
    pub defined_cwd: Option<String>,
    /// Diretório absoluto canonicalizado efetivo.
    pub resolved_cwd: String,
    /// Variáveis de ambiente mescladas (`grupo -> item`).
    pub env: BTreeMap<String, String>,
    /// Flag de execução contínua.
    pub continuous: bool,
    /// Template selecionado (no modo Multi-Abas).
    pub template_id: Option<TemplateId>,
    /// Executável de shell resolvido para o item.
    pub shell_path: String,
    /// Dialeto do shell resolvido para o item.
    pub shell_dialect: ShellDialect,
}

/// Contadores derivados dos estados dos itens de uma execução.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunCounts {
    /// Total de itens na execução.
    pub total: u32,
    /// Quantidade em `pending`.
    pub pending: u32,
    /// Quantidade em `starting`.
    pub starting: u32,
    /// Quantidade em `running`.
    pub running: u32,
    /// Quantidade em `cancelling`.
    pub cancelling: u32,
    /// Quantidade em `succeeded`.
    pub succeeded: u32,
    /// Quantidade em `failed`.
    pub failed: u32,
    /// Quantidade em `skipped`.
    pub skipped: u32,
    /// Quantidade em `cancelled`.
    pub cancelled: u32,
    /// Quantidade em `interrupted`.
    pub interrupted: u32,
}

/// Estado detalhado de um item dentro de uma execução (`GroupRunItem`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunItem {
    /// Identificador único do item na execução.
    pub id: GroupRunItemId,
    /// Identificador da execução pai.
    pub run_id: GroupRunId,
    /// Posição 0-based.
    pub position: u32,
    /// Snapshot imutável do item.
    pub snapshot: GroupRunItemSnapshot,
    /// Estado atual do item.
    pub status: ItemStatus,
    /// Versão monotônica do item.
    pub version: u64,
    /// PTY associado (quando aplicável).
    pub pty_id: Option<PtyId>,
    /// Aba associada (Sequencial / Multi-Abas).
    pub tab_id: Option<TabId>,
    /// Painel associado (Sequencial / Multi-Abas).
    pub pane_id: Option<PaneId>,
    /// Sessão SQLite associada.
    pub session_id: Option<SessionId>,
    /// Bloco de comando OSC 133 correlacionado.
    pub command_block_id: Option<CommandBlockId>,
    /// Código de saída do processo.
    pub exit_code: Option<i32>,
    /// Sinal de encerramento, quando encerrado por sinal.
    pub signal: Option<i32>,
    /// Código de erro estruturado em caso de falha de lançamento/acompanhamento.
    pub error_code: Option<String>,
    /// Mensagem de erro sanitizada em `pt-BR`.
    pub error_message: Option<String>,
    /// Timestamp de início (epoch ms).
    pub started_at: Option<i64>,
    /// Timestamp de término (epoch ms).
    pub finished_at: Option<i64>,
    /// Indica se `output_tail` sofreu truncamento no limite de 256 KiB.
    pub output_truncated: bool,
}

/// Resumo de uma execução de grupo para listagem paginada (sem `output_tail` e sem lista de itens).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunSummary {
    /// Identificador único da execução.
    pub id: GroupRunId,
    /// Chave de idempotência original.
    pub request_id: GroupRequestId,
    /// ID do grupo (ou `None` se o grupo tiver sido excluído posteriormente).
    pub group_id: Option<CommandGroupId>,
    /// Projeto onde foi executado.
    pub project_id: ProjectId,
    /// Sessão de origem quando aplicável.
    pub origin_session_id: Option<SessionId>,
    /// Snapshot do grupo no lançamento.
    pub snapshot: GroupRunSnapshot,
    /// Modo efetivo da execução.
    pub mode: ExecutionMode,
    /// Estado agregado atual.
    pub status: RunStatus,
    /// Contadores agregados dos itens.
    pub counts: GroupRunCounts,
    /// Versão monotônica da execução (`seq` nos eventos).
    pub version: u64,
    /// Último código de saída observado.
    pub last_exit_code: Option<i32>,
    /// Timestamp de criação (epoch ms).
    pub created_at: i64,
    /// Timestamp de início efetivo (epoch ms).
    pub started_at: Option<i64>,
    /// Timestamp em que o cancelamento foi solicitado (epoch ms).
    pub cancel_requested_at: Option<i64>,
    /// Timestamp de conclusão total (epoch ms).
    pub finished_at: Option<i64>,
}

/// Detalhe completo de uma execução de grupo (`GroupRun`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRun {
    /// Identificador único da execução.
    pub id: GroupRunId,
    /// Chave de idempotência original.
    pub request_id: GroupRequestId,
    /// ID do grupo (ou `None` se excluído).
    pub group_id: Option<CommandGroupId>,
    /// Projeto onde foi executado.
    pub project_id: ProjectId,
    /// Sessão de origem quando aplicável.
    pub origin_session_id: Option<SessionId>,
    /// Snapshot imutável do grupo.
    pub snapshot: GroupRunSnapshot,
    /// Modo efetivo de execução.
    pub mode: ExecutionMode,
    /// Estado agregado atual.
    pub status: RunStatus,
    /// Contadores por estado de item.
    pub counts: GroupRunCounts,
    /// Versão monotônica da execução (`seq`).
    pub version: u64,
    /// Último código de saída observado.
    pub last_exit_code: Option<i32>,
    /// Timestamp de criação (epoch ms).
    pub created_at: i64,
    /// Timestamp de início (epoch ms).
    pub started_at: Option<i64>,
    /// Timestamp de pedido de cancelamento (epoch ms).
    pub cancel_requested_at: Option<i64>,
    /// Timestamp de conclusão (epoch ms).
    pub finished_at: Option<i64>,
    /// Lista ordenada de itens da execução.
    pub items: Vec<GroupRunItem>,
}

/// Página retornada por `command_group_run_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunListPage {
    /// Resumos de execução da página.
    pub items: Vec<GroupRunSummary>,
    /// Cursor opaco para a próxima página, se houver.
    pub next_cursor: Option<String>,
}

/// Códigos de erro canônicos da Fase 15 (`commands.group.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupErrorCode {
    /// Falha de validação de campos do grupo/item.
    Validation,
    /// Grupo, execução ou item não encontrado.
    NotFound,
    /// Conflito de nome (`name_key`) no mesmo escopo/projeto.
    NameConflict,
    /// Conflito de revisão otimista (`expected_revision`).
    RevisionConflict,
    /// Lista de IDs inválida/incompleta em `command_group_reorder`.
    ReorderMismatch,
    /// `project_id` ausente para operação que exige projeto.
    ProjectRequired,
    /// Tentativa de alterar escopo/projeto ou executar grupo local em outro projeto.
    ScopeMismatch,
    /// `cwd` absoluto ou que escapa da raiz do projeto/worktree.
    CwdOutsideProject,
    /// `cwd` relativo não existe no disco.
    CwdNotFound,
    /// Segredo literal detectado em comando ou variável de ambiente.
    SecretDetected,
    /// Executável de shell fora de `bash`, `zsh` ou `fish`.
    UnsupportedShell,
    /// Flag `env -C` indisponível no ambiente para override isolado de `cwd`.
    EnvCwdUnavailable,
    /// Painel ocupado, em alt-screen, sem prompt pronto ou com outro lease ativo.
    PaneBusy,
    /// Shell Integration (OSC 133) indisponível para correlação confiável.
    TrackingUnavailable,
    /// Nenhum template de ambiente Shell compatível com `continuous` encontrado.
    TemplateIncompatible,
    /// Mesmo `request_id` reutilizado com payload divergente.
    RequestConflict,
    /// Falha ao criar PTY headless.
    SpawnFailed,
    /// Falha ao abrir aba no `WorkspaceManager`.
    TabOpenFailed,
    /// Falha de persistência no SQLite.
    PersistenceFailed,
}

impl GroupErrorCode {
    /// Retorna o código estático `commands.group.*`.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Validation => "commands.group.validation",
            Self::NotFound => "commands.group.not_found",
            Self::NameConflict => "commands.group.name_conflict",
            Self::RevisionConflict => "commands.group.revision_conflict",
            Self::ReorderMismatch => "commands.group.reorder_mismatch",
            Self::ProjectRequired => "commands.group.project_required",
            Self::ScopeMismatch => "commands.group.scope_mismatch",
            Self::CwdOutsideProject => "commands.group.cwd_outside_project",
            Self::CwdNotFound => "commands.group.cwd_not_found",
            Self::SecretDetected => "commands.group.secret_detected",
            Self::UnsupportedShell => "commands.group.unsupported_shell",
            Self::EnvCwdUnavailable => "commands.group.env_cwd_unavailable",
            Self::PaneBusy => "commands.group.pane_busy",
            Self::TrackingUnavailable => "commands.group.tracking_unavailable",
            Self::TemplateIncompatible => "commands.group.template_incompatible",
            Self::RequestConflict => "commands.group.request_conflict",
            Self::SpawnFailed => "commands.group.spawn_failed",
            Self::TabOpenFailed => "commands.group.tab_open_failed",
            Self::PersistenceFailed => "commands.group.persistence_failed",
        }
    }

    /// Constrói um `MtError` com a mensagem em `pt-BR`.
    #[must_use]
    pub fn into_error(self, message: impl Into<String>) -> MtError {
        MtError::new(mt_core::ErrorCode(self.as_str()), message.into())
    }

    /// Constrói um `MtError` incluindo a lista de caminhos de campos afetados em `details.fields`.
    #[must_use]
    pub fn into_error_with_fields(
        self,
        message: impl Into<String>,
        fields: Vec<String>,
    ) -> MtError {
        MtError::new(mt_core::ErrorCode(self.as_str()), message.into())
            .with_details(serde_json::json!({ "fields": fields }))
    }
}
