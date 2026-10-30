# Plano de Execução — Implementação 01: Contratos TypeScript, Tema, Stream Hub e Controle de Fluxo

> Especificação de referência: [`./spec.md`](./spec.md)
> Fase 04: Motor de Terminal Híbrido

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Instalar dependências npm do terminal e configurar capability `opener` no Tauri
1. Adicionar ao `package.json` (fixando versões compatíveis no lockfile):
   - `@xterm/xterm`
   - `@xterm/addon-webgl`
   - `@xterm/addon-fit`
   - `@xterm/addon-search`
   - `@xterm/addon-web-links`
   - `@xterm/addon-unicode11`
   - `vue-web-terminal`
   - `@tauri-apps/plugin-opener` (caso ainda não instalado na Fase 01)
2. Verificar `src-tauri/capabilities/default.json` e garantir a permissão restrita para abertura de URLs HTTP/HTTPS:
   ```json
   {
     "identifier": "opener:allow-open-url",
     "allow": [
       { "url": "http://*" },
       { "url": "https://*" }
     ]
   }
   ```
   Caso `tauri-plugin-opener` ainda não esteja registrado em `src-tauri/Cargo.toml` e `src-tauri/src/lib.rs`, registrá-lo sem adicionar lógica de domínio Rust.

### Passo 2: Criar contratos TypeScript e classes de erro (`src/types/terminal.ts`)
1. Criar `src/types/terminal.ts` exportando todas as interfaces, tipos e constantes definidos em `spec.md` §3.1:
   - `Disposable`, `RendererKind`, `InputMode`, `MouseTrackingMode`
   - `TerminalErrorCode`, `MtTerminalError`, `TriggerConflictError`
   - `TerminalPreferences`, `DEFAULT_TERMINAL_PREFERENCES`, `FlowConfig`, `DEFAULT_FLOW_CONFIG`
   - `TerminalModes`, `TerminalFlowState`, `TerminalSearchState`, `TerminalInstanceState`
   - `TerminalTheme`, `TerminalThemeProvider`
   - `TerminalContext`, `TerminalContextProvider`, `TerminalExtension`, `TerminalInstanceHandle`
   - `TerminalLinkHandler`, `InputModeHint`, `InputModeHintProvider`
   - `CompletionItem`, `CompletionSection`, `CompletionResult`, `TriggerQuery`, `TriggerSelectOutcome`, `TriggerDefinition`
   - `CommandBlockStatus`, `CommandBlockModel`, `CommandBlockFeed`, `BlockAction`
2. Garantir que `PaneId`, `PtyId`, `ProjectId`, `CommandBlockId`, `ExitInfo`, `OutputCursor` e `AttachmentId` sejam importados diretamente de `@/types/bindings` (sem redeclarar structs Rust).

### Passo 3: Implementar catálogo de atalhos (`src/terminal/keybindings.ts`) e Tema Padrão (`src/terminal/theme/`)
1. Criar `src/terminal/keybindings.ts` com `TERMINAL_KEYBINDINGS` e os predicados puros `isCopyShortcut`, `isPasteShortcut`, `isSearchShortcut`, `isToggleInputModeShortcut` e `isInterruptShortcut`.
2. Criar `src/terminal/theme/defaultTheme.ts` implementando `resolveCssToken` e `createMaxNavyTealTheme()`, mapeando os tokens CSS `--max-*` e `--background-*` para a estrutura `TerminalTheme` (paleta ANSI de 16 cores + cores de UI `blockBorder`, `blockHeaderBackground`, `richInputBackground`).
3. Criar `src/terminal/theme/themeService.ts` com `themeService` (`getActiveTheme`, `setThemeProvider`, `onChange`, `notifyThemeChanged`).

### Passo 4: Implementar o adaptador IPC fino (`src/terminal/stream/ptyClient.ts`)
1. Criar `src/terminal/stream/ptyClient.ts` encapsulando todas as chamadas de `src/ipc/pty.ts` (`ptySpawn`, `ptyAttach`, `ptyDetach`, `ptyAck`, `ptyWrite`, `ptyResize`, `ptyKill`, `ptyList`, `onPtyExit`, `onPtyCleanupFailed`) e reexportando os helpers de frame de `src/ipc/ptyFrame.ts` (`PtyFrameKind`, `PtyFrameError`, `decodeFrame`, `decodeGap`, `decodeEnd`, `encodeBinary`).
2. Garantir que nenhum outro arquivo em `src/terminal/**` importe `src/ipc/pty.ts` diretamente.

### Passo 5: Implementar `FlowController` (`src/terminal/engine/flowController.ts`)
1. Implementar a classe `FlowController` conforme `spec.md` §3.5:
   - Rastreamento separado de `pendingBytes` (recebidos, ainda não parseados) e `consumedTotal: bigint` (parseados em `onWritten`).
   - Disparo de `sink.ack(consumedTotal.toString())` somente após `onWritten`, ao atingir `ackThresholdBytes` (`32 KiB`) ou pelo timer de `ackIntervalMs` (`16 ms`).
   - Coalescência serializada (no máximo 1 `ack` em voo por vez; se `consumedTotal` avançar durante o request, envia o novo total acumulado ao finalizar).
   - Supressão de envio antes de `setAttachmentReady(true)`.
   - `reset()` exclusivo para mudança de `attachmentId`.

### Passo 6: Implementar `WriteBatcher` (`src/terminal/engine/writeBatcher.ts`)
1. Implementar a classe `WriteBatcher` conforme `spec.md` §3.6:
   - Fila FIFO de frames `{ seq, byteOffset, bytes, onParsed }` com teto estrito de `1 MiB` (`queuedBytes + inFlightBytes`).
   - Agendamento por `requestAnimationFrame` quando visível (`visible && !document.hidden`) e `setTimeout(16)` quando oculto, trocando imediatamente o agendador em `setVisible(v)`.
   - Concatenação de frames inteiros até `frameBudgetBytes` (`256 KiB`) em um único `Uint8Array` pré-alocado por `flush()`.
   - Garantia de no máximo 1 `target.write(batch, cb)` em voo; avanço de `confirmedCursor = { nextSeq: (seq + 1n).toString(), byteOffset: (byteOffset + BigInt(len)).toString() }` somente dentro do callback `cb` de parse do xterm.

### Passo 7: Implementar `PtyStreamHub` (`src/terminal/stream/ptyStreamHub.ts`)
1. Implementar `createPtyStreamHub()` e o singleton `ptyStreamHub` conforme `spec.md` §3.7:
   - `spawn(spec)` desanexado (sem Channel/ACK).
   - `subscribe(ptyId, sink, options)` com verificação de assinante único (`MtTerminalError('stream.already_subscribed')`), token de geração, fila pré-`AttachInfo` e `ptyDetach` garantido mesmo se `dispose()` ocorrer antes da resolução de `ptyAttach`.
   - Ordem estrita de processamento: `Gap?` → `Replay*` → `Data*` → `End`, aguardando a resolução de todos os `sink.onData` anteriores antes de invocar `sink.onExit(exit)`.
   - Sincronização com `ptyClient.onExit` sem descartar bytes pendentes na fila antes de `End`/`finalSeq`.

### Passo 8: Implementar `InputPipeline` (`src/terminal/engine/inputPipeline.ts`)
1. Implementar `InputPipeline` conforme `spec.md` §3.8:
   - Coalescência de `onData` via `queueMicrotask` e preservação de bytes `0x00..0xff` em `onBinary` via `encodeBinary` (`PtyInput = { kind: 'bytes', bytes }`).
   - Fatiamento seguro em blocos de `64 KiB` UTF-8 (sem partir surrogate pairs UTF-16) ou binários.
   - Orçamento de fila de `1 MiB`, retry de `10 ms` (até 50×) restrito ao código `pty.write_queue_full`, `paste(text)` delegando ao `pasteToXterm`, `sendLine(text)` com bloqueio de `isComposing` e deduplicação de duplo `Enter`, e `cancelPendingPasteAndInterrupt()` fechando envelope `\x1b[201~` se necessário antes de `\x03`.

### Passo 9: Criar Mock do Xterm e Suíte de Testes Unitários (`tests/terminal/`)
1. Criar `tests/terminal/mocks/xtermMock.ts` fornecendo `createMockTerminal()`, `encodeTestFrame(kind, seq, byteOffset, payload)` e controles determinísticos para o callback de `term.write(data, cb)`.
2. Criar e executar os 4 arquivos de teste obrigatórios desta etapa:
   - `tests/terminal/writeBatcher.spec.ts`
   - `tests/terminal/flowController.spec.ts`
   - `tests/terminal/ptyStreamHub.spec.ts`
   - `tests/terminal/inputPipeline.spec.ts`

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificação de tipos TypeScript (tsgo)
npm run typecheck

# 2. Execução dos testes unitários da Implementação 01
npx --no-install vitest run tests/terminal/writeBatcher.spec.ts tests/terminal/flowController.spec.ts tests/terminal/ptyStreamHub.spec.ts tests/terminal/inputPipeline.spec.ts

# 3. Verificação de lint (ESLint)
npm run lint

# 4. Verificação do workspace Rust (caso capabilities/Cargo.toml tenham sido tocados)
cargo clippy --workspace -- -D warnings
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 04)

- **[CA-5.1.1]** `npm run typecheck` (`tsgo --noEmit`) e `npm run lint` executam com zero erros nos novos módulos.
- **[CA-5.1.3a — `writeBatcher.spec.ts`]** N chunks no mesmo frame resultam em exatamente **1** `term.write`; orçamento de `256 KiB` por frame é respeitado (excedente fica para o próximo frame); ordem de bytes preservada; instância oculta (`visible = false` ou `document.hidden`) usa `setTimeout(16)` em vez de `requestAnimationFrame`; apenas 1 write em voo por vez.
- **[CA-5.1.3b — `flowController.spec.ts`]** `pty_ack` só é chamado **depois** do callback de parse (`onWritten`), nunca em `onEnqueued`; emite 1 ack ao acumular `32 KiB`; emite ack após `16 ms` quando há resto não confirmado; acks cumulativos de `Replay` + `Data` são serializados; `Gap` e `End` não somam crédito; `reset()` só ocorre ao trocar `attachmentId`.
- **[CA-5.1.3c — `ptyStreamHub.spec.ts`]** `spawn` desanexado não cria `Channel` nem envia `pty_ack`; `Replay` precede `Data`; reanexação da mesma instância usa cursor confirmado `{ nextSeq, byteOffset }`; `Gap` aciona `onGap` sem injetar bytes sintéticos no parser; `End` aguarda callbacks anteriores de `onData` antes de emitir `onExit`; `dispose` durante `attach` em voo chama `ptyDetach` assim que resolver; `u64` acima de `Number.MAX_SAFE_INTEGER` preserva precisão exata em `bigint`/string decimal.
- **[CA-5.1.5 / CA-5.1.6 — `inputPipeline.spec.ts`]** Bytes `0x80` e `0xff` em `onBinary` chegam intactos via `PtyInput.kind = 'bytes'`; colagens > `64 KiB` são fatiadas sem quebrar pares substitutos UTF-16; `pty.write_queue_full` retenta apenas a fatia rejeitada; `Enter` com `isComposing: true` ou duplo `Enter` não duplica submissão; excesso de `1 MiB` rejeita a operação inteira sem corromper a fila.
