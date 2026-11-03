import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { PtyInput } from '@/types/bindings';
import { MtTerminalError } from '@/types/terminal';
import { ptyClient } from '@/terminal/stream/ptyClient';
import { InputPipeline, splitTextUtf8Safe } from '@/terminal/engine/inputPipeline';

describe('InputPipeline', () => {
    let writtenInputs: PtyInput[] = [];

    beforeEach(() => {
        writtenInputs = [];
        vi.restoreAllMocks();
        vi.spyOn(ptyClient, 'write').mockImplementation(async (_ptyId, input) => {
            writtenInputs.push(input);
        });
    });

    it('coalesce chamadas onData no mesmo tick e separa estritamente texto de bytes (onBinary 0..255)', async () => {
        const pipeline = new InputPipeline({
            ptyId: 'pty-1',
            pasteToXterm: () => {},
            isBracketedPasteEnabled: () => false
        });

        pipeline.onData('ls ');
        pipeline.onData('-la');
        pipeline.onBinary('\x1b[<0;10;20M\xff');
        pipeline.onData('\r');

        await pipeline.flushNow();

        expect(writtenInputs).toEqual([
            { kind: 'text', text: 'ls -la' },
            { kind: 'bytes', bytes: [0x1b, 0x5b, 0x3c, 0x30, 0x3b, 0x31, 0x30, 0x3b, 0x32, 0x30, 0x4d, 0xff] },
            { kind: 'text', text: '\r' }
        ]);

        // Rejeita code units > 255 em onBinary.
        expect(() => pipeline.onBinary('olá \u0100')).toThrowError(MtTerminalError);
        pipeline.dispose();
    });

    it('fatia textos grandes em blocos <= 64 KiB sem quebrar pares substitutos UTF-16 e faz retry em pty.write_queue_full', async () => {
        // String com emojis (4 bytes UTF-8 / 2 code units UTF-16 cada).
        const emojiRepeated = '🚀'.repeat(20_000); // 80.000 bytes UTF-8
        const chunks = splitTextUtf8Safe(emojiRepeated, InputPipeline.MAX_CHUNK_BYTES);
        expect(chunks.length).toBe(2);
        for (const chunk of chunks) {
            expect(new TextEncoder().encode(chunk).byteLength).toBeLessThanOrEqual(InputPipeline.MAX_CHUNK_BYTES);
            // Garante que nenhum surrogate isolado foi produzido.
            expect(chunk.includes('\uFFFD')).toBe(false);
        }

        let failCount = 0;
        vi.spyOn(ptyClient, 'write').mockImplementation(async (_ptyId, input) => {
            if (failCount < 2) {
                failCount += 1;
                throw { code: 'pty.write_queue_full', message: 'Fila cheia' };
            }
            writtenInputs.push(input);
        });

        const pipeline = new InputPipeline({
            ptyId: 'pty-retry',
            pasteToXterm: () => {},
            isBracketedPasteEnabled: () => false
        });

        pipeline.onData('echo ok\r');
        await pipeline.flushNow();

        expect(failCount).toBe(2);
        expect(writtenInputs).toEqual([{ kind: 'text', text: 'echo ok\r' }]);
        pipeline.dispose();
    });

    it('respeita bloqueio de IME (isComposing), deduplica duplo Enter e avisa multilinha sem bracketed paste', async () => {
        let warnedMultiline = false;
        const pipeline = new InputPipeline({
            ptyId: 'pty-line',
            pasteToXterm: (text) => {
                pipeline.onData(text);
            },
            isBracketedPasteEnabled: () => false,
            onMultilineUnbracketedWarning: () => {
                warnedMultiline = true;
            }
        });

        await pipeline.sendLine('ignorado', { isComposing: true });
        expect(writtenInputs).toEqual([]);

        const p1 = pipeline.sendLine('linha 1\nlinha 2');
        const p2 = pipeline.sendLine('linha 1\nlinha 2'); // duplo Enter no mesmo tick
        await Promise.all([p1, p2]);

        expect(warnedMultiline).toBe(true);
        expect(writtenInputs).toEqual([{ kind: 'text', text: 'linha 1\nlinha 2\r' }]);

        // Testa paste direto (> 4096 bytes para exercitar chunking cooperativo) e markExited.
        pipeline.paste('x'.repeat(6000));
        await pipeline.flushNow();
        pipeline.markExited();
        pipeline.onData('ignorado apos exit');
        await pipeline.flushNow();
        expect(writtenInputs.length).toBeGreaterThanOrEqual(2);
        pipeline.dispose();
    });
});
