import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_FLOW_CONFIG } from '@/types/terminal';
import { FlowController } from '@/terminal/engine/flowController';

describe('FlowController', () => {
    beforeEach(() => {
        vi.useFakeTimers();
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('só envia pty_ack em onWritten (pós-parse), acumulando consumedTotal em bigint sem zerar na mesma geração', async () => {
        const acked: string[] = [];
        const states: Array<{ paused: boolean; pendingBytes: number }> = [];

        const controller = new FlowController(
            DEFAULT_FLOW_CONFIG,
            {
                async ack(consumedBytes) {
                    acked.push(consumedBytes);
                }
            },
            (state) => {
                states.push(state);
            }
        );

        controller.setAttachmentReady(true);

        // onEnqueued nunca envia ACK.
        controller.onEnqueued(40_000);
        expect(acked).toEqual([]);
        expect(states[states.length - 1]?.pendingBytes).toBe(40_000);

        // Ao concluir o parse de 32 KiB (limiar), envia imediatamente o total acumulado.
        controller.onWritten(32_768);
        await Promise.resolve();
        expect(acked).toEqual(['32768']);

        // Mais 1000 bytes (< 32 KiB): aguarda timer de 16 ms e envia o total cumulativo (33768).
        controller.onWritten(1000);
        expect(acked).toEqual(['32768']);
        vi.advanceTimersByTime(16);
        await Promise.resolve();
        expect(acked).toEqual(['32768', '33768']);

        controller.dispose();
    });

    it('serializa ACKs com 1 em voo, coalesce valores intermediários e suporta inteiros u64 > Number.MAX_SAFE_INTEGER', async () => {
        const acked: string[] = [];
        let resolveFirstAck: (() => void) | null = null;

        const controller = new FlowController(
            {
                ...DEFAULT_FLOW_CONFIG,
                ackThresholdBytes: 100
            },
            {
                ack(consumedBytes) {
                    acked.push(consumedBytes);
                    if (acked.length === 1) {
                        return new Promise<void>((resolve) => {
                            resolveFirstAck = resolve;
                        });
                    }
                    return Promise.resolve();
                }
            },
            () => {}
        );

        controller.setAttachmentReady(true);

        // Primeiro ACK dispara e fica em voo.
        controller.onWritten(200);
        expect(acked).toEqual(['200']);

        // Novos bytes processados enquanto o primeiro ACK está em voo são coalescidos.
        controller.onWritten(300);
        controller.onWritten(500);
        expect(acked).toEqual(['200']);

        resolveFirstAck!();
        await Promise.resolve();
        await Promise.resolve();

        expect(acked).toEqual(['200', '1000']);
        controller.dispose();
    });

    it('sinaliza paused = true ao atingir pausedHintBytes (1 MiB) e ignora pty.attachment_stale sem falhar', async () => {
        let lastPaused = false;
        const controller = new FlowController(
            DEFAULT_FLOW_CONFIG,
            {
                async ack() {
                    const staleErr = { code: 'pty.attachment_stale', message: 'Stale' };
                    throw staleErr;
                }
            },
            (state) => {
                lastPaused = state.paused;
            }
        );

        controller.setAttachmentReady(true);
        controller.onEnqueued(1024 * 1024);
        expect(lastPaused).toBe(true);

        controller.onWritten(64 * 1024);
        await Promise.resolve();
        expect(lastPaused).toBe(false);

        controller.dispose();
    });
});
