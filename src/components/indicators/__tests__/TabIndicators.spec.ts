import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import TabIndicators from '@/components/indicators/TabIndicators.vue';
import { useGitStore } from '@/stores/git';
import { usePortsStore } from '@/stores/ports';
import { useTabsStore } from '@/stores/tabs';
import type { TabView_Serialize } from '@/types/bindings';

vi.mock('@/ipc/git', () => ({
    gitStatusForPath: vi.fn(),
    gitStatusRefresh: vi.fn(),
    gitWorktreeList: vi.fn(),
    gitWorktreeCreate: vi.fn(),
    gitWorktreeRemove: vi.fn(),
    onGitStatusChanged: vi.fn(async () => () => {})
}));

vi.mock('@/ipc/ports', () => ({
    portsListForTab: vi.fn(),
    portsOpenBrowser: vi.fn(),
    portsKill: vi.fn(),
    portsSetPollMode: vi.fn(),
    onPortOpened: vi.fn(async () => () => {}),
    onPortUpdated: vi.fn(async () => () => {}),
    onPortClosed: vi.fn(async () => () => {})
}));

describe('TabIndicators.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('integra WorktreeBadge, GitIndicator e PortBadge para a aba', () => {
        const tabsStore = useTabsStore();
        const gitStore = useGitStore();
        const portsStore = usePortsStore();

        const tab: TabView_Serialize = {
            id: 'tab-1',
            projectId: 'proj-1',
            templateId: null,
            title: 'Dev',
            icon: 'lucide:terminal',
            position: 0,
            origin: 'manual',
            continuous: true,
            lifecycle: 'ready',
            revision: 1,
            layoutRevision: 1,
            layout: { kind: 'leaf', paneId: 'pane-1' },
            panes: [
                {
                    id: 'pane-1',
                    ptyId: 'pty-1',
                    generation: 1,
                    cwd: '/repo/.worktrees/wt-dev',
                    state: 'live',
                    exit: null,
                    error: null
                }
            ],
            activePaneId: 'pane-1',
            primaryPaneId: 'pane-1',
            initialDispatch: 'none',
            tabSessionId: 'tsess-1'
        };
        tabsStore.byId['tab-1'] = tab;

        gitStore.applyContext(
            {
                kind: 'repository',
                repoKey: '/repo/.worktrees/wt-dev',
                worktreeRoot: '/repo/.worktrees/wt-dev',
                gitDir: '/repo/.git/worktrees/wt-dev',
                commonDir: '/repo/.git',
                branch: { state: 'named', name: 'wt/wt-dev', oid: 'abcdef12' },
                changes: {
                    staged: 0,
                    unstaged: 0,
                    untracked: 0,
                    conflicted: 0,
                    dirty: false,
                    summaryTruncated: false
                },
                upstream: null,
                worktree: {
                    kind: 'max_terminal',
                    name: 'wt-dev',
                    path: '/repo/.worktrees/wt-dev',
                    mainWorktreePath: '/repo',
                    isLinked: true,
                    locked: false,
                    prunable: false
                },
                revision: 1,
                refreshedAtMs: 1000
            },
            ['pane-1']
        );

        portsStore.applyOpened({
            tabId: 'tab-1',
            paneId: 'pane-1',
            port: {
                id: 'pane-1:tcp:5177',
                tabId: 'tab-1',
                paneId: 'pane-1',
                ptyId: 'pty-1',
                port: 5177,
                protocol: 'tcp',
                addresses: [],
                browserUrl: 'http://127.0.0.1:5177/',
                pid: 4100,
                startTimeTicks: 900,
                processName: 'vite',
                cmdlinePreview: null,
                detectedAtMs: 1000
            },
            revision: 1
        });

        const wrapper = mount(TabIndicators, {
            props: { tab },
            global: { stubs: { MaxIcon: true } }
        });

        expect(wrapper.text()).toContain('WT: wt-dev');
        expect(wrapper.text()).toContain('wt/wt-dev');
        expect(wrapper.text()).toContain(':5177');
    });
});
