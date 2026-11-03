//! Tipos e contratos do protocolo de Issues e Ciclo Automatizado de Bugs (`issues_protocol_v1` e `bugs_cycle_v1` — Subetapa 11/06).

use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

/// Ação unitária solicitada sobre uma issue do GitHub enviada ao `MaxCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueCommandKind {
    /// Abre a issue no painel do `MaxCode` (`issue.open`).
    Open,
    /// Inicia análise e criação de plano (`BUG PLAN` / `issue.plan`).
    Plan,
    /// Inicia execução de correção (`BUG FIX` / `issue.fix`).
    Fix,
}

impl IssueCommandKind {
    /// Rota correspondente na URI `vscode://johnattas.maxcode/issue/<rota>`.
    #[must_use]
    pub const fn uri_action_segment(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Plan => "plan",
            Self::Fix => "fix",
        }
    }
}

/// Estágio de workflow do `MaxCode` associado ao comando da issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueStage {
    /// Apenas visualização/abertura.
    Inspect,
    /// Modo de planejamento (`BUG PLAN`).
    BugPlan,
    /// Modo de correção (`BUG FIX`).
    BugFix,
}

/// Payload de comando unitário de issue (`issue.command` — `MaxTerminal` → `MaxCode`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueCommandPayload {
    /// Tipo de ação solicitada (`open`, `plan`, `fix`).
    pub kind: IssueCommandKind,
    /// Caminho absoluto do repositório local do projeto.
    pub project_path: String,
    /// Repositório GitHub no formato `"owner/repo"`.
    pub repo_full_name: String,
    /// Número da issue (`>= 1`).
    pub issue_number: u64,
    /// Título sanitizado da issue (até 512 bytes).
    pub issue_title: String,
    /// URL HTML opcional da issue no GitHub.
    pub issue_url: Option<String>,
    /// Labels atualmente aplicadas na issue.
    pub labels: Vec<String>,
    /// Estágio alvo no `MaxCode`.
    pub stage: IssueStage,
    /// Se `true`, solicita que a janela do VS Code / `MaxCode` ganhe foco.
    pub focus_editor: bool,
}

/// Status retornado pelo `MaxCode` ao confirmar um `IssueCommandPayload`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueAckStatus {
    /// Ação aceita e iniciada imediatamente.
    Accepted,
    /// Ação enfileirada na fila do `MaxCode`.
    Queued,
    /// Ação rejeitada (ver `error`).
    Rejected,
}

/// Confirmação de comando unitário de issue (`issue.command_ack`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueCommandAckPayload {
    /// ID da requisição original.
    pub request_id: Uuid,
    /// Status de aceitação.
    pub status: IssueAckStatus,
    /// Identificador opcional da conversa aberta/associada no `MaxCode`.
    pub conversation_id: Option<String>,
    /// Caminho da worktree criada/associada pelo `MaxCode`, se houver.
    pub worktree_path: Option<String>,
    /// Mensagem de erro caso `status == Rejected`.
    pub error: Option<String>,
}

/// Filtros opcionais para seleção das issues elegíveis no ciclo de bugs em lote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BugsCycleFilters {
    /// Filtrar apenas issues que possuam ao menos uma destas labels (ex.: `["Bug"]`).
    pub include_labels: Vec<String>,
    /// Ignorar issues que possuam qualquer uma destas labels.
    pub exclude_labels: Vec<String>,
    /// Lista explícita opcional de números de issues selecionadas.
    pub explicit_issue_numbers: Vec<u64>,
    /// Limite máximo opcional de issues processadas na execução.
    pub max_issues: Option<u32>,
}

/// Requisição `MaxTerminal` → `MaxCode` para iniciar ou simular o ciclo automatizado de bugs (`bugs.cycle_start`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BugsCycleStartPayload {
    /// Identificador único do ciclo (`UUID v4`).
    pub cycle_id: Uuid,
    /// Caminho absoluto do repositório local do projeto.
    pub project_path: String,
    /// Repositório GitHub no formato `"owner/repo"`.
    pub repo_full_name: String,
    /// Números das issues candidatas ordenadas.
    pub issue_numbers: Vec<u64>,
    /// Filtros aplicados na seleção.
    pub filters: BugsCycleFilters,
    /// Se `true`, apenas calcula a prévia (`elegíveis` vs `puladas`) sem iniciar mutações.
    pub dry_run: bool,
}

/// Item ignorado/pulado na prévia ou início do ciclo de bugs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BugsCycleSkipEntry {
    /// Número da issue ignorada.
    pub issue_number: u64,
    /// Motivo amigável pelo qual a issue foi ignorada (ex.: `"Já possui label Checked Approved"`).
    pub reason: String,
}

/// Confirmação do disparo ou prévia do ciclo de bugs (`bugs.cycle_ack`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BugsCycleAckPayload {
    /// ID da requisição original.
    pub request_id: Uuid,
    /// ID do ciclo confirmado.
    pub cycle_id: Uuid,
    /// Indica se o ciclo foi aceito pelo `MaxCode`.
    pub accepted: bool,
    /// Lista ordenada de números de issues enfileiradas para execução.
    pub queued_issues: Vec<u64>,
    /// Lista de issues ignoradas e seus motivos.
    pub skipped_issues: Vec<BugsCycleSkipEntry>,
    /// Mensagem de erro caso `accepted == false`.
    pub error: Option<String>,
}

/// Status global de um ciclo automatizado de bugs em andamento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BugCycleOverallStatus {
    /// Ciclo em execução normal.
    Running,
    /// Ciclo pausado aguardando intervenção.
    Paused,
    /// Ciclo concluído com sucesso.
    Completed,
    /// Ciclo cancelado pelo usuário.
    Cancelled,
    /// Ciclo interrompido por falha crítica.
    Failed,
}

/// Etapa individual do pipeline de resolução de bugs do `MaxCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BugCycleItemStage {
    /// Deduplicação de issues similares.
    Deduplicating,
    /// Criação do plano de implementação (`Waiting Plan` → `Planned`).
    Planning,
    /// Execução das alterações na worktree (`Waiting Execute` → `Executed`).
    Executing,
    /// Verificação de testes e critérios (`Waiting Check` → `Checked Approved`).
    Checking,
    /// Concluída.
    Done,
}

/// Status do passo atual dentro de uma etapa do ciclo de bugs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BugCycleStepStatus {
    /// Iniciado / em progresso.
    InProgress,
    /// Etapa concluída com sucesso.
    Succeeded,
    /// Etapa pulada.
    Skipped,
    /// Etapa falhou.
    Failed,
}

/// Evento de progresso em tempo real emitido pelo `MaxCode` durante o ciclo de bugs (`bugs.cycle_progress`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BugCycleProgressPayload {
    /// Identificador do ciclo.
    pub cycle_id: Uuid,
    /// Caminho absoluto do projeto.
    pub project_path: String,
    /// Status global do ciclo.
    pub overall_status: BugCycleOverallStatus,
    /// Issue sendo processada no momento, se houver.
    pub current_issue_number: Option<u64>,
    /// Etapa atual da issue em processamento.
    pub stage: BugCycleItemStage,
    /// Status do passo atual.
    pub step_status: BugCycleStepStatus,
    /// Quantidade de issues já concluídas no ciclo.
    pub completed_count: u32,
    /// Total de issues enfileiradas no ciclo.
    pub total_count: u32,
    /// Label canônica resultante aplicada à issue, se houve transição.
    pub updated_label: Option<String>,
    /// Mensagem descritiva do progresso atual.
    pub message: Option<String>,
}

/// Resultado de entrega retornado pelos helpers de despacho de ações de issue da Fase 11/06.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum IssueActionDispatchOutcome {
    /// Entregue via loopback WebSocket/HTTP com ACK confirmado pelo `MaxCode`.
    Loopback {
        /// Dados do ACK retornado pelo `MaxCode`.
        ack: IssueCommandAckPayload,
    },
    /// Entregue via fallback de URI `vscode://johnattas.maxcode/issue/...`.
    UriFallback {
        /// URI disparada no sistema operacional.
        uri: String,
    },
}

/// Capacidades do protocolo de Issues suportadas pela instância conectada do `MaxCode`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueProtocolSupport {
    /// `true` se o peer anunciou `issues_protocol_v1`.
    pub issues_protocol_v1: bool,
    /// `true` se o peer anunciou `bugs_cycle_v1`.
    pub bugs_cycle_v1: bool,
}
