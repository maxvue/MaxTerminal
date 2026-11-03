<template>
    <div class="skills-memory-prefs" data-testid="skills-memory-preferences">
        <SkillsSyncPanel :project-id="selectedProjectId" />
        <section v-if="selectedProjectId && activePaneId" class="skills-memory-prefs__memory">
            <h3 class="skills-memory-prefs__heading">Memória Contextual de Comandos do Projeto</h3>
            <CommandMemoryList :project-id="selectedProjectId" :pane-id="activePaneId" />
        </section>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import CommandMemoryList from '@/components/skills/CommandMemoryList.vue';
    import SkillsSyncPanel from '@/components/skills/SkillsSyncPanel.vue';
    import { useProjectsStore } from '@/stores/projects';
    import { useTabsStore } from '@/stores/tabs';
    import type { PaneId, ProjectId } from '@/types/bindings';

    const projectsStore = useProjectsStore();
    const tabsStore = useTabsStore();

    const selectedProjectId = computed<ProjectId | null>(() => projectsStore.selectedId);
    const activePaneId = computed<PaneId | null>(
        () => tabsStore.activeTabForSelectedProject?.activePaneId ?? null
    );
</script>

<style lang="scss" scoped>
    .skills-memory-prefs {
        display: flex;
        flex-direction: column;
        gap: 1.25rem;
    }

    .skills-memory-prefs__memory {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 0 1rem 1rem;
    }

    .skills-memory-prefs__heading {
        margin: 0;
        font-size: 0.875rem;
        font-weight: 600;
        color: var(--background-100);
    }
</style>
