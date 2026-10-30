# Plano de Execução — Implementação 06: Camada Frontend Headless: Decodificador Binário, Wrappers IPC, `usePtyStream` e `usePtyStore`

## 1. Checklist Sequencial de Implementação

### Passo 1: Codec Binário de Frames no Frontend (`src/ipc/ptyFrame.ts` e `src/ipc/__tests__/ptyFrame.spec.ts`)
1. Criar `src/ipc/ptyFrame.ts` implementando `PtyFrameKind`, `PtyFrame`, `PtyFrameError`, `decodeFrame`, `decodeEnd`, `decodeGap` e `encodeBinary`.
2. Criar `src/ipc/__tests__/ptyFrame.spec.ts` testando:
   - Decodificação de frames `Data`, `Replay`, `Gap` e `End` com `seq` e `byteOffset` em `bigint` (incluindo valores > `Number.MAX_SAFE_INTEGER`).
   - Rejeição de buffers menores que 24 bytes, `version !== 1`, `kind` inválido, `reserved !== 0` e `len` divergente do `byteLength`.
   - Preservação de sequências UTF-8 multibyte (ex.: emojis de 4 bytes e acentos pt-BR) cortadas entre dois frames consecutivos e reconstruídas com `new TextDecoder('utf-8', { stream: true })` (CA-09).
   - `encodeBinary` convertendo code units `0..=255` e rejeitando caracteres `> 255`.

### Passo 2: Wrappers IPC Tipados (`src/ipc/pty.ts`)
1. Criar `src/ipc/pty.ts` implementando `ptySpawn`, `ptyAttach`, `ptyDetach`, `ptyWrite`, `ptyResize`, `ptyKill`, `ptyAck`, `ptyList`, `ptyProcessInfo`, `onPtyExit`, `onPtySpawned`, `onPtyForegroundChanged` e `onPtyCleanupFailed`.
2. Em `ptyWrite`, implementar o fatiamento automático para entradas maiores que `1 MiB` (`MAX_WRITE_PAYLOAD_BYTES`), garantindo que strings UTF-8 grandes não sejam cortadas no meio de um code point (surrogate pair) entre fatias.
3. Após evidência Rust/exportador do gate T-06 (`05/spec.md` §3.1), em `ptyAttach`, instanciar `new Channel<PtyChannelMessage>()` (tipo manual `PtyChannelMessage = ArrayBuffer` de `ptyFrame.ts`, exceção única à regra de bindings), registrar `channel.onmessage` decodificando com `decodeFrame` antes de invocar `commands.ptyAttach` com o único cast localizado `channel as Parameters<typeof commands.ptyAttach>[2]` e retornar `{ info, channel }`.

### Passo 3: Composable `usePtyStream` (`src/composables/usePtyStream.ts` e `src/composables/__tests__/usePtyStream.spec.ts`)
1. Criar `src/composables/usePtyStream.ts` implementando:
   - Fila FIFO de frames por geração (`generationToken`).
   - Buffer de frames que chegam antes de `AttachInfo` resolver.
   - Aguardo estrito de `await consumer.write(frame.payload)` antes de avançar `confirmedCursor` e acumular bytes para `ptyAck` (CA-35).
   - Agregação de `ptyAck` cumulativo por limiar de bytes (`32 KiB`) ou timer (`setTimeout` de `16 ms`).
   - Reconexão automática a partir de `confirmedCursor` quando `pty.attachment_stale` ocorre.
   - Tratamento de `Gap`, `End`, falha em `consumer.write`, troca reativa de `ptyId` e limpeza em `onScopeDispose`.
2. Criar `src/composables/__tests__/usePtyStream.spec.ts` cobrindo todos os cenários do **CA-30** e **CA-35**:
   - Ordem estrita de frames `Gap → Replay → Data → End`.
   - Retenção de ACK enquanto a Promise de `consumer.write` está pendente e envio do ACK cumulativo assim que resolve.
   - Frames entregues pelo `Channel` antes da resolução da Promise de `ptyAttach`.
   - Reconexão (`attach` após `detach` ou expiração) enviando o `OutputCursor` confirmado exato.
   - Ignorar callbacks tardios de uma geração anterior após `detach` ou troca de `ptyId`.
   - Chegada do evento global `pty://exit` antes do frame `End` do Channel sem descartar os frames `Data` restantes na fila.
   - Rejeição em `consumer.write` interrompendo o ACK, mantendo o cursor no último frame válido e desanexando.

### Passo 4: Store Pinia `usePtyStore` (`src/stores/pty.ts` e `src/stores/__tests__/pty.spec.ts`)
1. Criar `src/stores/pty.ts` implementando a store `usePtyStore`:
   - Estado em memória (`ptys`, `processReports`, `initialized`).
   - Getters `alive`, `byTab` e `exitOf`.
   - Actions `init`, `spawn`, `kill`, `refreshProcessInfo`, `handleExit` e `dispose`.
   - Deduplicação de chamadas concorrentes a `init()`, rollback de listeners em caso de falha e reconciliação de eventos em buffer usando `revision` (`bigint`) sem regredir `exited` para `running`.
2. Criar `src/stores/__tests__/pty.spec.ts` cobrindo todos os cenários de store do **CA-30**:
   - `init()` idempotente e concorrente.
   - Reconciliação entre `ptyList()` e eventos `pty://spawned` / `pty://exit` que chegam durante o carregamento da lista.
   - Manutenção de tombstone após `pty://exit` e consulta via `exitOf(ptyId)` e `byTab(tabId)`.
   - Atualização de foreground via `pty://foreground-changed` e de status via `pty://cleanup-failed`.
   - Remoção completa de listeners ao chamar `dispose()`.

### Passo 5: Verificação Integrada Final da Fase 02
1. Executar `npm run typecheck` (`tsgo`), `npm run lint`, `npm run test` (Vitest) e `cargo test --workspace` para confirmar que todas as 6 implementações da Fase 02 funcionam perfeitamente integradas.
2. Completar o gate T-06 (`05/spec.md` §3.1): comprovar o `ArrayBuffer` em WebView real, preservação de bytes/ordem e ausência de JSON/base64, além de verificar a única declaração manual `PtyChannelMessage` em `src/ipc/ptyFrame.ts`. Registrar comandos, resultados e versões; sem essa prova CA-35 continua pendente, mesmo com mocks e typecheck aprovados.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificar tipagem estrita TypeScript (tsgo) e linter (ESLint)
npm run typecheck
npm run lint

# 2. Executar os testes unitários e de integração frontend no Vitest
npx --no-install vitest run src/ipc/__tests__/ptyFrame.spec.ts src/composables/__tests__/usePtyStream.spec.ts src/stores/__tests__/pty.spec.ts

# 3. Verificação completa da Fase 02 (Rust + Bindings + Frontend)
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --workspace -- --test-threads=4
git diff --exit-code src/types/bindings.ts
```

---

## 3. Critérios de Aceite da Implementação 06

| Critério da Fase 02 | Descrição da Validação nesta Implementação |
|---------------------|--------------------------------------------|
| **CA-03** | `npm run typecheck` (`tsgo`) e `npm run test` (Vitest) passam sem erros; `src/types/bindings.ts` permanece limpo no `git diff`. |
| **CA-09** | `decodeFrame` preserva `Uint8Array` intacto e `ptyFrame.spec.ts` comprova reconstrução de UTF-8 multibyte e ANSI divididos entre frames. |
| **CA-30** | `ptyFrame.spec.ts`, `usePtyStream.spec.ts` e `pty.spec.ts` cobrem e aprovam todos os cenários de frames, ACK cumulativo, replay, reconexão, `detach`, consumidor lento/falho, troca de `ptyId`, exit antecipado, tombstone, reconciliação de lista/eventos e liberação de listeners. |
| **CA-35 (TS)** | `usePtyStream.spec.ts` demonstra que a entrega no `Channel` não envia `ptyAck` enquanto a Promise de `consumer.write` não tiver concluído. |
