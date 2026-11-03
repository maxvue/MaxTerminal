<template>
    <section class="project-workspace" aria-label="Workspace do projeto">
        <header v-if="props.project" class="project-workspace__header" :style="{ '--project-color': props.project.color, '--mt-project-color': props.project.color }">
            <span class="project-workspace__avatar" aria-hidden="true">
                <MaxIcon :icon="props.project.icon" class="project-workspace__icon" />
            </span>
            <div class="project-workspace__meta">
                <div class="project-workspace__title-row">
                    <h2 class="project-workspace__title">{{ props.project.title }}</h2>
                    <span class="project-workspace__stack">{{ stackLabel }}</span>
                    <span v-if="props.project.missing" class="project-workspace__missing" role="status">Pasta não encontrada no disco</span>
                    <MaxIconButton icon="lucide:settings" aria-label="Configurações do projeto" title="Configurações do projeto" class="project-workspace__settings-btn" @click="handleOpenSettings" />
                </div>
                <span class="project-workspace__path">{{ props.project.path }}</span>
            </div>
            <div class="project-workspace__actions">
                <button type="button" class="project-workspace__search-btn" title="Buscar em tudo… (Ctrl+K)" aria-label="Buscar em tudo… (Ctrl+K)" @click="searchStore.show(undefined, props.project.id)">
                    <MaxIcon icon="lucide:search" />
                    <span>Buscar em tudo…</span>
                    <kbd class="project-workspace__kbd">Ctrl+K</kbd>
                </button>
                <AiStatusIndicator />
                <BridgeStatusIndicator />
                <ShortcutStatusBadge />
                <button type="button" class="project-workspace__prefs-btn" title="Preferências do sistema" aria-label="Abrir preferências do sistema" @click="windowStore.openPreferences('system')">
                    <MaxIcon icon="lucide:settings" />
                </button>
            </div>
        </header>
        <header v-else class="project-workspace__header project-workspace__header--empty">
            <div class="project-workspace__actions ml-auto">
                <button type="button" class="project-workspace__search-btn" title="Buscar em tudo… (Ctrl+K)" aria-label="Buscar em tudo… (Ctrl+K)" @click="searchStore.show()">
                    <MaxIcon icon="lucide:search" />
                    <span>Buscar em tudo…</span>
                    <kbd class="project-workspace__kbd">Ctrl+K</kbd>
                </button>
                <AiStatusIndicator />
                <BridgeStatusIndicator />
                <ShortcutStatusBadge />
                <button type="button" class="project-workspace__prefs-btn" title="Preferências do sistema" aria-label="Abrir preferências do sistema" @click="windowStore.openPreferences('system')">
                    <MaxIcon icon="lucide:settings" />
                </button>
            </div>
        </header>
        <div v-if="props.project" class="project-workspace__body">
            <slot>
                <TabWorkspaceView :project="props.project" />
            </slot>
        </div>
        <div v-else class="project-workspace__empty">
            <slot>
                <AppEmptyState title="Nenhum projeto selecionado" description="Selecione um projeto na coluna ao lado ou adicione um novo projeto para iniciar suas sessões de terminal." />
            </slot>
        </div>
    </section>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import MaxIconButton from '@maxvue/max-components-ui/components/MaxIconButton';
    import AiStatusIndicator from '@/components/ai/AiStatusIndicator.vue';
    import BridgeStatusIndicator from '@/components/bridge/BridgeStatusIndicator.vue';
    import ShortcutStatusBadge from '@/components/os/ShortcutStatusBadge.vue';
    import AppEmptyState from '@/components/shell/AppEmptyState.vue';
    import TabWorkspaceView from '@/components/workspace/TabWorkspaceView.vue';
    import { STACK_LABELS_PT_BR } from '@/constants/projectIdentity';
    import { useSearchStore } from '@/stores/search';
    import { useWindowStore } from '@/stores/window';
    import type { Project_Serialize } from '@/types/bindings';

    export interface ProjectWorkspaceProps {
        project: Project_Serialize | null;
    }

    const props = defineProps<ProjectWorkspaceProps>();
    const searchStore = useSearchStore();
    const windowStore = useWindowStore();

    const stackLabel = computed<string>(() => {
        if (!props.project) {
            return '';
        }
        return STACK_LABELS_PT_BR[props.project.stack] ?? 'Genérico';
    });

    function handleOpenSettings(): void {
        if (props.project) {
            windowStore.openProjectSettings(props.project.id);
        }
    }
</script>

<style lang="scss" scoped>
    .project-workspace {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
    }

    .project-workspace__header {
        display: flex;
        align-items: center;
        gap: 0.75rem;
        padding: 0.625rem 1rem;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .project-workspace__header--empty {
        min-height: 2.75rem;
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .project-workspace__actions {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        margin-left: auto;
        flex-shrink: 0;
    }

    .project-workspace__search-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.25rem 0.625rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 0.375rem;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
            color: var(--mt-ui-text-strong, var(--background-950));
        }
    }

    .project-workspace__kbd {
        padding: 0.0625rem 0.25rem;
        border-radius: 0.25rem;
        background: var(--mt-ui-surface-hover, var(--background-75));
        color: var(--mt-ui-text-muted, var(--background-400));
        font-family: var(--mt-font-mono);
        font-size: 0.625rem;
    }

    .project-workspace__prefs-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        padding: 0;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 0.25rem;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-muted, var(--background-400));
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
            color: var(--mt-ui-text-strong, var(--background-950));
        }
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

    .project-workspace__settings-btn {
        margin-left: auto;
        flex-shrink: 0;
        color: var(--mt-ui-text-muted, var(--background-400));

        &:hover {
            color: var(--mt-ui-text-strong, var(--background-950));
        }
    }

    .project-workspace__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .project-workspace__stack {
        font-size: 0.6875rem;
        font-weight: 600;
        padding: 0.0625rem 0.4375rem;
        border-radius: 9999px;
        background: var(--mt-ui-surface-hover, var(--background-75));
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .project-workspace__missing {
        font-size: 0.6875rem;
        font-weight: 600;
        padding: 0.0625rem 0.5rem;
        border-radius: 9999px;
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--max-warning-400);
        border: 1px solid var(--max-warning-500);
    }

    .project-workspace__path {
        font-size: 0.75rem;
        color: var(--mt-ui-text-muted, var(--background-400));
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
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .project-workspace__placeholder-desc {
        margin: 0;
        font-size: 0.8125rem;
        color: var(--mt-ui-text-muted, var(--background-400));
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