import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Channel } from '@tauri-apps/api/core';
import * as ptyIpc from '@/ipc/pty';
import type { AttachInfo, ExitInfo, PtyExitEvent, PtySummary, SpawnSpec } from '@/types/bindings';
import { MtTerminalError } from '@/types/terminal';
import { PtyFrameKind, ptyClient, type PtyFrame } from '@/terminal/stream/ptyClient';
import { createPtyStreamHub } from '@/terminal/stream/ptyStreamHub';

function encodeGapPayload(dropped: bigint): Uint8Array {
    const buf = new ArrayBuffer(8);
    new DataView(buf).setBigUint64(0, dropped, true);
    return new Uint8Array(buf);
}

function encodeEndPayload(exit: ExitInfo): Uint8Array {
    return new TextEncoder().encode(JSON.stringify(exit));
}

describe('PtyStreamHub', () => {
    let exitCallback: ((e: PtyExitEvent) => void) | null = null;
    let onFrameCallback: ((frame: PtyFrame) => void) | null = null;

    beforeEach(() => {
        exitCallback = null;
        onFrameCallback = null;
        vi.restoreAllMocks();

        vi.spyOn(ptyClient, 'onExit').mockImplementation(async (cb) => {
            exitCallback = cb;
            return () => {
                exitCallback = null;
            };
        });

        vi.spyOn(ptyClient, 'onCleanupFailed').mockImplementation(async () => {
            return () => {};
        });
    });

    it('rejeita segunda assinatura simultânea para o mesmo PtyId com stream.already_subscribed', async () => {
        const hub = createPtyStreamHub();
        vi.spyOn(ptyClient, 'attach').mockImplementation(async (_ptyId, onFrame) => {
            onFrameCallback = onFrame;
            const info: AttachInfo = {
                attachmentId: 'att-1',
                replayFrom: { nextSeq: '1', byteOffset: '0' },
                liveFrom: { nextSeq: '1', byteOffset: '0' },
                truncated: false
            };
            return { info, channel: {} as Channel<ArrayBuffer> };
        });
        vi.spyOn(ptyClient, 'detach').mockResolvedValue();

        const sink = {
            async onData() {},
            onGap() {},
            onExit() {}
        };

        const sub1 = await hub.subscribe('pty-1', sink);
        await expect(hub.subscribe('pty-1', sink)).rejects.toThrowError(MtTerminalError);

        sub1.dispose();
        hub.resetForTests();
    });

    it('processa Gap, Replay, Data e End em ordem e só entrega onExit uma única vez após drenar todos os frames Data', async () => {
        const hub = createPtyStreamHub();
        const events: string[] = [];

        vi.spyOn(ptyClient, 'attach').mockImplementation(async (_ptyId, onFrame) => {
            onFrameCallback = onFrame;
            return {
                info: {
                    attachmentId: 'att-2',
                    replayFrom: { nextSeq: '5', byteOffset: '1024' },
                    liveFrom: { nextSeq: '6', byteOffset: '1028' },
                    truncated: true
                },
                channel: {} as Channel<ArrayBuffer>
            };
        });
        vi.spyOn(ptyClient, 'detach').mockResolvedValue();

        const sub = await hub.subscribe('pty-2', {
            async onData(frame) {
                await new Promise((r) => setTimeout(r, 5));
                events.push(`${frame.isReplay ? 'replay' : 'data'}:${frame.seq.toString()}`);
            },
            onGap(dropped, cursor) {
                events.push(`gap:${dropped.toString()}@${cursor.nextSeq}`);
            },
            onExit(exit) {
                events.push(`exit:${exit.code}`);
            }
        });

        // Simula evento global pty://exit chegando ANTES de os frames terminarem de drenar.
        exitCallback?.({
            ptyId: 'pty-2',
            owner: { tabId: null, paneId: 'pane-1' },
            exit: {
                code: 0,
                signal: null,
                signalName: null,
                reason: 'exited',
                atMs: '1700000000000'
            },
            finalSeq: '6',
            revision: '1'
        });

        onFrameCallback?.({
            kind: PtyFrameKind.Gap,
            seq: 5n,
            byteOffset: 1024n,
            payload: encodeGapPayload(1024n)
        });
        onFrameCallback?.({
            kind: PtyFrameKind.Replay,
            seq: 5n,
            byteOffset: 1024n,
            payload: new Uint8Array([1, 2, 3, 4])
        });
        onFrameCallback?.({
            kind: PtyFrameKind.Data,
            seq: 6n,
            byteOffset: 1028n,
            payload: new Uint8Array([5, 6])
        });
        onFrameCallback?.({
            kind: PtyFrameKind.End,
            seq: 7n,
            byteOffset: 1030n,
            payload: encodeEndPayload({
                code: 0,
                signal: null,
                signalName: null,
                reason: 'exited',
                atMs: '1700000000000'
            })
        });

        await new Promise((r) => setTimeout(r, 30));

        expect(events).toEqual([
            'gap:1024@5',
            'replay:5',
            'data:6',
            'exit:0'
        ]);

        sub.dispose();
        hub.resetForTests();
    });

    it('desanexa automaticamente se dispose() for chamado enquanto ptyClient.attach ainda está em voo', async () => {
        const hub = createPtyStreamHub();
        let resolveAttach: ((val: { info: AttachInfo; channel: Channel<ArrayBuffer> }) => void) | null = null;

        vi.spyOn(ptyClient, 'attach').mockImplementation(() => {
            return new Promise((resolve) => {
                resolveAttach = resolve;
            });
        });
        const detachSpy = vi.spyOn(ptyClient, 'detach').mockResolvedValue();

        const subPromise = hub.subscribe('pty-race', {
            async onData() {},
            onGap() {},
            onExit() {}
        });

        // Libera a rota antes de attach resolver.
        await new Promise((r) => setTimeout(r, 0));
        hub.release('pty-race');

        resolveAttach!({
            info: {
                attachmentId: 'att-orphan',
                replayFrom: { nextSeq: '1', byteOffset: '0' },
                liveFrom: { nextSeq: '1', byteOffset: '0' },
                truncated: false
            },
            channel: {} as Channel<ArrayBuffer>
        });

        await subPromise;
        expect(detachSpy).toHaveBeenCalledWith('pty-race', 'att-orphan');
        hub.resetForTests();
    });

    it('delega todos os métodos de ptyClient aos comandos IPC tipados e exercita spawn, list e ack no PtyStreamHub', async () => {
        vi.restoreAllMocks();
        const dummySummary = {
            id: 'pty-ipc',
            owner: { tabId: null, paneId: 'pane-1' },
            shell: '/bin/zsh',
            args: [],
            cwdInitial: '/tmp',
            size: { rows: 24, cols: 80, pixelWidth: 0, pixelHeight: 0 },
            status: { kind: 'running' as const },
            pids: { shellPid: 1234, pgid: 1234, sid: 1234, foregroundPgid: 1234 },
            attached: false,
            createdAtMs: '1000',
            bytesOut: '0',
            bytesIn: '0',
            droppedBytes: '0',
            revision: '1'
        } as unknown as PtySummary;
        const dummySpec: SpawnSpec = {
            shell: '/bin/zsh',
            args: [],
            login: true,
            cwd: '/tmp',
            env: {},
            envRemove: [],
            size: { rows: 24, cols: 80, pixelWidth: 0, pixelHeight: 0 },
            owner: { tabId: null, paneId: 'pane-1' }
        };

        vi.spyOn(ptyIpc, 'ptySpawn').mockResolvedValue(dummySummary);
        vi.spyOn(ptyIpc, 'ptyList').mockResolvedValue([dummySummary]);
        vi.spyOn(ptyIpc, 'ptyAttach').mockResolvedValue({
            info: {
                attachmentId: 'att-ipc',
                replayFrom: { nextSeq: '1', byteOffset: '0' },
                liveFrom: { nextSeq: '1', byteOffset: '0' },
                truncated: false
            },
            channel: {} as Channel<ArrayBuffer>
        });
        vi.spyOn(ptyIpc, 'ptyDetach').mockResolvedValue();
        vi.spyOn(ptyIpc, 'ptyAck').mockResolvedValue();
        vi.spyOn(ptyIpc, 'ptyWrite').mockResolvedValue();
        vi.spyOn(ptyIpc, 'ptyResize').mockResolvedValue();
        vi.spyOn(ptyIpc, 'ptyKill').mockResolvedValue();
        vi.spyOn(ptyIpc, 'onPtyExit').mockResolvedValue(() => {});
        vi.spyOn(ptyIpc, 'onPtyCleanupFailed').mockResolvedValue(() => {});

        const hub = createPtyStreamHub();
        const spawned = await hub.spawn(dummySpec);
        expect(spawned.id).toBe('pty-ipc');
        const list = await ptyClient.list();
        expect(list).toHaveLength(1);

        await ptyClient.write('pty-ipc', { kind: 'text', text: 'ls' });
        await ptyClient.resize('pty-ipc', dummySpec.size);
        await ptyClient.kill('pty-ipc', 'graceful');

        const sub = await hub.subscribe('pty-ipc', {
            async onData() {},
            onGap() {},
            onExit() {}
        });
        await sub.ack('1024');
        expect(ptyIpc.ptyAck).toHaveBeenCalledWith('pty-ipc', 'att-ipc', '1024');
        sub.dispose();
        hub.resetForTests();
    });
});
