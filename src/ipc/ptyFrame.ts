import type { ExitInfo } from '@/types/bindings';

/** Versão suportada do protocolo binário de frames do PTY. */
export const FRAME_VERSION = 1;

/** Tamanho fixo do cabeçalho binário em bytes. */
export const FRAME_HEADER_LEN = 24;

/** Limite máximo de payload para frames `Data` e `Replay` (256 KiB). */
export const MAX_DATA_FRAME_BYTES = 256 * 1024;

/** Tamanho exato do payload de um frame `Gap` (`u64 LE` = 8 bytes). */
export const GAP_PAYLOAD_LEN = 8;

/** Limite máximo do payload JSON UTF-8 de um frame `End` (4 KiB). */
export const MAX_END_PAYLOAD_LEN = 4 * 1024;

/** Tipos de frame binário v1 do canal de saída do PTY. */
export const enum PtyFrameKind {
    Data = 1,
    Replay = 2,
    Gap = 3,
    End = 4
}

/** Representação decodificada de um frame binário v1 do PTY. */
export interface PtyFrame {
    kind: PtyFrameKind;
    seq: bigint;
    byteOffset: bigint;
    payload: Uint8Array;
}

/**
 * Mensagem entregue pelo `Channel` de saída do PTY (`InvokeResponseBody::Raw` → `ArrayBuffer`).
 * EXCEÇÃO ÚNICA à regra "tipos só via tauri-specta": `BinaryPtyFrame` transporta bytes, não um DTO JSON.
 * A representação Specta é validada pelo gate T-06 antes de integrar este wrapper.
 */
export type PtyChannelMessage = ArrayBuffer;

/** Erro lançado quando um frame binário v1 recebido do `Channel` é inválido ou corrompido. */
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
