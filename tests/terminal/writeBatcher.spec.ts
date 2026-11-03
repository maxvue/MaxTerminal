import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_FLOW_CONFIG, MtTerminalError } from '@/types/terminal';
import { WriteBatcher } from '@/terminal/engine/writeBatcher';

describe('WriteBatcher', () => {
    beforeEach(() => {
        vi.useFakeTimers();
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('agrupa múltiplos frames em um único Uint8Array respeitando o orçamento de 256 KiB e preserva fronteiras de frame', () => {
        const written: Uint8Array[] = [];
        const callbacks: Array<() => void> = [];
        const parsedEvents: Array<{ byteLength: number; confirmedCursor: { nextSeq: string; byteOffset: string } }> = [];

        const batcher = new WriteBatcher({
            target: {
                write(data, cb) {
                    written.push(data);
                    if (cb) {
                        callbacks.push(cb);
                    }
                }
            },
            flowConfig: {
                ...DEFAULT_FLOW_CONFIG,
                frameBudgetBytes: 10
            },
            onBatchParsed(info) {
                parsedEvents.push(info);
            }
        });

        // Enfileira 3 frames de 4 bytes cada; orçamento = 10 bytes => lote 1 terá frames 1 e 2 (8 bytes), lote 2 terá frame 3 (4 bytes).
        batcher.push({
            seq: 1n,
            byteOffset: 0n,
            bytes: new Uint8Array([1, 2, 3, 4])
        });
        batcher.push({
            seq: 2n,
            byteOffset: 4n,
            bytes: new Uint8Array([5, 6, 7, 8])
        });
        batcher.push({
            seq: 3n,
            byteOffset: 8n,
            bytes: new Uint8Array([9, 10, 11, 12])
        });

        batcher.flush();
        expect(written).toHaveLength(1);
        expect(Array.from(written[0]!)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
        expect(batcher.isWriteInFlight()).toBe(true);
        expect(batcher.getConfirmedCursor()).toBeNull();

        // Enquanto o primeiro write está em voo, flush() adicional não dispara segundo write.
        batcher.flush();
        expect(written).toHaveLength(1);

        // Conclui o callback do primeiro write.
        callbacks.shift()!();
        expect(parsedEvents).toHaveLength(1);
        expect(parsedEvents[0]).toEqual({
            byteLength: 8,
            confirmedCursor: { nextSeq: '3', byteOffset: '8' }
        });

        // Drena o segundo lote agendado.
        batcher.flush();
        expect(written).toHaveLength(2);
        expect(Array.from(written[1]!)).toEqual([9, 10, 11, 12]);
        callbacks.shift()!();

        expect(batcher.getConfirmedCursor()).toEqual({
            nextSeq: '4',
            byteOffset: '12'
        });

        batcher.dispose();
    });

    it('preserva UTF-8 multibyte e sequências ANSI partidos entre frames sem decodificar como string', () => {
        const chunks: Uint8Array[] = [];
        const batcher = new WriteBatcher({
            target: {
                write(data, cb) {
                    chunks.push(data);
                    cb?.();
                }
            },
            flowConfig: DEFAULT_FLOW_CONFIG
        });

        // Emoji 🚀 (4 bytes UTF-8: F0 9F 9A 80) partido em 2 frames + CSI colorido partido.
        batcher.push({
            seq: 10n,
            byteOffset: 100n,
            bytes: new Uint8Array([0xf0, 0x9f, 0x1b])
        });
        batcher.push({
            seq: 11n,
            byteOffset: 103n,
            bytes: new Uint8Array([0x9a, 0x80, 0x5b, 0x33, 0x31, 0x6d])
        });

        batcher.flush();
        expect(chunks).toHaveLength(1);
        expect(Array.from(chunks[0]!)).toEqual([0xf0, 0x9f, 0x1b, 0x9a, 0x80, 0x5b, 0x33, 0x31, 0x6d]);
        batcher.dispose();
    });

    it('alterna para setTimeout(16) quando oculto (setVisible(false)) e rejeita acima de 1 MiB', () => {
        let writtenCount = 0;
        const batcher = new WriteBatcher({
            target: {
                write(_data, cb) {
                    writtenCount += 1;
                    cb?.();
                }
            },
            flowConfig: DEFAULT_FLOW_CONFIG
        });

        batcher.setVisible(false);
        batcher.push({
            seq: 1n,
            byteOffset: 0n,
            bytes: new Uint8Array([65, 66])
        });

        vi.advanceTimersByTime(16);
        expect(writtenCount).toBe(1);

        // Testa overflow > 1 MiB.
        const huge = new Uint8Array(1024 * 1024 + 1);
        expect(() => {
            batcher.push({
                seq: 2n,
                byteOffset: 2n,
                bytes: huge
            });
        }).toThrowError(MtTerminalError);

        batcher.dispose();
    });
});
