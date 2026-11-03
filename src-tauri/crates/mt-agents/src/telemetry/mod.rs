//! Tipos e contratos do motor de telemetria multi-fonte (`StatusLine`) por execução de agente.

pub mod aggregator;
pub mod claude_statusline;
pub mod claude_transcript;
pub mod codex_session;
pub mod output_heuristic;
pub mod tail;

use std::collections::BTreeMap;
use std::path::PathBuf;

use async_trait::async_trait;
use mt_core::{PaneId, SessionId, TabId};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::ports::RunBinding;
use crate::registry::AgentKind;

pub use aggregator::{TelemetryAggregator, TelemetrySink};
pub use claude_statusline::ClaudeStatusLineParser;
pub use claude_transcript::ClaudeTranscriptSource;
pub use codex_session::CodexSessionSource;
pub use output_heuristic::OutputHeuristicSource;
pub use tail::{JsonlTail, TailReadBatch};

/// Fonte que produziu uma observação de telemetria.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum TelemetrySourceKind {
    /// Hook JSON estruturado da `statusLine` do Claude Code (prioridade máxima).
    ClaudeStatusLine,
    /// Arquivo JSONL de transcript da sessão do Claude Code.
    ClaudeTranscript,
    /// Arquivo JSONL de rollout da sessão do Codex correlacionado por `thread-id`.
    CodexSession,
    /// Heurística opt-in sobre a saída do PTY.
    OutputHeuristic,
    /// Modelo configurado na abertura da aba/template.
    Configured,
}

impl TelemetrySourceKind {
    /// Prioridade estrita para desempate por campo (maior vence):
    /// `ClaudeStatusLine` (40) > `ClaudeTranscript` / `CodexSession` (30) > `OutputHeuristic` (20) > `Configured` (10).
    #[must_use]
    pub const fn priority(&self) -> u8 {
        match self {
            Self::ClaudeStatusLine => 40,
            Self::ClaudeTranscript | Self::CodexSession => 30,
            Self::OutputHeuristic => 20,
            Self::Configured => 10,
        }
    }
}

/// Qualidade epistemológica de uma medição de telemetria.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum MeasurementQuality {
    /// Reportado diretamente por canal estruturado oficial do agente.
    Reported,
    /// Derivado matematicamente de contadores explícitos em arquivo de sessão validado.
    Derived,
    /// Estimado por heurística opt-in.
    Estimated,
    /// Configurado estaticamente na abertura da aba (não observado em runtime).
    Configured,
}

/// Chave de métrica monitorada na `StatusLine`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum MetricKey {
    /// Modelo ativo ou configurado.
    Model,
    /// Ocupação da janela de contexto.
    Context,
    /// Consumo da cota de 5 horas (`300 min`).
    #[serde(rename = "quota5h")]
    Quota5h,
    /// Consumo da cota semanal (`10080 min`).
    QuotaWeekly,
    /// Custo acumulado da sessão em USD.
    CostUsd,
}

impl MetricKey {
    /// Todas as 5 métricas monitoradas.
    pub const ALL: [Self; 5] = [
        Self::Model,
        Self::Context,
        Self::Quota5h,
        Self::QuotaWeekly,
        Self::CostUsd,
    ];
}

/// Estado de disponibilidade de uma métrica individual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum MetricAvailability {
    /// Dado disponível e recente.
    Available,
    /// Métrica não suportada pelo agente.
    Unsupported,
    /// Métrica sem contrato documentado validado.
    Unverified,
    /// Coleta desativada nas configurações.
    Disabled,
    /// Suportado, porém aguardando primeira emissão do agente.
    AwaitingData,
    /// Último payload recebido continha valor fora de domínio (`NaN`, `< 0`, `> 100`).
    Invalid,
    /// Última medição excedeu `stale_after_s` (padrão 60 s).
    Stale,
}

/// Metadados de disponibilidade por métrica.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MetricState {
    /// Disponibilidade atual.
    pub availability: MetricAvailability,
    /// Justificativa legível em `pt-BR`.
    pub reason: Option<String>,
    /// Timestamp UNIX em ms da verificação.
    pub checked_at_ms: i64,
}

/// Envelope de um campo de telemetria com proveniência, qualidade e timestamps próprios.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Field<T> {
    /// Valor observado.
    pub value: T,
    /// Fonte que produziu o valor.
    pub source: TelemetrySourceKind,
    /// Timestamp UNIX em ms em que o dado foi produzido/observado.
    pub observed_at_ms: i64,
    /// Timestamp UNIX em ms em que o agregador recebeu o dado.
    pub received_at_ms: i64,
    /// Qualidade da medição.
    pub quality: MeasurementQuality,
    /// Timestamp UNIX em ms opcional de expiração do valor.
    pub expires_at_ms: Option<i64>,
}

/// Identificação do modelo reportado ou configurado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// Identificador técnico do modelo (ex.: `"claude-opus-4-6"`).
    pub id: Option<String>,
    /// Nome legível para exibição (ex.: `"Opus 4.6"`).
    pub display_name: String,
}

/// Ocupação da janela de contexto do modelo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    /// Quantidade de tokens utilizados na janela atual.
    pub used_tokens: Option<u64>,
    /// Capacidade total da janela de contexto em tokens.
    pub window_tokens: Option<u64>,
    /// Percentual utilizado (`0.0..=100.0`).
    pub percent: f32,
}

/// Consumo de uma janela de cota de uso (5h ou semanal).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// Percentual consumido da cota (`0.0..=100.0`).
    pub used_percent: f32,
    /// Timestamp UNIX em ms em que a janela reinicia.
    pub resets_at_ms: Option<i64>,
    /// Duração da janela em minutos (`300` para 5h, `10080` para semanal).
    pub window_minutes: u32,
}

/// Snapshot consolidado de telemetria de uma aba de agente (`agent://telemetry`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTelemetry {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Identificador do painel.
    pub pane_id: PaneId,
    /// Identificador da sessão SQLite.
    pub session_id: SessionId,
    /// Identificador único da execução do agente.
    pub run_id: String,
    /// Revisão monotônica por execução (`>= 1`).
    pub revision: u64,
    /// Tipo do agente CLI.
    pub agent: AgentKind,
    /// Referência da sessão externa (ex.: `session_id` do Claude ou `thread-id` do Codex).
    pub session_ref: Option<String>,
    /// Modelo ativo ou configurado.
    pub model: Option<Field<ModelInfo>>,
    /// Ocupação da janela de contexto.
    pub context: Option<Field<ContextUsage>>,
    /// Consumo da cota de 5 horas.
    #[serde(rename = "quota5h")]
    pub quota_5h: Option<Field<QuotaWindow>>,
    /// Consumo da cota semanal.
    pub quota_weekly: Option<Field<QuotaWindow>>,
    /// Custo acumulado em USD.
    pub cost_usd: Option<Field<f64>>,
    /// Estado de disponibilidade de cada métrica.
    pub availability: BTreeMap<MetricKey, MetricState>,
    /// Indica se o processo do agente ainda está ativo na aba.
    pub agent_active: bool,
    /// Timestamp UNIX em ms da última atualização do snapshot.
    pub updated_at_ms: i64,
}

/// Semântica de atualização individual por campo em `TelemetryUpdate`.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldUpdate<T> {
    /// A fonte não enviou alteração para este campo nesta mensagem.
    Unchanged,
    /// A fonte limpou explicitamente o campo (ex.: `null` ou compactação) ou reportou dado inválido.
    Clear {
        /// Motivo opcional da limpeza/invalidação.
        reason: Option<String>,
        /// Se `true`, marca a disponibilidade como `Invalid` em vez de `AwaitingData`.
        invalid: bool,
    },
    /// A fonte observou um novo valor válido.
    Set {
        /// Valor observado.
        value: T,
        /// Qualidade da medição.
        quality: MeasurementQuality,
        /// Expiração opcional do valor em ms.
        expires_at_ms: Option<i64>,
    },
}

/// Lote de atualizações enviado por uma fonte de telemetria ao `TelemetryAggregator`.
#[derive(Debug, Clone)]
pub struct TelemetryUpdate {
    /// Identidade da execução proprietária.
    pub binding: RunBinding,
    /// Fonte que produziu a atualização.
    pub source: TelemetrySourceKind,
    /// Atualização do modelo.
    pub model: FieldUpdate<ModelInfo>,
    /// Atualização da janela de contexto.
    pub context: FieldUpdate<ContextUsage>,
    /// Atualização da cota de 5 horas.
    pub quota_5h: FieldUpdate<QuotaWindow>,
    /// Atualização da cota semanal.
    pub quota_weekly: FieldUpdate<QuotaWindow>,
    /// Atualização do custo em USD.
    pub cost_usd: FieldUpdate<f64>,
    /// Identificador externo de sessão observado.
    pub session_ref: Option<String>,
    /// Se `true`, limpa todos os candidatos anteriores de `context` de qualquer fonte.
    pub invalidate_previous_context: bool,
    /// Timestamp UNIX em ms da observação.
    pub observed_at_ms: i64,
}

/// Dica de correlação de sessão recebida via relay (`transcript_path` ou `thread-id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHint {
    /// Identificador da sessão externa (`session_id` ou `thread-id`).
    pub session_id: String,
    /// Caminho do arquivo de transcript JSONL, quando informado pelo agente.
    pub transcript_path: Option<PathBuf>,
}

/// Contexto passado para execução de um `TelemetrySource` em background.
pub struct TelemetryContext {
    /// Execução monitorada.
    pub binding: RunBinding,
    /// Versão do binário do agente.
    pub binary_version: Option<String>,
    /// Canal watch com a dica de sessão correlacionada.
    pub session_hint: watch::Receiver<Option<SessionHint>>,
}

/// Trait para fontes assíncronas de telemetria de longa duração.
#[async_trait]
pub trait TelemetrySource: Send + Sync {
    /// Tipo da fonte.
    fn kind(&self) -> TelemetrySourceKind;
    /// Verifica se a fonte suporta o agente informado.
    fn supports(&self, agent: AgentKind) -> bool;
    /// Loop assíncrono de coleta até o cancelamento da execução.
    async fn run(
        &self,
        ctx: TelemetryContext,
        sink: mpsc::Sender<TelemetryUpdate>,
        cancel: CancellationToken,
    );
}
