# Especificação Técnica — Implementação 06: Camada Frontend Headless: Decodificador Binário, Wrappers IPC, `usePtyStream` e `usePtyStore`

## 1. Contexto, Objetivo e Escopo

Esta é a sexta e última unidade de implementação da **Fase 02 — Core PTY e Supervisão de Processos**. Seu objetivo é construir a camada Frontend Headless (sem componentes visuais `.vue`, que pertencem à Fase 04) que consome os contratos gerados em `src/types/bindings.ts` e o protocolo binário v1 do `Channel`:
1. O decodificador de frames binários e codificador de bytes brutos (`src/ipc/ptyFrame.ts`): `PtyFrameKind`, `PtyFrame`, `PtyFrameError`, `decodeFrame`, `decodeEnd`, `decodeGap` e `encodeBinary`.
2. Os wrappers IPC tipados (`src/ipc/pty.ts`): `ptySpawn`, `ptyAttach`, `ptyDetach`, `ptyWrite` (com fatiamento transparente em blocos de 1 MiB quando uma colagem exceder `max_write_bytes`), `ptyResize`, `ptyKill`, `ptyAck`, `ptyList`, `ptyProcessInfo`, e os 4 listeners de eventos (`onPtyExit`, `onPtySpawned`, `onPtyForegroundChanged`, `onPtyCleanupFailed`).
3. O composable de streaming com backpressure `usePtyStream` (`src/composables/usePtyStream.ts`): fila FIFO local, aguardo da Promise de `consumer.write(bytes)` antes de avançar o cursor e contabilizar bytes consumidos, agregação de `ptyAck` cumulativo a cada `32 KiB` ou `16 ms` (via `setTimeout`, nunca `requestAnimationFrame`), retenção de frames que chegam antes da resolução de `AttachInfo`, isolamento de geração por token local e limpeza automática em `onScopeDispose`.
4. A store Pinia em memória `usePtyStore` (`src/stores/pty.ts`): gerenciamento reativo de `ptys` e `processReports`, inicialização idempotente (`init()`) com registro prévio de listeners e reconciliação monotônica por `revision`, preservação de tombstones e getters `alive`, `byTab` e `exitOf`.
5. A suíte completa de testes unitários e de integração frontend em Vitest (`src/ipc/__tests__/ptyFrame.spec.ts`, `src/composables/__tests__/usePtyStream.spec.ts`, `src/stores/__tests__/pty.spec.ts`).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Criados
- `src/ipc/ptyFrame.ts` — decodificador de frames binários v1 (`ArrayBuffer` → `PtyFrame`) e utilitários (`decodeEnd`, `decodeGap`, `encodeBinary`), além do tipo manual `export type PtyChannelMessage = ArrayBuffer;` — **exceção única** à regra "tipos só via `tauri-specta`" (contrato T-06, `05/spec.md` §3.1).
- `src/ipc/pty.ts` — wrappers tipados dos 9 comandos `pty_*` e dos 4 eventos `pty://*` usando o helper `unwrap` / `call` de `src/ipc/core.ts`.
- `src/composables/usePtyStream.ts` — composable de consumo de stream de PTY com controle de fluxo, cursor e reconexão.
- `src/stores/pty.ts` — store Pinia `usePtyStore` em memória para ciclo de vida e metadados de PTYs.
- `src/ipc/__tests__/ptyFrame.spec.ts` — testes Vitest do protocolo binário v1.
- `src/composables/__tests__/usePtyStream.spec.ts` — testes Vitest de streaming, ACK cumulativo, reconexão, consumidor lento/falho e inversão de ordem entre `End` e `pty://exit`.
- `src/stores/__tests__/pty.spec.ts` — testes Vitest da store `usePtyStore`, reconciliação de `revision`, tombstones e dispose.

---

## 3. Especificação Técnica Detalhada e Assinaturas TypeScript

Regras de estilo obrigatórias (`AGENTS.md` §3): recuo de 4 espaços, aspas simples (`'single'`), ponto e vírgula obrigatório (`;`), sem vírgula final (`comma-dangle: 'never'`).

### 3.1. Decodificador de Frames Binários (`src/ipc/ptyFrame.ts`)

```ts
import type { ExitInfo } from '@/types/bindings';

export const FRAME_VERSION = 1;
export const FRAME_HEADER_LEN = 24;
export const MAX_DATA_FRAME_BYTES = 256 * 1024;
export const GAP_PAYLOAD_LEN = 8;
export const MAX_END_PAYLOAD_LEN = 4 * 1024;

export const enum PtyFrameKind {
    Data = 1,
    Replay = 2,
    Gap = 3,
    End = 4
}

export interface PtyFrame {
    kind: PtyFrameKind;
    seq: bigint;
    byteOffset: bigint;
    payload: Uint8Array;
}

/**
 * Mensagem entregue pelo `Channel` de saída do PTY (`InvokeResponseBody::Raw` → `ArrayBuffer`).
 * EXCEÇÃO ÚNICA à regra "tipos só via tauri-specta": `BinaryPtyFrame` transporta bytes, não um DTO JSON.
 * A representação Specta deve ser validada pelo gate T-06 antes de integrar este wrapper.
 */
export type PtyChannelMessage = ArrayBuffer;

export class PtyFrameError extends Error {
    public readonly code = 'pty.frame_invalid';

    public constructor(message: string) {
        super(message);
        this.name = 'PtyFrameError';
    }
}

/**
 * Decodifica um frame binário v1 recebido do Channel Tauri sem decodificar o payload como texto.
 */
export function decodeFrame(buffer: ArrayBuffer): PtyFrame {
    if (buffer.byteLength < FRAME_HEADER_LEN) {
        throw new PtyFrameError(`Frame truncado: ${buffer.byteLength} bytes (mínimo ${FRAME_HEADER_LEN})`);
    }

    const view = new DataView(buffer);
    const version = view.getUint8(0);
    if (version !== FRAME_VERSION) {
        throw new PtyFrameError(`Versão de frame inválida: ${version}`);
    }

    const kindRaw = view.getUint8(1);
    if (kindRaw < PtyFrameKind.Data || kindRaw > PtyFrameKind.End) {
        throw new PtyFrameError(`Tipo de frame inválido: ${kindRaw}`);
    }

    const reserved = view.getUint16(2, true);
    if (reserved !== 0) {
        throw new PtyFrameError(`Campo reservado inválido: ${reserved}`);
    }

    const seq = view.getBigUint64(4, true);
    const byteOffset = view.getBigUint64(12, true);
    const len = view.getUint32(20, true);

    if (FRAME_HEADER_LEN + len !== buffer.byteLength) {
        throw new PtyFrameError(
            `Tamanho de payload inconsistente: esperado ${FRAME_HEADER_LEN + len}, recebido ${buffer.byteLength}`
        );
    }

    const kind = kindRaw as PtyFrameKind;
    if ((kind === PtyFrameKind.Data || kind === PtyFrameKind.Replay) && (len === 0 || len > MAX_DATA_FRAME_BYTES)) {
        throw new PtyFrameError(`Tamanho inválido para frame de dados: ${len}`);
    }
    if (kind === PtyFrameKind.Gap && len !== GAP_PAYLOAD_LEN) {
        throw new PtyFrameError(`Tamanho inválido para frame Gap: ${len}`);
    }
    if (kind === PtyFrameKind.End && (len === 0 || len > MAX_END_PAYLOAD_LEN)) {
        throw new PtyFrameError(`Tamanho inválido para frame End: ${len}`);
    }

    const payload = new Uint8Array(buffer, FRAME_HEADER_LEN, len);
    return {
        kind,
        seq,
        byteOffset,
        payload
    };
}

/**
 * Decodifica o payload JSON UTF-8 de um frame `End`.
 */
export function decodeEnd(frame: PtyFrame): ExitInfo {
    if (frame.kind !== PtyFrameKind.End) {
        throw new PtyFrameError(`Esperado frame End (4), recebido ${frame.kind}`);
    }
    const text = new TextDecoder('utf-8', { fatal: true }).decode(frame.payload);
    return JSON.parse(text) as ExitInfo;
}

/**
 * Decodifica a quantidade de bytes descartados (`u64 LE`) de um frame `Gap`.
 */
export function decodeGap(frame: PtyFrame): bigint {
    if (frame.kind !== PtyFrameKind.Gap || frame.payload.byteLength !== GAP_PAYLOAD_LEN) {
        throw new PtyFrameError('Frame Gap inválido');
    }
    const view = new DataView(frame.payload.buffer, frame.payload.byteOffset, frame.payload.byteLength);
    return view.getBigUint64(0, true);
}

/**
 * Converte uma string binária (code units 0..=255) em `Uint8Array` sem aplicar UTF-8.
 */
export function encodeBinary(data: string): Uint8Array {
    const out = new Uint8Array(data.length);
    for (let i = 0; i < data.length; i++) {
        const code = data.charCodeAt(i);
        if (code > 0xff) {
            throw new PtyFrameError(`Code unit fora do intervalo 0..255 no índice ${i}: ${code}`);
        }
        out[i] = code;
    }
    return out;
}
```

### 3.2. Wrappers IPC (`src/ipc/pty.ts`)

```ts
import { Channel } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import { decodeFrame, type PtyChannelMessage, type PtyFrame } from '@/ipc/ptyFrame';
import {
    commands,
    type AttachInfo,
    type AttachmentId,
    type KillMode,
    type OutputCursor,
    type PtyCleanupFailedEvent,
    type PtyExitEvent,
    type PtyForegroundChangedEvent,
    type PtyId,
    type PtyInput,
    type PtyProcessReport,
    type PtySize,
    type PtySpawnedEvent,
    type PtySummary,
    type SpawnSpec
} from '@/types/bindings';

export const MAX_WRITE_PAYLOAD_BYTES = 1024 * 1024;

export function ptySpawn(spec: SpawnSpec): Promise<PtySummary> {
    return unwrap(commands.ptySpawn(spec));
}

export async function ptyAttach(
    ptyId: PtyId,
    onFrame: (frame: PtyFrame) => void,
    cursor?: OutputCursor
): Promise<{ info: AttachInfo; channel: Channel<PtyChannelMessage> }> {
    const channel = new Channel<PtyChannelMessage>();
    channel.onmessage = (raw: PtyChannelMessage) => {
        const frame = decodeFrame(raw);
        onFrame(frame);
    };
    // Único cast da fronteira binária; válido apenas após a prova de transporte do gate T-06.
    const info = await unwrap(commands.ptyAttach(ptyId, cursor ?? null, channel as Parameters<typeof commands.ptyAttach>[2]));
    return { info, channel };
}

export function ptyDetach(ptyId: PtyId, attachmentId: AttachmentId): Promise<void> {
    return unwrap(commands.ptyDetach(ptyId, attachmentId));
}

/**
 * Envia entrada ao PTY. Se o payload codificado exceder 1 MiB (`max_write_bytes`),
 * fatia automaticamente em múltiplos comandos sequenciais respeitando fronteiras UTF-8.
 */
export async function ptyWrite(ptyId: PtyId, input: PtyInput): Promise<void>;
export function ptyResize(ptyId: PtyId, size: PtySize): Promise<void>;
export function ptyKill(ptyId: PtyId, mode?: KillMode): Promise<void>;
export function ptyAck(ptyId: PtyId, attachmentId: AttachmentId, consumedBytes: string): Promise<void>;
export function ptyList(): Promise<PtySummary[]>;
export function ptyProcessInfo(ptyId: PtyId): Promise<PtyProcessReport>;
export function onPtyExit(cb: (e: PtyExitEvent) => void): Promise<UnlistenFn>;
export function onPtySpawned(cb: (e: PtySpawnedEvent) => void): Promise<UnlistenFn>;
export function onPtyForegroundChanged(cb: (e: PtyForegroundChangedEvent) => void): Promise<UnlistenFn>;
export function onPtyCleanupFailed(cb: (e: PtyCleanupFailedEvent) => void): Promise<UnlistenFn>;
```

### 3.3. Composable `usePtyStream` (`src/composables/usePtyStream.ts`)

```ts
import { getCurrentScope, onScopeDispose, readonly, ref, watch, type DeepReadonly, type Ref } from 'vue';
import type { ExitInfo, OutputCursor, PtyId } from '@/types/bindings';

export interface PtyStreamConsumer {
    write: (bytes: Uint8Array) => Promise<void> | void;
    onGap?: (droppedBytes: bigint) => void;
    onEnd?: (exit: ExitInfo) => void;
    onError?: (err: unknown) => void;
}

export interface UsePtyStreamOptions {
    ptyId: Ref<PtyId | null>;
    consumer: PtyStreamConsumer;
    ackThresholdBytes?: number; // padrão: 32768 (32 KiB)
    ackFlushIntervalMs?: number; // padrão: 16 ms
}

export interface UsePtyStreamReturn {
    attached: Readonly<Ref<boolean>>;
    lastSeq: Readonly<Ref<bigint | null>>;
    ended: Readonly<Ref<ExitInfo | null>>;
    confirmedCursor: Readonly<Ref<OutputCursor | null>>;
    attach: () => Promise<void>;
    detach: () => Promise<void>;
}

export function usePtyStream(opts: UsePtyStreamOptions): UsePtyStreamReturn;
```

Regras críticas de `usePtyStream`:
1. **Isolamento por Geração (`generationToken`)**:
   - Cada chamada a `attach()` ou `detach()` incrementa um contador local `generationToken`.
   - O callback `onFrame` registrado no `Channel` antes do `invoke` de `ptyAttach` captura o `token` daquela tentativa.
   - Se frames chegarem pelo `Channel` **antes** que a Promise de `ptyAttach` resolva com `AttachInfo`, eles são enfileirados em `pendingFrames`; o envio de `ptyAck` aguarda `attachmentId` estar disponível.
   - Se `detach()` ou `onScopeDispose` ocorrer enquanto `ptyAttach` ainda está em voo, assim que `ptyAttach` resolver com `{ info }`, o composable chama imediatamente `ptyDetach(ptyId, info.attachmentId).catch(() => {})` e descarta todos os frames daquela geração.
2. **Processamento FIFO Estrito e Semântica de ACK (CA-35)**:
   - Os frames enfileirados são drenados sequencialmente por um loop assíncrono (`drainQueue`).
   - Para cada frame `Data` ou `Replay`:
     - Invoca `await opts.consumer.write(frame.payload)`.
     - **Somente após a resolução bem-sucedida de `consumer.write`**:
       - Atualiza `lastSeq.value = frame.seq` (para frames `Data` e `Replay`).
       - Atualiza `confirmedCursor.value = { nextSeq: (frame.seq + 1n).toString(), byteOffset: (frame.byteOffset + BigInt(frame.payload.byteLength)).toString() }`.
       - Incrementa `consumedBytesInAttachment += BigInt(frame.payload.byteLength)` e `unflushedAckBytes += frame.payload.byteLength`.
     - Se `consumer.write` lançar exceção ou rejeitar: **não** avança o cursor, **não** envia ACK para esse frame, executa `detach()` imediato e notifica `opts.consumer.onError?.(err)`.
   - Para frame `Gap`:
     - Extrai `droppedBytes = decodeGap(frame)`.
     - Avança `confirmedCursor.value = { nextSeq: frame.seq.toString(), byteOffset: frame.byteOffset.toString() }`.
     - Invoca `opts.consumer.onGap?.(droppedBytes)`.
   - Para frame `End`:
     - Decodifica `exitInfo = decodeEnd(frame)`.
     - Faz flush imediato de qualquer ACK pendente antes de finalizar.
     - Define `ended.value = exitInfo` e invoca `opts.consumer.onEnd?.(exitInfo)`.
3. **Agregação de ACK Cumulativo (`32 KiB` ou `16 ms`)**:
   - Quando `unflushedAckBytes >= ackThresholdBytes` (`32768`), dispara imediatamente `flushAck()` enviando `ptyAck(ptyId, attachmentId, consumedBytesInAttachment.toString())`.
   - Quando `0 < unflushedAckBytes < ackThresholdBytes`, agenda um `setTimeout(..., ackFlushIntervalMs)` (`16 ms`) para garantir o envio mesmo em janelas ocultas/minimizadas (onde `requestAnimationFrame` fica pausado).
   - Se um `ptyAck` falhar com código `pty.attachment_stale` (ex.: lease de 5 s expirou), invalida a geração local, limpa a fila pendente não consumida, aguarda qualquer `consumer.write` em andamento terminar e reconecta a partir de `confirmedCursor.value`.
4. **Troca de `ptyId` e `onScopeDispose`**:
   - Um `watch(opts.ptyId)` executa `detach()` do PTY anterior, reseta `lastSeq.value = null`, `ended.value = null`, `confirmedCursor.value = null` e, se o novo `ptyId` não for nulo, permite anexar ao novo PTY.
   - Se `getCurrentScope()` existir, registra `onScopeDispose(() => { void detach(); })`.

### 3.4. Store Pinia `usePtyStore` (`src/stores/pty.ts`)

```ts
import { defineStore } from 'pinia';
import type {
    ExitInfo,
    KillMode,
    PtyCleanupFailedEvent,
    PtyExitEvent,
    PtyForegroundChangedEvent,
    PtyId,
    PtyProcessReport,
    PtySpawnedEvent,
    PtySummary,
    SpawnSpec,
    TabId
} from '@/types/bindings';

export interface PtyStoreState {
    ptys: Record<PtyId, PtySummary>;
    processReports: Record<PtyId, PtyProcessReport>;
    initialized: boolean;
}
```

Regras da `usePtyStore`:
- **Getters**:
  - `alive(state): PtySummary[]` — filtra `Object.values(state.ptys)` cujo `status.kind` seja `'running'`, `'exiting'` ou `'cleanupFailed'`.
  - `byTab(state): (tabId: TabId) => PtySummary[]` — filtra PTYs cujo `owner.tabId === tabId`.
  - `exitOf(state): (ptyId: PtyId) => ExitInfo | null` — retorna `pty.status.exit` quando `pty.status.kind === 'exited'`, senão `null`.
- **Reconciliação por `revision` e `init()` Concorrente**:
  - Se `init()` for chamado concorrentemente enquanto já há uma inicialização em andamento, retorna a mesma `Promise<void>` em voo.
  - Em `init()`:
    1. Registra **primeiro** os 4 listeners (`onPtySpawned`, `onPtyExit`, `onPtyForegroundChanged`, `onPtyCleanupFailed`). Enquanto `ptyList()` não retorna, os eventos recebidos são retidos em um buffer temporário (limitado a 1 024 eventos). Se qualquer registro de listener ou `ptyList()` falhar, executa rollback chamando todos os `unlisten` já criados e propaga o erro.
    2. Chama `const snapshot = await ptyList()`.
    3. Aplica cada item do `snapshot` em `this.ptys[item.id]` usando `applySummaryWithRevision`.
    4. Reproduz na ordem os eventos do buffer temporário aplicando a regra monotônica de `BigInt(event.revision) > BigInt(existing.revision)` e a invariante de estado terminal: um PTY já em `status.kind === 'exited'` **nunca** regride para `'running'` por um snapshot ou evento atrasado.
    5. Marca `this.initialized = true`.
- **`handleExit(e: PtyExitEvent)`**:
  - Mantém o `PtySummary` em `this.ptys[e.ptyId]` como **tombstone** com `status = { kind: 'exited', exit: e.exit }`, `seq = e.finalSeq` e `revision = e.revision`.
- **`dispose()`**:
  - Invoca todos os `unlisten()`, limpa referências de listeners e define `this.initialized = false`.

---

## 4. Critérios de Aceite Mapeados

- **CA-03**: `npm run typecheck` (`tsgo`) e `npm run test` (Vitest) passam sem erros; `src/types/bindings.ts` está sincronizado.
- **CA-09 (frontend)**: `decodeFrame` retorna `Uint8Array` bruto sem decodificar texto; teste com `TextDecoder('utf-8', { stream: true })` reconstrói caracteres UTF-8 multibyte divididos entre frames sem corrupção.
- **CA-30**: Suíte Vitest valida `decodeFrame`/`decodeEnd`/`decodeGap`/`encodeBinary`, ACK cumulativo somente após `consumer.write`, ACKs duplicados/antigos/futuros, replay sob crédito, reconexão simultânea ao output, `detach` explícito, consumidor lento/falho, troca de `ptyId`, inversão de chegada entre `pty://exit` e frame `End`, store mantendo tombstone, reconciliação por `revision` e limpeza de listeners no `dispose`.
- **CA-35 (frontend)**: Teste comprova que enquanto a Promise de `consumer.write` está pendente, nenhum `ptyAck` é enviado para aquele frame (ACK representa consumo concluído, não entrega no transporte). Completar o gate T-06 de `05/spec.md` §3.1 com tipagem e prova binária em WebView real; os testes mockados não encerram esse gate. O teste de contrato deve localizar a única declaração manual de `PtyChannelMessage` em `src/ipc/ptyFrame.ts`, não em `src/types/`.
