import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ref } from 'vue';
import { useIssueFilters } from '@/composables/useIssueFilters';
import {
    CANONICAL_CATEGORY_LABELS,
    CANONICAL_CLOSURE_LABELS,
    CANONICAL_LABELS,
    CANONICAL_STATE_LABELS,
    computeContrastTextColor,
    findCanonicalSpec,
    sortLabelsCanonically
} from '@/constants/githubLabels';
import { githubIpc } from '@/ipc/github';
import { useGithubIssuesStore } from '@/stores/githubIssues';
import type { IssueSummary } from '@/types/bindings';

vi.mock('@/ipc/github', () => ({
    githubIpc: {
        repoResolve: vi.fn(),
        issueCounts: vi.fn(),
        issuesList: vi.fn(),
        issueDetail: vi.fn(),
        refresh: vi.fn(),
        authStatus: vi.fn(),
        authSetToken: vi.fn(),
        authClearToken: vi.fn(),
        labelsCatalog: vi.fn(),
        repoLabels: vi.fn(),
        labelsEnsure: vi.fn(),
        issueCreate: vi.fn(),
        issueLabelsUpdate: vi.fn(),
        issueSendMaxcode: vi.fn(),
        issueWorktreeResolve: vi.fn(),
        issueWorktreeCreate: vi.fn(),
        issueOpenTab: vi.fn(),
        bugCyclePreview: vi.fn(),
        bugCycleStart: vi.fn(),
        bugCycleStatus: vi.fn(),
        onIssuesChanged: vi.fn(),
        onIssueUpdated: vi.fn(),
        onLabelsSynced: vi.fn(),
        onAuthChanged: vi.fn(),
        onBugCycleProgress: vi.fn(),
        onRateLimit: vi.fn()
    }
}));

function makeIssueSummary(num: number, title: string): IssueSummary {
    return {
        repoKey: 'github.com/johnattas/maxterminal',
        number: num,
        title,
        state: 'open',
        stateReason: null,
        authorLogin: 'johnattas',
        authorAvatarUrl: null,
        assignees: [],
        milestoneTitle: null,
        labels: [
            {
                name: 'Seguranca',
                nameLower: 'seguranca',
                colorHex: 'ee0701',
                description: 'Categoria: segurança',
                canonicalGroup: 'category'
            },
            {
                name: 'Waiting Plan',
                nameLower: 'waiting plan',
                colorHex: 'fbca04',
                description: 'Aguardando plano',
                canonicalGroup: 'state'
            }
        ],
        activeStateLabel: 'Waiting Plan',
        activeClosureLabel: null,
        categoryLabels: ['Seguranca'],
        commentsCount: 2,
        hasPlan: true,
        htmlUrl: `https://github.com/johnattas/MaxTerminal/issues/${num}`,
        createdAtMs: 1700000000000,
        updatedAtMs: 1700000005000,
        closedAtMs: null
    };
}

describe('Fase 16 — Catálogo Canônico, Store Pinia e Filtros', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('expõe exatamente as 24 labels canônicas (15 state, 3 closure, 6 category) e ordena por prioridade', () => {
        expect(CANONICAL_STATE_LABELS).toHaveLength(15);
        expect(CANONICAL_CLOSURE_LABELS).toHaveLength(3);
        expect(CANONICAL_CATEGORY_LABELS).toHaveLength(6);
        expect(CANONICAL_LABELS).toHaveLength(24);

        expect(findCanonicalSpec('waiting plan')?.name).toBe('Waiting Plan');
        expect(findCanonicalSpec('checked approved')?.group).toBe('state');
        expect(findCanonicalSpec('duplicated')?.group).toBe('closure');
        expect(findCanonicalSpec('seguranca')?.group).toBe('category');

        const sorted = sortLabelsCanonically([
            { name: 'custom-z', colorHex: 'cccccc', canonicalGroup: null },
            { name: 'Seguranca', colorHex: 'ee0701', canonicalGroup: 'category' },
            { name: 'Planned', colorHex: '0e8a16', canonicalGroup: 'state' }
        ]);
        expect(sorted.map((l) => l.name)).toEqual(['Planned', 'Seguranca', 'custom-z']);
        expect(computeContrastTextColor('fbca04')).toBe('#0f172a');
        expect(computeContrastTextColor('b60205')).toBe('#f8fafc');
    });

    it('resolve repositório GitHub e carrega listagem paginada de issues na store', async () => {
        const store = useGithubIssuesStore();
        vi.mocked(githubIpc.repoResolve).mockResolvedValueOnce({
            kind: 'git_hub',
            identity: {
                repoKey: 'github.com/johnattas/maxterminal',
                host: 'github.com',
                owner: 'johnattas',
                repo: 'MaxTerminal',
                remoteName: 'origin',
                htmlUrl: 'https://github.com/johnattas/MaxTerminal',
                defaultBranch: 'main'
            }
        });
        vi.mocked(githubIpc.issueCounts).mockResolvedValueOnce({
            projectId: 'proj-1',
            repoKey: 'github.com/johnattas/maxterminal',
            openCount: 4,
            closedCount: 1,
            byStateLabel: { 'Waiting Plan': 4 },
            byCategoryLabel: { Seguranca: 2 },
            withPlanCount: 2
        });

        const res = await store.resolveProjectRepo('proj-1');
        expect(res.kind).toBe('git_hub');
        expect(store.isGithubRepo('proj-1')).toBe(true);
        expect(store.openCountFor('proj-1')).toBe(4);

        vi.mocked(githubIpc.issuesList).mockResolvedValueOnce({
            repoKey: 'github.com/johnattas/maxterminal',
            items: [makeIssueSummary(10, 'Corrigir PTY'), makeIssueSummary(11, 'Ajustar UI')],
            totalMatching: 2,
            openIssuesCount: 4,
            stale: false,
            transportUsed: 'gh_cli',
            lastSuccessAtMs: 1700000005000,
            missingCanonicalLabelsCount: 3,
            rateLimit: null
        });

        await store.loadIssues('proj-1');
        expect(store.issuesFor('proj-1')).toHaveLength(2);
        expect(store.selectedIssueNumberFor('proj-1')).toBe(10);
        expect(store.missingCanonicalCountByProject['proj-1']).toBe(3);
    });

    it('useIssueFilters atualiza estado, labels, busca e reseta filtros disparando loadIssues', async () => {
        const store = useGithubIssuesStore();
        vi.mocked(githubIpc.issuesList).mockResolvedValue({
            repoKey: 'github.com/johnattas/maxterminal',
            items: [],
            totalMatching: 0,
            openIssuesCount: 0,
            stale: false,
            transportUsed: 'gh_cli',
            lastSuccessAtMs: 1700000005000,
            missingCanonicalLabelsCount: 0,
            rateLimit: null
        });

        const projectId = ref('proj-1');
        const filters = useIssueFilters(projectId);

        expect(filters.hasActiveFilters.value).toBe(false);
        await filters.toggleStateLabel('Waiting Plan');
        expect(store.filterFor('proj-1').stateLabels).toEqual(['Waiting Plan']);
        expect(filters.hasActiveFilters.value).toBe(true);

        await filters.setSearchText('termo');
        expect(store.filterFor('proj-1').searchText).toBe('termo');

        await filters.resetFilters();
        expect(filters.hasActiveFilters.value).toBe(false);

        await filters.setStateFilter('closed');
        expect(store.filterFor('proj-1').state).toBe('closed');
        await filters.toggleCategoryLabel('Seguranca');
        expect(store.filterFor('proj-1').categoryLabels).toEqual(['Seguranca']);
        await filters.toggleCategoryLabel('Seguranca');
        expect(store.filterFor('proj-1').categoryLabels).toEqual([]);
        await filters.toggleClosureLabel('Duplicated');
        expect(store.filterFor('proj-1').closureLabels).toEqual(['Duplicated']);
        await filters.toggleClosureLabel('Duplicated');
        expect(store.filterFor('proj-1').closureLabels).toEqual([]);
        await filters.setHasPlan(true);
        expect(store.filterFor('proj-1').hasPlan).toBe(true);
        await filters.setSort('created_desc');
        expect(store.filterFor('proj-1').sort).toBe('created_desc');
    });

    it('exercita ações completas de useGithubIssuesStore e useGithubIssueEvents', async () => {
        const { useGithubIssueEvents } = await import('@/composables/useGithubIssueEvents');
        const store = useGithubIssuesStore();

        const summary10 = makeIssueSummary(10, 'Issue 10');
        vi.mocked(githubIpc.issueDetail).mockResolvedValue({
            summary: summary10,
            bodyMarkdown: '## PLANO DE IMPLEMENTAÇÃO\n- item',
            comments: []
        } as never);

        const detail = await store.selectIssue('proj-1', 10);
        expect(detail?.summary.number).toBe(10);
        expect(store.selectedDetailFor('proj-1')?.summary.number).toBe(10);
        // Hit de cache sem forceRefresh
        expect((await store.selectIssue('proj-1', 10, false))?.summary.number).toBe(10);

        vi.mocked(githubIpc.refresh).mockResolvedValueOnce({
            repoKey: 'github.com/johnattas/maxterminal',
            items: [summary10],
            totalMatching: 1,
            openIssuesCount: 1,
            stale: false,
            transportUsed: 'gh_cli',
            lastSuccessAtMs: 1700000010000,
            missingCanonicalLabelsCount: 0,
            rateLimit: {
                limit: 5000,
                remaining: 4990,
                used: 10,
                resetAtMs: 1700009999000,
                retryAfterUntilMs: null,
                lowRemaining: false
            }
        });
        await store.refreshProject('proj-1');
        expect(store.rateLimit?.remaining).toBe(4990);

        vi.mocked(githubIpc.authStatus).mockResolvedValueOnce({
            ghCli: { installed: true, authenticated: true, accountLogin: 'johnattas', scopes: [] },
            restToken: { configured: false, source: 'none' },
            preferredTransport: 'gh_cli',
            rateLimit: null
        } as never);
        await store.loadAuthStatus();
        expect(store.authStatus?.ghCli.accountLogin).toBe('johnattas');

        vi.mocked(githubIpc.authSetToken).mockResolvedValueOnce({
            ghCli: { installed: true, authenticated: true, accountLogin: 'johnattas', scopes: [] },
            restToken: { configured: true, source: 'sqlite' },
            preferredTransport: 'gh_cli',
            rateLimit: null
        } as never);
        await store.saveRestToken('ghp_123');
        expect(store.authStatus?.restToken.configured).toBe(true);

        vi.mocked(githubIpc.authClearToken).mockResolvedValueOnce({
            ghCli: { installed: true, authenticated: true, accountLogin: 'johnattas', scopes: [] },
            restToken: { configured: false, source: 'none' },
            preferredTransport: 'gh_cli',
            rateLimit: null
        } as never);
        await store.clearRestToken();
        expect(store.authStatus?.restToken.configured).toBe(false);

        vi.mocked(githubIpc.labelsCatalog).mockResolvedValueOnce([
            {
                name: 'Waiting Plan',
                colorHex: 'fbca04',
                description: 'Aguardando',
                group: 'state',
                order: 1
            }
        ]);
        expect(await store.loadLabelsCatalog()).toHaveLength(1);
        expect(await store.loadLabelsCatalog()).toHaveLength(1);

        vi.mocked(githubIpc.repoLabels).mockResolvedValueOnce([
            {
                name: 'Waiting Plan',
                nameLower: 'waiting plan',
                colorHex: 'fbca04',
                description: 'Aguardando',
                canonicalGroup: 'state'
            }
        ]);
        expect(await store.loadRepoLabels('proj-1')).toHaveLength(1);

        vi.mocked(githubIpc.labelsEnsure).mockResolvedValueOnce({
            created: ['Planned'],
            unchanged: ['Waiting Plan'],
            missing: [],
            repoLabels: []
        } as never);
        const report = await store.ensureCanonicalLabels('proj-1', false);
        expect(report.created).toEqual(['Planned']);

        vi.mocked(githubIpc.issueCreate).mockResolvedValueOnce(makeIssueSummary(99, 'Nova Issue'));
        const created = await store.createIssue({
            projectId: 'proj-1',
            title: 'Nova Issue',
            body: 'Corpo',
            stateLabel: 'Waiting Plan',
            categoryLabels: ['Seguranca'],
            assignees: []
        } as never);
        expect(created.number).toBe(99);

        vi.mocked(githubIpc.issueLabelsUpdate).mockResolvedValueOnce(
            makeIssueSummary(99, 'Nova Issue Atualizada')
        );
        const updated = await store.updateIssueLabels({
            projectId: 'proj-1',
            issueNumber: 99
        } as never);
        expect(updated.title).toBe('Nova Issue Atualizada');

        vi.mocked(githubIpc.issueSendMaxcode).mockResolvedValueOnce({
            dispatchedAtMs: 1700000000000,
            command: 'bug_plan'
        } as never);
        await store.sendToMaxCode({
            projectId: 'proj-1',
            issueNumber: 99,
            action: 'bug_plan'
        });

        vi.mocked(githubIpc.issueWorktreeResolve).mockResolvedValueOnce({
            status: 'existing_worktree',
            path: '/tmp/wt',
            branch: 'fix/99'
        });
        await store.resolveIssueWorktree('proj-1', 99);

        vi.mocked(githubIpc.issueWorktreeCreate).mockResolvedValueOnce({
            status: 'existing_worktree',
            path: '/tmp/wt',
            branch: 'fix/99'
        });
        await store.createIssueWorktree({
            projectId: 'proj-1',
            issueNumber: 99,
            branchName: null,
            baseRef: null
        });

        vi.mocked(githubIpc.issueOpenTab).mockResolvedValueOnce({
            tabId: 'tab-99',
            paneId: 'pane-99',
            cwd: '/tmp/wt',
            branch: 'fix/99'
        } as never);
        store.setPinnedTabActive('proj-1', true);
        expect(store.isPinnedTabActive('proj-1')).toBe(true);
        await store.openIssueTab({
            projectId: 'proj-1',
            issueNumber: 99,
            target: { kind: 'terminal' },
            directory: { strategy: 'use_project_root' }
        });
        expect(store.isPinnedTabActive('proj-1')).toBe(false);

        vi.mocked(githubIpc.bugCyclePreview).mockResolvedValueOnce({
            projectId: 'proj-1',
            repoKey: 'github.com/johnattas/maxterminal',
            candidates: [],
            skipped: [],
            activeCycle: null
        });
        await store.previewBugCycle('proj-1');

        vi.mocked(githubIpc.bugCycleStart).mockResolvedValueOnce({
            cycleId: 'cyc-1',
            projectId: 'proj-1',
            repoKey: 'github.com/johnattas/maxterminal',
            status: 'running',
            startedAtMs: 1000,
            updatedAtMs: 1000,
            finishedAtMs: null,
            currentIssue: 99,
            currentStage: 'plan',
            items: [],
            lastError: null
        } as never);
        await store.startBugCycle({ projectId: 'proj-1', includeReplanAndRejected: true });
        vi.mocked(githubIpc.bugCycleStatus).mockResolvedValueOnce(null);
        await store.controlBugCycle('proj-1', 'cancel');

        // Eventos Tauri github:*
        const unlisten = vi.fn();
        vi.mocked(githubIpc.onIssuesChanged).mockImplementationOnce(async (cb) => {
            cb({
                projectId: 'proj-1',
                repoKey: 'github.com/johnattas/maxterminal',
                openIssuesCount: 12,
                transportUsed: 'gh_cli',
                syncedAtMs: 2000
            });
            return unlisten;
        });
        vi.mocked(githubIpc.onIssueUpdated).mockImplementationOnce(async (cb) => {
            cb({
                projectId: 'proj-1',
                repoKey: 'github.com/johnattas/maxterminal',
                issue: makeIssueSummary(99, 'Via Evento')
            });
            return unlisten;
        });
        vi.mocked(githubIpc.onLabelsSynced).mockImplementationOnce(async (cb) => {
            cb({
                projectId: 'proj-1',
                repoKey: 'github.com/johnattas/maxterminal',
                totalLabels: 24,
                missingCanonicalCount: 0
            });
            return unlisten;
        });
        vi.mocked(githubIpc.onAuthChanged).mockImplementationOnce(async (cb) => {
            cb({
                status: {
                    ghCli: {
                        installed: true,
                        authenticated: true,
                        accountLogin: 'johnattas',
                        scopes: []
                    },
                    restToken: { configured: false, source: 'none' },
                    preferredTransport: 'gh_cli',
                    rateLimit: {
                        limit: 5000,
                        remaining: 4000,
                        used: 1000,
                        resetAtMs: 9000,
                        retryAfterUntilMs: null,
                        lowRemaining: false
                    }
                } as never
            });
            return unlisten;
        });
        vi.mocked(githubIpc.onBugCycleProgress).mockImplementationOnce(async (cb) => {
            cb({
                cycle: {
                    cycleId: 'cyc-1',
                    projectId: 'proj-1',
                    repoKey: 'github.com/johnattas/maxterminal',
                    status: 'completed',
                    startedAtMs: 1000,
                    updatedAtMs: 2000,
                    finishedAtMs: 2000,
                    currentIssue: null,
                    currentStage: null,
                    items: [],
                    lastError: null
                } as never
            });
            return unlisten;
        });
        vi.mocked(githubIpc.onRateLimit).mockImplementationOnce(async (cb) => {
            cb({
                rateLimit: {
                    limit: 5000,
                    remaining: 3500,
                    used: 1500,
                    resetAtMs: 9500,
                    retryAfterUntilMs: null,
                    lowRemaining: false
                }
            } as never);
            return unlisten;
        });

        const events = useGithubIssueEvents();
        await events.start();
        expect(store.openCountFor('proj-1')).toBe(12);
        expect(store.rateLimit?.remaining).toBe(3500);
        events.stop();
        expect(unlisten).toHaveBeenCalledTimes(6);
    });
});
