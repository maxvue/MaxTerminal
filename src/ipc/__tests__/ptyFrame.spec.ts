import { describe, expect, it } from 'vitest';
import ptyFrameRaw from '../ptyFrame.ts?raw';
import {
    decodeEnd,
    decodeFrame,
    decodeGap,
    encodeBinary,
    FRAME_HEADER_LEN,
    FRAME_VERSION,
    MAX_DATA_FRAME_BYTES,
    MAX_END_PAYLOAD_LEN,
    PtyFrameError,
    PtyFrameKind
} from '@/ipc/ptyFrame';

function buildRawFrame(
    kind: number,
    seq: bigint,
    byteOffset: bigint,
    payload: Uint8Array,
    opts?: { version?: number; reserved?: number; declaredLen?: number }
): ArrayBuffer {
    const buffer = new ArrayBuffer(FRAME_HEADER_LEN + payload.byteLength);
    const view = new DataView(buffer);
    view.setUint8(0, opts?.version ?? FRAME_VERSION);
    view.setUint8(1, kind);
    view.setUint16(2, opts?.reserved ?? 0, true);
    view.setBigUint64(4, seq, true);
    view.setBigUint64(12, byteOffset, true);
    view.setUint32(20, opts?.declaredLen ?? payload.byteLength, true);
    new Uint8Array(buffer, FRAME_HEADER_LEN).set(payload);
    return buffer;
}

describe('ptyFrame — protocolo binário v1', () => {
    it('decodifica frames Data e Replay preservando seq e byteOffset acima de MAX_SAFE_INTEGER', () => {
        const largeSeq = BigInt(Number.MAX_SAFE_INTEGER) + 42n;
        const largeOffset = (1n << 56n) + 123456n;
        const payload = new Uint8Array([0x00, 0x1b, 0x5b, 0x33, 0x31, 0x6d, 0xff]);

        const rawData = buildRawFrame(PtyFrameKind.Data, largeSeq, largeOffset, payload);
        const decodedData = decodeFrame(rawData);
        expect(decodedData.kind).toBe(PtyFrameKind.Data);
        expect(decodedData.seq).toBe(largeSeq);
        expect(decodedData.byteOffset).toBe(largeOffset);
        expect(Array.from(decodedData.payload)).toEqual(Array.from(payload));

        const rawReplay = buildRawFrame(PtyFrameKind.Replay, 7n, 1024n, payload);
        const decodedReplay = decodeFrame(rawReplay);
        expect(decodedReplay.kind).toBe(PtyFrameKind.Replay);
        expect(decodedReplay.seq).toBe(7n);
        expect(decodedReplay.byteOffset).toBe(1024n);
        expect(Array.from(decodedReplay.payload)).toEqual(Array.from(payload));
    });

    it('decodifica frames Gap e End com ExitInfo completo', () => {
        const gapPayload = new Uint8Array(8);
        new DataView(gapPayload.buffer).setBigUint64(0, 65536n, true);
        const gapRaw = buildRawFrame(PtyFrameKind.Gap, 15n, 65536n, gapPayload);
        const gapFrame = decodeFrame(gapRaw);
        expect(gapFrame.kind).toBe(PtyFrameKind.Gap);
        expect(decodeGap(gapFrame)).toBe(65536n);

        const exitPayloadJson = JSON.stringify({
            code: 0,
            signal: null,
            signalName: null,
            reason: 'exited',
            atMs: '1730000000000'
        });
        const endPayload = new TextEncoder().encode(exitPayloadJson);
        const endRaw = buildRawFrame(PtyFrameKind.End, 20n, 131072n, endPayload);
        const endFrame = decodeFrame(endRaw);
        expect(endFrame.kind).toBe(PtyFrameKind.End);

        const exitInfo = decodeEnd(endFrame);
        expect(exitInfo.code).toBe(0);
        expect(exitInfo.reason).toBe('exited');
        expect(exitInfo.atMs).toBe('1730000000000');
    });

    it('rejeita cabeçalhos truncados, versão inválida, kind inválido, reserved != 0 e tamanhos inconsistentes', () => {
        expect(() => decodeFrame(new ArrayBuffer(23))).toThrow(PtyFrameError);

        const validPayload = new Uint8Array([1, 2, 3]);
        expect(() =>
            decodeFrame(buildRawFrame(PtyFrameKind.Data, 1n, 0n, validPayload, { version: 2 }))
        ).toThrow(/Versão de frame inválida/);

        expect(() => decodeFrame(buildRawFrame(0, 1n, 0n, validPayload))).toThrow(/Tipo de frame inválido/);
        expect(() => decodeFrame(buildRawFrame(5, 1n, 0n, validPayload))).toThrow(/Tipo de frame inválido/);

        expect(() =>
            decodeFrame(buildRawFrame(PtyFrameKind.Data, 1n, 0n, validPayload, { reserved: 1 }))
        ).toThrow(/Campo reservado inválido/);

        expect(() =>
            decodeFrame(buildRawFrame(PtyFrameKind.Data, 1n, 0n, validPayload, { declaredLen: 99 }))
        ).toThrow(/Tamanho de payload inconsistente/);

        // Data vazio ou > 256 KiB
        expect(() => decodeFrame(buildRawFrame(PtyFrameKind.Data, 1n, 0n, new Uint8Array(0)))).toThrow(
            /Tamanho inválido para frame de dados/
        );
        expect(() =>
            decodeFrame(buildRawFrame(PtyFrameKind.Data, 1n, 0n, new Uint8Array(MAX_DATA_FRAME_BYTES + 1)))
        ).toThrow(/Tamanho inválido para frame de dados/);

        // Gap != 8 bytes
        expect(() => decodeFrame(buildRawFrame(PtyFrameKind.Gap, 1n, 0n, new Uint8Array(4)))).toThrow(
            /Tamanho inválido para frame Gap/
        );

        // End vazio ou > 4 KiB
        expect(() => decodeFrame(buildRawFrame(PtyFrameKind.End, 1n, 0n, new Uint8Array(0)))).toThrow(
            /Tamanho inválido para frame End/
        );
        expect(() =>
            decodeFrame(buildRawFrame(PtyFrameKind.End, 1n, 0n, new Uint8Array(MAX_END_PAYLOAD_LEN + 1)))
        ).toThrow(/Tamanho inválido para frame End/);

        // decodeEnd / decodeGap em tipos errados
        const dataFrame = decodeFrame(buildRawFrame(PtyFrameKind.Data, 1n, 0n, validPayload));
        expect(() => decodeEnd(dataFrame)).toThrow(PtyFrameError);
        expect(() => decodeGap(dataFrame)).toThrow(PtyFrameError);
    });

    it('preserva sequências UTF-8 multibyte e escapes ANSI cortados entre frames (CA-09)', () => {
        // String com emoji de 4 bytes (🚀 = F0 9F 9A 80), acentuação pt-BR (ação = 61 C3 A7 C3 A3 6F) e ANSI
        const originalText = '\u001b[32mExecução concluída 🚀 com sucesso!\u001b[0m';
        const fullBytes = new TextEncoder().encode(originalText);

        // Corta exatamente no meio do emoji 🚀 e no meio do 'ç'
        const split1 = 10;
        const split2 = 27;
        const chunk1 = fullBytes.slice(0, split1);
        const chunk2 = fullBytes.slice(split1, split2);
        const chunk3 = fullBytes.slice(split2);

        const f1 = decodeFrame(buildRawFrame(PtyFrameKind.Data, 1n, 0n, chunk1));
        const f2 = decodeFrame(buildRawFrame(PtyFrameKind.Data, 2n, BigInt(split1), chunk2));
        const f3 = decodeFrame(buildRawFrame(PtyFrameKind.Data, 3n, BigInt(split2), chunk3));

        const streamDecoder = new TextDecoder('utf-8');
        const reconstructed =
            streamDecoder.decode(f1.payload, { stream: true }) +
            streamDecoder.decode(f2.payload, { stream: true }) +
            streamDecoder.decode(f3.payload, { stream: false });

        expect(reconstructed).toBe(originalText);
    });

    it('encodeBinary converte todos os bytes 0..=255 e rejeita code units > 255', () => {
        let binaryStr = '';
        for (let b = 0; b <= 255; b++) {
            binaryStr += String.fromCharCode(b);
        }
        const encoded = encodeBinary(binaryStr);
        expect(encoded.byteLength).toBe(256);
        for (let b = 0; b <= 255; b++) {
            expect(encoded[b]).toBe(b);
        }

        expect(() => encodeBinary('abc\u0100def')).toThrow(PtyFrameError);
    });

    it('mantém exatamente uma declaração manual de PtyChannelMessage = ArrayBuffer em src/ipc/ptyFrame.ts (T-06)', () => {
        const matches = ptyFrameRaw.match(/^export type PtyChannelMessage = ArrayBuffer;/gm) ?? [];
        expect(matches).toHaveLength(1);
    });
});
