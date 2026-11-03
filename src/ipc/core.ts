import type { MtError } from '@/types/bindings';
import { IPC_TRANSPORT_ERROR, IPC_TRANSPORT_MESSAGE } from '@/types/errors';

/** Alias para o envelope Result gerado pelo tauri-specta com ErrorHandlingMode::Result. */
export type IpcResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: MtError };

/** Exceção tipada lançada na camada de fronteira IPC do frontend. */
export class MtIpcError extends Error {
    public readonly code: string;
    public readonly details: unknown;
    public readonly command: string;

    public constructor(payload: MtError, command: string) {
        super(payload.message);
        this.name = 'MtIpcError';
        this.code = payload.code;
        this.details = payload.details;
        this.command = command;
    }
}

/** Verifica se um valor desconhecido é uma instância de `MtIpcError`. */
export function isMtIpcError(value: unknown): value is MtIpcError {
    return value instanceof MtIpcError;
}

function isMtErrorPayload(value: unknown): value is MtError {
    if (typeof value !== 'object' || value === null) {
        return false;
    }
    const candidate = value as Record<string, unknown>;
    return (
        typeof candidate.code === 'string' &&
        candidate.code.length > 0 &&
        typeof candidate.message === 'string' &&
        candidate.message.length > 0
    );
}

function createTransportError(command: string): MtIpcError {
    return new MtIpcError(
        {
            code: IPC_TRANSPORT_ERROR,
            message: IPC_TRANSPORT_MESSAGE
        },
        command
    );
}

function decodeResult<T>(raw: unknown, command: string): T {
    if (typeof raw !== 'object' || raw === null) {
        throw createTransportError(command);
    }

    const envelope = raw as Record<string, unknown>;
    if (envelope.status === 'ok' && 'data' in envelope) {
        return envelope.data as T;
    }

    if (envelope.status === 'error' && isMtErrorPayload(envelope.error)) {
        throw new MtIpcError(envelope.error, command);
    }

    throw createTransportError(command);
}

/**
 * Executa uma chamada IPC gerada pelo tauri-specta capturando tanto rejeições assíncronas
 * quanto exceções síncronas e validando o envelope `IpcResult<T>`.
 */
export async function call<T>(command: string, request: () => Promise<IpcResult<T>>): Promise<T> {
    let raw: unknown;
    try {
        raw = await request();
    } catch (err: unknown) {
        if (isMtIpcError(err)) {
            throw err;
        }
        if (isMtErrorPayload(err)) {
            throw new MtIpcError(err, command);
        }
        throw createTransportError(command);
    }

    return decodeResult<T>(raw, command);
}

/**
 * Desembrulha uma Promise `IpcResult<T>` já iniciada usando a mesma validação de `call()`.
 */
export async function unwrap<T>(request: Promise<IpcResult<T>>, command = 'ipc'): Promise<T> {
    return call(command, () => request);
}
