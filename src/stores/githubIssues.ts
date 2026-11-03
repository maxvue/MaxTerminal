import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { githubIpc } from '@/ipc/github';
import { useProjectsStore } from '@/stores/projects';
import type {
    BugCyclePreview,
    BugCycleRunState,
    CanonicalLabelSpec,
    CreateIssueInput,
    CreateIssueWorktreeRequest,
    GithubAuthStatus,
    GithubTransportKind,
    IssueCounts,
    IssueDetail,
    IssueFilter,
    IssueSummary,
    IssueWorktreeResolution,
    LabelEnsureReport,
    LabelUpdatePatch,
    OpenIssueTabReceipt,
    OpenIssueTabRequest,
    ProjectId,
    RateLimitSnapshot,
    RepoLabel,
    RepoResolution,
    SendIssueToMaxCodeReceipt,
    SendIssueToMaxCodeRequest,
    StartBugCycleRequest
} from '@/types/bindings';

const MAX_CACHED_DETAILS_PER_PROJECT = 20;

export function createDefaultIssueFilter(): IssueFilter {
    return {
        state: 'open',
        stateLabels: [],
        closureLabels: [],
        categoryLabels: [],
        customLabels: [],
        searchText: null,
        hasPlan: null,
        assignee: null,
        author: null,
        sort: 'updated_desc',
        limit: 50,
        offset: 0
    };
}

export const useGithubIssuesStore = defineStore('githubIssues', () => {
    const resolutionsByProject = ref<Record<ProjectId, RepoResolution>>({});
    const itemsByProject = ref<Record<ProjectId, IssueSummary[]>>({});
    const totalMatchingByProject = ref<Record<ProjectId, number>>({});
    const openCountByProject = ref<Record<ProjectId, number | null>>({});
    const countsByProject = ref<Record<ProjectId, IssueCounts>>({});
    const filtersByProject = ref<Record<ProjectId, IssueFilter>>({});
    const selectedIssueNumberByProject = ref<Record<ProjectId, number | null>>({});
    const detailsByProject = ref<Record<ProjectId, Record<number, IssueDetail>>>({});
    const detailLruByProject = ref<Record<ProjectId, number[]>>({});
    const repoLabelsByProject = ref<Record<ProjectId, RepoLabel[]>>({});
    const missingCanonicalCountByProject = ref<Record<ProjectId, number>>({});
    const lastTransportByProject = ref<Record<ProjectId, GithubTransportKind>>({});
    const lastSuccessAtByProject = ref<Record<ProjectId, number | null>>({});
    const staleByProject = ref<Record<ProjectId, boolean>>({});
    const loadingListByProject = ref<Record<ProjectId, boolean>>({});
    const loadingDetailByKey = ref<Record<string, boolean>>({});
    const mutatingByKey = ref<Record<string, boolean>>({});
    const errorByProject = ref<Record<ProjectId, string | null>>({});

    const canonicalCatalog = ref<CanonicalLabelSpec[]>([]);
    const authStatus = ref<GithubAuthStatus | null>(null);
    const rateLimit = ref<RateLimitSnapshot | null>(null);
    const activeCycleByProject = ref<Record<ProjectId, BugCycleRunState | null>>({});
    const cyclePreviewByProject = ref<Record<ProjectId, BugCyclePreview | null>>({});
    const pinnedIssuesTabActiveByProject = ref<Record<ProjectId, boolean>>({});

    const projectsStore = useProjectsStore();

    const selectedProjectId = computed<ProjectId | null>(
        () => projectsStore.selectedProject?.id ?? null
    );

    function isGithubRepo(projectId: ProjectId): boolean {
        return resolutionsByProject.value[projectId]?.kind === 'git_hub';
    }

    function resolutionFor(projectId: ProjectId): RepoResolution | null {
        return resolutionsByProject.value[projectId] ?? null;
    }

    function openCountFor(projectId: ProjectId): number | null {
        const res = resolutionsByProject.value[projectId];
        if (res && res.kind !== 'git_hub') {
            return null;
        }
        const count = openCountByProject.value[projectId];
        if (typeof count !== 'number' || count <= 0) {
            return null;
        }
        return count;
    }

    function filterFor(projectId: ProjectId): IssueFilter {
        const current = filtersByProject.value[projectId];
        if (current) {
            return current;
        }
        const def = createDefaultIssueFilter();
        filtersByProject.value[projectId] = def;
        return def;
    }

    function issuesFor(projectId: ProjectId): IssueSummary[] {
        return itemsByProject.value[projectId] ?? [];
    }

    function selectedIssueNumberFor(projectId: ProjectId): number | null {
        return selectedIssueNumberByProject.value[projectId] ?? null;
    }

    function detailFor(projectId: ProjectId, issueNumber: number): IssueDetail | null {
        return detailsByProject.value[projectId]?.[issueNumber] ?? null;
    }

    function selectedDetailFor(projectId: ProjectId): IssueDetail | null {
        const num = selectedIssueNumberFor(projectId);
        if (!num) {
            return null;
        }
        return detailFor(projectId, num);
    }

    function isIssueMutating(projectId: ProjectId, issueNumber: number): boolean {
        return Boolean(mutatingByKey.value[`${projectId}:${issueNumber}`]);
    }

    function setPinnedTabActive(projectId: ProjectId, active: boolean): void {
        pinnedIssuesTabActiveByProject.value[projectId] = active;
    }

    function isPinnedTabActive(projectId: ProjectId): boolean {
        return Boolean(pinnedIssuesTabActiveByProject.value[projectId]);
    }

    function storeDetailWithLru(projectId: ProjectId, detail: IssueDetail): void {
        const map = { ...(detailsByProject.value[projectId] ?? {}) };
        const order = [...(detailLruByProject.value[projectId] ?? [])].filter(
            (n) => n !== detail.summary.number
        );
        order.push(detail.summary.number);
        while (order.length > MAX_CACHED_DETAILS_PER_PROJECT) {
            const evicted = order.shift();
            if (evicted !== undefined) {
                delete map[evicted];
            }
        }
        map[detail.summary.number] = detail;
        detailsByProject.value[projectId] = map;
        detailLruByProject.value[projectId] = order;
    }

    async function resolveProjectRepo(projectId: ProjectId): Promise<RepoResolution> {
        try {
            const res = await githubIpc.repoResolve(projectId);
            resolutionsByProject.value[projectId] = res;
            if (res.kind !== 'git_hub') {
                openCountByProject.value[projectId] = null;
                pinnedIssuesTabActiveByProject.value[projectId] = false;
            } else if (openCountByProject.value[projectId] === undefined) {
                const counts = await githubIpc.issueCounts(projectId).catch(() => null);
                if (counts) {
                    countsByProject.value[projectId] = counts;
                    openCountByProject.value[projectId] = counts.openCount;
                }
            }
            return res;
        } catch (err) {
            const message = err instanceof Error ? err.message : 'Falha ao resolver repositório.';
            const fallback: RepoResolution = { kind: 'error', message };
            resolutionsByProject.value[projectId] = fallback;
            openCountByProject.value[projectId] = null;
            return fallback;
        }
    }

    async function loadIssues(
        projectId: ProjectId,
        customFilter?: Partial<IssueFilter>,
        forceRefresh = false
    ): Promise<void> {
        const baseFilter = filterFor(projectId);
        const merged: IssueFilter = {
            ...baseFilter,
            ...customFilter
        };
        filtersByProject.value[projectId] = merged;
        loadingListByProject.value[projectId] = true;
        errorByProject.value[projectId] = null;

        try {
            const page = await githubIpc.issuesList(projectId, merged, forceRefresh);
            itemsByProject.value[projectId] = page.items;
            totalMatchingByProject.value[projectId] = page.totalMatching;
            openCountByProject.value[projectId] = page.openIssuesCount;
            staleByProject.value[projectId] = page.stale;
            lastTransportByProject.value[projectId] = page.transportUsed;
            lastSuccessAtByProject.value[projectId] = page.lastSuccessAtMs ?? null;
            missingCanonicalCountByProject.value[projectId] = page.missingCanonicalLabelsCount;
            if (page.rateLimit) {
                rateLimit.value = page.rateLimit;
            }
            if (
                selectedIssueNumberByProject.value[projectId] == null &&
                page.items.length > 0 &&
                page.items[0]
            ) {
                selectedIssueNumberByProject.value[projectId] = page.items[0].number;
            }
        } catch (err) {
            errorByProject.value[projectId] =
                err instanceof Error ? err.message : 'Falha ao carregar issues do GitHub.';
        } finally {
            loadingListByProject.value[projectId] = false;
        }
    }

    async function refreshProject(projectId: ProjectId): Promise<void> {
        loadingListByProject.value[projectId] = true;
        errorByProject.value[projectId] = null;
        try {
            const page = await githubIpc.refresh(projectId);
            itemsByProject.value[projectId] = page.items;
            totalMatchingByProject.value[projectId] = page.totalMatching;
            openCountByProject.value[projectId] = page.openIssuesCount;
            staleByProject.value[projectId] = page.stale;
            lastTransportByProject.value[projectId] = page.transportUsed;
            lastSuccessAtByProject.value[projectId] = page.lastSuccessAtMs ?? null;
            missingCanonicalCountByProject.value[projectId] = page.missingCanonicalLabelsCount;
            if (page.rateLimit) {
                rateLimit.value = page.rateLimit;
            }
            const selected = selectedIssueNumberByProject.value[projectId];
            if (selected) {
                await selectIssue(projectId, selected, true);
            }
        } catch (err) {
            errorByProject.value[projectId] =
                err instanceof Error ? err.message : 'Falha ao atualizar issues.';
        } finally {
            loadingListByProject.value[projectId] = false;
        }
    }

    async function selectIssue(
        projectId: ProjectId,
        issueNumber: number,
        forceRefresh = false
    ): Promise<IssueDetail | null> {
        selectedIssueNumberByProject.value[projectId] = issueNumber;
        const cached = detailFor(projectId, issueNumber);
        if (cached && !forceRefresh) {
            return cached;
        }

        const key = `${projectId}:${issueNumber}`;
        loadingDetailByKey.value[key] = true;
        try {
            const detail = await githubIpc.issueDetail(projectId, issueNumber, forceRefresh);
            storeDetailWithLru(projectId, detail);
            upsertIssueSummaryInList(projectId, detail.summary);
            return detail;
        } catch (err) {
            errorByProject.value[projectId] =
                err instanceof Error
                    ? err.message
                    : `Falha ao carregar detalhes da issue #${issueNumber}.`;
            return cached;
        } finally {
            loadingDetailByKey.value[key] = false;
        }
    }

    function upsertIssueSummaryInList(projectId: ProjectId, summary: IssueSummary): void {
        const current = [...(itemsByProject.value[projectId] ?? [])];
        const idx = current.findIndex((i) => i.number === summary.number);
        if (idx >= 0) {
            current[idx] = summary;
        } else {
            current.unshift(summary);
        }
        itemsByProject.value[projectId] = current;

        const existingDetail = detailsByProject.value[projectId]?.[summary.number];
        if (existingDetail) {
            storeDetailWithLru(projectId, {
                ...existingDetail,
                summary
            });
        }
    }

    async function loadAuthStatus(): Promise<GithubAuthStatus | null> {
        try {
            const status = await githubIpc.authStatus();
            authStatus.value = status;
            if (status.rateLimit) {
                rateLimit.value = status.rateLimit;
            }
            return status;
        } catch {
            return null;
        }
    }

    async function saveRestToken(token: string): Promise<GithubAuthStatus> {
        const status = await githubIpc.authSetToken(token);
        authStatus.value = status;
        return status;
    }

    async function clearRestToken(): Promise<GithubAuthStatus> {
        const status = await githubIpc.authClearToken();
        authStatus.value = status;
        return status;
    }

    async function loadLabelsCatalog(): Promise<CanonicalLabelSpec[]> {
        if (canonicalCatalog.value.length > 0) {
            return canonicalCatalog.value;
        }
        const catalog = await githubIpc.labelsCatalog();
        canonicalCatalog.value = catalog;
        return catalog;
    }

    async function loadRepoLabels(
        projectId: ProjectId,
        forceRefresh = false
    ): Promise<RepoLabel[]> {
        const labels = await githubIpc.repoLabels(projectId, forceRefresh);
        repoLabelsByProject.value[projectId] = labels;
        return labels;
    }

    async function ensureCanonicalLabels(
        projectId: ProjectId,
        dryRun = false
    ): Promise<LabelEnsureReport> {
        const report = await githubIpc.labelsEnsure(projectId, dryRun);
        repoLabelsByProject.value[projectId] = report.repoLabels;
        missingCanonicalCountByProject.value[projectId] = report.missing.length;
        return report;
    }

    async function createIssue(input: CreateIssueInput): Promise<IssueSummary> {
        const summary = await githubIpc.issueCreate(input);
        upsertIssueSummaryInList(input.projectId, summary);
        selectedIssueNumberByProject.value[input.projectId] = summary.number;
        const prevCount = openCountByProject.value[input.projectId] ?? 0;
        openCountByProject.value[input.projectId] = prevCount + 1;
        return summary;
    }

    async function updateIssueLabels(patch: LabelUpdatePatch): Promise<IssueSummary> {
        const key = `${patch.projectId}:${patch.issueNumber}`;
        if (mutatingByKey.value[key]) {
            throw new Error('Já existe uma atualização de labels em andamento para esta issue.');
        }
        mutatingByKey.value[key] = true;
        try {
            const updated = await githubIpc.issueLabelsUpdate(patch);
            upsertIssueSummaryInList(patch.projectId, updated);
            return updated;
        } finally {
            mutatingByKey.value[key] = false;
        }
    }

    async function sendToMaxCode(
        request: SendIssueToMaxCodeRequest
    ): Promise<SendIssueToMaxCodeReceipt> {
        return githubIpc.issueSendMaxcode(request);
    }

    async function resolveIssueWorktree(
        projectId: ProjectId,
        issueNumber: number
    ): Promise<IssueWorktreeResolution> {
        return githubIpc.issueWorktreeResolve(projectId, issueNumber);
    }

    async function createIssueWorktree(
        request: CreateIssueWorktreeRequest
    ): Promise<IssueWorktreeResolution> {
        return githubIpc.issueWorktreeCreate(request);
    }

    async function openIssueTab(request: OpenIssueTabRequest): Promise<OpenIssueTabReceipt> {
        const receipt = await githubIpc.issueOpenTab(request);
        pinnedIssuesTabActiveByProject.value[request.projectId] = false;
        return receipt;
    }

    async function previewBugCycle(
        projectId: ProjectId,
        includeReplanAndRejected = true
    ): Promise<BugCyclePreview> {
        const preview = await githubIpc.bugCyclePreview(projectId, includeReplanAndRejected);
        cyclePreviewByProject.value[projectId] = preview;
        if (preview.activeCycle) {
            activeCycleByProject.value[projectId] = preview.activeCycle;
        }
        return preview;
    }

    async function startBugCycle(request: StartBugCycleRequest): Promise<BugCycleRunState> {
        const cycle = await githubIpc.bugCycleStart(request);
        activeCycleByProject.value[request.projectId] = cycle;
        return cycle;
    }

    async function controlBugCycle(
        projectId: ProjectId,
        action: 'pause' | 'resume' | 'cancel' | null = null
    ): Promise<BugCycleRunState | null> {
        const cycle = await githubIpc.bugCycleStatus(projectId, action);
        activeCycleByProject.value[projectId] = cycle;
        return cycle;
    }

    return {
        resolutionsByProject,
        itemsByProject,
        totalMatchingByProject,
        openCountByProject,
        countsByProject,
        filtersByProject,
        selectedIssueNumberByProject,
        detailsByProject,
        repoLabelsByProject,
        missingCanonicalCountByProject,
        lastTransportByProject,
        lastSuccessAtByProject,
        staleByProject,
        loadingListByProject,
        loadingDetailByKey,
        mutatingByKey,
        errorByProject,
        canonicalCatalog,
        authStatus,
        rateLimit,
        activeCycleByProject,
        cyclePreviewByProject,
        pinnedIssuesTabActiveByProject,
        selectedProjectId,
        isGithubRepo,
        resolutionFor,
        openCountFor,
        filterFor,
        issuesFor,
        selectedIssueNumberFor,
        detailFor,
        selectedDetailFor,
        isIssueMutating,
        setPinnedTabActive,
        isPinnedTabActive,
        resolveProjectRepo,
        loadIssues,
        refreshProject,
        selectIssue,
        upsertIssueSummaryInList,
        loadAuthStatus,
        saveRestToken,
        clearRestToken,
        loadLabelsCatalog,
        loadRepoLabels,
        ensureCanonicalLabels,
        createIssue,
        updateIssueLabels,
        sendToMaxCode,
        resolveIssueWorktree,
        createIssueWorktree,
        openIssueTab,
        previewBugCycle,
        startBugCycle,
        controlBugCycle
    };
});
