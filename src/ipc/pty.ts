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
    type ShellIntegrationRequest,
    type SpawnSpec
} from '@/types/bindings';

/** Limite máximo de bytes por comando individual `pty_write` (1 MiB). */
export const MAX_WRITE_PAYLOAD_BYTES = 1024 * 1024;

/**
 * Divide uma string UTF-8 em fatias cuja representação em bytes UTF-8 nunca exceda `maxBytes`,
 * preservando fronteiras de code points Unicode (inclusive pares substitutos).
 */
function splitUtf8Chunks(text: string, maxBytes: number): string[] {
    const encoder = new TextEncoder();
    const totalBytes = encoder.encode(text);
    if (totalBytes.byteLength <= maxBytes) {
        return [text];
    }

    const chunks: string[] = [];
    let startIdx = 0;
    let currentBytes = 0;

    for (let i = 0; i < text.length; ) {
        const codePoint = text.codePointAt(i);
        if (codePoint === undefined) {
            break;
        }
        const charLen = codePoint > 0xffff ? 2 : 1;
        let byteLen = 4;
        if (codePoint <= 0x7f) {
            byteLen = 1;
        } else if (codePoint <= 0x7ff) {
            byteLen = 2;
        } else if (codePoint <= 0xffff) {
            byteLen = 3;
        }

        if (currentBytes + byteLen > maxBytes && i > startIdx) {
            chunks.push(text.slice(startIdx, i));
            startIdx = i;
            currentBytes = 0;
        }

        currentBytes += byteLen;
        i += charLen;
    }

    if (startIdx < text.length) {
        chunks.push(text.slice(startIdx));
    }

    return chunks;
}

/** Cria e inicia um novo PTY a partir da especificação e configuração opcional de integração de shell. */
export function ptySpawn(
    spec: SpawnSpec,
    integration?: ShellIntegrationRequest | null
): Promise<PtySummary> {
    return unwrap(commands.ptySpawn(spec, integration ?? null), 'pty_spawn');
}

/**
 * Anexa um consumidor de frames binários ao PTY indicado, opcionalmente retomando de `cursor`.
 */
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
    // Único cast da fronteira binária; válido conforme contrato T-06 (`InvokeResponseBody::Raw` -> `ArrayBuffer`).
    const info = await unwrap(
        commands.ptyAttach(
            ptyId,
            cursor ?? null,
            channel as unknown as Parameters<typeof commands.ptyAttach>[2]
        ),
        'pty_attach'
    );
    return { info, channel };
}

/** Desanexa explicitamente uma geração de consumidor de saída do PTY. */
export async function ptyDetach(ptyId: PtyId, attachmentId: AttachmentId): Promise<void> {
    await unwrap(commands.ptyDetach(ptyId, attachmentId), 'pty_detach');
}

/**
 * Envia entrada ao PTY. Se o payload codificado exceder 1 MiB (`MAX_WRITE_PAYLOAD_BYTES`),
 * fatia automaticamente em múltiplos comandos sequenciais respeitando fronteiras UTF-8.
 */
export async function ptyWrite(ptyId: PtyId, input: PtyInput): Promise<void> {
    if (input.kind === 'text') {
        if (input.text.length === 0) {
            return;
        }
        const chunks = splitUtf8Chunks(input.text, MAX_WRITE_PAYLOAD_BYTES);
        for (const chunk of chunks) {
            await unwrap(commands.ptyWrite(ptyId, { kind: 'text', text: chunk }), 'pty_write');
        }
        return;
    }

    if (input.bytes.length === 0) {
        return;
    }

    if (input.bytes.length <= MAX_WRITE_PAYLOAD_BYTES) {
        await unwrap(commands.ptyWrite(ptyId, input), 'pty_write');
        return;
    }

    for (let offset = 0; offset < input.bytes.length; offset += MAX_WRITE_PAYLOAD_BYTES) {
        const slice = input.bytes.slice(offset, offset + MAX_WRITE_PAYLOAD_BYTES);
        await unwrap(commands.ptyWrite(ptyId, { kind: 'bytes', bytes: slice }), 'pty_write');
    }
}

/** Redimensiona a janela do PTY (`rows` e `cols` entre `1` e `1000`). */
export async function ptyResize(ptyId: PtyId, size: PtySize): Promise<void> {
    await unwrap(commands.ptyResize(ptyId, size), 'pty_resize');
}

/** Solicita o encerramento do PTY com modo opcional (`graceful` ou `force`). */
export async function ptyKill(ptyId: PtyId, mode?: KillMode): Promise<void> {
    await unwrap(commands.ptyKill(ptyId, mode ?? null), 'pty_kill');
}

/** Registra confirmação cumulativa de bytes consumidos (`consumedBytes` decimal) para a conexão. */
export async function ptyAck(
    ptyId: PtyId,
    attachmentId: AttachmentId,
    consumedBytes: string
): Promise<void> {
    await unwrap(commands.ptyAck(ptyId, attachmentId, consumedBytes), 'pty_ack');
}

/** Lista todos os PTYs gerenciados pelo backend. */
export function ptyList(): Promise<PtySummary[]> {
    return unwrap(commands.ptyList(), 'pty_list');
}

/** Inspeciona a árvore de processos e o processo em foreground do PTY. */
export function ptyProcessInfo(ptyId: PtyId): Promise<PtyProcessReport> {
    return unwrap(commands.ptyProcessInfo(ptyId), 'pty_process_info');
}

/** Escuta o evento global `pty://exit`. */
export function onPtyExit(cb: (e: PtyExitEvent) => void): Promise<UnlistenFn> {
    return listen<PtyExitEvent>('pty://exit', (event) => {
        cb(event.payload);
    });
}

/** Escuta o evento global `pty://spawned`. */
export function onPtySpawned(cb: (e: PtySpawnedEvent) => void): Promise<UnlistenFn> {
    return listen<PtySpawnedEvent>('pty://spawned', (event) => {
        cb(event.payload);
    });
}

/** Escuta o evento global `pty://foreground-changed`. */
export function onPtyForegroundChanged(
    cb: (e: PtyForegroundChangedEvent) => void
): Promise<UnlistenFn> {
    return listen<PtyForegroundChangedEvent>('pty://foreground-changed', (event) => {
        cb(event.payload);
    });
}

/** Escuta o evento global `pty://cleanup-failed`. */
export function onPtyCleanupFailed(cb: (e: PtyCleanupFailedEvent) => void): Promise<UnlistenFn> {
    return listen<PtyCleanupFailedEvent>('pty://cleanup-failed', (event) => {
        cb(event.payload);
    });
}
