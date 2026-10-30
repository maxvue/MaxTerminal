# Plano de Execução — Implementação 02: Pipeline de I/O de Alto Volume, Frames Binários, Backpressure e Replay

## 1. Checklist Sequencial de Implementação

### Passo 1: Codec de Frames Binários v1 (`src/io/frame.rs`)
1. Criar `src-tauri/crates/mt-pty/src/io/frame.rs` implementando o cabeçalho binário de 24 bytes Little-Endian (`version = 1`, `kind`, `reserved = 0`, `seq: u64`, `byte_offset: u64`, `len: u32`) e funções `encode_data_frame`, `encode_replay_frame`, `encode_gap_frame`, `encode_end_frame`, `decode_frame`, `decode_gap_payload` e `decode_end_payload`.
2. Escrever testes unitários e testes baseados em propriedades (`proptest`) em `frame.rs` validando:
   - Roundtrip exato para payloads de `0..=256 KiB` com bytes arbitrários `0..=255`.
   - Rejeição de `version != 1`, `kind` inválido, `reserved != 0`, truncamento de buffer e `len` excedendo o limite de cada tipo de frame.

### Passo 2: Controle de Fluxo e Lease de Conexão (`src/io/flow.rs`)
1. Criar `src-tauri/crates/mt-pty/src/io/flow.rs` com a trait `Clock`, `MonotonicClock`, `MockClock` (para testes), `FlowStatus` (`Flowing`, `Paused`, `Detached`) e `FlowControl`.
2. Implementar a contabilidade por geração (`AttachmentId`):
   - Reserva de crédito antes do envio (`sent_bytes += bytes`), transição para `Paused` ao atingir `high_watermark` (`1 MiB`), transição de volta para `Flowing` quando `record_ack` reduz `unacked <= low_watermark` (`256 KiB`).
   - Rejeição de ACK com `AttachmentId` antigo (`PtyError::AttachmentStale`) ou `consumed_bytes > sent_bytes` (`PtyError::AckInvalid`), e tratamento idempotente de ACK repetido/menor.
   - Verificação de `ack_timeout` (`5 s`) ativa somente quando `sent_bytes > consumed_bytes`, transicionando para `Detached` ao expirar.
3. Escrever testes unitários determinísticos com `MockClock` exercitando todas as arestas do diagrama de estados (`Flowing ↔ Paused → Detached → Flowing`).

### Passo 3: Ring Buffer de Replay e Cursores (`src/io/replay.rs`)
1. Criar `src-tauri/crates/mt-pty/src/io/replay.rs` com `StoredFrame`, `ReplayRing` e `ReplayPlan`.
2. Implementar `push_frame` com expulsão de frames inteiros quando `retained_bytes > capacity_bytes` (`1 MiB`) e `prepare_attach` com cálculo de `Gap` (`first_retained_seq`, `first_retained_offset`, `dropped_bytes`) e validação rigorosa de `OutputCursor`.
3. Escrever testes unitários validando:
   - Cursor `None` (início do stream), cursor exato no meio do ring, cursor truncado gerando `Gap` exato e cursor no futuro ou com `byte_offset` desalinhado retornando `PtyError::CursorInvalid`.

### Passo 4: Utilitários de I/O, Thread de Leitura e Thread de Escrita (`src/io/mod.rs`, `reader.rs`, `writer.rs`)
1. Criar `src-tauri/crates/mt-pty/src/io/mod.rs` com `EventFdWake` (usando `rustix::event::eventfd` com `EFD_CLOEXEC | EFD_NONBLOCK`) e `GlobalBufferPool` (orçamento atômico de `256 MiB`).
2. Criar `src-tauri/crates/mt-pty/src/io/reader.rs` implementando `ReaderWorker`:
   - Loop `poll(2)` monitorando o master FD e o `EventFdWake`.
   - Coalescing de `4 ms` ou `256 KiB` com fast-path para o primeiro byte após ociosidade.
   - Invocação de `TapRegistry::dispatch_output` uma única vez por frame original antes do envio ao `OutputSink`.
   - Entrega ordenada de `Gap` + `Replay` sob controle de crédito antes de retomar frames `Data` ao vivo.
   - Tratamento de `EINTR`, `EAGAIN`, `POLLHUP` (drenando bytes remanescentes) e `EIO` como EOF normal no Linux.
3. Criar `src-tauri/crates/mt-pty/src/io/writer.rs` implementando `WriteQueue` e `WriterWorker`:
   - Reserva atômica de vaga (`<= 1024` mensagens) e orçamento (`<= 4 MiB`, `<= 1 MiB` por pedido).
   - Escrita não bloqueante fatiada em blocos de `4 KiB` com `poll(POLLOUT, cancel_fd)` e cancelamento imediato ao fechar.
4. Escrever testes com par de descritores (`socketpair` / `openpty`) verificando:
   - Preservação de SHA-256 em escrita de 1 MiB e leitura de sequências UTF-8/ANSI fragmentadas (CA-08, CA-09).
   - Bloqueio do escritor do lado slave quando o sink não envia ACK e retomada sem perda ao confirmar (CA-11, CA-12).
   - Cancelamento imediato do `WriterWorker` quando o slave não lê stdin (CA-34).

### Passo 5: Benchmark de Vazão (`benches/throughput.rs`)
1. Configurar `[[bench]] name = "throughput" harness = false` em `src-tauri/crates/mt-pty/Cargo.toml`.
2. Implementar `src-tauri/crates/mt-pty/benches/throughput.rs` com `criterion`, medindo a vazão do pipeline (`ReaderWorker` + `ReplayRing` + `FlowControl` + `encode_data_frame`) sobre fixture determinística com `AckingSink`, `SlowSink` e `NullSink`.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificar formatação e clippy estrito (incluindo benches e testes)
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# 2. Executar todos os testes unitários e de proptest do pipeline de I/O
cargo test --manifest-path src-tauri/Cargo.toml -p mt-pty io::

# 3. Validar compilação do benchmark sem executá-lo por completo
cargo bench --manifest-path src-tauri/Cargo.toml -p mt-pty --no-run
```

---

## 3. Critérios de Aceite da Implementação 02

| Critério da Fase 02 | Descrição da Validação nesta Implementação |
|---------------------|--------------------------------------------|
| **CA-08** | Escrita fatiada em chunks de 4 KiB de 1 MiB e bytes `0..=255` produz SHA-256 idêntico no destino. |
| **CA-09** | Sequências UTF-8 multibyte e escapes ANSI divididos entre leituras chegam byte a byte íntegros nos frames `Data` e `Replay`. |
| **CA-10** | Benchmark `benches/throughput.rs` compila e valida integridade de hash e vazão core com `AckingSink`, `SlowSink` e `NullSink`. |
| **CA-11 / CA-12** | Sem ACK, `unacked` nunca ultrapassa `high_watermark` (1 MiB) e o `ReaderWorker` para de ler do master FD; ao enviar ACKs, a sequência `seq` prossegue sem lacunas ou duplicatas. |
| **CA-13** | Em `Detached`, `ReaderWorker` continua alimentando `ReplayRing` (limitado a 1 MiB) e `OutputTap`; novo `attach` emite `Gap` + `Replay` + `Data` continuamente. |
| **CA-14 / CA-15** | Coalescing respeita teto de 256 KiB por frame e janela de 4 ms, com fast-path ocioso para eco sub-10 ms. |
| **CA-33** | `OutputTap::on_output` recebe cada frame bruto original exatamente uma vez e não é invocado para frames de `Replay`. |
