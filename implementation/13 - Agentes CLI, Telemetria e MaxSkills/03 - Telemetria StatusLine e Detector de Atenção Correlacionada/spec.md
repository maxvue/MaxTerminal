# Implementação 03 — Telemetria `StatusLine` e Detector de Atenção Correlacionada (`spec.md`)

> Fase: **13 — Agentes CLI, Telemetria e MaxSkills**
> Referências: [`../instructions.md`](../instructions.md) (I-08, I-10, I-11, I-12; CA-17, CA-18, CA-20, CA-22, CA-23, CA-24, CA-25, CA-34) e [`../spec.md`](../spec.md) (§2.9, §2.10, §3, §4, §7.1–§7.4, §9).

---

## 1. Contexto, Objetivo e Escopo

Esta terceira unidade implementa o motor de telemetria multi-fonte para a barra da aba (`StatusLine` — modelo ativo, % de ocupação da janela de contexto, cotas de 5h e semanal e custo) e o detector de pausa por permissão/input com integração às Fases 05 e 09:

1. **Tailing Incremental e Seguro de JSONL (`telemetry/tail.rs`)**:
   - Leitura inicial limitada aos últimos 256 KiB do arquivo (descartando a primeira linha parcial), leitura incremental por offset via `notify`, limite de 64 KiB por linha, descarte de JSON inválido sem erro fatal (CA-18) e detecção de truncamento/mudança de `inode` para reiniciar o parser e invalidar dados antigos (CA-34).
2. **Fontes de Telemetria (`telemetry/*`) e `TelemetryAggregator`**:
   - `ClaudeStatusLineSource`: consome mensagens `RelayMessage::Statusline` autenticadas pelo token da execução (`model.id`, `model.display_name`, `context_window.used_percentage`, `rate_limits.five_hour`, `rate_limits.seven_day`, `cost.total_cost_usd`, `session_id`, `transcript_path`).
   - `ClaudeTranscriptSource`: tailing do `transcript_path` somente quando correlacionado pelo relay/sessão e com versão validada; deriva ocupação de contexto apenas quando houver denominador explícito e `usage` (`input_tokens + cache_creation_input_tokens + cache_read_input_tokens`), limpando o valor quando ocorrer compactação ou `current_usage == null`.
   - `CodexSessionSource`: correlacionado estritamente pelo `thread-id` recebido via `codex-notify` (nunca por "arquivo mais recente no mesmo cwd"); extrai tokens e janelas explícitas de 300 min (5h) e 10 080 min (semanal).
   - `OutputHeuristicSource`: fonte opt-in com qualidade `Estimated`.
   - `TelemetryAggregator`: mantém candidatos por `(campo, fonte)`, aplica precedência (`Reported` estruturado → `Derived` arquivo validado → `Estimated` opt-in → `Configured`), respeita `FieldUpdate::{Unchanged, Clear, Set}` (nunca ignorando `null` explícito — CA-34), marca campos sem observação por > 60 s como `stale`, aplica throttle trailing de ≤ 1 Hz por aba (CA-20) e persiste snapshots sanitizados em `agent_telemetry_snapshots`.
3. **Detector de Atenção (`attention/`)**:
   - Máquina de estados por execução (`Inactive`, `Working`, `Suspected`, `Awaiting`) com rastreamento de episódio (`episode: u64`).
   - Confirmação de espera (`AwaitingApproval` / `AwaitingInput`) exclusivamente por fonte nativa correlacionada (`Notification` do Claude Code com `notification_type = "permission_prompt"` ou `"idle_prompt"`), acionando `TabStatePort::attention_required` (`TabStatus = 'error'`, `seen = false`) e `NotifyPort` (destaque no tray e 1 notificação nativa por episódio quando a janela não estiver focada com a aba ativa — CA-23).
   - Separação estrita entre espera de permissão e fim de turno (`Stop` do Claude e `agent-turn-complete` do Codex nunca geram `error`).
   - Indícios heurísticos (`Suspected`) via `AttentionTap` (`OutputTap` com `TailBuffer` de 8 KiB sem ANSI, parser incremental de OSC 9 / OSC 777 / BEL e regex de duas âncoras em `patterns/default.toml` avaliados após `silence_ms`).
   - Reconhecimento visual (`agent_attention_ack` / `UserInput`) vs resolução real da espera (`UserPromptSubmit`, pós-ferramenta correlacionado, `CommandFinished` ou saída do PTY — CA-24).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Criados
- `src-tauri/crates/mt-agents/src/telemetry/mod.rs`
- `src-tauri/crates/mt-agents/src/telemetry/tail.rs`
- `src-tauri/crates/mt-agents/src/telemetry/aggregator.rs`
- `src-tauri/crates/mt-agents/src/telemetry/claude_statusline.rs`
- `src-tauri/crates/mt-agents/src/telemetry/claude_transcript.rs`
- `src-tauri/crates/mt-agents/src/telemetry/codex_session.rs`
- `src-tauri/crates/mt-agents/src/telemetry/output_heuristic.rs`
- `src-tauri/crates/mt-agents/src/attention/mod.rs`
- `src-tauri/crates/mt-agents/src/attention/patterns.rs`
- `src-tauri/crates/mt-agents/src/attention/detector.rs`
- `src-tauri/crates/mt-agents/src/patterns/default.toml`
- `src-tauri/crates/mt-agents/tests/telemetry.rs`
- `src-tauri/crates/mt-agents/tests/attention.rs`
- `src-tauri/crates/mt-agents/tests/fixtures/claude/statusline.json`
- `src-tauri/crates/mt-agents/tests/fixtures/claude/transcript_sample.jsonl`
- `src-tauri/crates/mt-agents/tests/fixtures/codex/rollout_sample.jsonl`
- `src-tauri/crates/mt-agents/tests/fixtures/attention/positive_prompts.txt`
- `src-tauri/crates/mt-agents/tests/fixtures/attention/negative_outputs.txt`

### 2.2. Arquivos Modificados
- `src-tauri/crates/mt-agents/src/lib.rs` — exposição de `telemetry` e `attention` e integração com o loop de eventos do `RelayServer`.
- `src-tauri/src/agents/adapters.rs` — implementação concreta de `TabStatePort` (Fase 05), `NotifyPort` (Fase 09) e `TelemetrySink` (`AppHandle::emit("agent://telemetry", ...)`).
- `src-tauri/src/commands/agents.rs` — adição de `agent_telemetry_get`, `agent_telemetry_list`, `agent_attention_get` e `agent_attention_ack`.
- `src-tauri/src/specta.rs` — registro dos novos comandos e dos eventos `agent://telemetry` e `agent://attention`.

---

## 3. Especificação Técnica Detalhada

### 3.1. Tipos e Contratos de Telemetria (`crates/mt-agents/src/telemetry/mod.rs`)

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;
use async_trait::async_trait;
use mt_core::{PaneId, SessionId, TabId};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use crate::ports::RunBinding;
use crate::registry::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TelemetrySourceKind {
    ClaudeStatusLine,
    ClaudeTranscript,
    CodexSession,
    OutputHeuristic,
    Configured,
}

impl TelemetrySourceKind {
    /// Prioridade estrita para desempate por campo (maior vence):
    /// ClaudeStatusLine (40) > ClaudeTranscript / CodexSession (30) > OutputHeuristic (20) > Configured (10)
    pub fn priority(&self) -> u8;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum MeasurementQuality {
    Reported,
    Derived,
    Estimated,
    Configured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum MetricKey {
    Model,
    Context,
    #[serde(rename = "quota5h")]
    Quota5h,
    QuotaWeekly,
    CostUsd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum MetricAvailability {
    Available,
    Unsupported,
    Unverified,
    Disabled,
    AwaitingData,
    Invalid,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MetricState {
    pub availability: MetricAvailability,
    pub reason: Option<String>,
    pub checked_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Field<T> {
    pub value: T,
    pub source: TelemetrySourceKind,
    pub observed_at_ms: i64,
    pub received_at_ms: i64,
    pub quality: MeasurementQuality,
    pub expires_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: Option<String>,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub used_tokens: Option<u64>,
    pub window_tokens: Option<u64>,
    pub percent: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub used_percent: f32,
    pub resets_at_ms: Option<i64>,
    pub window_minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTelemetry {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub session_id: SessionId,
    pub run_id: String,
    pub revision: u64,
    pub agent: AgentKind,
    pub session_ref: Option<String>,
    pub model: Option<Field<ModelInfo>>,
    pub context: Option<Field<ContextUsage>>,
    #[serde(rename = "quota5h")]
    pub quota_5h: Option<Field<QuotaWindow>>,
    pub quota_weekly: Option<Field<QuotaWindow>>,
    pub cost_usd: Option<Field<f64>>,
    pub availability: BTreeMap<MetricKey, MetricState>,
    pub agent_active: bool,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldUpdate<T> {
    Unchanged,
    Clear { reason: Option<String> },
    Set { value: T, quality: MeasurementQuality, expires_at_ms: Option<i64> },
}

#[derive(Debug, Clone)]
pub struct TelemetryUpdate {
    pub binding: RunBinding,
    pub source: TelemetrySourceKind,
    pub model: FieldUpdate<ModelInfo>,
    pub context: FieldUpdate<ContextUsage>,
    pub quota_5h: FieldUpdate<QuotaWindow>,
    pub quota_weekly: FieldUpdate<QuotaWindow>,
    pub cost_usd: FieldUpdate<f64>,
    pub session_ref: Option<String>,
    pub invalidate_previous_context: bool,
    pub observed_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHint {
    pub session_id: String,
    pub transcript_path: Option<PathBuf>,
}

pub struct TelemetryContext {
    pub binding: RunBinding,
    pub binary_version: Option<String>,
    pub session_hint: watch::Receiver<Option<SessionHint>>,
}

#[async_trait]
pub trait TelemetrySource: Send + Sync {
    fn kind(&self) -> TelemetrySourceKind;
    fn supports(&self, agent: AgentKind) -> bool;
    async fn run(
        &self,
        ctx: TelemetryContext,
        sink: mpsc::Sender<TelemetryUpdate>,
        cancel: CancellationToken,
    );
}
```

---

### 3.2. `JsonlTail` (`crates/mt-agents/src/telemetry/tail.rs`)

```rust
use std::path::PathBuf;

pub struct JsonlTail {
    path: PathBuf,
    offset: u64,
    dev_ino: Option<(u64, u64)>,
    partial: Vec<u8>,
}

#[derive(Debug)]
pub struct TailReadBatch {
    pub records: Vec<serde_json::Value>,
    pub reset_detected: bool,
}

impl JsonlTail {
    pub const INITIAL_WINDOW: u64 = 256 * 1024;
    pub const MAX_LINE_BYTES: usize = 64 * 1024;

    pub async fn open_from_end(path: PathBuf) -> std::io::Result<Self>;
    pub async fn read_new(&mut self) -> std::io::Result<TailReadBatch>;
}
```

#### Regras de `JsonlTail`:
- Em `open_from_end`: verifica se o caminho é arquivo regular (`metadata.is_file()`) e pertence ao mesmo UID do usuário (`MetadataExt::uid() == geteuid()`). Se `len > INITIAL_WINDOW`, posiciona `offset = len - INITIAL_WINDOW` e descarta os bytes até o primeiro `\n` para não parsear um fragmento inicial truncado.
- Em `read_new`: compara `(metadata.dev(), metadata.ino())` com `self.dev_ino` e verifica se `metadata.len() < self.offset`. Se o `inode` mudou ou o arquivo encolheu (truncamento/rotação), limpa `self.partial`, reposiciona `self.offset = 0` e define `reset_detected = true` (CA-34).
- Linhas com mais de `MAX_LINE_BYTES` (64 KiB) ou JSON malformado são descartadas sem interromper a leitura das linhas seguintes (CA-18).

---

### 3.3. `TelemetryAggregator` (`crates/mt-agents/src/telemetry/aggregator.rs`)

Regras de agregação, validade e throttle (`spec.md` §7.1):
1. **Identidade de execução**: cada aba mantém os candidatos indexados pelo `run_id` ativo. Atualizações de um `run_id` encerrado ou com `observed_at_ms` regressivo para a mesma `(fonte, campo)` são descartadas.
2. **Semântica `Unchanged` vs `Clear` vs `Set`**:
   - Em `ClaudeStatusLineSource` (snapshot completo do statusline JSON), um campo ausente ou `null` no payload gera `FieldUpdate::Clear` para a fonte `ClaudeStatusLine`, removendo o candidato daquela fonte imediatamente (CA-34).
   - Se `invalidate_previous_context == true` (ex.: compactação de contexto, truncamento de JSONL ou mudança de modelo ativo), todos os candidatos de `context` anteriores são limpos.
3. **Validação de valores (nunca fabricar zero)**:
   - Percentuais (`context.percent`, `quota.used_percent`) devem ser finitos (`f32::is_finite()`) e estar em `0.0..=100.0`; valores inválidos/NaN/negativos viram `FieldUpdate::Clear` com `MetricAvailability::Invalid`.
   - `resets_at` do Claude Code em segundos epoch é convertido para ms via `checked_mul(1000)`. Se `now_ms >= resets_at_ms`, a janela de cota expirou e é limpa até chegar nova medição (nunca resetada artificialmente para `0%`).
4. **Envelhecimento (`stale`) e expiração**:
   - `stale_after_s` (padrão 60 s): quando `now_ms - field.observed_at_ms > stale_after_s * 1000`, `availability[key].availability` passa para `MetricAvailability::Stale` mantendo o último valor conhecido com seu `observed_at_ms` original (CA-19).
   - TTL máximo de medição: 300 s (ou `resets_at_ms`, o que vier primeiro).
5. **Throttle de 1 Hz (CA-20)**:
   - `apply` atualiza o estado em memória imediatamente (`snapshot(tab_id)` sempre reflete o dado mais recente).
   - A emissão do evento `agent://telemetry` respeita janela mínima de `1000 ms` por `tab_id` com timer *trailing*: se chegarem 50 atualizações em 1 segundo, emite a primeira imediatamente e agenda 1 única emissão consolidada no fim da janela de 1 s.
6. **Persistência de Snapshot**:
   - Grava `AgentTelemetry` sanitizado em `agent_telemetry_snapshots` no máximo a cada 30 s por aba e ao encerrar/remover a aba. Ao hidratar do banco no startup, marca `agent_active = false`.

---

### 3.4. Detector de Atenção (`crates/mt-agents/src/attention/`)

```rust
// attention/mod.rs
use mt_core::{PaneId, ProjectId, SessionId, TabId};
use serde::{Deserialize, Serialize};
use crate::ports::AttentionKind;
use crate::registry::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AttentionSource {
    NativeHook,
    CodexNotify,
    OscNotification,
    Pattern,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum AttentionState {
    Inactive,
    Working,
    Suspected {
        kind: AttentionKind,
        since_ms: i64,
        source: AttentionSource,
        pattern_id: Option<String>,
    },
    Awaiting {
        kind: AttentionKind,
        episode: u64,
        since_ms: i64,
        source: AttentionSource,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttentionSnapshot {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub project_id: ProjectId,
    pub session_id: SessionId,
    pub run_id: String,
    pub agent: AgentKind,
    pub revision: u64,
    pub acknowledged: bool,
    pub state: AttentionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AckReason {
    UserInput,
    Dismissed,
}
```

#### Regras da Máquina de Estados de Atenção (`detector.rs`):
1. **Confirmação de Espera (`Awaiting`)**:
   - Somente `AttentionSignal::HookNotification` do Claude Code (autenticado pelo token da execução raiz) com `notification_type == Some("permission_prompt")` → `AttentionState::Awaiting { kind: AwaitingApproval, episode, ... }` ou `notification_type == Some("idle_prompt")` → `AttentionState::Awaiting { kind: AwaitingInput, episode, ... }`.
   - `notification_type == Some("auth_success")` ou tipos desconhecidos são ignorados (CA-23).
   - Ao entrar em um novo episódio `Awaiting`:
     - Incrementa `episode += 1`, define `acknowledged = false`.
     - Invoca `TabStatePort::attention_required(&run, episode)` (colocando a aba em `TabStatus::Error` com `seen = false`) e `NotifyPort::set_attention(tab_id, kind)`.
     - Se `settings.notify == true` e `!NotifyPort::is_tab_focused_and_active(tab_id)`, dispara **uma única vez por episódio** `NotifyPort::notify(tab_id, title, body)` com texto local seguro (ex.: `"Claude Code aguarda aprovação"` ou `"Claude Code aguarda resposta"`, sem incluir texto bruto do prompt — §7.2, CA-23).
   - Sinais repetidos para o mesmo episódio em andamento não incrementam `episode`, não resetam `seen` e não reenviam notificação.
2. **Término de Turno (`HookStop` e `CodexTurnComplete`)**:
   - Se a aba estava em `AwaitingInput` ou `Suspected`, resolve o estado para `Working`.
   - **Nunca** transiciona para `Awaiting` nem marca a aba como `error` (CA-23). Se `settings.on_turn_complete == AttentionLevel::Notify` e a aba não estiver focada/ativa, emite apenas notificação informativa de turno concluído.
3. **Indícios Heurísticos (`Suspected`)**:
   - `OscNotify` (OSC 9 / OSC 777), `Bell` ou `PatternMatch` (avaliado pelo timer de 100 ms quando `now_ms - last_output_ms >= silence_ms` e o processo do agente está em foreground) transicionam `Working -> Suspected` (nunca `Awaiting`, e nunca sobrescrevem um `Awaiting` ativo — CA-22).
4. **Reconhecimento (`agent_attention_ack` / `UserInput`) vs Resolução Correlacionada (CA-24)**:
   - `UserInput` (tecla no xterm) ou `agent_attention_ack` com `(run_id, episode)` correspondentes marcam `acknowledged = true` (reconhecendo visualização), mas **mantêm** o estado `Awaiting`/`Suspected` até que a espera seja realmente resolvida.
   - Resolução de `AwaitingInput`: `HookUserPromptSubmit` correlacionado, `CommandFinished` ou saída do PTY.
   - Resolução de `AwaitingApproval`: atividade de execução confirmada após a aprovação, `HookStop`, `CommandFinished` ou saída do PTY (`PreToolUse` sozinho **não** resolve aprovação pois precede o prompt de permissão — §7.2, CA-24).
    - Ao resolver um episódio `Awaiting`, invoca `TabStatePort::attention_resolved(&run, episode)` e `NotifyPort::clear_attention(run.tab_id)` somente se a execução ainda corresponder ao episódio; um evento atrasado não limpa a notificação de outra execução.
5. **Padrões Versionados (`patterns/default.toml` e `patterns.rs`)**:
   - Cada regra possui `id`, `agent`, `kind`, `regex` (1ª âncora), `confirm` (2ª âncora obrigatória no mesmo tail de 8 KiB sem ANSI) e `verified: bool`.
   - Linhas citadas (iniciadas por `>`, `|`, `+`, `-` de diff ou dentro de bloco markdown ```) são filtradas antes de casar `regex`/`confirm` para evitar falsos positivos quando o próprio código ou log contém "Allow?" ou "Yes" (R-03, CA-22).
    - Padrões sem fixture validado (`verified = false`) permanecem desabilitados por padrão.

#### Adaptador `TabStatePort` (contrato da Fase 05, T-16)

- `attention_required` e `attention_resolved` montam `RuntimeBinding` copiando os **quatro campos**
  exigidos pela Fase 05: `tab_id`, `pane_id`, `pty_id`, `session_id`. Antes disso, validam `run_id` e
  a geração do owner da Fase 07 contra o índice de runs da 13, sem reatribuir eventos antigos.
  `RuntimeBinding` não tem `generation`; `ExecutionRef.generation` pertence ao bloco da Fase 05,
  não ao owner da 07 nem ao contador de episódios.
- Criam `attention_id = format!("agent:{}:{}", run.run_id, episode)`; o mesmo ID é usado para abrir e
  limpar o episódio. Chamam `registry.apply(run.pane_id, StateInput::AttentionRequired { binding,
  attention_id, execution: None, at_ms })` e `registry.apply(run.pane_id,
  StateInput::AttentionCleared { binding, attention_id, at_ms })`, respectivamente. `at_ms` é o instante
  local observado. `None` em `execution` evita inventar bloco de comando para um hook do agente.
- A aplicação usa a assinatura de **dois argumentos** da proprietária 05. `attention_required` retorna
  `apply(...).is_some()`; limpar não substitui o estado base por `Idle`, recompõe a sobreposição.
- Testes do adaptador devem provar que eventos com PTY, sessão ou geração anteriores não alteram o
  painel reiniciado e que resolver o episódio N não limpa N+1; validar também dois painéis na mesma aba.
