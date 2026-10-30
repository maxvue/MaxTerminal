# Especificação Técnica — Implementação 02: Pipeline de I/O de Alto Volume, Frames Binários, Backpressure e Replay

## 1. Contexto, Objetivo e Escopo

Esta é a segunda unidade de implementação da **Fase 02 — Core PTY e Supervisão de Processos**. Seu objetivo é construir todo o subsistema de I/O de alto volume (`src-tauri/crates/mt-pty/src/io/`) e seu benchmark de vazão (`benches/throughput.rs`), garantindo:
1. Codificação e decodificação de frames binários versão 1 com cabeçalho fixo de 24 bytes (`Data`, `Replay`, `Gap`, `End`), zero overhead de JSON/base64 no caminho quente e preservação byte a byte de sequências UTF-8 multibyte ou ANSI cortadas entre chunks.
2. Controle de fluxo ponta a ponta (`FlowControl` e `AttachmentState`) baseado em crédito de bytes não confirmados (`high_watermark = 1 MiB`, `low_watermark = 256 KiB`), com pausa real da leitura no master FD para propagar backpressure até o `write(2)` do processo filho pelo buffer do kernel, e expiração de lease (`ack_timeout = 5 s`) com relógio injetável (`Clock`).
3. Ring buffer de replay de 1 MiB por PTY (`ReplayRing`), expulsão de frames inteiros, cálculo exato de `Gap` e entrega ordenada `Gap? → Replay* → Data*` sob controle de crédito.
4. Thread dedicada de leitura (`reader.rs`) com `poll(2)`, coalescing adaptativo (janela de 4 ms ou 256 KiB, flush imediato em primeiro byte após ociosidade para latência de eco p99 < 10 ms) e despacho único para `OutputTap`.
5. Thread dedicada de escrita (`writer.rs`) com fila limitada por quantidade (1024 pedidos) e orçamento de bytes (4 MiB por PTY + orçamento global), fragmentação em blocos de 4 KiB e escrita não bloqueante via `poll(POLLOUT, cancel_fd)`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Modificados
- `src-tauri/crates/mt-pty/Cargo.toml` — configuração de `[[bench]] name = "throughput" harness = false`.
- `src-tauri/crates/mt-pty/src/lib.rs` — exposição do módulo `pub mod io;`.

### 2.2. Arquivos Criados
- `src-tauri/crates/mt-pty/src/io/mod.rs` — reexports e tipos compartilhados do pipeline de I/O (`BufferPool`, `EventFdWake`).
- `src-tauri/crates/mt-pty/src/io/frame.rs` — formato binário de frame v1 (24 bytes de header + payload), `FrameKind`, `FrameHeader`, `DecodedFrame`, funções `encode_data_frame`, `encode_replay_frame`, `encode_gap_frame`, `encode_end_frame`, `decode_frame`.
- `src-tauri/crates/mt-pty/src/io/flow.rs` — trait `Clock`, `MonotonicClock`, `FlowState` (`Flowing`, `Paused`, `Detached`), `AttachmentLedger`, `FlowControl`.
- `src-tauri/crates/mt-pty/src/io/replay.rs` — `StoredFrame`, `ReplayRing`, `ReplaySnapshot` e validação de `OutputCursor`.
- `src-tauri/crates/mt-pty/src/io/reader.rs` — `ReaderWorker`, loop `poll(2)` + coalescing (4 ms / 256 KiB), integração com `TapRegistry`, `ReplayRing`, `FlowControl` e `OutputSink`.
- `src-tauri/crates/mt-pty/src/io/writer.rs` — `WriteQueue`, `WriterWorker`, reserva atômica de orçamento de bytes, fragmentação em chunks de 4 KiB e `poll(POLLOUT)`.
- `src-tauri/crates/mt-pty/benches/throughput.rs` — benchmark `criterion` avaliando vazão em MiB/s e frames/s com `AckingSink`, `SlowSink` e `NullSink`.

---

## 3. Especificação Técnica Detalhada e Assinaturas Rust

### 3.1. Formato de Frame Binário v1 (`src/io/frame.rs`)

Layout exato em Little-Endian (24 bytes de cabeçalho):
- `0..1`: `version: u8 = 0x01`
- `1..2`: `kind: u8` (`0x01 = Data`, `0x02 = Replay`, `0x03 = Gap`, `0x04 = End`)
- `2..4`: `reserved: u16 = 0x0000`
- `4..12`: `seq: u64` (LE; primeiro `Data` = 1; `End` carrega `final_seq`, `0` se nenhum `Data` foi emitido)
- `12..20`: `byte_offset: u64` (LE; offset absoluto no stream bruto antes do payload deste frame; em `End`, total de bytes produzidos)
- `20..24`: `len: u32` (LE; tamanho do payload em bytes)
- `24..(24 + len)`: `payload`

```rust
use bytes::{BufMut, Bytes, BytesMut};
use crate::error::{PtyError, PtyResult};
use crate::types::ExitInfo;

pub const FRAME_VERSION: u8 = 0x01;
pub const FRAME_HEADER_LEN: usize = 24;
pub const MAX_DATA_FRAME_BYTES: usize = 256 * 1024;
pub const GAP_PAYLOAD_LEN: usize = 8;
pub const MAX_END_PAYLOAD_LEN: usize = 4 * 1024;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Data = 0x01,
    Replay = 0x02,
    Gap = 0x03,
    End = 0x04,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    pub kind: FrameKind,
    pub seq: u64,
    pub byte_offset: u64,
    pub payload: Bytes,
}

pub fn encode_frame(kind: FrameKind, seq: u64, byte_offset: u64, payload: &[u8]) -> PtyResult<Bytes>;
pub fn encode_data_frame(seq: u64, byte_offset: u64, payload: &[u8]) -> PtyResult<Bytes>;
pub fn encode_replay_frame(seq: u64, byte_offset: u64, payload: &[u8]) -> PtyResult<Bytes>;
pub fn encode_gap_frame(first_retained_seq: u64, first_retained_offset: u64, dropped_bytes: u64) -> Bytes;
pub fn encode_end_frame(final_seq: u64, total_bytes: u64, exit: &ExitInfo) -> PtyResult<Bytes>;
pub fn decode_frame(raw: Bytes) -> PtyResult<DecodedFrame>;
pub fn decode_gap_payload(frame: &DecodedFrame) -> PtyResult<u64>;
pub fn decode_end_payload(frame: &DecodedFrame) -> PtyResult<ExitInfo>;
```

Regras de validação em `decode_frame`:
- Rejeita `raw.len() < 24`, `version != 1`, `reserved != 0`, `kind` fora de `1..=4`.
- Rejeita se `24 + (len as usize) != raw.len()`.
- Para `Data` e `Replay`: rejeita se `len == 0` ou `len > MAX_DATA_FRAME_BYTES`.
- Para `Gap`: exige `len == 8`.
- Para `End`: exige `1 <= len <= MAX_END_PAYLOAD_LEN`.

### 3.2. Controle de Fluxo e Lease de Conexão (`src/io/flow.rs`)

```rust
use std::sync::Arc;
use std::time::{Duration, Instant};
use crate::error::{PtyError, PtyResult};
use crate::sink::OutputSink;
use crate::types::{AttachmentId, OutputCursor};

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Instant;
}

#[derive(Debug, Default)]
pub struct MonotonicClock;
impl Clock for MonotonicClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowStatus {
    Flowing,
    Paused,
    Detached,
}

pub struct AttachmentState {
    pub id: AttachmentId,
    pub sink: Arc<dyn OutputSink>,
    pub sent_bytes: u64,
    pub consumed_bytes: u64,
    pub last_progress_at: Instant,
    pub next_cursor: OutputCursor,
    pub paused: bool,
}

pub struct FlowControl {
    high_watermark: u64,
    low_watermark: u64,
    ack_timeout: Duration,
    clock: Arc<dyn Clock>,
    // Protege AttachmentState e coordena com a thread de leitura
}

impl FlowControl {
    pub fn new(high_watermark: usize, low_watermark: usize, ack_timeout: Duration, clock: Arc<dyn Clock>) -> Self;
    pub fn install_attachment(&self, id: AttachmentId, sink: Arc<dyn OutputSink>, initial_cursor: OutputCursor);
    pub fn detach(&self, attachment_id: AttachmentId) -> PtyResult<bool>;
    pub fn force_detach_current(&self);
    pub fn record_ack(&self, attachment_id: AttachmentId, consumed_bytes: u64) -> PtyResult<FlowStatus>;
    pub fn available_credit(&self, attachment_id: AttachmentId) -> Option< usize>;
    pub fn reserve_credit(&self, attachment_id: AttachmentId, bytes: usize) -> Result<(), ()>;
    pub fn check_ack_timeout(&self) -> bool;
    pub fn status(&self) -> FlowStatus;
    pub fn unacked_bytes(&self) -> u64;
}
```

Regras do `FlowControl`:
- `record_ack(attachment_id, consumed_bytes)`:
  - Se não houver conexão ativa ou `active.id != attachment_id`, retorna `Err(PtyError::AttachmentStale)`.
  - Se `consumed_bytes > active.sent_bytes`, retorna `Err(PtyError::AckInvalid(...))`.
  - Se `consumed_bytes <= active.consumed_bytes`, é **no-op idempotente** (`Ok(status)`), sem atualizar `last_progress_at`.
  - Se `consumed_bytes > active.consumed_bytes`: atualiza `active.consumed_bytes = consumed_bytes` e `active.last_progress_at = clock.now()`.
  - `unacked = sent_bytes - consumed_bytes`. Se `paused && unacked <= low_watermark`, muda `paused = false` e acorda a thread de leitura (via `eventfd` de controle e `Condvar`).
- Quando `unacked >= high_watermark`, `paused = true`.
- `check_ack_timeout()`: o deadline (`ack_timeout = 5 s`) **só corre quando `sent_bytes > consumed_bytes`**. Se um shell estiver ocioso (`sent_bytes == consumed_bytes`), a conexão nunca expira por falta de ACK. Se expirar com bytes pendentes sem progresso por `>= ack_timeout`, invalida a geração atual, transiciona para `Detached` e acorda a thread de leitura para que ela drene para o replay sem bloquear o processo filho.

### 3.3. Ring Buffer de Replay e Cursores (`src/io/replay.rs`)

```rust
use std::collections::VecDeque;
use bytes::Bytes;
use crate::error::{PtyError, PtyResult};
use crate::types::OutputCursor;

#[derive(Debug, Clone)]
pub struct StoredFrame {
    pub seq: u64,
    pub byte_offset: u64,
    pub payload: Bytes,
}

impl StoredFrame {
    pub fn end_offset(&self) -> u64 {
        self.byte_offset + (self.payload.len() as u64)
    }
}

#[derive(Debug, Clone)]
pub struct ReplayPlan {
    pub replay_from: OutputCursor,
    pub live_from: OutputCursor,
    pub truncated: bool,
    pub gap: Option<(u64, u64, u64)>, // (first_retained_seq, first_retained_offset, dropped_bytes)
    pub frames: Vec<StoredFrame>,
}

pub struct ReplayRing {
    capacity_bytes: usize,
    retained_bytes: usize,
    frames: VecDeque<StoredFrame>,
    next_seq: u64,
    total_bytes: u64,
}

impl ReplayRing {
    pub fn new(capacity_bytes: usize) -> Self;
    pub fn push_frame(&mut self, payload: Bytes) -> StoredFrame;
    pub fn prepare_attach(&self, requested: Option<OutputCursor>) -> PtyResult<ReplayPlan>;
    pub fn next_seq(&self) -> u64;
    pub fn total_bytes(&self) -> u64;
}
```

Regras do `ReplayRing`:
- `push_frame` atribui `seq = self.next_seq` (iniciando em 1) e `byte_offset = self.total_bytes`, incrementa `next_seq += 1` e `total_bytes += payload.len() as u64`.
- Enquanto `self.retained_bytes + payload.len() > self.capacity_bytes` e houver frames antigos em `self.frames`, remove frames inteiros pela frente (`pop_front`) — **jamais** corta bytes no meio de um frame.
- `prepare_attach(requested)`:
  - `None` equivale a `OutputCursor { next_seq: 1, byte_offset: 0 }`.
  - Se `requested.next_seq == 0` ou `requested.next_seq > self.next_seq` ou `requested.byte_offset > self.total_bytes`: retorna `Err(PtyError::CursorInvalid(...))`.
  - Se `requested.next_seq == self.next_seq`: exige `requested.byte_offset == self.total_bytes` (caso contrário `CursorInvalid`); retorna `ReplayPlan` vazio com `truncated = false`.
  - Se o cursor solicitado apontar para um frame ainda retido no ring: verifica que o `byte_offset` do frame com `seq == requested.next_seq` coincide exatamente com `requested.byte_offset` (se divergir, retorna `CursorInvalid`); retorna todos os frames a partir dele com `truncated = false`.
  - Se o cursor solicitado for anterior ao primeiro frame retido no ring (ou se o ring estiver vazio após expulsão): `truncated = true`, `dropped_bytes = first_retained_offset - requested.byte_offset`, `gap = Some((first_retained_seq, first_retained_offset, dropped_bytes))`, seguido dos frames retidos.

### 3.4. Thread de Leitura e Coalescing (`src/io/reader.rs`)

- Usa `nix::poll::poll` em dois descritores:
  1. `pty_master_fd` (`POLLIN | POLLHUP | POLLERR`)
  2. `wake_event_fd` (`POLLIN` — acionado em attach, detach, ACK que destrava pausa, ou shutdown).
- **Lógica de Coalescing e Eco Rápido**:
  - Quando o buffer de coalescing está vazio e o PTY estava ocioso (sem flush nos últimos 4 ms), o primeiro `read(2)` que retorna dados tenta drenar leituras não bloqueantes imediatas disponíveis no kernel (até `min(max_frame_bytes, available_credit)`) e faz **flush imediato** se a segunda leitura retornar `EAGAIN` e o tamanho for pequeno (eco interativo de tecla com latência < 1 ms, atendendo com folga o **CA-14**: p99 < 10 ms).
  - Em rajada contínua (leituras subsequentes ou após um flush recente), acumula até completar a janela de `coalesce_window` (`4 ms` desde o primeiro byte do frame atual) **ou** atingir `max_frame_bytes` (`256 KiB`, respeitando o crédito disponível quando anexado), fazendo flush assim que qualquer condição for satisfeita (**CA-15**).
- **Ordem no Flush de Dados Novos**:
  1. Registra o payload no `ReplayRing::push_frame`, obtendo `(seq, byte_offset)`.
  2. Invoca `TapRegistry::dispatch_output(pty_id, seq, &payload)` **uma única vez** (mesmo se `Detached`; replay nunca chama taps — **CA-33**).
  3. Se houver anexo ativo, reserva crédito (`payload.len()`), codifica `encode_data_frame(seq, byte_offset, &payload)` e envia via `sink.send(encoded)`. Se `sink.send` retornar `Err(SinkClosed)`, invalida a geração atual e passa para `Detached`.
- **Tratamento de `EIO` e `POLLHUP` no Linux**:
  - No Linux, quando todos os descritores do slave PTY fecham, `read(2)` no master retorna `EIO` (`errno == 5`). Quando `POLLHUP` aparece junto com `POLLIN`, o reader continua lendo até que `read` retorne `0` (EOF) ou `EIO`, garantindo que nenhum byte final seja perdido (**CA-04**, **CA-19**).

### 3.5. Thread de Escrita (`src/io/writer.rs`)

- `WriteQueue`: controla atomicamente `queued_messages <= 1024`, `queued_bytes <= 4 MiB` (incluindo o pedido atualmente em escrita pelo worker) e cota no `GlobalBufferPool` (`256 MiB`).
- Se um pedido individual exceder `max_write_bytes` (`1 MiB`), retorna `PtyError::InputInvalid`.
- Se não houver espaço na fila (`queued_messages == 1024` ou `queued_bytes + len > 4 MiB`), rejeita o pedido inteiro imediatamente com `PtyError::WriteQueueFull`.
- O worker retira cada pedido da fila e escreve no master FD (configurado com `O_NONBLOCK`) em fatias de até `write_chunk_bytes` (`4 KiB`).
- Quando `write(2)` retorna `EAGAIN` / `EWOULDBLOCK`, o worker executa `poll([master_fd (POLLOUT), cancel_event_fd (POLLIN)])`. Se `cancel_event_fd` disparar (transição para `Exiting` ou `Shutdown`), aborta imediatamente a escrita bloqueada sem travar o encerramento do PTY (**CA-34**).

---

## 4. Regras de Negócio, Performance e Casos de Borda

1. **Zero Conversão UTF-8 no Pipeline de Bytes**: `reader.rs`, `frame.rs` e `replay.rs` operam estritamente sobre `&[u8]` / `Bytes`. Sequências UTF-8 de 2–4 bytes ou códigos ANSI CSI/OSC divididos entre dois reads consecutivos são preservados sem corrupção nem substituição por `U+FFFD` (**CA-09**).
2. **Controle de Crédito no Replay**: Ao executar `attach`, os frames de `Replay` também consomem crédito de `FlowControl`. Se o replay tiver 1 MiB e o consumidor demorar a confirmar, o envio de novos frames fica pausado até o `pty_ack` reduzir `unacked <= low_watermark`. O frame `End` (≤ 4 KiB) e o frame `Gap` (8 bytes) são frames de controle de fronteira e não incrementam `seq`.
3. **Orçamento Global de Memória**: `GlobalBufferPool` rastreia com `AtomicUsize` a memória reservada para rings e filas de todos os PTYs, respeitando `global_buffer_budget` (256 MiB).

---

## 5. Critérios de Aceite Mapeados

- **CA-08**: Escrita de 1 MiB em consumidor com slave raw preserva SHA-256 idêntico; suporta todos os bytes `0..=255`.
- **CA-09**: Sequências UTF-8 multibyte e ANSI cortadas entre chunks chegam íntegras byte a byte.
- **CA-10**: Fixture determinística de 200 MiB preserva hash SHA-256 e ordem, atingindo vazão core ≥ 150 MiB/s na máquina de referência (`benches/throughput.rs`).
- **CA-11 / CA-12**: Com sink anexado que não envia ACK, bytes em trânsito nunca excedem `high_watermark` (1 MiB) e o leitor pausa; ao retomar ACKs, o fluxo continua sem perda nem duplicação (`seq` contínuo).
- **CA-13**: Com sink desanexado (`Detached`), a leitura não bloqueia, o ring retém os últimos 1 MiB e um novo `attach` entrega `Gap` + `Replay` + `Data` sem lacunas após o replay.
- **CA-14**: Flush imediato após ociosidade garante latência p99 < 10 ms em eco de tecla.
- **CA-15**: Coalescing faz flush por janela de 4 ms ou teto de 256 KiB por frame.
- **CA-33**: `OutputTap` recebe cada frame bruto original exatamente uma vez (inclusive desanexado) e nunca é rechamado durante `Replay`.
