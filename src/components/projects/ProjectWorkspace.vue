<template>
    <section class="project-workspace" aria-label="Workspace do projeto">
        <template v-if="props.project">
            <header class="project-workspace__header" :style="{ '--project-color': props.project.color, '--mt-project-color': props.project.color }">
                <span class="project-workspace__avatar" aria-hidden="true">
                    <MaxIcon :icon="props.project.icon" class="project-workspace__icon" />
                </span>
                <div class="project-workspace__meta">
                    <div class="project-workspace__title-row">
                        <h2 class="project-workspace__title">{{ props.project.title }}</h2>
                        <span class="project-workspace__stack">{{ stackLabel }}</span>
                        <span v-if="props.project.missing" class="project-workspace__missing" role="status">Pasta não encontrada no disco</span>
                    </div>
                    <span class="project-workspace__path">{{ props.project.path }}</span>
                </div>
            </header>
            <div class="project-workspace__body">
                <slot>
                    <TabWorkspaceView :project="props.project" />
                </slot>
            </div>
        </template>
        <div v-else class="project-workspace__empty">
            <slot>
                <AppEmptyState title="Nenhum projeto selecionado" description="Selecione um projeto na coluna ao lado ou adicione um novo projeto para iniciar suas sessões de terminal." />
            </slot>
        </div>
    </section>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import AppEmptyState from '@/components/shell/AppEmptyState.vue';
    import TabWorkspaceView from '@/components/workspace/TabWorkspaceView.vue';
    import { STACK_LABELS_PT_BR } from '@/constants/projectIdentity';
    import type { Project_Serialize } from '@/types/bindings';

    export interface ProjectWorkspaceProps {
        project: Project_Serialize | null;
    }

    const props = defineProps<ProjectWorkspaceProps>();

    const stackLabel = computed<string>(() => {
        if (!props.project) {
            return '';
        }
        return STACK_LABELS_PT_BR[props.project.stack] ?? 'Genérico';
    });
</script>

<style lang="scss" scoped>
    .project-workspace {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        background: var(--background-900);
        color: var(--background-100);
    }

    .project-workspace__header {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        padding: 0.625rem 1rem;
        border-bottom: 1px solid var(--background-700);
        background: var(--background-800);
    }

    .project-workspace__avatar {
        width: 2.25rem;
        height: 2.25rem;
        border-radius: 0.5rem;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: color-mix(in srgb, var(--mt-project-color, #14B8A6) 15%, transparent);
        color: var(--mt-project-color, #14B8A6);
        border: 1px solid color-mix(in srgb, var(--mt-project-color, #14B8A6) 35%, transparent);
        flex-shrink: 0;
    }

    .project-workspace__icon {
        font-size: 1.25rem;
    }

    .project-workspace__meta {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
    }

    .project-workspace__title-row {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        min-width: 0;
    }

    .project-workspace__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
        color: var(--background-50);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .project-workspace__stack {
        font-size: 0.6875rem;
        font-weight: 600;
        padding: 0.0625rem 0.4375rem;
        border-radius: 9999px;
        background: var(--background-700);
        color: var(--background-200);
    }

    .project-workspace__missing {
        font-size: 0.6875rem;
        font-weight: 600;
        padding: 0.0625rem 0.5rem;
        border-radius: 9999px;
        background: var(--background-900);
        color: var(--max-warning-400);
        border: 1px solid var(--max-warning-500);
    }

    .project-workspace__path {
        font-size: 0.75rem;
        color: var(--background-400);
        font-family: var(--mt-font-mono);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .project-workspace__body {
        flex: 1;
        min-width: 0;
        min-height: 0;
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .project-workspace__placeholder {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        padding: 2rem;
        text-align: center;
        gap: 0.5rem;
    }

    .project-workspace__placeholder-title {
        margin: 0;
        font-size: 1rem;
        font-weight: 600;
        color: var(--background-100);
    }

    .project-workspace__placeholder-desc {
        margin: 0;
        font-size: 0.8125rem;
        color: var(--background-400);
        max-width: 28rem;
    }

    .project-workspace__placeholder-warn {
        margin: 0;
        font-size: 0.8125rem;
        color: var(--max-warning-400);
        max-width: 28rem;
    }

    .project-workspace__empty {
        flex: 1;
        display: flex;
        align-items: center;
        justify-content: center;
        min-height: 0;
    }
</style>
