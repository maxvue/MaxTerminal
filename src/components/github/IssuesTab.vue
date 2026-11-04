<template>
    <div class="issues-tab" role="region" aria-label="Aba Fixa de Issues do GitHub">
        <header class="issues-tab__toolbar">
            <div class="issues-tab__repo-info">
                <MaxIcon icon="lucide:github" />
                <span class="issues-tab__repo-slug">{{ repoSlug }}</span>
                <span v-if="openCount !== null" class="issues-tab__open-badge">{{ openCount }} abertas</span>
            </div>
            <div class="issues-tab__actions">
                <MaxButton label="Nova Issue" icon="lucide:plus" class="issues-tab__btn issues-tab__btn--primary" @click="createModalOpen = true" />
                <MaxButton label="Corrigir todos os Bugs" icon="lucide:sparkles" severity="secondary" class="issues-tab__btn issues-tab__btn--cycle" @click="onOpenBugCycleModal" />
                <MaxButton label="Autenticação" icon="lucide:key-round" severity="secondary" class="issues-tab__btn" title="Autenticação GitHub (`gh` CLI e REST)" @click="onOpenAuthPanel" />
                <MaxButton label="Atualizar" icon="lucide:refresh-cw" severity="secondary" class="issues-tab__btn" :disabled="isLoadingList" :loading="isLoadingList" title="Sincronizar com o GitHub agora" @click="onRefresh" />
            </div>
        </header>
        <LabelSyncBanner :missing-count="missingLabelsCount" @sync="onSyncCanonicalLabels" />
        <BugCycleProgress :cycle="activeCycle" @control="onControlCycle" />
        <div v-if="projectError" class="issues-tab__error" role="alert">
            <span>{{ projectError }}</span>
            <MaxButton label="OK" variant="text" severity="secondary" size="small" class="issues-tab__error-dismiss" @click="clearProjectError" />
        </div>
        <IssueFilters :filter="filter" :has-active-filters="hasActiveFilters" @set-state="setState" @toggle-state-label="toggleStateLabel" @toggle-category-label="toggleCategoryLabel" @set-search="setSearchText" @set-has-plan="setHasPlan" @set-sort="setSort" @reset="resetFilters" />
        <div class="issues-tab__split">
            <div class="issues-tab__left">
                <IssueList :issues="issues" :selected-number="selectedIssueNumber" :loading="isLoadingList" :stale="isStale" :total-matching="totalMatching" @select="onSelectIssue" />
            </div>
            <div class="issues-tab__right">
                <IssueDetail :project-id="props.project.id" :detail="selectedDetail" :mutating="isSelectedIssueMutating" @update-labels="onUpdateLabels" @send-maxcode="onSendMaxCode" @open-tab-modal="onOpenTabModal" />
            </div>
        </div>
        <IssueCreateModal :open="createModalOpen" :project-id="props.project.id" :submitting="creatingIssue" @close="createModalOpen = false" @create="onCreateIssue" />
        <IssueOpenTabModal :open="openTabModalOpen" :project-id="props.project.id" :issue="selectedSummary" :resolution="worktreeResolution" :submitting="openingTab" @close="openTabModalOpen = false" @confirm="onConfirmOpenTab" />
        <BugCycleConfirmModal :open="bugCycleModalOpen" :project-id="props.project.id" :preview="cyclePreview" :submitting="startingCycle" @close="bugCycleModalOpen = false" @start="onStartBugCycle" />
        <GithubAuthPanel :open="authPanelOpen" :status="githubStore.authStatus" @close="authPanelOpen = false" @save-token="onSaveRestToken" @clear-token="onClearRestToken" />
    </div>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import { MaxButton, MaxIcon } from '@maxvue/max-components-ui';
    import { useIssueFilters } from '@/composables/useIssueFilters';
    import { useGithubIssuesStore } from '@/stores/githubIssues';
    import type {
        BugCyclePreview,
        BugCycleRunState,
        CreateIssueInput,
        IssueDetail as IssueDetailDto,
        IssueSummary,
        IssueWorktreeResolution,
        LabelUpdatePatch,
        MaxCodeIssueAction,
        OpenIssueTabRequest,
        Project_Serialize,
        StartBugCycleRequest
    } from '@/types/bindings';
    import BugCycleConfirmModal from './BugCycleConfirmModal.vue';
    import BugCycleProgress from './BugCycleProgress.vue';
    import GithubAuthPanel from './GithubAuthPanel.vue';
    import IssueCreateModal from './IssueCreateModal.vue';
    import IssueDetail from './IssueDetail.vue';
    import IssueFilters from './IssueFilters.vue';
    import IssueList from './IssueList.vue';
    import IssueOpenTabModal from './IssueOpenTabModal.vue';
    import LabelSyncBanner from './LabelSyncBanner.vue';

    export interface IssuesTabProps {
        project: Project_Serialize;
    }

    const props = defineProps<IssuesTabProps>();

    const githubStore = useGithubIssuesStore();
    const projectIdRef = computed<string>(() => props.project.id);

    const {
        filter,
        hasActiveFilters,
        setState,
        toggleStateLabel,
        toggleCategoryLabel,
        setSearchText,
        setHasPlan,
        setSort,
        resetFilters
    } = useIssueFilters(projectIdRef);

    const createModalOpen = ref<boolean>(false);
    const creatingIssue = ref<boolean>(false);
    const openTabModalOpen = ref<boolean>(false);
    const openingTab = ref<boolean>(false);
    const worktreeResolution = ref<IssueWorktreeResolution | null>(null);
    const bugCycleModalOpen = ref<boolean>(false);
    const startingCycle = ref<boolean>(false);
    const authPanelOpen = ref<boolean>(false);

    const repoSlug = computed<string>(() => {
        const res = githubStore.resolutionFor(props.project.id);
        if (res && res.kind === 'git_hub') {
            return `${res.identity.owner}/${res.identity.repo}`;
        }
        return props.project.title;
    });

    const openCount = computed<number | null>(() => githubStore.openCountFor(props.project.id));
    const issues = computed<IssueSummary[]>(() => githubStore.issuesFor(props.project.id));
    const totalMatching = computed<number>(
        () => githubStore.totalMatchingByProject[props.project.id] ?? issues.value.length
    );
    const selectedIssueNumber = computed<number | null>(() =>
        githubStore.selectedIssueNumberFor(props.project.id)
    );
    const selectedDetail = computed<IssueDetailDto | null>(() =>
        githubStore.selectedDetailFor(props.project.id)
    );
    const selectedSummary = computed<IssueSummary | null>(() => {
        if (selectedDetail.value) {
            return selectedDetail.value.summary;
        }
        if (!selectedIssueNumber.value) {
            return null;
        }
        return issues.value.find((i) => i.number === selectedIssueNumber.value) ?? null;
    });
    const isLoadingList = computed<boolean>(() =>
        Boolean(githubStore.loadingListByProject[props.project.id])
    );
    const isStale = computed<boolean>(() => Boolean(githubStore.staleByProject[props.project.id]));
    const missingLabelsCount = computed<number>(
        () => githubStore.missingCanonicalCountByProject[props.project.id] ?? 0
    );
    const projectError = computed<string | null>(
        () => githubStore.errorByProject[props.project.id] ?? null
    );
    const activeCycle = computed<BugCycleRunState | null>(
        () => githubStore.activeCycleByProject[props.project.id] ?? null
    );
    const cyclePreview = computed<BugCyclePreview | null>(
        () => githubStore.cyclePreviewByProject[props.project.id] ?? null
    );
    const isSelectedIssueMutating = computed<boolean>(() => {
        if (!selectedIssueNumber.value) {
            return false;
        }
        return githubStore.isIssueMutating(props.project.id, selectedIssueNumber.value);
    });

    watch(
        () => props.project.id,
        async (projectId) => {
            await githubStore.loadIssues(projectId);
            const sel = githubStore.selectedIssueNumberFor(projectId);
            if (sel) {
                await githubStore.selectIssue(projectId, sel);
            }
        },
        { immediate: true }
    );

    function clearProjectError(): void {
        githubStore.errorByProject[props.project.id] = null;
    }

    async function onRefresh(): Promise<void> {
        await githubStore.refreshProject(props.project.id);
    }

    async function onSelectIssue(issueNumber: number): Promise<void> {
        await githubStore.selectIssue(props.project.id, issueNumber);
    }

    async function onSyncCanonicalLabels(): Promise<void> {
        await githubStore.ensureCanonicalLabels(props.project.id, false);
    }

    async function onUpdateLabels(patch: LabelUpdatePatch): Promise<void> {
        await githubStore.updateIssueLabels(patch);
    }

    async function onSendMaxCode(action: MaxCodeIssueAction): Promise<void> {
        if (!selectedIssueNumber.value) {
            return;
        }
        await githubStore.sendToMaxCode({
            projectId: props.project.id,
            issueNumber: selectedIssueNumber.value,
            action
        });
    }

    async function onOpenTabModal(): Promise<void> {
        if (!selectedIssueNumber.value) {
            return;
        }
        openTabModalOpen.value = true;
        worktreeResolution.value = await githubStore
            .resolveIssueWorktree(props.project.id, selectedIssueNumber.value)
            .catch(() => null);
    }

    async function onConfirmOpenTab(request: OpenIssueTabRequest): Promise<void> {
        openingTab.value = true;
        try {
            await githubStore.openIssueTab(request);
            openTabModalOpen.value = false;
        } finally {
            openingTab.value = false;
        }
    }

    async function onCreateIssue(input: CreateIssueInput): Promise<void> {
        creatingIssue.value = true;
        try {
            await githubStore.createIssue(input);
            createModalOpen.value = false;
        } finally {
            creatingIssue.value = false;
        }
    }

    async function onOpenBugCycleModal(): Promise<void> {
        bugCycleModalOpen.value = true;
        await githubStore.previewBugCycle(props.project.id, true).catch(() => null);
    }

    async function onStartBugCycle(request: StartBugCycleRequest): Promise<void> {
        startingCycle.value = true;
        try {
            await githubStore.startBugCycle(request);
            bugCycleModalOpen.value = false;
        } finally {
            startingCycle.value = false;
        }
    }

    async function onControlCycle(action: 'pause' | 'resume' | 'cancel'): Promise<void> {
        await githubStore.controlBugCycle(props.project.id, action);
    }

    async function onOpenAuthPanel(): Promise<void> {
        authPanelOpen.value = true;
        await githubStore.loadAuthStatus();
    }

    async function onSaveRestToken(token: string): Promise<void> {
        await githubStore.saveRestToken(token);
    }

    async function onClearRestToken(): Promise<void> {
        await githubStore.clearRestToken();
    }
</script>

<style lang="scss" scoped>
    .issues-tab {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background: var(--mt-ui-surface, var(--background-0));
    }

    .issues-tab__toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 0.5rem 0.875rem;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .issues-tab__repo-info {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.8125rem;
        font-weight: 700;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .issues-tab__open-badge {
        padding: 0.125rem 0.5rem;
        border-radius: 9999px;
        background: var(--mt-ui-issue-subtle, color-mix(in srgb, var(--max-purple-500, #a855f7) 20%, transparent));
        color: var(--mt-ui-issue-category-hover, var(--max-purple-400, #c084fc));
        font-size: 0.7188rem;
        font-weight: 700;
    }

    .issues-tab__actions {
        display: flex;
        align-items: center;
        gap: 0.375rem;
    }

    .issues-tab__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.3125rem;
        height: 1.75rem;
        padding: 0 0.625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }

        &--primary {
            border-color: var(--max-primary-500);
            background: color-mix(in srgb, var(--max-primary-500) 18%, transparent);
            color: var(--max-primary-400);
        }

        &--cycle {
            border-color: var(--mt-ui-issue-category, var(--max-purple-500, #a855f7));
            background: var(--mt-ui-issue-subtle, color-mix(in srgb, var(--max-purple-500, #a855f7) 20%, transparent));
            color: var(--max-purple-300, #d8b4fe);
        }
    }

    .issues-tab__error {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.375rem 0.875rem;
        background: var(--mt-ui-danger-subtle, color-mix(in srgb, var(--max-danger-500) 15%, transparent));
        border-bottom: 1px solid var(--max-danger-500);
        color: var(--max-danger-300);
        font-size: 0.7813rem;
    }

    .issues-tab__error-dismiss {
        padding: 0.125rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--max-danger-500);
        background: transparent;
        color: var(--max-danger-300);
        font-size: 0.7188rem;
        cursor: pointer;
    }

    .issues-tab__split {
        flex: 1;
        min-height: 0;
        display: flex;
        overflow: hidden;
    }

    .issues-tab__left {
        width: 22rem;
        min-width: 16rem;
        max-width: 30rem;
        height: 100%;
        flex-shrink: 0;
    }

    .issues-tab__right {
        flex: 1;
        min-width: 0;
        height: 100%;
    }
</style>
