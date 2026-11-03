import { ref } from 'vue';
import { getBlockMarkers, useCommandBlocksStore } from '@/stores/commandBlocks';
import { useCommandGroupsStore } from '@/stores/commandGroups';
import { useGithubIssuesStore } from '@/stores/githubIssues';
import { useProjectsStore } from '@/stores/projects';
import { useSearchStore } from '@/stores/search';
import { useSessionsStore } from '@/stores/sessions';
import { useTabsStore } from '@/stores/tabs';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import type {
    CommandBlockId,
    PaneId,
    ProjectId,
    SearchTarget_Serialize,
    SessionId,
    TabId
} from '@/types/bindings';

export type SearchNavigationAction = 'default' | 'open-in-new-tab';

export interface SearchNavigationResult {
    ok: boolean;
    mode?:
        | 'live-block'
        | 'archived-session'
        | 'scrollback-chunk'
        | 'live-session'
        | 'project'
        | 'template-detail'
        | 'template-new-tab'
        | 'group'
        | 'group-run'
        | 'issue';
    reason?: 'target_missing' | 'navigation_failed';
    message?: string;
}

export interface ReadonlySessionNavigationState {
    open: boolean;
    projectId: ProjectId | null;
    tabId: TabId | null;
    sessionId: SessionId | null;
    paneId: PaneId | null;
    blockId: CommandBlockId | null;
    chunkSeq: number | null;
    unlinkedHistory: boolean;
}

export const FLASH_HIGHLIGHT_DURATION_MS = 1500;
export const FLASH_HIGHLIGHT_CLASS = 'max-search-highlight-flash';

const readonlyViewerState = ref<ReadonlySessionNavigationState>({
    open: false,
    projectId: null,
    tabId: null,
    sessionId: null,
    paneId: null,
    blockId: null,
    chunkSeq: null,
    unlinkedHistory: false
});

const activeFlashTimers = new Map<string, ReturnType<typeof setTimeout>>();

function prefersReducedMotion(): boolean {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') {
        return false;
    }
    return Boolean(window.matchMedia('(prefers-reduced-motion: reduce)').matches);
}

function showTargetMissingToast(onRefresh?: () => void): void {
    onRefresh?.();
    void import('@maxvue/max-components-ui')
        .then((mod) => {
            mod.Toast?.show?.({
                title: 'Este resultado não está mais disponível',
                severity: 'warning'
            });
        })
        .catch(() => {
            // Ambiente de teste sem Toast montado.
        });
}

/** Aplica a classe `.max-search-highlight-flash` por `1500 ms` no elemento alvo. */
export function flashElementTemporary(selectorOrElement: string | HTMLElement | null): boolean {
    if (typeof document === 'undefined' || !selectorOrElement) {
        return false;
    }
    const el =
        typeof selectorOrElement === 'string'
            ? document.querySelector<HTMLElement>(selectorOrElement)
            : selectorOrElement;
    if (!el) {
        return false;
    }

    const key =
        typeof selectorOrElement === 'string'
            ? selectorOrElement
            : (el.id || el.dataset.blockId || 'el');
    const prevTimer = activeFlashTimers.get(key);
    if (prevTimer) {
        clearTimeout(prevTimer);
    }

    el.classList.add(FLASH_HIGHLIGHT_CLASS);
    if (prefersReducedMotion()) {
        el.setAttribute('data-reduced-motion', 'true');
    }

    const timer = setTimeout(() => {
        el.classList.remove(FLASH_HIGHLIGHT_CLASS);
        activeFlashTimers.delete(key);
    }, FLASH_HIGHLIGHT_DURATION_MS);
    activeFlashTimers.set(key, timer);
    return true;
}

/** Rola o buffer xterm e/ou DOM até o bloco `blockId` no painel `paneId`. */
export function scrollToCommandBlockInPane(paneId: PaneId, blockId: CommandBlockId): boolean {
    const blocksStore = useCommandBlocksStore();
    const block = blocksStore.byPane[paneId]?.blocks[blockId];
    if (!block) {
        return false;
    }

    const instance = terminalInstanceRegistry.get(paneId);
    const markerRec = getBlockMarkers(paneId, blockId);
    const targetLine =
        markerRec?.promptMarker && !markerRec.promptMarker.isDisposed && markerRec.promptMarker.line >= 0
            ? markerRec.promptMarker.line
            : block.promptLine;

    if (instance?.xterm && typeof instance.xterm.scrollToLine === 'function' && targetLine >= 0) {
        instance.xterm.scrollToLine(targetLine);
    }

    if (typeof document !== 'undefined') {
        const domEl = document.querySelector<HTMLElement>(
            `[data-block-id="${blockId}"], #command-block-${blockId}`
        );
        if (domEl) {
            if (typeof domEl.scrollIntoView === 'function') {
                domEl.scrollIntoView({
                    behavior: prefersReducedMotion() ? 'auto' : 'smooth',
                    block: 'center'
                });
            }
            flashElementTemporary(domEl);
        }
    }

    return true;
}

/**
 * Orquestra a navegação contextual ao acionar um resultado da Busca Global (`CA-12`, `CA-13`).
 */
export function useSearchNavigation() {
    const projectsStore = useProjectsStore();
    const tabsStore = useTabsStore();
    const sessionsStore = useSessionsStore();
    const commandBlocksStore = useCommandBlocksStore();
    const commandGroupsStore = useCommandGroupsStore();
    const githubIssuesStore = useGithubIssuesStore();
    const searchStore = useSearchStore();

    function ensureProjectSelected(projectId: ProjectId | null | undefined): boolean {
        if (!projectId) {
            return true;
        }
        const exists = projectsStore.projectById(projectId);
        if (!exists) {
            return false;
        }
        projectsStore.select(projectId);
        return true;
    }

    function closeReadonlyViewer(): void {
        readonlyViewerState.value = {
            ...readonlyViewerState.value,
            open: false
        };
    }

    async function openTarget(
        target: SearchTarget_Serialize,
        action: SearchNavigationAction = 'default'
    ): Promise<SearchNavigationResult> {
        try {
            switch (target.kind) {
                case 'command': {
                    if (target.projectId && !ensureProjectSelected(target.projectId)) {
                        showTargetMissingToast(() => searchStore.invalidate('target_missing'));
                        return {
                            ok: false,
                            reason: 'target_missing',
                            message: 'Este resultado não está mais disponível'
                        };
                    }

                    const liveTab = target.tabId ? tabsStore.byId[target.tabId] : undefined;
                    const paneId = target.paneId ?? liveTab?.primaryPaneId ?? null;
                    const hasLiveBlock = Boolean(
                        paneId && commandBlocksStore.byPane[paneId]?.blocks[target.blockId]
                    );

                    if (liveTab && paneId && hasLiveBlock) {
                        await tabsStore.focusTab(liveTab.id, paneId);
                        scrollToCommandBlockInPane(paneId, target.blockId);
                        return {
                            ok: true,
                            mode: 'live-block'
                        };
                    }

                    // Sessão arquivada ou bloco fora do buffer vivo -> abre SessionReadonlyViewer sem iniciar PTY
                    let unlinkedHistory = !target.tabId;
                    if (target.tabId) {
                        try {
                            const summaries = await sessionsStore.loadSessions(target.tabId, true);
                            const matchedSummary = summaries.find((s) => s.id === target.sessionId);
                            if (matchedSummary) {
                                await sessionsStore.loadSessionDetail(matchedSummary.id);
                                await sessionsStore.loadScrollback(
                                    matchedSummary.id,
                                    target.sessionId,
                                    true
                                );
                            } else {
                                unlinkedHistory = true;
                            }
                        } catch {
                            unlinkedHistory = true;
                        }
                    }

                    readonlyViewerState.value = {
                        open: true,
                        projectId: target.projectId ?? null,
                        tabId: target.tabId ?? null,
                        sessionId: target.sessionId,
                        paneId: target.paneId ?? null,
                        blockId: target.blockId,
                        chunkSeq: null,
                        unlinkedHistory
                    };
                    return {
                        ok: true,
                        mode: 'archived-session'
                    };
                }

                case 'output': {
                    if (target.projectId && !ensureProjectSelected(target.projectId)) {
                        showTargetMissingToast(() => searchStore.invalidate('target_missing'));
                        return {
                            ok: false,
                            reason: 'target_missing',
                            message: 'Este resultado não está mais disponível'
                        };
                    }

                    let unlinkedHistory = !target.tabId;
                    if (target.tabId) {
                        try {
                            const summaries = await sessionsStore.loadSessions(target.tabId, true);
                            const first = summaries[0];
                            if (first) {
                                await sessionsStore.loadSessionDetail(first.id);
                                await sessionsStore.loadScrollback(
                                    first.id,
                                    target.sessionId,
                                    true
                                );
                            } else {
                                unlinkedHistory = true;
                            }
                        } catch {
                            unlinkedHistory = true;
                        }
                    }

                    readonlyViewerState.value = {
                        open: true,
                        projectId: target.projectId ?? null,
                        tabId: target.tabId ?? null,
                        sessionId: target.sessionId,
                        paneId: target.paneId ?? null,
                        blockId: null,
                        chunkSeq: target.chunkSeq,
                        unlinkedHistory
                    };
                    if (typeof document !== 'undefined') {
                        flashElementTemporary(`[data-chunk-seq="${target.chunkSeq}"]`);
                    }
                    return {
                        ok: true,
                        mode: 'scrollback-chunk'
                    };
                }

                case 'session': {
                    if (target.projectId && !ensureProjectSelected(target.projectId)) {
                        showTargetMissingToast(() => searchStore.invalidate('target_missing'));
                        return {
                            ok: false,
                            reason: 'target_missing',
                            message: 'Este resultado não está mais disponível'
                        };
                    }

                    const liveTab = target.tabId ? tabsStore.byId[target.tabId] : undefined;
                    if (liveTab) {
                        await tabsStore.focusTab(liveTab.id, liveTab.primaryPaneId);
                        return {
                            ok: true,
                            mode: 'live-session'
                        };
                    }

                    readonlyViewerState.value = {
                        open: true,
                        projectId: target.projectId ?? null,
                        tabId: target.tabId ?? null,
                        sessionId: target.sessionId,
                        paneId: null,
                        blockId: null,
                        chunkSeq: null,
                        unlinkedHistory: !target.tabId
                    };
                    return {
                        ok: true,
                        mode: 'archived-session'
                    };
                }

                case 'project': {
                    if (!ensureProjectSelected(target.projectId)) {
                        showTargetMissingToast(() => searchStore.invalidate('target_missing'));
                        return {
                            ok: false,
                            reason: 'target_missing',
                            message: 'Este resultado não está mais disponível'
                        };
                    }
                    githubIssuesStore.setPinnedTabActive(target.projectId, false);
                    return {
                        ok: true,
                        mode: 'project'
                    };
                }

                case 'template': {
                    if (!ensureProjectSelected(target.projectId)) {
                        showTargetMissingToast(() => searchStore.invalidate('target_missing'));
                        return {
                            ok: false,
                            reason: 'target_missing',
                            message: 'Este resultado não está mais disponível'
                        };
                    }

                    if (action === 'open-in-new-tab') {
                        await tabsStore.openTemplateTab(
                            target.projectId,
                            target.templateId,
                            null,
                            true
                        );
                        return {
                            ok: true,
                            mode: 'template-new-tab'
                        };
                    }

                    return {
                        ok: true,
                        mode: 'template-detail'
                    };
                }

                case 'group': {
                    if (target.projectId && !ensureProjectSelected(target.projectId)) {
                        showTargetMissingToast(() => searchStore.invalidate('target_missing'));
                        return {
                            ok: false,
                            reason: 'target_missing',
                            message: 'Este resultado não está mais disponível'
                        };
                    }

                    const grp = await commandGroupsStore.get(target.groupId);
                    commandGroupsStore.openEditModal(grp);
                    return {
                        ok: true,
                        mode: 'group'
                    };
                }

                case 'group_run': {
                    if (target.projectId && !ensureProjectSelected(target.projectId)) {
                        showTargetMissingToast(() => searchStore.invalidate('target_missing'));
                        return {
                            ok: false,
                            reason: 'target_missing',
                            message: 'Este resultado não está mais disponível'
                        };
                    }

                    await commandGroupsStore.loadRun(target.runKey);
                    commandGroupsStore.activeRunPanelId = target.runKey;
                    return {
                        ok: true,
                        mode: 'group-run'
                    };
                }

                case 'issue': {
                    if (!ensureProjectSelected(target.projectId)) {
                        showTargetMissingToast(() => searchStore.invalidate('target_missing'));
                        return {
                            ok: false,
                            reason: 'target_missing',
                            message: 'Este resultado não está mais disponível'
                        };
                    }

                    githubIssuesStore.setPinnedTabActive(target.projectId, true);
                    await githubIssuesStore.selectIssue(target.projectId, target.number);
                    if (target.commentKey && typeof document !== 'undefined') {
                        flashElementTemporary(`[data-comment-id="${target.commentKey}"]`);
                    }
                    return {
                        ok: true,
                        mode: 'issue'
                    };
                }
            }
        } catch {
            showTargetMissingToast(() => searchStore.invalidate('target_missing'));
            return {
                ok: false,
                reason: 'target_missing',
                message: 'Este resultado não está mais disponível'
            };
        }
    }

    return {
        readonlyViewerState,
        closeReadonlyViewer,
        openTarget
    };
}
