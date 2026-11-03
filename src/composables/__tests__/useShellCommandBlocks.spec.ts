import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CommandFinished_Serialize, CommandStarted_Serialize } from '@/types/bindings';

vi.mock('@tauri-apps/api/core', () => ({
    invoke: vi.fn(() => Promise.resolve({ items: [], nextCursor: null })),
    Channel: class {}
}));

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn(() => Promise.resolve(vi.fn()))
}));

import {
    UNAVAILABLE_COMMAND_LABEL,
    useShellCommandBlocks
} from '@/composables/useShellCommandBlocks';
import { useShellMarkers } from '@/composables/useShellMarkers';
import { useCommandBlocksStore } from '@/stores/commandBlocks';
import { useShellHistoryStore } from '@/stores/shellHistory';
import { useTabStatusStore } from '@/stores/tabStatus';
import { resolveTerminalContext } from '@/terminal/extensions/terminalContext';
import {
    createShellIntegrationExtension,
    installShellIntegrationExtension
} from '@/terminal/extensions/shellIntegration';
import { resolveInputMode } from '@/terminal/input/inputModeResolver';
import type { CommandBlockDto_Serialize } from '@/types/bindings';

const PANE_ID = '01960000-0000-7000-8000-000000000201';
const PTY_ID = '01960000-0000-7000-8000-000000000301';
const SESSION_ID = '01960000-0000-7000-8000-000000000401';

describe('useShellCommandBlocks', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('projeta eventos started/finished no useCommandBlocksStore().feed existente, deduplica por execução e mapeia status', () => {
        const blocksStore = useCommandBlocksStore();
        const adapter = useShellCommandBlocks({ paneId: PANE_ID, ptyId: PTY_ID });

        const startedEv: CommandStarted_Serialize = {
            binding: {
                ptyId: PTY_ID,
                sessionId: SESSION_ID,
                tabId: '01960000-0000-7000-8000-000000000101',
                paneId: PANE_ID
            },
            execution: {
                blockId: 'blk-1',
                commandSeq: 1,
                startedAt: 1000,
                commandPreview: null,
                source: 'osc133'
            },
            commandText: null,
            commandTextQuality: 'unavailable',
            cwd: '/tmp',
            startPos: { ptySeq: 1, byteOffset: 0 },
            outputStartPos: { ptySeq: 1, byteOffset: 10 },
            confidence: 'exact',
            persistence: 'queued'
        };

        adapter.applyStarted(startedEv);
        // Chamada duplicada para a mesma execução é ignorada
        adapter.applyStarted(startedEv);

        const listAfterStart = blocksStore.blocksOf(PANE_ID);
        expect(listAfterStart).toHaveLength(1);
        expect(listAfterStart[0]?.command).toBe(UNAVAILABLE_COMMAND_LABEL);
        expect(listAfterStart[0]?.status).toBe('running');

        const finishedEv: CommandFinished_Serialize = {
            binding: startedEv.binding,
            blockId: 'blk-1',
            commandSeq: 1,
            commandText: 'php artisan migrate',
            commandTextQuality: 'exact',
            cwd: '/tmp',
            startedAt: 1000,
            endedAt: 1250,
            durationMs: 250,
            status: 'failed',
            endReason: 'exit_non_zero',
            exitCode: 2,
            signal: null,
            startPos: { ptySeq: 1, byteOffset: 0 },
            outputStartPos: { ptySeq: 1, byteOffset: 10 },
            endPos: { ptySeq: 2, byteOffset: 80 },
            outputBytes: 70,
            outputLines: 2,
            outputPreview: 'SQLSTATE error',
            errorExcerpt: 'SQLSTATE error',
            inspectable: true,
            source: 'osc133',
            confidence: 'exact',
            persistence: 'persisted'
        };

        adapter.applyFinished(finishedEv);
        const updated = blocksStore.lastBlock(PANE_ID);
        expect(updated?.command).toBe('php artisan migrate');
        expect(updated?.status).toBe('error');
        expect(updated?.exitCode).toBe(2);
        expect(updated?.meta.inspectable).toBe(true);
        expect(updated?.meta.persistence).toBe('persisted');

        // Testa mapeamento de resultado desconhecido (exitCode: null -> aborted)
        adapter.applyFinished({
            ...finishedEv,
            blockId: 'blk-2',
            commandSeq: 2,
            status: 'pty_exited',
            endReason: 'pty_exited',
            exitCode: null,
            inspectable: false
        });
        const unknownBlock = blocksStore.lastBlock(PANE_ID);
        expect(unknownBlock?.id).toBe('blk-2');
        expect(unknownBlock?.status).toBe('aborted');
        expect(unknownBlock?.exitCode).toBeNull();

        adapter.stop();
    });

    it('hidrata CommandBlockDto, resolve marcadores best-effort e integra extensão shell.integration ao TerminalContext e InputMode', async () => {
        const blocksStore = useCommandBlocksStore();
        const historyStore = useShellHistoryStore();
        const tabStatusStore = useTabStatusStore();
        const markers = useShellMarkers();
        const adapter = useShellCommandBlocks({
            paneId: PANE_ID,
            ptyId: PTY_ID,
            sessionId: SESSION_ID
        });

        await adapter.start();

        const dto: CommandBlockDto_Serialize = {
            id: 'blk-dto-1',
            sessionId: SESSION_ID,
            tabId: '01960000-0000-7000-8000-000000000101',
            paneId: PANE_ID,
            ptyId: PTY_ID,
            commandSeq: 3,
            commandText: 'npm run dev',
            commandTextQuality: 'exact',
            cwd: '/home/user/app',
            startedAt: 2000,
            endedAt: 2500,
            durationMs: 500,
            exitCode: 0,
            signal: null,
            status: 'succeeded',
            endReason: 'exit_zero',
            isContinuous: true,
            source: 'osc133',
            confidence: 'exact',
            outputText: 'ready',
            outputKind: 'head_tail_truncated',
            outputBytes: 65536,
            outputLines: 200,
            errorExcerpt: null,
            inspectable: false,
            startPtySeq: 1,
            startByteOffset: 0,
            outputStartPtySeq: 1,
            outputStartByteOffset: 4,
            endPtySeq: 2,
            endByteOffset: 100,
            persistence: 'persisted'
        };

        adapter.applyDto(dto);
        const hydrated = blocksStore.lastBlock(PANE_ID);
        expect(hydrated?.id).toBe('blk-dto-1');
        expect(hydrated?.status).toBe('success');
        expect(hydrated?.meta.outputTruncated).toBe(true);

        const anchor = markers.resolveAnchor(PANE_ID);
        expect(anchor.positionQuality).toBe('unavailable');
        markers.markPaneMarkersUnavailable(PANE_ID);
        expect(markers.getPositionQuality(PANE_ID, 'blk-dto-1')).toBe('unavailable');

        // Testa extensão shell.integration
        const ext = createShellIntegrationExtension();
        const subAct = ext.activate({
            paneId: PANE_ID,
            ptyId: PTY_ID
        } as never);
        subAct?.dispose();

        const extDisposable = installShellIntegrationExtension();
        historyStore.handleCwdChanged({
            binding: {
                ptyId: PTY_ID,
                sessionId: SESSION_ID,
                tabId: dto.tabId,
                paneId: PANE_ID
            },
            cwd: '/home/user/trusted',
            at: 3000
        });
        historyStore.handleIntegrationMode({
            binding: {
                ptyId: PTY_ID,
                sessionId: SESSION_ID,
                tabId: dto.tabId,
                paneId: PANE_ID
            },
            mode: 'osc133',
            shellKind: 'bash',
            reason: 'first_osc133_received'
        });

        const ctxOsc = resolveTerminalContext({ paneId: PANE_ID, ptyId: PTY_ID });
        expect(ctxOsc.cwd).toBe('/home/user/trusted');
        const defaultModes = {
            altScreen: false,
            mouseTracking: 'none' as const,
            applicationCursorKeys: false,
            bracketedPaste: true
        };
        const modeIdle = resolveInputMode({
            context: ctxOsc,
            modes: defaultModes,
            override: null,
            richInputEnabled: true
        });
        expect(modeIdle.mode).toBe('rich');

        // Quando muda para fallback_proc ou comando em execução, cwd não é confiável e input vai para passthrough
        historyStore.handleIntegrationMode({
            binding: {
                ptyId: PTY_ID,
                sessionId: SESSION_ID,
                tabId: dto.tabId,
                paneId: PANE_ID
            },
            mode: 'fallback_proc',
            shellKind: 'bash',
            reason: 'fallback'
        });
        tabStatusStore.applySnapshot({
            tabId: dto.tabId ?? 'tab-1',
            revision: 1,
            status: 'running',
            focused: true,
            panes: [
                {
                    paneId: PANE_ID,
                    ptyId: PTY_ID,
                    sessionId: SESSION_ID,
                    status: 'running',
                    integrationMode: 'fallback_proc',
                    continuousPolicy: 'auto',
                    cwd: '/home/user/untrusted',
                    activeExecution: {
                        blockId: 'blk-dto-1',
                        commandSeq: 3,
                        startedAt: 2000,
                        commandPreview: 'npm run dev',
                        source: 'proc_fallback'
                    },
                    lastCompletion: null,
                    updatedAt: 3000
                }
            ],
            lastCompletion: null,
            attention: null,
            updatedAt: 3000
        });

        const ctxFallback = resolveTerminalContext({ paneId: PANE_ID, ptyId: PTY_ID });
        expect(ctxFallback.cwd).toBeNull();
        const modeRunning = resolveInputMode({
            context: ctxFallback,
            modes: defaultModes,
            override: null,
            richInputEnabled: true
        });
        expect(modeRunning.mode).toBe('passthrough');

        extDisposable.dispose();
        adapter.stop();
    });
});
