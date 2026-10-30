# Especificação Técnica — Implementação 03: Tracker de Execução, Captura Sanitizada, Fallback `/proc` e Máquina de Estados das Abas

> **Fase:** 05 — Shell Integration OSC 133 e Máquina de Estados das Abas
> **Implementação:** `03 - Tracker de Execução, Captura Sanitizada, Fallback proc e Máquina de Estados`
> **Fontes da verdade:** [`../../AGENTS.md`](../../../AGENTS.md), [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§§1, 3, 5.2, 5.3, 5.4, 6 e 10).

---

## 1. Contexto, Objetivo e Escopo

Esta implementação completa o núcleo de runtime do crate `mt-shell`: o sequenciador de comandos por PTY (`CommandTracker`), a pipeline de captura de saída combinada com remoção ANSI (`vte`) e sanitização streaming (`StreamSanitizer`) antes de qualquer retenção de cabeça/cauda, o monitor de fallback `/proc` (`ProcFallbackMonitor`), a máquina de estados pura das abas/painéis (`TabStatus`, `PaneState`, `TabStateRegistry`), o classificador de processos contínuos (`ContinuousDetector`) e o orquestrador `ShellService`.

### Escopo Incluído
1. **Classificador de Execução Contínua (`state/daemon.rs`):**
   - `ContinuousDetector` combinando `ContinuousPolicy` (`Auto`, `On`, `Off`) com heurísticas conservadoras tokenizadas (sem regex com look-around e sem avaliação de shell).
2. **Máquina de Estados Pura e Registro Agregador (`state/machine.rs`, `state/registry.rs`):**
   - Redutor determinístico `transition(state, input) -> Option<StateChange>` implementando toda a tabela normativa da §6.1 da spec.
   - `TabStateRegistry` gerenciando múltiplos painéis por aba, prioridade de buckets (`error/attention não visto > running > continuous > finished não visto > idle`), desempate por ordinal monotônico e `PaneId` lexical, `counts_for(tab_ids)`, `has_active_processes` (OR sobre todos os panes), validação de foco por janela e confirmação de visualização exata por `(ptyId, blockId, generation, observedRevision)`.
3. **Tracker de Comandos e Sequenciador de Segmentos (`tracker.rs`):**
   - `CommandTracker` com fases `Unknown`, `Prompt`, `Input`, `Executing`, separação estrita entre `hook_generation` (protocolo OSC) e `ExecutionRef.generation` (contador runtime monotônico por PTY), e fatiamento intra-frame (`texto anterior → C → texto do bloco → D → prompt`).
4. **Fila Limitada e Captura Sanitizada (`capture.rs`, `persist.rs`):**
   - Mailbox por PTY limitada a **8 MiB e 4.096 mensagens**, com **64 slots reservados para controle** e **1 slot terminal dedicado** para `Exit`/`CaptureOverflow`.
   - Captura com parser incremental `vte` (removendo CSI/OSC/DCS, preservando `\r\n` e tratando `\r` isolado como reset da linha corrente), alimentando `StreamSanitizer` **antes** de reter `head` (16 KiB), `tail` (48 KiB) e ring de erro (até 200 linhas × 4 KiB).
   - Tratamento fail-closed em `Gap` (`sanitizer.on_gap()`, descarte de fragmento parcial até fronteira segura e inserção de marcador `[... saída omitida por sobrecarga ...]`).
   - Cache LRU global sanitizado de blocos finalizados (máx. 32 blocos / 4 MiB) para servir `error_context` antes do commit ou quando `persistence == Failed`.
5. **Fallback `/proc` (`fallback.rs`) e Orquestração (`runtime.rs`, `service.rs`):**
   - Timeout de 5 s sem handshake (`Pending → Proc`), polling de 250 ms via `PtyManager::process_report`/`foreground_process`, detecção inferida de grupo foreground (`ObservationSource::Proc`, `ObservationConfidence::Inferred`, `exit_code = None` enquanto o PTY vive), resultado `Failed` com o `exit_code` do PTY quando este encerra com código `!= 0` durante um bloco ativo em modo `Proc` (T-07; ver §4 item 2) e reconciliação na chegada tardia de handshake (`BlockEndReason::ModeSwitched`).
   - Implementação de `OutputTap`, `SpawnLifecycle` e `SessionProvider` (`PtyScopedSessionProvider`) em `ShellService`.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho do Arquivo | Responsabilidade |
|------|--------------------|------------------|
| Criar | `src-tauri/crates/mt-shell/src/state/mod.rs` | Re-export de `machine`, `daemon` e `registry` |
| Criar | `src-tauri/crates/mt-shell/src/state/daemon.rs` | `ContinuousDetector` e regras conservadoras de detecção de processos contínuos |
| Criar | `src-tauri/crates/mt-shell/src/state/machine.rs` | `PaneState`, `StateInput`, `StateChange` e redutor puro `transition` |
| Criar | `src-tauri/crates/mt-shell/src/state/registry.rs` | `TabStateRegistry`, agregação de `TabStateSnapshot`, `counts_for`, controle de foco e tombstones |
| Criar | `src-tauri/crates/mt-shell/src/tracker.rs` | `CommandTracker`, `OpenBlock`, `TrackerAction`, segmentação intra-chunk e mapeamento de gerações |
| Criar | `src-tauri/crates/mt-shell/src/capture.rs` | `BlockCapture`, normalizador ANSI (`vte`) + `StreamSanitizer` + buffers `head`/`tail`/`error_ring` |
| Criar | `src-tauri/crates/mt-shell/src/fallback.rs` | `ProcFallbackMonitor`, tick de 250 ms, timeout de 5 s de handshake e reconciliação com hooks |
| Criar | `src-tauri/crates/mt-shell/src/persist.rs` | `PersistenceWorker`, cache LRU sanitizado (32 blocos / 4 MiB) e escrita via `CommandBlocksRepo` |
| Criar | `src-tauri/crates/mt-shell/src/runtime.rs` | `SessionProvider`, `PtyScopedSessionProvider`, `PtySequencerMailbox` (8 MiB / 4096 msgs + 64 ctrl + 1 terminal) |
| Criar | `src-tauri/crates/mt-shell/src/service.rs` | `ShellService`, `ShellConfig`, `SpawnPlan`, `ShellEventSink` e implementação de `OutputTap` |
| Modificar | `src-tauri/crates/mt-shell/src/lib.rs` | Export de todos os novos módulos e tipos de serviço |
| Criar | `src-tauri/crates/mt-shell/tests/tracker.rs` | Testes determinísticos do `CommandTracker`, segmentação no mesmo chunk, C duplicado, `MissingEnd` e `capture.rs` |
| Criar | `src-tauri/crates/mt-shell/tests/state_table.rs` | Cobertura 100% da tabela de estados da §6.1, ACK por geração, agregação multi-pane e `ContinuousDetector` |

---

## 3. Assinaturas Exatas e Regras de Negócio (Rust)

### 3.1. Detector de Execução Contínua (`src-tauri/crates/mt-shell/src/state/daemon.rs`)

```rust
use mt_core::shell::{ContinuousPolicy, ObservationConfidence, ObservationSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuousClassification {
    pub continuous: bool,
    pub source: ObservationSource,
    pub confidence: ObservationConfidence,
}

pub struct ContinuousDetector;

impl ContinuousDetector {
    /// Avalia se o comando deve promover a aba para `TabStatus::Continuous`:
    /// - `ContinuousPolicy::On` => `continuous = true` imediatamente.
    /// - `ContinuousPolicy::Off` => `continuous = false` (vence qualquer heurística).
    /// - `ContinuousPolicy::Auto`:
    ///   - Se `command` for `None` ou `quality != Complete` (quando vindo de proc ambíguo ou comando composto com `;`/`&&`/`||`), retorna `continuous = false`.
    ///   - Ignora variáveis de ambiente iniciais (`KEY=VAL`), `env`, `sudo`, `nohup` e extrai o executável base (basename) + tokens.
    ///   - Rejeita imediatamente se contiver flags `--help`, `-h`, `--version`, `-V`, `-d`, `--detach`, ou subcomandos `build`, `test`, `lint`, `check`, `install`.
    ///   - Padrões positivos reconhecidos:
    ///     1. `npm` | `pnpm` | `yarn` | `bun` seguidos de `run dev` | `dev` | `start` | `serve` | `watch`
    ///     2. `vite` (`dev` | `serve` | `preview` ou sem subcomando `build`)
    ///     3. `php artisan` seguido de `serve` | `horizon` | `queue:work` | `queue:listen` | `reverb:start` | `octane:start` | `pail`
    ///     4. `cargo` seguido de `watch` | `tauri dev`
    ///     5. `tail` com `-f` ou `--follow`
    ///     6. `python` | `python3` com `-m http.server` | `manage.py runserver` | `uvicorn` | `gunicorn`
    ///     7. `docker compose up` / `docker-compose up` SEM `-d` / `--detach`
    pub fn classify(
        policy: ContinuousPolicy,
        command: Option<&str>,
        source: ObservationSource,
        confidence: ObservationConfidence,
    ) -> ContinuousClassification;
}
```

### 3.2. Máquina de Estados Pura (`src-tauri/crates/mt-shell/src/state/machine.rs`)

```rust
use mt_core::shell::{
    AttentionSnapshot, BlockEndReason, BlockStatus, CompletionSnapshot, ContinuousPolicy,
    ExecutionRef, IntegrationMode, ObservationConfidence, ObservationSource, PaneStateDto,
    RuntimeBinding, SeenRequest, TabStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneState {
    pub binding: RuntimeBinding,
    pub alive: bool,
    pub mode: IntegrationMode,
    pub status: TabStatus,
    pub execution: Option<ExecutionRef>,
    pub current_command: Option<String>,
    pub completion: Option<CompletionSnapshot>,
    pub seen_execution: Option<ExecutionRef>,
    pub attention: Option<AttentionSnapshot>,
    pub policy: ContinuousPolicy,
    pub source: ObservationSource,
    pub confidence: ObservationConfidence,
    pub cwd: Option<String>,
    pub revision: u64,
    pub change_ordinal: u64,
    pub changed_at_ms: i64,
    pub last_reason: String,
}

#[derive(Debug, Clone)]
pub enum StateInput {
    CommandStarted {
        binding: RuntimeBinding,
        execution: ExecutionRef,
        command: Option<String>,
        cwd: Option<String>,
        continuous: bool,
        source: ObservationSource,
        confidence: ObservationConfidence,
        at_ms: i64,
    },
    CommandFinished {
        binding: RuntimeBinding,
        execution: ExecutionRef,
        outcome: BlockStatus,
        exit_code: Option<i32>,
        reason: BlockEndReason,
        at_ms: i64,
    },
    MarkSeen {
        request: SeenRequest,
        at_ms: i64,
    },
    SetPolicy {
        binding: RuntimeBinding,
        policy: ContinuousPolicy,
        at_ms: i64,
    },
    ModeChanged {
        binding: RuntimeBinding,
        mode: IntegrationMode,
        reason: Option<String>,
        at_ms: i64,
    },
    CwdChanged {
        binding: RuntimeBinding,
        cwd: String,
        source: ObservationSource,
        confidence: ObservationConfidence,
        at_ms: i64,
    },
    PtyExited {
        binding: RuntimeBinding,
        at_ms: i64,
    },
    AttentionRequired {
        binding: RuntimeBinding,
        attention_id: String,
        execution: Option<ExecutionRef>,
        at_ms: i64,
    },
    AttentionCleared {
        binding: RuntimeBinding,
        attention_id: String,
        at_ms: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChange {
    pub next: PaneState,
    pub reason: &'static str,
}

/// Redutor puro determinístico:
/// - Retorna `None` se `input.binding != state.binding` (ex.: evento atrasado de PTY substituído)
///   ou se a transição for idempotente (nenhum campo semântico mudou).
/// - Incrementa `next.revision = state.revision + 1` exclusivamente quando retorna `Some(StateChange)`.
pub fn transition(state: &PaneState, input: StateInput, next_ordinal: u64) -> Option<StateChange>;

impl PaneState {
    pub fn is_seen(&self) -> bool {
        match &self.completion {
            Some(c) if matches!(self.status, TabStatus::Finished | TabStatus::Error) => {
                self.seen_execution.as_ref() == Some(&c.execution)
            }
            _ => true,
        }
    }

    /// Calcula o bucket semântico projetado para badges (Seção 6.4 da spec):
    /// 1. Se `attention` está presente com `seen == false` => `TabStatus::Error`.
    /// 2. Se `status == Error` e `!is_seen()` => `TabStatus::Error`.
    /// 3. Se `status == Running` => `TabStatus::Running`.
    /// 4. Se `status == Continuous` => `TabStatus::Continuous`.
    /// 5. Se `status == Finished` e `!is_seen()` => `TabStatus::Finished`.
    /// 6. Caso contrário (incluindo `Finished`/`Error` já vistos ou `Unknown`) => `TabStatus::Idle`.
    pub fn badge_bucket(&self) -> TabStatus;

    pub fn to_dto(&self) -> PaneStateDto;
}
```

### 3.3. Registro de Estados e Agregação (`src-tauri/crates/mt-shell/src/state/registry.rs`)

```rust
use std::collections::HashMap;
use mt_core::{PaneId, PtyId, TabId};
use mt_core::shell::{
    ContinuousPolicy, FocusSnapshot, RuntimeBinding, SeenRequest, TabStateListResponse,
    TabStateSnapshot, TabStatus,
};
use mt_core::MtError;
use crate::state::machine::{PaneState, StateInput};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TabStatusCounts {
    pub running: u32,
    pub continuous: u32,
    pub finished: u32,
    pub error: u32,
    pub idle: u32,
}

pub struct TabStateRegistry { /* RwLock interno com abas, panes, tombstones, estado de foco por janela e watermark */ }

impl TabStateRegistry {
    pub fn new() -> Self;

    /// Registra um novo vínculo `(tab_id, pane_id, pty_id, session_id)` em estado `Idle`.
    /// Se o `pane_id` já possuir um PTY `alive == true` diferente, retorna `Err(MtError::shell_binding_conflict())`.
    pub fn register_binding(&self, binding: RuntimeBinding, policy: ContinuousPolicy, now_ms: i64)
        -> Result<TabStateSnapshot, MtError>;

    /// Aplica uma transição a um painel e recalcula o `TabStateSnapshot` agregado da aba.
    pub fn apply(&self, pane_id: PaneId, input: StateInput) -> Option<TabStateSnapshot>;

    /// Registra o snapshot de foco de uma janela (identificada pelo rótulo da janela Tauri).
    /// Ignora snapshots cujo `focus_revision` seja menor ou igual ao último visto para a mesma janela.
    /// NÃO confirma automaticamente nenhuma execução.
    pub fn set_window_focus(&self, window_label: &str, focus: FocusSnapshot);

    /// Confirma a visualização de uma conclusão (`SeenRequest`):
    /// - Se `require_active_focus == true`, verifica se há alguma janela focada, com `document_visible == true`,
    ///   `tab_id == Some(request.tab_id)` e `visible_pane_ids` contendo `request.pane_id`.
    /// - Valida se `request.observed_revision >= completion_revision` e `<= current_tab_revision`.
    /// - Se a execução não corresponder à conclusão atual (ex.: geração antiga ou comando ainda `running`),
    ///   trata como no-op idempotente e retorna o snapshot atual.
    pub fn mark_seen(
        &self,
        window_label: &str,
        request: SeenRequest,
        require_active_focus: bool,
        now_ms: i64,
    ) -> Result<TabStateSnapshot, MtError>;

    pub fn set_continuous_policy(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        policy: ContinuousPolicy,
        now_ms: i64,
    ) -> Result<TabStateSnapshot, MtError>;

    /// Remove um painel encerrado; se era o último painel da aba, grava tombstone e retorna `(revision, true)`.
    pub fn remove_pane(&self, tab_id: TabId, pane_id: PaneId) -> Option<(u64, Option<TabStateSnapshot>)>;

    pub fn get_tab(&self, tab_id: TabId) -> Result<TabStateSnapshot, MtError>;
    pub fn list_tabs(&self) -> TabStateListResponse;

    /// Conta cada `TabId` único e existente em exatamente um bucket (`badge_bucket`).
    /// Abas inexistentes ou removidas (tombstone) são ignoradas e não contam como `idle`.
    pub fn counts_for(&self, tab_ids: &[TabId]) -> TabStatusCounts;

    /// Retorna `true` se QUALQUER painel de qualquer aba tiver `alive == true` e `status` em `Running | Continuous`.
    pub fn has_active_processes(&self) -> bool;
}
```

### 3.4. Tracker de Comandos e Segmentação (`src-tauri/crates/mt-shell/src/tracker.rs`)

```rust
use std::time::Instant;
use mt_core::{CommandBlockId, PtyId};
use mt_core::shell::{
    BlockEndReason, BlockStatus, CommandTextQuality, ContinuousPolicy, ExecutionRef,
    IntegrationMode, ObservationConfidence, ObservationSource, RuntimeBinding, StreamPosition,
};
use crate::osc::ScannedMark;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptPhase {
    Unknown,
    Prompt,
    Input,
    Executing,
}

#[derive(Debug, Clone)]
pub struct OpenBlock {
    pub execution: ExecutionRef,
    pub hook_generation: Option<u64>,
    pub seq: u64,
    pub command: Option<String>,
    pub command_quality: CommandTextQuality,
    pub cwd: Option<String>,
    pub continuous: bool,
    pub source: ObservationSource,
    pub confidence: ObservationConfidence,
    pub started_at_ms: i64,
    pub started_mono: Instant,
    pub output_start: StreamPosition,
}

#[derive(Debug, Clone)]
pub enum TrackerEvent {
    HandshakeConfirmed,
    ModeChanged { mode: IntegrationMode, reason: Option<String> },
    PromptPhaseChanged { phase: PromptPhase },
    CwdUpdated { cwd: String, source: ObservationSource, confidence: ObservationConfidence },
    BlockStarted(OpenBlock),
    BlockOutputSlice {
        execution: ExecutionRef,
        frame_seq: u64,
        bytes: Vec<u8>,
    },
    BlockFinished {
        block: OpenBlock,
        status: BlockStatus,
        exit_code: Option<i32>,
        reason: BlockEndReason,
        ended_at_ms: i64,
        duration_ms: u64,
        output_end: Option<StreamPosition>,
    },
}

pub struct CommandTracker { /* binding, mode, phase, next_runtime_gen, next_seq, pending_meta, open_block */ }

impl CommandTracker {
    pub fn new(
        binding: RuntimeBinding,
        initial_mode: IntegrationMode,
        initial_next_seq: u64,
        policy: ContinuousPolicy,
        initial_cwd: Option<String>,
    ) -> Self;

    /// Processa um frame bruto junto com as marcas escaneadas nesse frame:
    /// - Fatura o slice `bytes` usando `mark.start.byte_offset` e `mark.end.byte_offset`:
    ///   bytes anteriores a `C` não entram no novo bloco; bytes entre `C.end` e `D.start`
    ///   são emitidos como `BlockOutputSlice` da execução aberta; bytes após `D.end` não entram no bloco fechado.
    /// - Ignora qualquer `ScannedMark` com `nonce_valid == false`.
    pub fn process_frame(
        &mut self,
        frame_seq: u64,
        bytes: &[u8],
        marks: &[ScannedMark],
        now_ms: i64,
        now_mono: Instant,
    ) -> Vec<TrackerEvent>;

    /// Encerramento do PTY. `exit_code` é o código do `ExitInfo` do PTY.
    /// - Sem bloco ativo ou bloco já finalizado por `D`: não altera nenhum resultado.
    /// - `IntegrationMode::Hooks` com bloco ativo sem `D`: fecha `BlockStatus::Unknown` (`exit_code: None`, `PtyExited`).
    /// - `IntegrationMode::Proc` com bloco ativo e `exit_code == Some(n)` com `n != 0`: fecha `BlockStatus::Failed`
    ///   com `exit_code = Some(n)` e `BlockEndReason::PtyExited` (a aba vai para `TabStatus::Error`; "Inspecionar" habilitado).
    /// - `IntegrationMode::Proc` com bloco ativo e `exit_code == Some(0)` ou `None` (sem informação): `BlockStatus::Unknown`.
    pub fn on_pty_exit(&mut self, exit_code: Option<i32>, now_ms: i64, now_mono: Instant) -> Vec<TrackerEvent>;
    pub fn on_capture_overflow(&mut self, now_ms: i64, now_mono: Instant) -> Vec<TrackerEvent>;
}
```

### 3.5. Captura Sanitizada (`src-tauri/crates/mt-shell/src/capture.rs`)

```rust
use mt_db::sanitizer::{SanitizedText, StreamSanitizer};

pub const HEAD_CAP_BYTES: usize = 16 * 1024;       // 16 KiB
pub const TAIL_CAP_BYTES: usize = 48 * 1024;       // 48 KiB
pub const ERROR_RING_MAX_LINES: usize = 200;       // máx. 200 linhas
pub const ERROR_LINE_MAX_BYTES: usize = 4 * 1024;  // 4 KiB por linha

#[derive(Debug, Clone)]
pub struct FinalizedCapture {
    pub output_bytes: u64,
    pub dropped_bytes: u64,
    pub output_truncated: bool,
    pub output_text: Option<SanitizedText>,
    pub error_tail: Option<SanitizedText>,
    pub error_tail_lines: u32,
}

pub struct BlockCapture {
    sanitizer: StreamSanitizer,
    ansi_stripper: AnsiLineNormalizer, // baseado em `vte::Parser`
    head_buf: String,
    tail_ring: VecDeque<u8>,
    error_lines: VecDeque<String>,
    total_raw_bytes: u64,
    dropped_bytes: u64,
    truncated: bool,
}

impl BlockCapture {
    pub fn new() -> Self;
    /// Recebe bytes de saída do comando, remove sequências CSI/OSC/DCS via `vte`,
    /// trata `\r` isolado (sobrescrita de barra de progresso) e passa pelo `StreamSanitizer`
    /// ANTES de armazenar em `head_buf`, `tail_ring` e `error_lines`.
    pub fn push_bytes(&mut self, chunk: &[u8]);
    /// Registra perda de captura por pressão na fila: chama `self.sanitizer.on_gap()`,
    /// descarta linha/unidade incompleta e insere marcador `[... saída omitida ...]`.
    pub fn record_gap(&mut self, dropped: u64);
    /// Conclui o `StreamSanitizer::finish()`, monta `output_text` (`head` + marcador + `tail` quando truncado)
    /// e `error_tail` (até 200 linhas finais, revalidadas como `SanitizedText`).
    pub fn finish(self, is_failed: bool) -> FinalizedCapture;
}
```

### 3.6. Mailbox Limitada e `ShellService` (`runtime.rs`, `service.rs`)

- **Limites da Mailbox por PTY (`PtySequencerMailbox`):**
  - Capacidade de dados: `8 MiB` (`8 * 1024 * 1024` bytes) e `4.096` mensagens de dados (`OutputChunk { frame_seq, bytes, marks }`), cada segmento `<= 64 KiB`.
  - Reserva de controle: `64` posições exclusivas para `ControlMsg` (`HandshakeTimeout`, `ProcPollResult`, `PolicyChange`) + `1` slot atômico terminal (`AtomicSlot<TerminalSignal>`) para `PtyExited` / `CaptureOverflow`.
  - Se um `OutputChunk` exceder os `8 MiB` ou `4.096` itens, o `OutputTap::on_output` **não bloqueia a thread reader do PTY**: processa apenas as `ScannedMark`s nos slots de controle reservados e acumula `dropped_bytes` em um `PendingGap` atômico; se até os 64 slots de controle saturarem, aciona o latch `capture_overflow`, fecha o bloco atual como `Unknown / CaptureOverflow` e aguarda o próximo prompt seguro (`Handshake` ou `PromptStart`) para ressincronizar.

---

- **Propagação de saída do PTY (T-07):** o evento de lifecycle da Fase 02 entrega `ExitInfo`; o slot terminal preserva seu campo `code` e o binding completo. Após drenar as mensagens anteriores, o sequenciador chama `tracker.on_pty_exit(exit.code, now_ms, now_mono)`. Para cada `BlockFinished`, conclui a captura com `finish(status == BlockStatus::Failed)`, emite/aplica `StateInput::CommandFinished` e enfileira a persistência sanitizada. Só então aplica `StateInput::PtyExited`, que marca `alive=false` sem apagar a conclusão, seu código ou o badge de erro não visto. Saída por sinal sem `code` permanece `Unknown`, sem fabricar `128 + signal`. Bloco já concluído não é reaberto.
- **Contrato de atenção para a Fase 13 (T-16):** consumidores chamam `TabStateRegistry::apply(pane_id, StateInput::AttentionRequired/AttentionCleared { binding, ... })` com `RuntimeBinding { tab_id, pane_id, pty_id, session_id }` integral, conforme `05/01`. A geração de comando pertence a `ExecutionRef { pty_id, block_id, generation }`, repassado em `AttentionRequired.execution` quando houver execução; `AttentionCleared` usa o mesmo binding e o `attention_id` corrente. Não reconstruir o binding apenas a partir de `tab_id`/`pane_id`, inventar campo `generation` nele ou aceitar atenção de execução substituída.

## 4. Tratamento de Erros e Casos de Borda

1. **Chegada Tardia de Handshake após Fallback `/proc`:**
   - Quando um shell demora > 5 s no startup (ex.: `.zshrc` pesado) e entra em `IntegrationMode::Proc`, assim que o handshake `633;P;MT=1;n=<nonce>` chega com nonce válido, o `ProcFallbackMonitor` é cancelado, eventual bloco aberto por `/proc` é encerrado com `BlockStatus::Unknown` e `BlockEndReason::ModeSwitched`, e o modo passa para `IntegrationMode::Hooks` sem duplicar conclusão.
2. **Saída do Shell (`exec` ou `exit`) com Comando Aberto:**
   - Em `IntegrationMode::Hooks`, se o PTY encerrar enquanto um bloco está `Running` e sem marca `D` correspondente, o bloco fecha imediatamente como `BlockStatus::Unknown` (`exit_code: None`, `end_reason: BlockEndReason::PtyExited`) e a aba vai para `TabStatus::Idle`. O código de saída do processo do shell pertence ao `ExitInfo` do PTY, nunca é atribuído falsamente ao comando interrompido.
   - **Exceção em `IntegrationMode::Proc` (sem hooks/OSC 133, T-07):** não há marca `D`, então o `exit_code` do PTY é a única evidência de resultado. Se o PTY encerrar com `exit_code != 0` durante um bloco ativo, o bloco é finalizado como `BlockStatus::Failed` com esse `exit_code` (`end_reason: BlockEndReason::PtyExited`), a aba transita para `TabStatus::Error` (não vista) e `command_block_error_context` passa a aceitar o bloco (sem `shell.block_not_failed`), habilitando "Inspecionar". Blocos encerrados sem informação de código (`exit_code` ausente ou `0` com bloco ainda ativo, `ModeSwitched`, PID reutilizado ou erro de `/proc`) continuam `BlockStatus::Unknown` com `exit_code: None`, sem sucesso nem erro fabricados.
3. **Indisponibilidade ou Falha do SQLite:**
   - As transições do `TabStateRegistry` e o evento `tab://state-changed` ocorrem no sequenciador **antes** de enfileirar no `PersistenceWorker`. Se `Db::write` falhar após retry transitório limitado, o bloco permanece no cache LRU sanitizado (32 blocos / 4 MiB) com `PersistenceState::Failed` e o evento `shell://command-finished` informa `persistence: "failed"`.
