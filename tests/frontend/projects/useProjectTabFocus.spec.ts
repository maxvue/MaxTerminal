import { createPinia, setActivePinia } from 'pinia';
import { effectScope, nextTick, ref } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useProjectTabFocus } from '@/composables/useProjectTabFocus';
import * as tabStateIpc from '@/ipc/tabState';
import { useProjectsStore } from '@/stores/projects';
import { useTabStatusStore } from '@/stores/tabStatus';
import type {
    PaneId,
    Project_Serialize,
    SeenRequest_Deserialize,
    TabStateSnapshot_Serialize
} from '@/types/bindings';

vi.mock('@/ipc/tabState', () => ({
    tabStateList: vi.fn().mockResolvedValue([]),
    tabStateGet: vi.fn(),
    tabMarkSeen: vi.fn(),
    tabSetFocus: vi.fn().mockResolvedValue([]),
    tabSetContinuous: vi.fn(),
    onTabStateChanged: vi.fn().mockResolvedValue(() => {}),
    onTabStateRemoved: vi.fn().mockResolvedValue(() => {})
}));

function makeProject(id: string, sortOrder: number): Project_Serialize {
    return {
        id,
        title: `Proj ${id}`,
        icon: 'logos:vue',
        color: '#42B883',
        path: `/home/user/${id}`,
        rootId: null,
        stack: 'vue',
        identitySource: 'auto',
        sortOrder,
        missing: false,
        createdAt: 1000,
        updatedAt: 1000
    };
}

function makeUnseenTab(tabId: string, revision: number, blockId: string): TabStateSnapshot_Serialize {
    return {
        tabId,
        status: 'error_unseen',
        panes: [
            {
                paneId: 'pane-1',
                ptyId: 'pty-1',
                sessionId: 'sess-1',
                status: 'error_unseen',
                integrationMode: 'osc133',
                continuousPolicy: 'auto',
                cwd: '/home/user',
                activeExecution: null,
                lastCompletion: {
                    blockId,
                    ptyId: 'pty-1',
                    paneId: 'pane-1',
                    outcome: 'error',
                    exitCode: 1,
                    signal: null,
                    endReason: 'exit_non_zero',
                    endedAt: 2000,
                    durationMs: 100,
                    seen: false
                },
                updatedAt: 2000
            },
            {
                paneId: 'pane-hidden',
                ptyId: 'pty-2',
                sessionId: 'sess-2',
                status: 'error_unseen',
                integrationMode: 'osc133',
                continuousPolicy: 'auto',
                cwd: '/home/user',
                activeExecution: null,
                lastCompletion: {
                    blockId: `${blockId}-hidden`,
                    ptyId: 'pty-2',
                    paneId: 'pane-hidden',
                    outcome: 'error',
                    exitCode: 2,
                    signal: null,
                    endReason: 'exit_non_zero',
                    endedAt: 2000,
                    durationMs: 100,
                    seen: false
                },
                updatedAt: 2000
            }
        ],
        lastCompletion: {
            blockId,
            ptyId: 'pty-1',
            paneId: 'pane-1',
            outcome: 'error',
            exitCode: 1,
            signal: null,
            endReason: 'exit_non_zero',
            endedAt: 2000,
            durationMs: 100,
            seen: false
        },
        attention: null,
        focused: false,
        revision,
        updatedAt: 2000
    };
}

describe('useProjectTabFocus (Fase 06)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        vi.mocked(tabStateIpc.tabSetFocus).mockResolvedValue([]);
    });

    it('foco sozinho não marca seen; somente apresentação de conclusão em painel visível e janela focada envia ACK exato', async () => {
        const projectsStore = useProjectsStore();
        const tabStatusStore = useTabStatusStore();

        projectsStore.applySnapshot({
            epoch: 'e1',
            revision: 1,
            projects: [makeProject('p1', 0), makeProject('p2', 1)],
            roots: [],
            discovered: []
        });

        projectsStore.attachTab('tab-1', 'p1');
        projectsStore.setActiveTab('p1', 'tab-1');
        tabStatusStore.applySnapshot(makeUnseenTab('tab-1', 4, 'block-1'));

        const visiblePaneIds = ref<readonly PaneId[]>(['pane-1']);
        const presentedConclusions = ref<readonly SeenRequest_Deserialize[]>([]);

        const scope = effectScope();
        const focusCtrl = scope.run(() =>
            useProjectTabFocus({
                visiblePaneIds,
                presentedConclusions
            })
        )!;

        // Seleciona projeto p1 -> atualiza foco, mas não marca seen sem presentedConclusions
        projectsStore.select('p1');
        await nextTick();
        expect(focusCtrl.activeTabId.value).toBe('tab-1');
        expect(tabStateIpc.tabSetFocus).toHaveBeenCalledWith({
            windowFocused: true,
            activeTabId: 'tab-1',
            activePaneId: 'pane-1'
        });
        expect(tabStateIpc.tabMarkSeen).not.toHaveBeenCalled();

        // Apresentação de painel oculto ('pane-hidden') NÃO envia ACK
        presentedConclusions.value = [
            { tabId: 'tab-1', paneId: 'pane-hidden', upToRevision: 4 }
        ];
        await nextTick();
        expect(tabStateIpc.tabMarkSeen).not.toHaveBeenCalled();

        // Janela oculta ou sem foco NÃO envia ACK
        focusCtrl.setWindowFocusedForTests(false, true);
        presentedConclusions.value = [
            { tabId: 'tab-1', paneId: 'pane-1', upToRevision: 4 }
        ];
        await nextTick();
        expect(tabStateIpc.tabMarkSeen).not.toHaveBeenCalled();

        // Quando a janela recupera foco e visibilidade, envia o ACK exato para pane-1 rev 4
        vi.mocked(tabStateIpc.tabMarkSeen).mockImplementation(async (req) => ({
            ...makeUnseenTab(req.tabId, 5, 'block-1'),
            status: 'idle',
            lastCompletion: {
                ...makeUnseenTab(req.tabId, 5, 'block-1').lastCompletion!,
                seen: true
            }
        }));

        focusCtrl.setWindowFocusedForTests(true, true);
        await nextTick();

        expect(tabStateIpc.tabMarkSeen).toHaveBeenCalledTimes(1);
        expect(tabStateIpc.tabMarkSeen).toHaveBeenCalledWith({
            tabId: 'tab-1',
            paneId: 'pane-1',
            upToRevision: 4
        });

        // Nova conclusão na mesma aba continuamente focada exige novo ACK com a nova revisão
        tabStatusStore.applySnapshot(makeUnseenTab('tab-1', 7, 'block-2'));
        await nextTick();
        expect(tabStateIpc.tabMarkSeen).toHaveBeenCalledTimes(1);

        presentedConclusions.value = [
            { tabId: 'tab-1', paneId: 'pane-1', upToRevision: 7 }
        ];
        await nextTick();
        expect(tabStateIpc.tabMarkSeen).toHaveBeenCalledTimes(2);
        expect(tabStateIpc.tabMarkSeen).toHaveBeenLastCalledWith({
            tabId: 'tab-1',
            paneId: 'pane-1',
            upToRevision: 7
        });

        scope.stop();
    });

    it('erro IPC de ACK anterior não altera seen de outra geração e troca/remoção cancela imediatamente', async () => {
        const projectsStore = useProjectsStore();
        const tabStatusStore = useTabStatusStore();

        projectsStore.applySnapshot({
            epoch: 'e1',
            revision: 1,
            projects: [makeProject('p1', 0), makeProject('p2', 1)],
            roots: [],
            discovered: []
        });

        projectsStore.attachTab('tab-1', 'p1');
        projectsStore.setActiveTab('p1', 'tab-1');
        projectsStore.select('p1');
        tabStatusStore.applySnapshot(makeUnseenTab('tab-1', 3, 'block-err'));

        const visiblePaneIds = ref<readonly PaneId[]>(['pane-1']);
        const presentedConclusions = ref<readonly SeenRequest_Deserialize[]>([]);

        const scope = effectScope();
        scope.run(() =>
            useProjectTabFocus({
                visiblePaneIds,
                presentedConclusions
            })
        );

        let rejectMarkSeen!: (err: Error) => void;
        vi.mocked(tabStateIpc.tabMarkSeen).mockImplementationOnce(
            () =>
                new Promise((_, reject) => {
                    rejectMarkSeen = reject;
                })
        );

        presentedConclusions.value = [
            { tabId: 'tab-1', paneId: 'pane-1', upToRevision: 3 }
        ];
        await nextTick();
        expect(tabStateIpc.tabMarkSeen).toHaveBeenCalledTimes(1);

        // Troca de projeto antes do retorno da chamada IPC
        projectsStore.select('p2');
        await nextTick();
        expect(tabStateIpc.tabSetFocus).toHaveBeenLastCalledWith({
            windowFocused: true,
            activeTabId: null,
            activePaneId: null
        });

        // Falha IPC tardia da geração anterior não deve corromper estado nem disparar novo ACK
        rejectMarkSeen(new Error('IPC error'));
        await nextTick();
        expect(tabStatusStore.snapshotOf('tab-1')?.status).toBe('error_unseen');

        scope.stop();
    });
});
