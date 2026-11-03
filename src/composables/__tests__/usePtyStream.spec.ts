import { effectScope, nextTick, ref } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MtIpcError } from '@/ipc/core';
import { FRAME_HEADER_LEN, FRAME_VERSION, PtyFrameKind } from '@/ipc/ptyFrame';
import type { AttachInfo, ExitInfo, OutputCursor, PtyId, PtyInput } from '@/types/bindings';

const { invokeMock, MockChannel } = vi.hoisted(() => {
    const invoke = vi.fn();

    class ChannelMock<T = ArrayBuffer> {
        public static instances: ChannelMock<ArrayBuffer>[] = [];
        public id: number;
        public onmessage: (response: T) => void = () => {};

        public constructor(onMessage?: (response: T) => void) {
            this.id = ChannelMock.instances.length + 1;
            if (onMessage !== undefined) {
                this.onmessage = onMessage;
            }
            ChannelMock.instances.push(this as unknown as ChannelMock<ArrayBuffer>);
        }
    }

    return { invokeMock: invoke, MockChannel: ChannelMock };
});

vi.mock('@tauri-apps/api/core', () => ({
    invoke: (...args: unknown[]) => invokeMock(...args),
    Channel: MockChannel
}));

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn()
}));

import { MAX_WRITE_PAYLOAD_BYTES, ptyWrite } from '@/ipc/pty';
import { usePtyStream } from '@/composables/usePtyStream';

function buildRawFrame(
    kind: PtyFrameKind,
    seq: bigint,
    byteOffset: bigint,
    payload: Uint8Array
): ArrayBuffer {
    const buffer = new ArrayBuffer(FRAME_HEADER_LEN + payload.byteLength);
    const view = new DataView(buffer);
    view.setUint8(0, FRAME_VERSION);
    view.setUint8(1, kind);
    view.setUint16(2, 0, true);
    view.setBigUint64(4, seq, true);
    view.setBigUint64(12, byteOffset, true);
    view.setUint32(20, payload.byteLength, true);
    new Uint8Array(buffer, FRAME_HEADER_LEN).set(payload);
    return buffer;
}

function makeAttachInfo(attachmentId: string): AttachInfo {
    return {
        attachmentId,
        replayFrom: { nextSeq: '1', byteOffset: '0' },
        liveFrom: { nextSeq: '1', byteOffset: '0' },
        truncated: false
    };
}

async function flushMicrotasks(ticks = 6): Promise<void> {
    for (let i = 0; i < ticks; i++) {
        await Promise.resolve();
    }
}

describe('usePtyStream e ptyWrite', () => {
    beforeEach(() => {
        invokeMock.mockReset();
        MockChannel.instances = [];
        vi.useRealTimers();
    });

    it('não envia pty_ack enquanto a Promise de consumer.write está pendente (CA-35)', async () => {
        vi.useFakeTimers();
        const ptyId = ref<PtyId | null>('01960000-0000-7000-8000-000000000001');

        let resolveWrite1: (() => void) | null = null;
        const writtenChunks: Uint8Array[] = [];

        invokeMock.mockImplementation((cmd: string) => {
            if (cmd === 'pty_attach') {
                return Promise.resolve(makeAttachInfo('att-1'));
            }
            if (cmd === 'pty_ack') {
                return Promise.resolve(null);
            }
            if (cmd === 'pty_detach') {
                return Promise.resolve(null);
            }
            return Promise.resolve(null);
        });

        const stream = usePtyStream({
            ptyId,
            consumer: {
                write: (bytes) => {
                    writtenChunks.push(bytes);
                    if (writtenChunks.length === 1) {
                        return new Promise<void>((resolve) => {
                            resolveWrite1 = resolve;
                        });
                    }
                    return Promise.resolve();
                }
            },
            ackThresholdBytes: 4,
            ackFlushIntervalMs: 16
        });

        await stream.attach();
        expect(stream.attached.value).toBe(true);

        const channel = MockChannel.instances[0];
        expect(channel).toBeDefined();

        // Emite frame de 8 bytes (>= ackThresholdBytes = 4)
        const payload1 = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]);
        channel!.onmessage(buildRawFrame(PtyFrameKind.Data, 1n, 0n, payload1));

        await flushMicrotasks();
        vi.advanceTimersByTime(100);
        await flushMicrotasks();

        // Enquanto consumer.write está pendente, nenhum ACK foi enviado e cursor ainda é null
        const ackCallsBefore = invokeMock.mock.calls.filter((c) => c[0] === 'pty_ack');
        expect(ackCallsBefore).toHaveLength(0);
        expect(stream.confirmedCursor.value).toBeNull();
        expect(stream.lastSeq.value).toBeNull();

        // Resolve consumer.write
        expect(resolveWrite1).not.toBeNull();
        resolveWrite1!();
        await flushMicrotasks();

        const ackCallsAfter = invokeMock.mock.calls.filter((c) => c[0] === 'pty_ack');
        expect(ackCallsAfter).toHaveLength(1);
        expect(ackCallsAfter[0]?.[1]).toEqual({
            ptyId: '01960000-0000-7000-8000-000000000001',
            attachmentId: 'att-1',
            consumedBytes: '8'
        });
        expect(stream.lastSeq.value).toBe(1n);
        expect(stream.confirmedCursor.value).toEqual({
            nextSeq: '2',
            byteOffset: '8'
        });
    });

    it('agrega ACKs cumulativos por timer de 16 ms ou imediatamente ao atingir 32 KiB (CA-30)', async () => {
        vi.useFakeTimers();
        const ptyId = ref<PtyId | null>('01960000-0000-7000-8000-000000000002');

        invokeMock.mockImplementation((cmd: string) => {
            if (cmd === 'pty_attach') {
                return Promise.resolve(makeAttachInfo('att-2'));
            }
            return Promise.resolve(null);
        });

        const stream = usePtyStream({
            ptyId,
            consumer: {
                write: () => {}
            },
            ackThresholdBytes: 32768,
            ackFlushIntervalMs: 16
        });

        await stream.attach();
        const channel = MockChannel.instances[0]!;

        // Dois frames pequenos (100 bytes cada < 32 KiB)
        channel.onmessage(buildRawFrame(PtyFrameKind.Data, 1n, 0n, new Uint8Array(100)));
        channel.onmessage(buildRawFrame(PtyFrameKind.Data, 2n, 100n, new Uint8Array(100)));
        await flushMicrotasks();

        // Ainda não atingiu 32 KiB nem 16 ms
        expect(invokeMock.mock.calls.filter((c) => c[0] === 'pty_ack')).toHaveLength(0);

        // Avança 16 ms -> envia ACK cumulativo único de 200 bytes
        vi.advanceTimersByTime(16);
        await flushMicrotasks();

        const ackCalls1 = invokeMock.mock.calls.filter((c) => c[0] === 'pty_ack');
        expect(ackCalls1).toHaveLength(1);
        expect(ackCalls1[0]?.[1]).toEqual({
            ptyId: '01960000-0000-7000-8000-000000000002',
            attachmentId: 'att-2',
            consumedBytes: '200'
        });

        // Agora envia um frame de 32768 bytes -> flush imediato sem esperar timer
        channel.onmessage(buildRawFrame(PtyFrameKind.Data, 3n, 200n, new Uint8Array(32768)));
        await flushMicrotasks();

        const ackCalls2 = invokeMock.mock.calls.filter((c) => c[0] === 'pty_ack');
        expect(ackCalls2).toHaveLength(2);
        expect(ackCalls2[1]?.[1]).toEqual({
            ptyId: '01960000-0000-7000-8000-000000000002',
            attachmentId: 'att-2',
            consumedBytes: '32968'
        });
    });

    it('retém frames recebidos antes da resolução de AttachInfo e processa Gap e End', async () => {
        const ptyId = ref<PtyId | null>('01960000-0000-7000-8000-000000000003');
        const gaps: bigint[] = [];
        const ends: ExitInfo[] = [];
        const received: number[] = [];

        invokeMock.mockImplementation((cmd: string, args: Record<string, unknown>) => {
            if (cmd === 'pty_attach') {
                const ch = args.onOutput as InstanceType<typeof MockChannel>;
                // Emite Gap + Replay + Data + End ANTES de resolver a Promise do invoke!
                const gapBuf = new Uint8Array(8);
                new DataView(gapBuf.buffer).setBigUint64(0, 1024n, true);
                ch.onmessage(buildRawFrame(PtyFrameKind.Gap, 5n, 1024n, gapBuf));
                ch.onmessage(buildRawFrame(PtyFrameKind.Replay, 5n, 1024n, new Uint8Array([10, 20])));
                ch.onmessage(buildRawFrame(PtyFrameKind.Data, 6n, 1026n, new Uint8Array([30])));

                const endJson = new TextEncoder().encode(
                    JSON.stringify({
                        code: 0,
                        signal: null,
                        signalName: null,
                        reason: 'exited',
                        atMs: '1730000000123'
                    })
                );
                ch.onmessage(buildRawFrame(PtyFrameKind.End, 7n, 1027n, endJson));
                return Promise.resolve(makeAttachInfo('att-early'));
            }
            return Promise.resolve(null);
        });

        const stream = usePtyStream({
            ptyId,
            consumer: {
                write: (bytes) => {
                    received.push(...Array.from(bytes));
                },
                onGap: (dropped) => gaps.push(dropped),
                onEnd: (exit) => ends.push(exit)
            }
        });

        await stream.attach();
        await flushMicrotasks(10);

        expect(gaps).toEqual([1024n]);
        expect(received).toEqual([10, 20, 30]);
        expect(ends).toHaveLength(1);
        expect(ends[0]?.code).toBe(0);
        expect(stream.ended.value?.reason).toBe('exited');
        expect(stream.confirmedCursor.value).toEqual({
            nextSeq: '7',
            byteOffset: '1027'
        });

        // O frame End deve ter forçado o flush do ACK acumulado (3 bytes de Replay + Data)
        const ackCalls = invokeMock.mock.calls.filter((c) => c[0] === 'pty_ack');
        expect(ackCalls).toHaveLength(1);
        expect(ackCalls[0]?.[1]).toEqual({
            ptyId: '01960000-0000-7000-8000-000000000003',
            attachmentId: 'att-early',
            consumedBytes: '3'
        });
    });

    it('reconecta a partir de confirmedCursor quando pty_ack falha com pty.attachment_stale', async () => {
        const ptyId = ref<PtyId | null>('01960000-0000-7000-8000-000000000004');
        let attachCount = 0;
        const capturedCursors: Array<OutputCursor | null> = [];

        invokeMock.mockImplementation((cmd: string, args: Record<string, unknown>) => {
            if (cmd === 'pty_attach') {
                attachCount += 1;
                capturedCursors.push((args.cursor as OutputCursor | null) ?? null);
                return Promise.resolve(makeAttachInfo(`att-gen-${attachCount}`));
            }
            if (cmd === 'pty_ack') {
                if (args.attachmentId === 'att-gen-1') {
                    return Promise.reject({
                        code: 'pty.attachment_stale',
                        message: 'Conexão substituída ou expirada'
                    });
                }
                return Promise.resolve(null);
            }
            return Promise.resolve(null);
        });

        const stream = usePtyStream({
            ptyId,
            consumer: {
                write: () => {}
            },
            ackThresholdBytes: 4
        });

        await stream.attach();
        expect(attachCount).toBe(1);
        expect(capturedCursors[0]).toBeNull();

        const channel1 = MockChannel.instances[0]!;
        channel1.onmessage(buildRawFrame(PtyFrameKind.Data, 10n, 500n, new Uint8Array([1, 2, 3, 4])));
        await flushMicrotasks(12);

        // Deve ter reconectado automaticamente usando o confirmedCursor { nextSeq: '11', byteOffset: '504' }
        expect(attachCount).toBe(2);
        expect(capturedCursors[1]).toEqual({
            nextSeq: '11',
            byteOffset: '504'
        });
    });

    it('interrompe avanço de cursor, faz detach e chama onError se consumer.write rejeitar', async () => {
        const ptyId = ref<PtyId | null>('01960000-0000-7000-8000-000000000005');
        const errors: unknown[] = [];

        invokeMock.mockImplementation((cmd: string) => {
            if (cmd === 'pty_attach') {
                return Promise.resolve(makeAttachInfo('att-err'));
            }
            return Promise.resolve(null);
        });

        const stream = usePtyStream({
            ptyId,
            consumer: {
                write: () => Promise.reject(new Error('Falha no xterm.write')),
                onError: (err) => errors.push(err)
            },
            ackThresholdBytes: 1
        });

        await stream.attach();
        const channel = MockChannel.instances[0]!;
        channel.onmessage(buildRawFrame(PtyFrameKind.Data, 1n, 0n, new Uint8Array([65, 66])));
        await flushMicrotasks(10);

        expect(errors).toHaveLength(1);
        expect(stream.attached.value).toBe(false);
        expect(stream.confirmedCursor.value).toBeNull();
        expect(invokeMock.mock.calls.filter((c) => c[0] === 'pty_ack')).toHaveLength(0);
        expect(invokeMock.mock.calls.filter((c) => c[0] === 'pty_detach')).toHaveLength(1);
    });

    it('executa detach automático em troca de ptyId e em onScopeDispose', async () => {
        const ptyId = ref<PtyId | null>('01960000-0000-7000-8000-000000000006');
        let counter = 0;

        invokeMock.mockImplementation((cmd: string) => {
            if (cmd === 'pty_attach') {
                counter += 1;
                return Promise.resolve(makeAttachInfo(`att-scope-${counter}`));
            }
            return Promise.resolve(null);
        });

        const scope = effectScope();
        const stream = scope.run(() =>
            usePtyStream({
                ptyId,
                consumer: { write: () => {} }
            })
        )!;

        await stream.attach();
        expect(stream.attached.value).toBe(true);

        // Troca de ptyId
        ptyId.value = '01960000-0000-7000-8000-000000000007';
        await nextTick();
        await flushMicrotasks(10);

        expect(invokeMock.mock.calls.filter((c) => c[0] === 'pty_detach')).toHaveLength(1);
        expect(stream.attached.value).toBe(true);

        // Dispose do escopo
        scope.stop();
        await flushMicrotasks(6);
        expect(invokeMock.mock.calls.filter((c) => c[0] === 'pty_detach')).toHaveLength(2);
    });

    it('fatia ptyWrite > 1 MiB preservando fronteiras de code points UTF-8 multibyte', async () => {
        const sentInputs: PtyInput[] = [];
        invokeMock.mockImplementation((cmd: string, args: Record<string, unknown>) => {
            if (cmd === 'pty_write') {
                sentInputs.push(args.input as PtyInput);
            }
            return Promise.resolve(null);
        });

        // Emoji 🚀 ocupa 4 bytes em UTF-8. 300.000 emojis = 1.200.000 bytes (> 1 MiB = 1.048.576 bytes)
        const emojiCount = 300_000;
        const bigText = '🚀'.repeat(emojiCount);

        await ptyWrite('01960000-0000-7000-8000-000000000008', {
            kind: 'text',
            text: bigText
        });

        expect(sentInputs.length).toBe(2);
        const encoder = new TextEncoder();
        let reconstructed = '';
        for (const item of sentInputs) {
            expect(item.kind).toBe('text');
            if (item.kind === 'text') {
                const encoded = encoder.encode(item.text);
                expect(encoded.byteLength).toBeLessThanOrEqual(MAX_WRITE_PAYLOAD_BYTES);
                // Nenhum caractere U+FFFD de substituição pode aparecer
                expect(item.text.includes('\uFFFD')).toBe(false);
                reconstructed += item.text;
            }
        }
        expect(reconstructed).toBe(bigText);

        // Verifica também erro MtIpcError propagado
        invokeMock.mockRejectedValueOnce({
            code: 'pty.write_queue_full',
            message: 'Fila de escrita cheia'
        });
        await expect(
            ptyWrite('01960000-0000-7000-8000-000000000008', { kind: 'text', text: 'oi' })
        ).rejects.toBeInstanceOf(MtIpcError);
    });
});
