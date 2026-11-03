import { getCurrentScope, onScopeDispose, readonly, ref, watch, type Ref } from 'vue';
import { isMtIpcError } from '@/ipc/core';
import { ptyAck, ptyAttach, ptyDetach } from '@/ipc/pty';
import { decodeEnd, decodeGap, PtyFrameKind, type PtyFrame } from '@/ipc/ptyFrame';
import type { AttachmentId, ExitInfo, OutputCursor, PtyId } from '@/types/bindings';

/** Limiar padrão de bytes acumulados para envio imediato de `pty_ack` (32 KiB). */
export const DEFAULT_ACK_THRESHOLD_BYTES = 32 * 1024;

/** Intervalo máximo padrão para flush de `pty_ack` via `setTimeout` (16 ms). */
export const DEFAULT_ACK_FLUSH_INTERVAL_MS = 16;

/** Consumidor de stream de saída binária do PTY. */
export interface PtyStreamConsumer {
    write: (bytes: Uint8Array) => Promise<void> | void;
    onGap?: (droppedBytes: bigint) => void;
    onEnd?: (exit: ExitInfo) => void;
    onError?: (err: unknown) => void;
}

/** Opções de configuração do composable `usePtyStream`. */
export interface UsePtyStreamOptions {
    ptyId: Ref<PtyId | null>;
    consumer: PtyStreamConsumer;
    ackThresholdBytes?: number;
    ackFlushIntervalMs?: number;
}

/** Retorno reativo e controles imperativos de `usePtyStream`. */
export interface UsePtyStreamReturn {
    attached: Readonly<Ref<boolean>>;
    lastSeq: Readonly<Ref<bigint | null>>;
    ended: Readonly<Ref<ExitInfo | null>>;
    confirmedCursor: Readonly<Ref<OutputCursor | null>>;
    attach: () => Promise<void>;
    detach: () => Promise<void>;
}

/**
 * Gerencia a conexão de streaming binário de um PTY com controle de fluxo baseado em créditos,
 * fila FIFO estrita, agregação de ACKs cumulativos e reconexão por cursor confirmado.
 */
export function usePtyStream(opts: UsePtyStreamOptions): UsePtyStreamReturn {
    const ackThresholdBytes = opts.ackThresholdBytes ?? DEFAULT_ACK_THRESHOLD_BYTES;
    const ackFlushIntervalMs = opts.ackFlushIntervalMs ?? DEFAULT_ACK_FLUSH_INTERVAL_MS;

    const attached = ref<boolean>(false);
    const lastSeq = ref<bigint | null>(null);
    const ended = ref<ExitInfo | null>(null);
    const confirmedCursor = ref<OutputCursor | null>(null);

    let generationToken = 0;
    let activePtyId: PtyId | null = null;
    let activeAttachmentId: AttachmentId | null = null;
    let frameQueue: PtyFrame[] = [];
    let isDraining = false;
    let currentWritePromise: Promise<void> | null = null;

    let consumedBytesInAttachment = 0n;
    let lastFlushedAckBytes = 0n;
    let unflushedAckBytes = 0;
    let ackTimer: ReturnType<typeof setTimeout> | null = null;

    function clearAckTimer(): void {
        if (ackTimer !== null) {
            clearTimeout(ackTimer);
            ackTimer = null;
        }
    }

    async function handleStaleAttachment(token: number): Promise<void> {
        if (token !== generationToken) {
            return;
        }
        generationToken += 1;
        clearAckTimer();
        frameQueue = [];
        activeAttachmentId = null;
        attached.value = false;
        unflushedAckBytes = 0;

        if (currentWritePromise !== null) {
            try {
                await currentWritePromise;
            } catch {
                // Ignora erro de escrita já tratado pelo loop principal
            }
        }

        if (opts.ptyId.value !== null) {
            await attach();
        }
    }

    async function flushAck(token: number): Promise<void> {
        if (token !== generationToken) {
            return;
        }
        clearAckTimer();

        if (
            activePtyId === null ||
            activeAttachmentId === null ||
            consumedBytesInAttachment <= lastFlushedAckBytes
        ) {
            return;
        }

        const targetPtyId = activePtyId;
        const targetAttachmentId = activeAttachmentId;
        const cumulativeToSend = consumedBytesInAttachment;
        lastFlushedAckBytes = cumulativeToSend;
        unflushedAckBytes = 0;

        try {
            await ptyAck(targetPtyId, targetAttachmentId, cumulativeToSend.toString());
        } catch (err: unknown) {
            if (token !== generationToken) {
                return;
            }
            if (isMtIpcError(err) && err.code === 'pty.attachment_stale') {
                await handleStaleAttachment(token);
                return;
            }
            opts.consumer.onError?.(err);
        }
    }

    function scheduleOrFlushAck(token: number): void {
        if (token !== generationToken || unflushedAckBytes <= 0) {
            return;
        }
        if (activeAttachmentId === null) {
            return;
        }
        if (unflushedAckBytes >= ackThresholdBytes) {
            void flushAck(token);
            return;
        }
        if (ackTimer === null) {
            ackTimer = setTimeout(() => {
                ackTimer = null;
                void flushAck(token);
            }, ackFlushIntervalMs);
        }
    }

    async function drainQueue(token: number): Promise<void> {
        if (isDraining || token !== generationToken) {
            return;
        }
        isDraining = true;

        try {
            while (frameQueue.length > 0 && token === generationToken) {
                const frame = frameQueue.shift();
                if (frame === undefined) {
                    break;
                }

                if (frame.kind === PtyFrameKind.Data || frame.kind === PtyFrameKind.Replay) {
                    try {
                        const writeResult = opts.consumer.write(frame.payload);
                        if (writeResult instanceof Promise) {
                            currentWritePromise = writeResult;
                            await writeResult;
                            currentWritePromise = null;
                        }
                    } catch (err: unknown) {
                        currentWritePromise = null;
                        if (token === generationToken) {
                            await detach();
                            opts.consumer.onError?.(err);
                        }
                        return;
                    }

                    if (token !== generationToken) {
                        return;
                    }

                    const payloadLen = frame.payload.byteLength;
                    lastSeq.value = frame.seq;
                    confirmedCursor.value = {
                        nextSeq: (frame.seq + 1n).toString(),
                        byteOffset: (frame.byteOffset + BigInt(payloadLen)).toString()
                    };

                    consumedBytesInAttachment += BigInt(payloadLen);
                    unflushedAckBytes += payloadLen;
                    scheduleOrFlushAck(token);
                } else if (frame.kind === PtyFrameKind.Gap) {
                    let droppedBytes: bigint;
                    try {
                        droppedBytes = decodeGap(frame);
                    } catch (err: unknown) {
                        await detach();
                        opts.consumer.onError?.(err);
                        return;
                    }

                    if (token !== generationToken) {
                        return;
                    }

                    confirmedCursor.value = {
                        nextSeq: frame.seq.toString(),
                        byteOffset: frame.byteOffset.toString()
                    };
                    opts.consumer.onGap?.(droppedBytes);
                } else if (frame.kind === PtyFrameKind.End) {
                    let exitInfo: ExitInfo;
                    try {
                        exitInfo = decodeEnd(frame);
                    } catch (err: unknown) {
                        await detach();
                        opts.consumer.onError?.(err);
                        return;
                    }

                    if (token !== generationToken) {
                        return;
                    }

                    if (unflushedAckBytes > 0 && activeAttachmentId !== null) {
                        await flushAck(token);
                    }

                    if (token !== generationToken) {
                        return;
                    }

                    ended.value = exitInfo;
                    opts.consumer.onEnd?.(exitInfo);
                }
            }
        } finally {
            if (token === generationToken) {
                isDraining = false;
                if (frameQueue.length > 0) {
                    void drainQueue(token);
                }
            } else {
                isDraining = false;
            }
        }
    }

    async function attach(): Promise<void> {
        const targetPtyId = opts.ptyId.value;
        if (targetPtyId === null) {
            return;
        }

        if (attached.value && activeAttachmentId !== null && activePtyId !== null) {
            const prevPtyId = activePtyId;
            const prevAttachmentId = activeAttachmentId;
            await ptyDetach(prevPtyId, prevAttachmentId).catch(() => {});
        }

        clearAckTimer();
        generationToken += 1;
        const myToken = generationToken;

        activePtyId = targetPtyId;
        activeAttachmentId = null;
        frameQueue = [];
        isDraining = false;
        consumedBytesInAttachment = 0n;
        lastFlushedAckBytes = 0n;
        unflushedAckBytes = 0;
        attached.value = false;

        const cursorToUse = confirmedCursor.value ?? undefined;

        try {
            const { info } = await ptyAttach(
                targetPtyId,
                (frame: PtyFrame) => {
                    if (myToken !== generationToken) {
                        return;
                    }
                    frameQueue.push(frame);
                    void drainQueue(myToken);
                },
                cursorToUse
            );

            if (myToken !== generationToken) {
                await ptyDetach(targetPtyId, info.attachmentId).catch(() => {});
                return;
            }

            activeAttachmentId = info.attachmentId;
            attached.value = true;

            if (unflushedAckBytes > 0) {
                if (ended.value !== null) {
                    await flushAck(myToken);
                } else {
                    scheduleOrFlushAck(myToken);
                }
            }
            if (frameQueue.length > 0 && !isDraining) {
                void drainQueue(myToken);
            }
        } catch (err: unknown) {
            if (myToken === generationToken) {
                attached.value = false;
                activeAttachmentId = null;
                opts.consumer.onError?.(err);
            }
            throw err;
        }
    }

    async function detach(): Promise<void> {
        generationToken += 1;
        clearAckTimer();
        frameQueue = [];
        isDraining = false;

        const prevPtyId = activePtyId;
        const prevAttachmentId = activeAttachmentId;

        activePtyId = null;
        activeAttachmentId = null;
        consumedBytesInAttachment = 0n;
        lastFlushedAckBytes = 0n;
        unflushedAckBytes = 0;
        attached.value = false;

        if (prevPtyId !== null && prevAttachmentId !== null) {
            await ptyDetach(prevPtyId, prevAttachmentId).catch(() => {});
        }
    }

    watch(
        opts.ptyId,
        async (newId, oldId) => {
            if (newId === oldId) {
                return;
            }
            await detach();
            lastSeq.value = null;
            ended.value = null;
            confirmedCursor.value = null;
            if (newId !== null) {
                await attach().catch(() => {});
            }
        }
    );

    if (getCurrentScope()) {
        onScopeDispose(() => {
            void detach();
        });
    }

    return {
        attached: readonly(attached),
        lastSeq: readonly(lastSeq),
        ended: readonly(ended),
        confirmedCursor: readonly(confirmedCursor),
        attach,
        detach
    };
}
