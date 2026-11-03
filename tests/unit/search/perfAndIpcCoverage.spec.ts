import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { usePerfTelemetry } from '@/composables/dev/usePerfTelemetry';
import { useGlobalSearch } from '@/composables/search/useGlobalSearch';
import { createDbBackup, listDbBackups, restoreDbBackup } from '@/ipc/backup';
import { githubIpc } from '@/ipc/github';
import {
    cancelSearch,
    getSearchIndexStats,
    onSearchIndexProgress,
    onSearchInvalidated,
    rebuildSearchIndex,
    releaseSearch,
    searchGlobal
} from '@/ipc/search';
import { commands } from '@/types/bindings';

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn(async (_event: string, cb: (ev: { payload: unknown }) => void) => {
        cb({ payload: { reason: 'mock', done: true } });
        return () => {};
    })
}));

describe('Cobertura de usePerfTelemetry, useGlobalSearch e wrappers IPC (search, backup, github)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('exercita ciclo completo de usePerfTelemetry (busca, eco de terminal, frames e exportação JSON)', async () => {
        const perf = usePerfTelemetry();
        perf.resetSamples();

        expect(perf.recordSearchPainted(100)).toBeNull();
        perf.recordSearchKey(10);
        perf.recordSearchDebounceEnd(25);
        perf.recordSearchIpcSpan(25, 45, { queueUs: 1200, sqlUs: 4500, serializeUs: 800 });
        const searchSample = perf.recordSearchPainted(52);
        expect(searchSample).not.toBeNull();
        expect(searchSample?.debounceMs).toBe(15);
        expect(searchSample?.ipcTotalMs).toBe(20);
        expect(searchSample?.keyToPaintMs).toBe(42);
        expect(perf.lastSearchSample.value?.keyToPaintMs).toBe(42);
        expect(perf.searchKeyToPaintSummary.value.count).toBe(1);

        expect(perf.recordTerminalPaint('inexistente', 50)).toBeNull();
        perf.recordTerminalConsume('inexistente', 50);
        perf.recordTerminalInput('probe-1', 100);
        perf.recordTerminalConsume('probe-1', 104);
        const termSample = perf.recordTerminalPaint('probe-1', 112);
        expect(termSample?.inputToConsumeMs).toBe(4);
        expect(termSample?.inputToPaintMs).toBe(12);
        expect(perf.terminalConsumeSummary.value.p50Ms).toBe(4);
        expect(perf.terminalPaintSummary.value.p95Ms).toBe(12);

        perf.recordFrameInterval(-5);
        perf.recordFrameInterval(16.67);
        perf.recordFrameInterval(16.12);
        expect(perf.frameIntervalSummary.value.count).toBe(2);

        vi.spyOn(commands, 'perfSnapshot').mockResolvedValueOnce({
            status: 'ok',
            data: {
                capturedAtMs: 1000,
                rssBytes: 50_000_000,
                sqliteWalBytes: 4096
            } as never
        });
        const snap = await perf.refreshBackendSnapshot();
        expect(snap?.capturedAtMs).toBe(1000);

        const exported = JSON.parse(perf.exportJsonSample());
        expect(exported.searchKeyToPaint.count).toBe(1);
        expect(exported.terminalInputToPaint.count).toBe(1);

        perf.resetSamples();
        expect(perf.searchKeyToPaintSummary.value.count).toBe(0);
    });

    it('exercita useGlobalSearch (foco anterior, composição IME e eventos)', async () => {
        const btn = document.createElement('button');
        document.body.appendChild(btn);
        btn.focus();

        const globalSearch = useGlobalSearch();
        globalSearch.openPalette('', 'proj-1');
        expect(globalSearch.searchStore.open).toBe(true);

        globalSearch.onCompositionStart();
        expect(globalSearch.searchStore.isComposingIme).toBe(true);
        globalSearch.onCompositionEnd('');
        expect(globalSearch.searchStore.isComposingIme).toBe(false);

        await globalSearch.attachEvents();
        await globalSearch.attachEvents(); // idempotente
        globalSearch.detachEvents();

        globalSearch.closePalette();
        expect(globalSearch.searchStore.open).toBe(false);
        btn.remove();
    });

    it('exercita wrappers IPC de search, backup e github', async () => {
        vi.spyOn(commands, 'searchQuery').mockResolvedValueOnce({
            status: 'ok',
            data: {
                requestId: 'r1',
                generation: 1,
                snapshotId: 's1',
                expiresAtMs: 2000,
                sections: [],
                indexedThroughMs: 1000
            }
        });
        vi.spyOn(commands, 'searchCancel').mockResolvedValueOnce({
            status: 'ok',
            data: { cancelled: true } as never
        });
        vi.spyOn(commands, 'searchRelease').mockResolvedValueOnce({
            status: 'ok',
            data: null
        });
        vi.spyOn(commands, 'searchIndexStats').mockResolvedValueOnce({
            status: 'ok',
            data: []
        });
        vi.spyOn(commands, 'searchIndexRebuild').mockResolvedValueOnce({
            status: 'ok',
            data: { jobId: 'j1', startedAtMs: 100 } as never
        });

        await searchGlobal({
            requestId: 'r1',
            generation: 1,
            query: 'git',
            filters: {
                projectId: null,
                tabId: null,
                sessionId: null,
                paneId: null,
                fromMs: null,
                untilMs: null,
                exit: { kind: 'any' },
                sources: []
            },
            sort: { kind: 'relevance' },
            pageSize: 25,
            cursor: null
        });
        await cancelSearch('r1');
        await releaseSearch('s1');
        await getSearchIndexStats();
        await rebuildSearchIndex([]);
        await (await onSearchIndexProgress(() => {}))();
        await (await onSearchInvalidated(() => {}))();

        vi.spyOn(commands, 'backupList').mockResolvedValueOnce({ status: 'ok', data: [] });
        vi.spyOn(commands, 'backupCreate').mockResolvedValueOnce({
            status: 'ok',
            data: { fileName: 'b1.db', createdAtMs: 100, sizeBytes: 1024, sha256Hex: 'abc' } as never
        });
        vi.spyOn(commands, 'backupRestoreSchedule').mockResolvedValueOnce({
            status: 'ok',
            data: { backupFileName: 'b1.db', scheduledAtMs: 100 } as never
        });

        expect(await listDbBackups()).toEqual([]);
        expect((await createDbBackup()).fileName).toBe('b1.db');
        expect((await restoreDbBackup('b1.db')).backupFileName).toBe('b1.db');

        // Exercita métodos diretos do githubIpc
        vi.spyOn(commands, 'githubRepoResolve').mockResolvedValueOnce({
            status: 'ok',
            data: { kind: 'not_git_repo' }
        });
        vi.spyOn(commands, 'githubAuthStatus').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubAuthSetToken').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubAuthClearToken').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubIssuesList').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubIssueDetail').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubIssueCounts').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubRefresh').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubLabelsCatalog').mockResolvedValueOnce({
            status: 'ok',
            data: []
        });
        vi.spyOn(commands, 'githubRepoLabels').mockResolvedValueOnce({
            status: 'ok',
            data: []
        });
        vi.spyOn(commands, 'githubLabelsEnsure').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubIssueCreate').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubIssueLabelsUpdate').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubIssueSendMaxcode').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubIssueWorktreeResolve').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubIssueWorktreeCreate').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubIssueOpenTab').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubBugCyclePreview').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubBugCycleStart').mockResolvedValueOnce({
            status: 'ok',
            data: {} as never
        });
        vi.spyOn(commands, 'githubBugCycleStatus').mockResolvedValueOnce({
            status: 'ok',
            data: null
        });

        await githubIpc.repoResolve('p1');
        await githubIpc.authStatus();
        await githubIpc.authSetToken('tok');
        await githubIpc.authClearToken();
        await githubIpc.issuesList('p1');
        await githubIpc.issueDetail('p1', 1);
        await githubIpc.issueCounts('p1');
        await githubIpc.refresh('p1');
        await githubIpc.labelsCatalog();
        await githubIpc.repoLabels('p1');
        await githubIpc.labelsEnsure('p1');
        await githubIpc.issueCreate({} as never);
        await githubIpc.issueLabelsUpdate({} as never);
        await githubIpc.issueSendMaxcode({} as never);
        await githubIpc.issueWorktreeResolve('p1', 1);
        await githubIpc.issueWorktreeCreate({} as never);
        await githubIpc.issueOpenTab({} as never);
        await githubIpc.bugCyclePreview('p1');
        await githubIpc.bugCycleStart({} as never);
        await githubIpc.bugCycleStatus('p1');
        await githubIpc.onIssuesChanged(() => {});
        await githubIpc.onIssueUpdated(() => {});
        await githubIpc.onLabelsSynced(() => {});
        await githubIpc.onAuthChanged(() => {});
        await githubIpc.onBugCycleProgress(() => {});
        await githubIpc.onRateLimit(() => {});
    });
});
