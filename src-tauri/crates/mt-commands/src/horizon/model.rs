//! Modelos e DTOs tipados do subsistema Laravel Horizon.

use mt_core::{PaneId, ProjectId, PtyId};
use serde::{Deserialize, Serialize};

/// Contexto de execução e escopo do Horizon para um projeto.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HorizonContext {
    /// Identificador do projeto cadastrado no MaxTerminal.
    pub project_id: ProjectId,
    /// Raiz canônica do projeto no disco.
    pub root: String,
    /// Revisão monotônica do contexto gerada a partir dos parâmetros de configuração.
    pub context_revision: String,
    /// Impressão digital opaca do destino efetivo (Redis/conexões/ambiente).
    pub scope_id: String,
}

/// Estado de disponibilidade do pacote Laravel Horizon no projeto.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HorizonAvailability {
    /// Verificação em andamento.
    Checking,
    /// Horizon não instalado ou não configurado na raiz Laravel.
    Unavailable,
    /// Horizon disponível e pronto para monitoramento.
    Available,
    /// Erro ao inspecionar a instalação ou ambiente.
    Error,
}

/// Estado do runtime do Horizon (supervisionado ou externo).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HorizonStatus {
    /// Daemon Horizon parado.
    Stopped,
    /// Daemon Horizon iniciando no PTY supervisionado.
    Starting,
    /// Daemon Horizon ativo e saudável.
    Running,
    /// Daemon Horizon em processo de encerramento supervisionado.
    Stopping,
    /// Horizon ativo via processo externo (iniciado fora do MaxTerminal).
    External,
    /// Falha no daemon ou erro na sondagem.
    Error,
}

/// Identificação de conexão e fila monitorada pelo Horizon.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct QueueScope {
    /// Nome da conexão de fila (ex: "redis").
    pub connection: String,
    /// Nome da fila (ex: "default").
    pub queue: String,
}

/// Identificação da sessão de PTY e painel do daemon Horizon.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HorizonSessionDto {
    /// Identificador único do PTY no PtyManager.
    pub pty_id: PtyId,
    /// Identificador do painel reservado para a geração do terminal.
    pub pane_id: PaneId,
    /// Número sequencial da geração de runtime.
    pub generation: u64,
}

/// Snapshot consolidado das métricas e estado do Laravel Horizon.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HorizonSnapshot {
    /// Contexto autorizado do projeto.
    pub context: HorizonContext,
    /// Disponibilidade do Horizon no projeto.
    pub availability: HorizonAvailability,
    /// Versão detectada do pacote `laravel/horizon`, se presente.
    pub horizon_version: Option<String>,
    /// Lista de pares únicos (conexão, fila) resolvidos dos supervisores ativos.
    pub queues: Vec<QueueScope>,
    /// Ambiente resolvido do Laravel (ex: "local", "production").
    pub environment: Option<String>,
    /// Estado atual do runtime.
    pub status: HorizonStatus,
    /// Total de tarefas pendentes (prontas + agendadas nas filas monitoradas).
    pub pending: Option<u64>,
    /// Total de tarefas reservadas/em execução nas filas monitoradas.
    pub running: Option<u64>,
    /// Total de falhas no namespace Horizon respeitando a retenção.
    pub failed: Option<u64>,
    /// Timestamp Unix em milissegundos da última medição.
    pub sampled_at_ms: i64,
    /// Número de revisão incremental do snapshot.
    pub revision: u64,
    /// Diagnóstico sanitizado em caso de advertência ou falha parcial.
    pub diagnostic: Option<String>,
    /// Indica se o comando de iniciar está habilitado.
    pub can_start: bool,
    /// Indica se o comando de parar está habilitado.
    pub can_stop: bool,
    /// Indica se o comando de limpeza de pendentes está habilitado.
    pub can_clear_pending: bool,
    /// Sessão de PTY supervisionada associada à geração atual, se houver.
    pub session: Option<HorizonSessionDto>,
}

/// Estimativa e token de confirmação para limpeza exclusiva de pendentes.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HorizonClearPreview {
    /// Token opaco temporário de confirmação de uso único.
    pub token: String,
    /// Revisão do contexto esperada na confirmação.
    pub context_revision: String,
    /// Impressão digital opaca do destino.
    pub scope_id: String,
    /// Filas que serão limpas.
    pub queues: Vec<QueueScope>,
    /// Estimativa de tarefas pendentes (ready + delayed) a serem removidas.
    pub estimated_pending: u64,
    /// Timestamp Unix em milissegundos de expiração da confirmação (máx 30 s).
    pub expires_at_ms: i64,
}

/// Contagem de tarefas removidas por fila.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueClearCount {
    /// Conexão de fila.
    pub connection: String,
    /// Nome da fila.
    pub queue: String,
    /// Quantidade de tarefas retiradas.
    pub cleared: u64,
}

/// Resultado consolidado da limpeza exclusiva de tarefas pendentes.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HorizonClearResult {
    /// Detalhamento das remoções por fila.
    pub cleared_per_queue: Vec<QueueClearCount>,
    /// Quantidade total de tarefas retiradas.
    pub total_cleared: u64,
    /// Indica se a operação foi parcial devido ao orçamento de tempo/quantidade.
    pub is_partial: bool,
    /// Aviso sanitizado se ocorreu inconsistência de metadados em conexão secundária.
    pub warning: Option<String>,
}
