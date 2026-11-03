import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
    FLASH_HIGHLIGHT_CLASS,
    FLASH_HIGHLIGHT_DURATION_MS,
    useSearchNavigation
} from '@/composables/search/useSearchNavigation';
import { useCommandBlocksStore } from '@/stores/commandBlocks';
import { useProjectsStore } from '@/stores/projects';
import { useSessionsStore } from '@/stores/sessions';
import { useTabsStore } from '@/stores/tabs';

describe('useSearchNavigation (CA-12, CA-13)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.useFakeTimers();
        document.body.innerHTML = '';
    });

    afterEach(() => {
        vi.useRealTimers();
        document.body.innerHTML = '';
    });

    it('seleciona o projeto, foca a aba viva, rola até o bloco e aplica realce temporário de 1,5 s', async () => {
        const projectsStore = useProjectsStore();
        const tabsStore = useTabsStore();
        const commandBlocksStore = useCommandBlocksStore();

        projectsStore.projects = [
            {
                id: 'proj-1',
                title: 'Projeto Alpha',
                icon: 'lucide:folder',
                color: '#00BCD4',
                path: '/workspace/alpha',
                rootId: null,
                stack: 'rust',
                identitySource: 'auto',
                sortOrder: 0,
                missing: false,
                createdAt: 1000,
                updatedAt: 1000
            }
        ];

        tabsStore.byId = {
            'tab-1': {
                id: 'tab-1',
                projectId: 'proj-1',
                templateId: null,
                title: 'Terminal 1',
                icon: 'lucide:terminal',
                position: 0,
                origin: 'manual',
                continuous: false,
                lifecycle: 'ready',
                layout: { kind: 'leaf', paneId: 'pane-1' },
                primaryPaneId: 'pane-1',
                activePaneId: 'pane-1',
                panes: [
                    {
                        id: 'pane-1',
                        ptyId: 'pty-1',
                        generation: 1,
                        cwd: '/workspace/alpha',
                        state: 'live',
                        exit: null,
                        error: null
                    }
                ],
                tabSessionId: 'ts-1',
                initialDispatch: 'none',
                revision: 1,
                layoutRevision: 1
            }
        };
        projectsStore.attachTab('tab-1', 'proj-1');
        vi.spyOn(tabsStore, 'focusTab').mockResolvedValue(tabsStore.byId['tab-1']!);

        commandBlocksStore.byPane['pane-1'] = {
            order: ['blk-42'],
            blocks: {
                'blk-42': {
                    id: 'blk-42',
                    paneId: 'pane-1',
                    command: 'cargo test',
                    cwd: '/workspace/alpha',
                    startedAt: 1000,
                    finishedAt: 1200,
                    exitCode: 0,
                    status: 'success',
                    promptLine: 10,
                    outputStartLine: 11,
                    outputEndLine: 15,
                    meta: {}
                }
            }
        };

        const blockEl = document.createElement('div');
        blockEl.setAttribute('data-block-id', 'blk-42');
        document.body.appendChild(blockEl);

        const { openTarget } = useSearchNavigation();
        const res = await openTarget({
            kind: 'command',
            projectId: 'proj-1',
            tabId: 'tab-1',
            sessionId: 'sess-1',
            paneId: 'pane-1',
            blockId: 'blk-42'
        });

        expect(res.ok).toBe(true);
        expect(res.mode).toBe('live-block');
        expect(projectsStore.selectedId).toBe('proj-1');
        expect(tabsStore.focusTab).toHaveBeenCalledWith('tab-1', 'pane-1');
        expect(blockEl.classList.contains(FLASH_HIGHLIGHT_CLASS)).toBe(true);

        await vi.advanceTimersByTimeAsync(FLASH_HIGHLIGHT_DURATION_MS);
        expect(blockEl.classList.contains(FLASH_HIGHLIGHT_CLASS)).toBe(false);
    });

    it('abre o visualizador somente leitura de histórico para sessão arquivada sem instanciar PTY', async () => {
        const projectsStore = useProjectsStore();
        const sessionsStore = useSessionsStore();

        projectsStore.projects = [
            {
                id: 'proj-1',
                title: 'Projeto Alpha',
                icon: 'lucide:folder',
                color: '#00BCD4',
                path: '/workspace/alpha',
                rootId: null,
                stack: 'rust',
                identitySource: 'auto',
                sortOrder: 0,
                missing: false,
                createdAt: 1000,
                updatedAt: 1000
            }
        ];

        vi.spyOn(sessionsStore, 'loadSessions').mockResolvedValue([]);

        const { openTarget, readonlyViewerState } = useSearchNavigation();
        const res = await openTarget({
            kind: 'command',
            projectId: 'proj-1',
            tabId: 'tab-archived',
            sessionId: 'sess-old',
            paneId: 'pane-old',
            blockId: 'blk-old'
        });

        expect(res.ok).toBe(true);
        expect(res.mode).toBe('archived-session');
        expect(readonlyViewerState.value.open).toBe(true);
        expect(readonlyViewerState.value.sessionId).toBe('sess-old');
        expect(readonlyViewerState.value.blockId).toBe('blk-old');
    });

    it('abre nova aba quando template é acionado com action="open-in-new-tab" (Ctrl+Enter)', async () => {
        const projectsStore = useProjectsStore();
        const tabsStore = useTabsStore();

        projectsStore.projects = [
            {
                id: 'proj-1',
                title: 'Projeto Alpha',
                icon: 'lucide:folder',
                color: '#00BCD4',
                path: '/workspace/alpha',
                rootId: null,
                stack: 'rust',
                identitySource: 'auto',
                sortOrder: 0,
                missing: false,
                createdAt: 1000,
                updatedAt: 1000
            }
        ];

        const openTplSpy = vi.spyOn(tabsStore, 'openTemplateTab').mockResolvedValue({} as never);

        const { openTarget } = useSearchNavigation();
        const res = await openTarget(
            {
                kind: 'template',
                projectId: 'proj-1',
                templateId: 'tpl-99'
            },
            'open-in-new-tab'
        );

        expect(res.ok).toBe(true);
        expect(res.mode).toBe('template-new-tab');
        expect(openTplSpy).toHaveBeenCalledWith('proj-1', 'tpl-99', null, true);
    });

    it('navega para output, session (viva e arquivada), project, template default, group, group_run, issue e trata target_missing', async () => {
        const projectsStore = useProjectsStore();
        const tabsStore = useTabsStore();
        const sessionsStore = useSessionsStore();
        const { useCommandGroupsStore } = await import('@/stores/commandGroups');
        const { useGithubIssuesStore } = await import('@/stores/githubIssues');
        const commandGroupsStore = useCommandGroupsStore();
        const githubIssuesStore = useGithubIssuesStore();

        projectsStore.projects = [
            {
                id: 'proj-1',
                title: 'Projeto Alpha',
                icon: 'lucide:folder',
                color: '#00BCD4',
                path: '/workspace/alpha',
                rootId: null,
                stack: 'rust',
                identitySource: 'auto',
                sortOrder: 0,
                missing: false,
                createdAt: 1000,
                updatedAt: 1000
            }
        ];

        tabsStore.byId = {
            'tab-live': {
                id: 'tab-live',
                projectId: 'proj-1',
                templateId: null,
                title: 'Terminal Vivo',
                icon: 'lucide:terminal',
                position: 0,
                origin: 'manual',
                continuous: false,
                lifecycle: 'ready',
                layout: { kind: 'leaf', paneId: 'pane-live' },
                primaryPaneId: 'pane-live',
                activePaneId: 'pane-live',
                panes: [
                    {
                        id: 'pane-live',
                        ptyId: 'pty-live',
                        generation: 1,
                        cwd: '/workspace/alpha',
                        state: 'live',
                        exit: null,
                        error: null
                    }
                ],
                tabSessionId: 'ts-live',
                initialDispatch: 'none',
                revision: 1,
                layoutRevision: 1
            }
        };

        vi.spyOn(tabsStore, 'focusTab').mockResolvedValue(tabsStore.byId['tab-live']!);
        vi.spyOn(sessionsStore, 'loadSessions').mockResolvedValue([
            {
                id: 'sess-1',
                tabId: 'tab-live',
                projectId: 'proj-1',
                startedAt: 100,
                endedAt: 200,
                status: 'archived',
                commandsCount: 1,
                scrollbackBytes: 64
            } as never
        ]);
        vi.spyOn(sessionsStore, 'loadSessionDetail').mockResolvedValue({} as never);
        vi.spyOn(sessionsStore, 'loadScrollback').mockResolvedValue({} as never);
        vi.spyOn(commandGroupsStore, 'get').mockResolvedValue({ id: 'grp-1', name: 'Build' } as never);
        vi.spyOn(commandGroupsStore, 'openEditModal').mockImplementation(() => {});
        vi.spyOn(commandGroupsStore, 'loadRun').mockResolvedValue({} as never);
        vi.spyOn(githubIssuesStore, 'selectIssue').mockResolvedValue(null);

        const { openTarget, closeReadonlyViewer, readonlyViewerState } = useSearchNavigation();

        const outRes = await openTarget({
            kind: 'output',
            projectId: 'proj-1',
            tabId: 'tab-live',
            sessionId: 'sess-1',
            paneId: 'pane-live',
            chunkSeq: 7
        });
        expect(outRes.mode).toBe('scrollback-chunk');
        expect(readonlyViewerState.value.chunkSeq).toBe(7);
        closeReadonlyViewer();
        expect(readonlyViewerState.value.open).toBe(false);

        const sessLive = await openTarget({
            kind: 'session',
            projectId: 'proj-1',
            tabId: 'tab-live',
            sessionId: 'sess-1'
        });
        expect(sessLive.mode).toBe('live-session');

        const sessArch = await openTarget({
            kind: 'session',
            projectId: 'proj-1',
            tabId: null,
            sessionId: 'sess-arch'
        });
        expect(sessArch.mode).toBe('archived-session');

        const projRes = await openTarget({
            kind: 'project',
            projectId: 'proj-1'
        });
        expect(projRes.mode).toBe('project');

        const tplDefault = await openTarget({
            kind: 'template',
            projectId: 'proj-1',
            templateId: 'tpl-1'
        });
        expect(tplDefault.mode).toBe('template-detail');

        const grpRes = await openTarget({
            kind: 'group',
            projectId: 'proj-1',
            groupId: 'grp-1'
        });
        expect(grpRes.mode).toBe('group');

        const runRes = await openTarget({
            kind: 'group_run',
            projectId: 'proj-1',
            runKey: 'run-1',
            itemKey: null
        });
        expect(runRes.mode).toBe('group-run');

        const commentEl = document.createElement('div');
        commentEl.setAttribute('data-comment-id', 'c-99');
        document.body.appendChild(commentEl);

        const issueRes = await openTarget({
            kind: 'issue',
            projectId: 'proj-1',
            repoKey: 'github.com/johnattas/maxterminal',
            number: 42,
            commentKey: 'c-99'
        });
        expect(issueRes.mode).toBe('issue');

        const missingRes = await openTarget({
            kind: 'project',
            projectId: 'proj-inexistente'
        });
        expect(missingRes.ok).toBe(false);
        expect(missingRes.reason).toBe('target_missing');
    });
});
