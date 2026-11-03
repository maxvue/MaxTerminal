import { computed, type Ref } from 'vue';
import { createDefaultIssueFilter, useGithubIssuesStore } from '@/stores/githubIssues';
import type { IssueFilter, IssueSortOrder, IssueStateFilter, ProjectId } from '@/types/bindings';

export function useIssueFilters(projectId: Ref<ProjectId>) {
    const store = useGithubIssuesStore();

    const filter = computed<IssueFilter>(() => store.filterFor(projectId.value));

    const hasActiveFilters = computed<boolean>(() => {
        const f = filter.value;
        return (
            f.state !== 'open' ||
            (f.stateLabels?.length ?? 0) > 0 ||
            (f.closureLabels?.length ?? 0) > 0 ||
            (f.categoryLabels?.length ?? 0) > 0 ||
            (f.customLabels?.length ?? 0) > 0 ||
            Boolean(f.searchText && f.searchText.trim()) ||
            (f.hasPlan !== null && f.hasPlan !== undefined) ||
            Boolean(f.assignee) ||
            Boolean(f.author)
        );
    });

    async function setStateFilter(state: IssueStateFilter): Promise<void> {
        await store.loadIssues(projectId.value, { state, offset: 0 });
    }

    async function toggleStateLabel(labelName: string): Promise<void> {
        const current = [...(filter.value.stateLabels ?? [])];
        const idx = current.indexOf(labelName);
        if (idx >= 0) {
            current.splice(idx, 1);
        } else {
            current.push(labelName);
        }
        await store.loadIssues(projectId.value, { stateLabels: current, offset: 0 });
    }

    async function toggleCategoryLabel(labelName: string): Promise<void> {
        const current = [...(filter.value.categoryLabels ?? [])];
        const idx = current.indexOf(labelName);
        if (idx >= 0) {
            current.splice(idx, 1);
        } else {
            current.push(labelName);
        }
        await store.loadIssues(projectId.value, { categoryLabels: current, offset: 0 });
    }

    async function toggleClosureLabel(labelName: string): Promise<void> {
        const current = [...(filter.value.closureLabels ?? [])];
        const idx = current.indexOf(labelName);
        if (idx >= 0) {
            current.splice(idx, 1);
        } else {
            current.push(labelName);
        }
        await store.loadIssues(projectId.value, { closureLabels: current, offset: 0 });
    }

    async function setSearchText(text: string): Promise<void> {
        const normalized = text.trim() ? text.trim() : null;
        await store.loadIssues(projectId.value, { searchText: normalized, offset: 0 });
    }

    async function setHasPlan(hasPlan: boolean | null): Promise<void> {
        await store.loadIssues(projectId.value, { hasPlan, offset: 0 });
    }

    async function setSort(sort: IssueSortOrder): Promise<void> {
        await store.loadIssues(projectId.value, { sort, offset: 0 });
    }

    async function resetFilters(): Promise<void> {
        await store.loadIssues(projectId.value, createDefaultIssueFilter());
    }

    return {
        filter,
        hasActiveFilters,
        setState: setStateFilter,
        setStateFilter,
        toggleStateLabel,
        toggleCategoryLabel,
        toggleClosureLabel,
        setSearchText,
        setHasPlan,
        setSort,
        resetFilters
    };
}
