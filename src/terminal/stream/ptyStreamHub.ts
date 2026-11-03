import type { UnlistenFn } from '@tauri-apps/api/event';
import type {
    ExitInfo,
    OutputCursor,
    PtyExitEvent,
    PtyId,
    PtySummary,
    ShellIntegrationRequest,
    SpawnSpec
} from '@/types/bindings';
import { MtTerminalError, type Disposable } from '@/types/terminal';
import {
    PtyFrameKind,
    decodeEnd,
    decodeGap,
    ptyClient,
    type PtyFrame
} from './ptyClient';

/** Consumidor de frames decodificados de um PTY no `PtyStreamHub`. */
export interface PtySink {
    onData(frame: {
        seq: bigint;
        byteOffset: bigint;
        bytes: Uint8Array;
        isReplay: boolean;
    }): Promise<void>;
    onGap(droppedBytes: bigint, nextCursor: { nextSeq: string; byteOffset: string }): void;
    onExit(exit: ExitInfo): void;
    onFrameError?(error: Error): void;
    onCleanupFailed?(survivorsCount: number): void;
}

/** Opções de assinatura de um PTY no `PtyStreamHub`. */
export interface SubscribeOptions {
    cursor?: OutputCursor | null | undefined;
    onAttachmentReady?: ((attachmentId: string, truncated: boolean) => void) | undefined;
    onAckError?: ((error: unknown) => void) | undefined;
}

/** Assinatura ativa de um PTY retornada por `PtyStreamHub.subscribe`. */
export interface PtyStreamSubscription extends Disposable {
    readonly attachmentId: string | null;
    ack(consumedBytes: string): Promise<void>;
}

/** Hub central de roteamento e controle de geração de streams binários do PTY. */
export interface PtyStreamHub {
    spawn(spec: SpawnSpec, integration?: ShellIntegrationRequest | null): Promise<PtySummary>;
    subscribe(
        ptyId: PtyId,
        sink: PtySink,
        options?: SubscribeOptions
    ): Promise<PtyStreamSubscription>;
    release(ptyId: PtyId): void;
    getActiveSubscriptionCount(): number;
    resetForTests(): void;
}

interface ActiveSlot {
    readonly ptyId: PtyId;
    readonly token: symbol;
    readonly sink: PtySink;
    readonly options: SubscribeOptions | undefined;
    attachmentId: string | null;
    disposed: boolean;
    exitDelivered: boolean;
    endReceived: boolean;
    lastProcessedSeq: bigint;
    expectedByteOffset: bigint | null;
    pendingAck: string | null;
    pendingExitFromEvent: { exit: ExitInfo; finalSeq: bigint } | null;
    processingChain: Promise<void>;
}

/** Cria uma nova instância isolada do `PtyStreamHub`. */
export function createPtyStreamHub(): PtyStreamHub {
    const activeSlots = new Map<PtyId, ActiveSlot>();
    const bufferedExitEvents = new Map<PtyId, { exit: ExitInfo; finalSeq: bigint }>();

    let unlistenExit: UnlistenFn | null = null;
    let unlistenCleanup: UnlistenFn | null = null;
    let listenersInitPromise: Promise<void> | null = null;

    async function ensureGlobalListeners(): Promise<void> {
        if (unlistenExit && unlistenCleanup) {
            return;
        }
        if (listenersInitPromise) {
            await listenersInitPromise;
            return;
        }
        listenersInitPromise = (async () => {
            if (!unlistenExit) {
                unlistenExit = await ptyClient.onExit((event: PtyExitEvent) => {
                    handleGlobalExitEvent(event);
                });
            }
            if (!unlistenCleanup) {
                unlistenCleanup = await ptyClient.onCleanupFailed((event) => {
                    const slot = activeSlots.get(event.ptyId);
                    if (slot && !slot.disposed) {
                        slot.sink.onCleanupFailed?.(event.survivors.length);
                    }
                });
            }
        })();
        try {
            await listenersInitPromise;
        } finally {
            listenersInitPromise = null;
        }
    }

    function maybeTeardownGlobalListeners(): void {
        if (activeSlots.size > 0) {
            return;
        }
        if (unlistenExit) {
            unlistenExit();
            unlistenExit = null;
        }
        if (unlistenCleanup) {
            unlistenCleanup();
            unlistenCleanup = null;
        }
    }

    function deliverExitIfReady(slot: ActiveSlot, exit: ExitInfo): void {
        if (slot.disposed || slot.exitDelivered) {
            return;
        }
        slot.exitDelivered = true;
        slot.sink.onExit(exit);
    }

    function handleGlobalExitEvent(event: PtyExitEvent): void {
        let finalSeq = 0n;
        try {
            finalSeq = BigInt(event.finalSeq);
        } catch {
            finalSeq = 0n;
        }
        const record = { exit: event.exit, finalSeq };
        bufferedExitEvents.set(event.ptyId, record);

        const slot = activeSlots.get(event.ptyId);
        if (!slot || slot.disposed) {
            return;
        }
        slot.pendingExitFromEvent = record;

        slot.processingChain = slot.processingChain.then(() => {
            if (slot.disposed || slot.exitDelivered) {
                return;
            }
            if (slot.endReceived || slot.lastProcessedSeq >= finalSeq) {
                deliverExitIfReady(slot, event.exit);
            }
        });
    }

    function enqueueFrame(slot: ActiveSlot, frame: PtyFrame): void {
        if (slot.disposed || activeSlots.get(slot.ptyId)?.token !== slot.token) {
            return;
        }

        slot.processingChain = slot.processingChain.then(async () => {
            if (slot.disposed || activeSlots.get(slot.ptyId)?.token !== slot.token) {
                return;
            }

            try {
                if (frame.kind === PtyFrameKind.Gap) {
                    const droppedBytes = decodeGap(frame);
                    slot.expectedByteOffset = frame.byteOffset;
                    if (frame.seq > 0n) {
                        slot.lastProcessedSeq = frame.seq - 1n;
                    }
                    slot.sink.onGap(droppedBytes, {
                        nextSeq: frame.seq.toString(10),
                        byteOffset: frame.byteOffset.toString(10)
                    });
                    return;
                }

                if (frame.kind === PtyFrameKind.Replay || frame.kind === PtyFrameKind.Data) {
                    if (
                        slot.expectedByteOffset !== null &&
                        frame.byteOffset < slot.expectedByteOffset
                    ) {
                        // Frame duplicado/anterior ao cursor já processado; ignorar com segurança.
                        return;
                    }
                    slot.expectedByteOffset = frame.byteOffset + BigInt(frame.payload.byteLength);
                    if (frame.seq > slot.lastProcessedSeq) {
                        slot.lastProcessedSeq = frame.seq;
                    }
                    await slot.sink.onData({
                        seq: frame.seq,
                        byteOffset: frame.byteOffset,
                        bytes: frame.payload,
                        isReplay: frame.kind === PtyFrameKind.Replay
                    });

                    if (
                        slot.pendingExitFromEvent &&
                        !slot.exitDelivered &&
                        slot.lastProcessedSeq >= slot.pendingExitFromEvent.finalSeq
                    ) {
                        deliverExitIfReady(slot, slot.pendingExitFromEvent.exit);
                    }
                    return;
                }

                if (frame.kind === PtyFrameKind.End) {
                    const exitInfo = decodeEnd(frame);
                    slot.endReceived = true;
                    deliverExitIfReady(slot, exitInfo);
                }
            } catch (err: unknown) {
                const errorObj =
                    err instanceof Error
                        ? err
                        : new MtTerminalError('stream.frame_invalid', 'Frame binário inválido.');
                slot.disposed = true;
                slot.sink.onFrameError?.(errorObj);
                if (slot.attachmentId) {
                    const attId = slot.attachmentId;
                    slot.attachmentId = null;
                    void ptyClient.detach(slot.ptyId, attId).catch(() => {});
                }
            }
        });
    }

    return {
        spawn(spec: SpawnSpec, integration?: ShellIntegrationRequest | null): Promise<PtySummary> {
            return ptyClient.spawn(spec, integration);
        },

        async subscribe(
            ptyId: PtyId,
            sink: PtySink,
            options?: SubscribeOptions
        ): Promise<PtyStreamSubscription> {
            const existing = activeSlots.get(ptyId);
            if (existing && !existing.disposed) {
                throw new MtTerminalError(
                    'stream.already_subscribed',
                    `O PTY "${ptyId}" já possui um consumidor anexado no PtyStreamHub.`
                );
            }

            const token = Symbol(`pty-sub-${ptyId}`);
            const slot: ActiveSlot = {
                ptyId,
                token,
                sink,
                options,
                attachmentId: null,
                disposed: false,
                exitDelivered: false,
                endReceived: false,
                lastProcessedSeq: 0n,
                expectedByteOffset: null,
                pendingAck: null,
                pendingExitFromEvent: bufferedExitEvents.get(ptyId) ?? null,
                processingChain: Promise.resolve()
            };

            activeSlots.set(ptyId, slot);
            await ensureGlobalListeners();

            const subscription: PtyStreamSubscription = {
                get attachmentId(): string | null {
                    return slot.attachmentId;
                },
                async ack(consumedBytes: string): Promise<void> {
                    if (slot.disposed || activeSlots.get(ptyId)?.token !== slot.token) {
                        return;
                    }
                    if (!slot.attachmentId) {
                        slot.pendingAck = consumedBytes;
                        return;
                    }
                    try {
                        await ptyClient.ack(ptyId, slot.attachmentId, consumedBytes);
                    } catch (err: unknown) {
                        options?.onAckError?.(err);
                        throw err;
                    }
                },
                dispose(): void {
                    if (slot.disposed) {
                        return;
                    }
                    slot.disposed = true;
                    if (activeSlots.get(ptyId)?.token === slot.token) {
                        activeSlots.delete(ptyId);
                    }
                    if (slot.attachmentId) {
                        const attId = slot.attachmentId;
                        slot.attachmentId = null;
                        void ptyClient.detach(ptyId, attId).catch(() => {});
                    }
                    maybeTeardownGlobalListeners();
                }
            };

            try {
                const cursorArg = options?.cursor ?? undefined;
                const { info } = await ptyClient.attach(
                    ptyId,
                    (frame) => {
                        enqueueFrame(slot, frame);
                    },
                    cursorArg
                );

                if (slot.disposed || activeSlots.get(ptyId)?.token !== slot.token) {
                    void ptyClient.detach(ptyId, info.attachmentId).catch(() => {});
                    return subscription;
                }

                slot.attachmentId = info.attachmentId;
                options?.onAttachmentReady?.(info.attachmentId, info.truncated);

                if (slot.pendingAck !== null) {
                    const ackToSend = slot.pendingAck;
                    slot.pendingAck = null;
                    void subscription.ack(ackToSend).catch(() => {});
                }

                if (slot.pendingExitFromEvent) {
                    const buffered = slot.pendingExitFromEvent;
                    slot.processingChain = slot.processingChain.then(() => {
                        if (!slot.disposed && !slot.exitDelivered && slot.lastProcessedSeq >= buffered.finalSeq) {
                            deliverExitIfReady(slot, buffered.exit);
                        }
                    });
                }

                return subscription;
            } catch (err: unknown) {
                slot.disposed = true;
                if (activeSlots.get(ptyId)?.token === slot.token) {
                    activeSlots.delete(ptyId);
                }
                maybeTeardownGlobalListeners();
                throw err;
            }
        },

        release(ptyId: PtyId): void {
            const slot = activeSlots.get(ptyId);
            if (slot) {
                slot.disposed = true;
                activeSlots.delete(ptyId);
                if (slot.attachmentId) {
                    const attId = slot.attachmentId;
                    slot.attachmentId = null;
                    void ptyClient.detach(ptyId, attId).catch(() => {});
                }
            }
            bufferedExitEvents.delete(ptyId);
            maybeTeardownGlobalListeners();
        },

        getActiveSubscriptionCount(): number {
            return activeSlots.size;
        },

        resetForTests(): void {
            for (const slot of activeSlots.values()) {
                slot.disposed = true;
            }
            activeSlots.clear();
            bufferedExitEvents.clear();
            maybeTeardownGlobalListeners();
        }
    };
}

/** Instância singleton do `PtyStreamHub`. */
export const ptyStreamHub: PtyStreamHub = createPtyStreamHub();
