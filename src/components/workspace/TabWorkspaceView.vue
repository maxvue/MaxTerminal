<template>
    <div class="tab-workspace">
        <TabBar :tabs="projectTabs" :active-tab-id="pinnedIssuesActive ? null : (activeTab?.id ?? null)" :templates="projectTemplates" :can-split-active-pane="canSplitActive && !pinnedIssuesActive" @select-tab="onSelectTab" @close-tab="onCloseTab" @reorder-tabs="onReorderTabs" @add-default-tab="onAddDefaultTab" @launch-template="onLaunchTemplate" @create-template="onOpenCreateTemplate" @edit-template="onOpenEditTemplate" @delete-template="onDeleteTemplate" @set-default-template="onSetDefaultTemplate" @split-active-pane="onSplitActivePane" @new-session="onStartNewSession" @open-session-history="onToggleHistoryDrawer">
            <template #pinned>
                <button v-if="isProjectGithubRepo" type="button" role="tab" class="tab-workspace__pinned-issues" :class="{ 'tab-workspace__pinned-issues--active': pinnedIssuesActive }" :aria-selected="pinnedIssuesActive" aria-label="Aba fixa Issues do GitHub" @click="onSelectPinnedIssuesTab">
                    <MaxIcon icon="lucide:circle-dot" />
                    <span>Issues</span>
                    <span v-if="projectOpenIssuesCount !== null" class="tab-workspace__pinned-badge">{{ projectOpenIssuesCount }}</span>
                </button>
            </template>
            <template #tab-indicators="{ tab }">
                <slot name="tab-indicators" :tab="tab">
                    <TabIndicators :tab="tab" />
                </slot>
            </template>
            <template #bar-extra>
                <slot name="bar-extra">
                    <GroupRunIndicator :active-count="activeProjectGroupRuns.length" :total-runs="projectGroupRuns.length" :open="Boolean(commandGroupsStore.activeRunForPanel)" @toggle="onToggleGroupRunPanel" />
                    <AgentLauncherMenu :project-id="props.project.id" />
                </slot>
            </template>
        </TabBar>
        <div v-if="tabsStore.error || templatesStore.error || sessionsStore.error" class="tab-workspace__error" role="alert">
            <span>{{ tabsStore.error || templatesStore.error || sessionsStore.error }}</span>
            <MaxButton label="OK" variant="ghost" size="small" class="tab-workspace__error-dismiss" aria-label="Dispensar erro" @click="clearErrors" />
        </div>
        <div class="tab-workspace__stage">
            <div class="tab-workspace__main">
                <IssuesTab v-if="isProjectGithubRepo && pinnedIssuesActive" :project="props.project" />
                <template v-else-if="activeTab">
                    <PaneLayout :tab-id="activeTab.id" :root-layout="activeTab.layout" :node="activeTab.layout" :panes-by-id="activePanesMap" :active-pane-id="activeTab.activePaneId" :visible="true" @focus-pane="onFocusPane" @split-pane="onSplitPane" @close-pane="onClosePane" @restart-pane="onRestartPane" @resize-node="onResizeNode" @ready-for-initial-command="onReadyForInitialCommand" />
                    <AgentStatusLine :tab-id="activeTab.id" :compact="false" />
                </template>
                <div v-else class="tab-workspace__empty">
                    <p class="tab-workspace__empty-title">Nenhuma aba aberta em {{ props.project.title }}</p>
                    <p class="tab-workspace__empty-desc">Abra uma nova aba com o Template Padrão do projeto ou escolha um Template configurado.</p>
                    <div class="tab-workspace__empty-actions">
                        <MaxButton label="Adicionar nova aba" icon="lucide:plus" variant="primary" class="tab-workspace__open-btn" @click="onAddDefaultTab" />
                        <MaxButton label="Configurar Template" icon="lucide:settings-2" variant="secondary" class="tab-workspace__tpl-btn" @click="onOpenCreateTemplate" />
                    </div>
                </div>
                <GroupRunPanel v-if="commandGroupsStore.activeRunForPanel || commandGroupsStore.busyPaneFallback" :run="commandGroupsStore.activeRunForPanel" :busy-pane-fallback="commandGroupsStore.busyPaneFallback" @cancel-run="onCancelGroupRun" @cancel-item="onCancelGroupRunItem" @show-output="onShowGroupItemOutput" @focus-tab="onFocusGroupItemTab" @run-in-new-tab="onRunBusyGroupInNewTab" @restart-busy-pane="onRestartPane" @close="onCloseGroupRunPanel" />
                <GroupRunOutputView v-if="selectedOutputTarget" :run-id="selectedOutputTarget.runId" :item="selectedOutputTarget.item" @close="selectedOutputTarget = null" />
            </div>
            <SessionHistoryDrawer :open="historyDrawerOpen" :sessions="activeTabSessions" :search-hits="activeTabSearchHits" :selected-detail="selectedSessionDetail" :selected-chunks="selectedSessionChunks" :has-more-chunks="hasMoreScrollbackChunks" @close="historyDrawerOpen = false" @new-session="onStartNewSession" @search="onSearchSessions" @select-session="onInspectSession" @inspect-session="onInspectSessionWithPane" @select-pane-session="onSelectPaneSessionInViewer" @load-more-chunks="onLoadMoreScrollback" @close-viewer="selectedSessionIdForViewer = null" />
        </div>
        <TemplateEditorModal :open="editorModalOpen" :template="editingTemplate" :saving="savingTemplate" :error-message="templatesStore.error" @close="editorModalOpen = false" @save="onSaveTemplate" />
        <AgentTabOpenDialog :model-value="agentsStore.openDialogState.open" :project-id="agentsStore.openDialogState.projectId ?? props.project.id" :initial-agent="agentsStore.openDialogState.agent" :initial-isolate="agentsStore.openDialogState.forceIsolation" @update:model-value="onUpdateAgentOpenDialog" />
        <AgentWorktreeCloseDialog :model-value="Boolean(agentsStore.activeCloseDialogInspection)" :inspection="agentsStore.activeCloseDialogInspection" @update:model-value="onUpdateAgentCloseDialog" />
        <CommandGroupModal :model-value="commandGroupsStore.editorModalState.open" :group="commandGroupsStore.editorModalState.group" :scope="commandGroupsStore.editorModalState.scope" :project-id="commandGroupsStore.editorModalState.projectId ?? props.project.id" :initial-name="commandGroupsStore.editorModalState.initialName" @update:model-value="onUpdateCommandGroupModal" />
    </div>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import { MaxButton } from '@maxvue/max-components-ui';
    import AgentLauncherMenu from '@/components/agents/AgentLauncherMenu.vue';
    import AgentStatusLine from '@/components/agents/AgentStatusLine.vue';
    import AgentTabOpenDialog from '@/components/agents/AgentTabOpenDialog.vue';
    import AgentWorktreeCloseDialog from '@/components/agents/AgentWorktreeCloseDialog.vue';
    import CommandGroupModal from '@/components/commandGroups/CommandGroupModal.vue';
    import GroupRunIndicator from '@/components/commandGroups/GroupRunIndicator.vue';
    import GroupRunOutputView from '@/components/commandGroups/GroupRunOutputView.vue';
    import GroupRunPanel from '@/components/commandGroups/GroupRunPanel.vue';
    import IssuesTab from '@/components/github/IssuesTab.vue';
    import TabIndicators from '@/components/indicators/TabIndicators.vue';
    import { useCommandGroupRun } from '@/composables/commandGroups/useCommandGroupRun';
    import { useAgentShortcuts } from '@/composables/useAgentShortcuts';
    import { useTabIndicatorsSync } from '@/composables/useTabIndicatorsSync';
    import { canSplitPane } from '@/composables/workspace/layoutTree';
    import { useActiveTabSeen } from '@/composables/workspace/useActiveTabSeen';
    import { usePaneLayout } from '@/composables/workspace/usePaneLayout';
    import { useAgentsStore } from '@/stores/agents';
    import { useCommandGroupsStore } from '@/stores/commandGroups';
    import { useGithubIssuesStore } from '@/stores/githubIssues';
    import { useSessionsStore } from '@/stores/sessions';
    import { useTabsStore } from '@/stores/tabs';
    import { useTemplatesStore } from '@/stores/templates';
    import type {
        PaneId,
        PaneView_Serialize,
        Project_Serialize,
        ScrollbackChunk,
        SessionDetail_Serialize,
        SessionId,
        SessionSearchHit,
        SessionSummary_Serialize,
        SplitDirection,
        TabId,
        TabSessionId,
        TabTemplate_Serialize,
        TabTemplateInput_Deserialize,
        TabView_Serialize,
        TemplateId
    } from '@/types/bindings';
    import type { GroupRun, GroupRunId, GroupRunItem, GroupRunItemId } from '@/types/commandGroups';
    import PaneLayout from './PaneLayout.vue';
    import SessionHistoryDrawer from './SessionHistoryDrawer.vue';
    import TabBar from './TabBar.vue';
    import TemplateEditorModal from './TemplateEditorModal.vue';

    export interface TabWorkspaceViewProps {
        project: Project_Serialize;
    }

    const props = defineProps<TabWorkspaceViewProps>();

    const tabsStore = useTabsStore();
    const templatesStore = useTemplatesStore();
    const sessionsStore = useSessionsStore();
    const agentsStore = useAgentsStore();
    const commandGroupsStore = useCommandGroupsStore();
    const githubIssuesStore = useGithubIssuesStore();
    const groupRunner = useCommandGroupRun();
    const paneLayout = usePaneLayout();
    useActiveTabSeen();
    useTabIndicatorsSync();
    useAgentShortcuts();

    const isProjectGithubRepo = computed<boolean>(() =>
        githubIssuesStore.isGithubRepo(props.project.id)
    );
    const pinnedIssuesActive = computed<boolean>(() =>
        githubIssuesStore.isPinnedTabActive(props.project.id)
    );
    const projectOpenIssuesCount = computed<number | null>(() =>
        githubIssuesStore.openCountFor(props.project.id)
    );

    const historyDrawerOpen = ref<boolean>(false);
    const selectedSessionIdForViewer = ref<TabSessionId | null>(null);
    const activeViewerPaneSessionId = ref<SessionId | null>(null);
    const editorModalOpen = ref<boolean>(false);
    const editingTemplate = ref<TabTemplate_Serialize | null>(null);
    const savingTemplate = ref<boolean>(false);
    const selectedOutputTarget = ref<{ runId: GroupRunId; item: GroupRunItem } | null>(null);

    const projectGroupRuns = computed<GroupRun[]>(() =>
        commandGroupsStore.runsForProject(props.project.id)
    );

    const activeProjectGroupRuns = computed<GroupRun[]>(() =>
        commandGroupsStore.activeRunsForProject(props.project.id)
    );

    const projectTabs = computed<TabView_Serialize[]>(() =>
        tabsStore.tabsForProject(props.project.id)
    );

    const projectTemplates = computed<TabTemplate_Serialize[]>(() =>
        templatesStore.templatesFor(props.project.id)
    );

    const activeTab = computed<TabView_Serialize | null>(() => {
        const selected = tabsStore.activeTabForSelectedProject;
        if (selected && selected.projectId === props.project.id) {
            return selected;
        }
        return projectTabs.value[0] ?? null;
    });

    const activePanesMap = computed<Record<PaneId, PaneView_Serialize>>(() => {
        const map: Record<PaneId, PaneView_Serialize> = {};
        if (!activeTab.value) {
            return map;
        }
        for (const p of activeTab.value.panes) {
            map[p.id] = p;
        }
        return map;
    });

    const canSplitActive = computed<boolean>(() => {
        if (!activeTab.value) {
            return false;
        }
        return canSplitPane(activeTab.value.layout, activeTab.value.activePaneId);
    });

    const activeTabSessions = computed<SessionSummary_Serialize[]>(() => {
        if (!activeTab.value) {
            return [];
        }
        return sessionsStore.sessionsForTab(activeTab.value.id);
    });

    const activeTabSearchHits = computed<SessionSearchHit[]>(() => {
        if (!activeTab.value) {
            return [];
        }
        return sessionsStore.searchHitsByTab[activeTab.value.id] ?? [];
    });

    const selectedSessionDetail = computed<SessionDetail_Serialize | null>(() => {
        if (!selectedSessionIdForViewer.value) {
            return null;
        }
        return sessionsStore.detailFor(selectedSessionIdForViewer.value);
    });

    const selectedSessionChunks = computed<ScrollbackChunk[]>(() => {
        if (!activeViewerPaneSessionId.value) {
            return [];
        }
        return sessionsStore.scrollbackFor(activeViewerPaneSessionId.value);
    });

    const hasMoreScrollbackChunks = computed<boolean>(() => {
        if (!activeViewerPaneSessionId.value) {
            return false;
        }
        return Boolean(sessionsStore.scrollbackCursorByPaneSession[activeViewerPaneSessionId.value]);
    });

    watch(
        () => props.project.id,
        (projectId) => {
            historyDrawerOpen.value = false;
            selectedSessionIdForViewer.value = null;
            selectedOutputTarget.value = null;
            void templatesStore.loadForProject(projectId).catch(() => {
                // Ignora quando backend de teste não provê mock de template_list.
            });
            void tabsStore.loadForProject(projectId).catch(() => {
                // Ignora quando backend de teste não provê mock de tab_list.
            });
            void commandGroupsStore.initListeners().catch(() => {
                // Ignora quando backend de teste não provê listeners de grupos.
            });
            void commandGroupsStore.load(projectId).catch(() => {
                // Ignora quando backend de teste não provê mock de command_group_list.
            });
            void githubIssuesStore.resolveProjectRepo(projectId).catch(() => {
                // Ignora quando backend de teste não provê mock de github_repo_resolve.
            });
        },
        { immediate: true }
    );

    function onSelectPinnedIssuesTab(): void {
        githubIssuesStore.setPinnedTabActive(props.project.id, true);
    }

    function clearErrors(): void {
        tabsStore.error = null;
        templatesStore.error = null;
        sessionsStore.error = null;
    }

    function onUpdateAgentOpenDialog(open: boolean): void {
        if (!open) {
            agentsStore.closeAgentDialog();
        }
    }

    function onUpdateAgentCloseDialog(open: boolean): void {
        if (!open) {
            agentsStore.activeCloseDialogInspection = null;
        }
    }

    function onUpdateCommandGroupModal(open: boolean): void {
        if (!open) {
            commandGroupsStore.closeEditorModal();
        }
    }

    function onToggleGroupRunPanel(): void {
        if (commandGroupsStore.activeRunPanelId) {
            commandGroupsStore.activeRunPanelId = null;
            return;
        }
        const firstActive = activeProjectGroupRuns.value[0] ?? projectGroupRuns.value[0] ?? null;
        if (firstActive) {
            commandGroupsStore.activeRunPanelId = firstActive.id;
        }
    }

    function onCloseGroupRunPanel(): void {
        commandGroupsStore.activeRunPanelId = null;
        commandGroupsStore.busyPaneFallback = null;
    }

    async function onCancelGroupRun(runId: GroupRunId): Promise<void> {
        await groupRunner.cancelRun(runId, null);
    }

    async function onCancelGroupRunItem(payload: {
        runId: GroupRunId;
        itemId: GroupRunItemId;
    }): Promise<void> {
        await groupRunner.cancelRun(payload.runId, payload.itemId);
    }

    function onShowGroupItemOutput(payload: { runId: GroupRunId; item: GroupRunItem }): void {
        selectedOutputTarget.value = payload;
    }

    async function onFocusGroupItemTab(payload: {
        tabId: TabId;
        paneId: PaneId | null;
    }): Promise<void> {
        try {
            await tabsStore.focusTab(payload.tabId, payload.paneId);
        } catch {
            // Erro já tratado na store.
        }
    }

    async function onRunBusyGroupInNewTab(payload: {
        groupId: string;
        projectId: string;
    }): Promise<void> {
        commandGroupsStore.busyPaneFallback = null;
        const catalog = commandGroupsStore.groupsForProject(payload.projectId);
        const found =
            catalog.localGroups.find((g) => g.id === payload.groupId) ??
            catalog.globalGroups.find((g) => g.id === payload.groupId);
        if (!found) {
            return;
        }
        try {
            await groupRunner.runInNewTab(found, payload.projectId);
        } catch {
            // Erro já tratado no runner/store.
        }
    }

    async function onSelectTab(tabId: TabId): Promise<void> {
        githubIssuesStore.setPinnedTabActive(props.project.id, false);
        try {
            await tabsStore.focusTab(tabId, null);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onCloseTab(tabId: TabId): Promise<void> {
        try {
            await tabsStore.closeTab(tabId);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onReorderTabs(orderedIds: TabId[]): Promise<void> {
        try {
            await tabsStore.reorderTabs(props.project.id, orderedIds);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onAddDefaultTab(): Promise<void> {
        githubIssuesStore.setPinnedTabActive(props.project.id, false);
        try {
            await tabsStore.openDefaultTab(props.project.id);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onLaunchTemplate(templateId: TemplateId): Promise<void> {
        githubIssuesStore.setPinnedTabActive(props.project.id, false);
        try {
            await tabsStore.openTemplateTab(props.project.id, templateId);
        } catch {
            // Erro já registrado na store.
        }
    }

    function onOpenCreateTemplate(): void {
        editingTemplate.value = null;
        editorModalOpen.value = true;
    }

    function onOpenEditTemplate(tpl: TabTemplate_Serialize): void {
        editingTemplate.value = tpl;
        editorModalOpen.value = true;
    }

    async function onSaveTemplate(input: TabTemplateInput_Deserialize): Promise<void> {
        savingTemplate.value = true;
        try {
            if (editingTemplate.value) {
                await templatesStore.updateTemplate(
                    editingTemplate.value.id,
                    editingTemplate.value.revision,
                    input
                );
            } else {
                await templatesStore.createTemplate(props.project.id, input);
            }
            editorModalOpen.value = false;
        } catch {
            // Mantém modal aberto exibindo erro.
        } finally {
            savingTemplate.value = false;
        }
    }

    async function onDeleteTemplate(tpl: TabTemplate_Serialize): Promise<void> {
        try {
            await templatesStore.deleteTemplate(props.project.id, tpl.id, tpl.revision);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onSetDefaultTemplate(templateId: TemplateId): Promise<void> {
        try {
            await templatesStore.setDefaultTemplate(props.project.id, templateId);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onFocusPane(paneId: PaneId): Promise<void> {
        if (!activeTab.value) {
            return;
        }
        try {
            await tabsStore.focusTab(activeTab.value.id, paneId);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onSplitActivePane(direction: SplitDirection): Promise<void> {
        if (!activeTab.value) {
            return;
        }
        await onSplitPane(activeTab.value.activePaneId, direction);
    }

    async function onSplitPane(paneId: PaneId, direction: SplitDirection): Promise<void> {
        try {
            await tabsStore.splitPane(paneId, direction);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onClosePane(paneId: PaneId): Promise<void> {
        try {
            await tabsStore.closePane(paneId);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onRestartPane(paneId: PaneId): Promise<void> {
        try {
            await tabsStore.restartPane(paneId, true);
        } catch {
            // Erro já registrado na store.
        }
    }

    function onResizeNode(nodeId: string, sizes: [number, number]): void {
        if (!activeTab.value) {
            return;
        }
        paneLayout.scheduleNodeResize(activeTab.value.id, nodeId, sizes);
    }

    function onReadyForInitialCommand(tabId: TabId, paneId: PaneId, generation: number): void {
        void tabsStore.dispatchInitialCommandIfPending(tabId, paneId, generation);
    }

    async function onStartNewSession(): Promise<void> {
        if (!activeTab.value) {
            return;
        }
        try {
            await sessionsStore.startNewSession(activeTab.value.id);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onToggleHistoryDrawer(): Promise<void> {
        historyDrawerOpen.value = !historyDrawerOpen.value;
        if (historyDrawerOpen.value && activeTab.value) {
            try {
                await sessionsStore.loadSessions(activeTab.value.id, true);
            } catch {
                // Erro já registrado na store.
            }
        }
    }

    async function onSearchSessions(query: string): Promise<void> {
        if (!activeTab.value) {
            return;
        }
        try {
            await sessionsStore.searchInTab(activeTab.value.id, query);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onInspectSession(tabSessionId: TabSessionId): Promise<void> {
        selectedSessionIdForViewer.value = tabSessionId;
        try {
            const detail = await sessionsStore.loadSessionDetail(tabSessionId);
            const firstPaneSession = detail.panes[0]?.sessionId ?? null;
            activeViewerPaneSessionId.value = firstPaneSession;
            if (firstPaneSession) {
                await sessionsStore.loadScrollback(tabSessionId, firstPaneSession, true);
            }
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onInspectSessionWithPane(
        tabSessionId: TabSessionId,
        sessionId: SessionId
    ): Promise<void> {
        selectedSessionIdForViewer.value = tabSessionId;
        activeViewerPaneSessionId.value = sessionId;
        try {
            await sessionsStore.loadSessionDetail(tabSessionId);
            await sessionsStore.loadScrollback(tabSessionId, sessionId, true);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onSelectPaneSessionInViewer(sessionId: SessionId): Promise<void> {
        if (!selectedSessionIdForViewer.value) {
            return;
        }
        activeViewerPaneSessionId.value = sessionId;
        try {
            await sessionsStore.loadScrollback(selectedSessionIdForViewer.value, sessionId, true);
        } catch {
            // Erro já registrado na store.
        }
    }

    async function onLoadMoreScrollback(sessionId: SessionId): Promise<void> {
        if (!selectedSessionIdForViewer.value) {
            return;
        }
        try {
            await sessionsStore.loadScrollback(selectedSessionIdForViewer.value, sessionId, false);
        } catch {
            // Erro já registrado na store.
        }
    }
</script>

<style lang="scss" scoped>
    .tab-workspace {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background: var(--background-900);
    }

    .tab-workspace__error {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 0.375rem 0.875rem;
        background: rgb(239 68 68 / 15%);
        border-bottom: 1px solid var(--max-danger-500);
        color: var(--max-danger-300);
        font-size: 0.7813rem;
    }

    .tab-workspace__error-dismiss {
        padding: 0.125rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--max-danger-500);
        background: transparent;
        color: var(--max-danger-300);
        font-size: 0.7188rem;
        font-weight: 600;
        cursor: pointer;
    }

    .tab-workspace__stage {
        flex: 1;
        min-width: 0;
        min-height: 0;
        display: flex;
        overflow: hidden;
    }

    .tab-workspace__main {
        flex: 1;
        min-width: 0;
        min-height: 0;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .tab-workspace__empty {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 0.625rem;
        padding: 2rem;
        text-align: center;
    }

    .tab-workspace__empty-title {
        margin: 0;
        font-size: 1rem;
        font-weight: 600;
        color: var(--background-100);
    }

    .tab-workspace__empty-desc {
        margin: 0;
        font-size: 0.8125rem;
        color: var(--background-400);
        max-width: 28rem;
    }

    .tab-workspace__empty-actions {
        display: flex;
        gap: 0.5rem;
        margin-top: 0.375rem;
    }

    .tab-workspace__open-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.4375rem 0.875rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-primary-500);
        background: rgb(20 184 166 / 18%);
        color: var(--max-primary-400);
        font-size: 0.8125rem;
        font-weight: 600;
        cursor: pointer;
    }

    .tab-workspace__tpl-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.4375rem 0.875rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-200);
        font-size: 0.8125rem;
        font-weight: 600;
        cursor: pointer;
    }

    .tab-workspace__pinned-issues {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        height: 2rem;
        padding: 0 0.75rem;
        border-radius: 0.375rem 0.375rem 0 0;
        border: 1px solid var(--background-600);
        border-bottom: none;
        background: var(--background-850, var(--background-800));
        color: var(--background-200);
        font-size: 0.75rem;
        font-weight: 700;
        cursor: pointer;
        white-space: nowrap;
        flex-shrink: 0;

        &:hover {
            background: var(--background-750, var(--background-700));
            color: var(--background-50);
        }

        &--active {
            background: var(--background-900);
            border-color: var(--max-purple-500, #a855f7);
            color: var(--max-purple-400, #c084fc);
        }
    }

    .tab-workspace__pinned-badge {
        padding: 0.0625rem 0.375rem;
        border-radius: 9999px;
        background: rgb(168 85 247 / 22%);
        color: var(--max-purple-300, #d8b4fe);
        font-size: 0.6875rem;
        font-weight: 700;
    }
</style>
