import type { Channel } from '@tauri-apps/api/core';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
    onPtyCleanupFailed,
    onPtyExit,
    ptyAck,
    ptyAttach,
    ptyDetach,
    ptyKill,
    ptyList,
    ptyResize,
    ptySpawn,
    ptyWrite
} from '@/ipc/pty';
import {
    PtyFrameError,
    PtyFrameKind,
    decodeEnd,
    decodeFrame,
    decodeGap,
    encodeBinary,
    type PtyFrame
} from '@/ipc/ptyFrame';
import type {
    AttachInfo,
    AttachmentId,
    KillMode,
    OutputCursor,
    PtyCleanupFailedEvent,
    PtyExitEvent,
    PtyId,
    PtyInput,
    PtySize,
    PtySummary,
    ShellIntegrationRequest,
    SpawnSpec
} from '@/types/bindings';

export {
    PtyFrameKind,
    PtyFrameError,
    decodeFrame,
    decodeGap,
    decodeEnd,
    encodeBinary
};
export type { PtyFrame };

/**
 * Fachada de cliente PTY da Fase 04.
 * Único módulo do motor de terminal que importa `src/ipc/pty.ts` e `src/ipc/ptyFrame.ts`.
 */
export interface PtyClient {
    spawn(spec: SpawnSpec, integration?: ShellIntegrationRequest | null): Promise<PtySummary>;
    attach(
        ptyId: PtyId,
        onFrame: (frame: PtyFrame) => void,
        cursor?: OutputCursor
    ): Promise<{ info: AttachInfo; channel: Channel<ArrayBuffer> }>;
    detach(ptyId: PtyId, attachmentId: AttachmentId): Promise<void>;
    ack(ptyId: PtyId, attachmentId: AttachmentId, consumedBytes: string): Promise<void>;
    write(ptyId: PtyId, input: PtyInput): Promise<void>;
    resize(ptyId: PtyId, size: PtySize): Promise<void>;
    kill(ptyId: PtyId, mode?: KillMode): Promise<void>;
    list(): Promise<PtySummary[]>;
    onExit(cb: (event: PtyExitEvent) => void): Promise<UnlistenFn>;
    onCleanupFailed(cb: (event: PtyCleanupFailedEvent) => void): Promise<UnlistenFn>;
}

/** Implementação padrão do `PtyClient` delegando aos wrappers tipados da Fase 02. */
export const ptyClient: PtyClient = {
    spawn(spec: SpawnSpec, integration?: ShellIntegrationRequest | null): Promise<PtySummary> {
        return ptySpawn(spec, integration);
    },
    attach(
        ptyId: PtyId,
        onFrame: (frame: PtyFrame) => void,
        cursor?: OutputCursor
    ): Promise<{ info: AttachInfo; channel: Channel<ArrayBuffer> }> {
        return ptyAttach(ptyId, onFrame, cursor);
    },
    detach(ptyId: PtyId, attachmentId: AttachmentId): Promise<void> {
        return ptyDetach(ptyId, attachmentId);
    },
    ack(ptyId: PtyId, attachmentId: AttachmentId, consumedBytes: string): Promise<void> {
        return ptyAck(ptyId, attachmentId, consumedBytes);
    },
    write(ptyId: PtyId, input: PtyInput): Promise<void> {
        return ptyWrite(ptyId, input);
    },
    resize(ptyId: PtyId, size: PtySize): Promise<void> {
        return ptyResize(ptyId, size);
    },
    kill(ptyId: PtyId, mode?: KillMode): Promise<void> {
        return ptyKill(ptyId, mode);
    },
    list(): Promise<PtySummary[]> {
        return ptyList();
    },
    onExit(cb: (event: PtyExitEvent) => void): Promise<UnlistenFn> {
        return onPtyExit(cb);
    },
    onCleanupFailed(cb: (event: PtyCleanupFailedEvent) => void): Promise<UnlistenFn> {
        return onPtyCleanupFailed(cb);
    }
};
