<template>
    <div class="app-layout">
        <Splitpanes v-if="ready" class="mt-columns default-theme" :horizontal="false" :maximize-panes="false" @resized="onResized">
            <Pane :size="leftSize" :min-size="15" :max-size="45">
                <ProjectList />
            </Pane>
            <Pane :size="100 - leftSize">
                <ProjectWorkspace :project="projectsStore.selectedProject">
                    <slot />
                </ProjectWorkspace>
            </Pane>
        </Splitpanes>
    </div>
</template>

<script setup lang="ts">
    import { Pane, Splitpanes } from 'splitpanes';
    import 'splitpanes/dist/splitpanes.css';
    import { onMounted } from 'vue';
    import ProjectList from '@/components/projects/ProjectList.vue';
    import ProjectWorkspace from '@/components/projects/ProjectWorkspace.vue';
    import { useGithubIssueEvents } from '@/composables/useGithubIssueEvents';
    import { useIssueBadgeProvider } from '@/composables/useIssueBadgeProvider';
    import { useLayoutPersistence } from '@/composables/useLayoutPersistence';
    import { useOsFocusTab } from '@/composables/useOsFocusTab';
    import { provideProjectBadgeRegistry } from '@/composables/useProjectBadgeRegistry';
    import { useProjectEvents } from '@/composables/useProjectEvents';
    import { useProjectTabFocus } from '@/composables/useProjectTabFocus';
    import { useWorkspaceEvents } from '@/composables/workspace/useWorkspaceEvents';
    import { useProjectsStore } from '@/stores/projects';
    import { useShellHistoryStore } from '@/stores/shellHistory';
    import { useTabStatusStore } from '@/stores/tabStatus';
    import { useTabsStore } from '@/stores/tabs';

    const badgeRegistry = provideProjectBadgeRegistry();
    useIssueBadgeProvider(badgeRegistry);

    const projectsStore = useProjectsStore();
    const tabsStore = useTabsStore();
    const tabStatusStore = useTabStatusStore();
    const shellHistoryStore = useShellHistoryStore();
    const { leftSize, onResized, ready } = useLayoutPersistence();
    const projectEvents = useProjectEvents();
    const workspaceEvents = useWorkspaceEvents();
    const githubIssueEvents = useGithubIssueEvents();
    useProjectTabFocus();
    useOsFocusTab();

    onMounted(() => {
        void tabStatusStore.init().catch(() => {});
        void shellHistoryStore.init().catch(() => {});
        void Promise.allSettled([
            projectEvents.start().catch(() => {
                // Em ambiente de teste sem mock de project_snapshot, mantém estado seguro.
            }),
            workspaceEvents.start().catch(() => {
                // Em ambiente de teste sem mock de workspace_snapshot, mantém estado seguro.
            })
        ]).then(() => {
            // Reconcilia vínculo de abas com projetos caso workspaceSnapshot tenha respondido antes de projectSnapshot.
            for (const tab of Object.values(tabsStore.byId)) {
                try {
                    projectsStore.attachTab(tab.id, tab.projectId);
                    if (!projectsStore.activeTabByProject[tab.projectId]) {
                        projectsStore.setActiveTab(tab.projectId, tab.id);
                    }
                } catch {
                    // Ignora se projeto não pertencer ao catálogo atual.
                }
            }
        });
        void githubIssueEvents.start().catch(() => {
            // Em ambiente de teste sem listeners GitHub, mantém estado seguro.
        });
    });
</script>

<style lang="scss" scoped>
    @use '@/styles/params' as *;

    .app-layout {
        display: flex;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
    }

    :deep(.mt-columns.splitpanes) {
        width: 100%;
        height: 100%;
        background: var(--background-900);
    }

    :deep(.mt-columns .splitpanes__pane) {
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background: transparent;
    }

    :deep(.mt-columns > .splitpanes__splitter) {
        width: 4px;
        background: var(--background-700);
        border: none;
        transition: background-color 120ms ease;

        &:hover,
        &:focus-visible {
            background: var(--max-primary-500);
        }
    }
</style>
