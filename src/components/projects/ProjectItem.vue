<template>
    <div class="project-item" :class="{ 'project-item--selected': props.selected, 'project-item--missing': props.project.missing }" :style="{ '--project-color': safeColor, '--mt-project-color': safeColor }" :data-project-id="props.project.id" @contextmenu.prevent="toggleMenu">
        <button type="button" class="project-item__main" :aria-current="props.selected ? 'true' : undefined" :title="props.project.path" @click="emit('select', props.project.id)">
            <span class="project-item__avatar" aria-hidden="true">
                <MaxIcon :icon="safeIcon" size="1.125" class="project-item__icon" />
            </span>
            <span class="project-item__content">
                <span class="project-item__header">
                    <span class="project-item__title">{{ props.project.title }}</span>
                    <span v-if="props.project.missing" class="project-item__missing-badge" title="Pasta não encontrada no disco">Pasta não encontrada no disco</span>
                </span>
                <span class="project-item__path">{{ props.project.path }}</span>
                <ProjectStatusBadges :project-id="props.project.id" :counters="props.counters" compact />
            </span>
        </button>

        <div class="project-item__actions">
            <MaxButton class="project-item__visibility-btn" :label="props.hidden ? 'Mostrar' : 'Ocultar'" :loading="props.pending" :disabled="props.disabled || props.pending" size="small" slim variant="text" :title="props.hidden ? 'Mostrar projeto' : 'Ocultar projeto'" :aria-label="props.hidden ? 'Mostrar projeto' : 'Ocultar projeto'" @click.stop="handleVisibilityToggle" />
            <MaxPopoverMenu ref="popoverMenuRef" :items="menuItems" aria-label="Opções do projeto">
                <template #trigger="slotProps">
                    <button type="button" class="project-item__menu-trigger" v-bind="(slotProps as any)" aria-label="Opções do projeto" title="Opções do projeto">
                        <MaxIcon icon="mdi:dots-vertical" />
                    </button>
                </template>
                <template #item="{ data }">
                    <span class="project-item__menu-btn" :class="{ 'project-item__menu-btn--danger': data.danger }" :data-action="data.actionKey">
                        <MaxIcon :icon="data.icon" />
                        <span>{{ data.label }}</span>
                    </span>
                </template>
            </MaxPopoverMenu>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { MaxButton } from '@maxvue/max-components-ui';
    import { computed, ref } from 'vue';
    import MaxPopoverMenu from '@maxvue/max-components-ui/components/MaxPopoverMenu';
    import ProjectStatusBadges from '@/components/projects/ProjectStatusBadges.vue';
    import { isValidProjectColor, isValidProjectIcon } from '@/constants/projectIdentity';
    import type { ProjectStatusCounters } from '@/stores/projects';
    import type { Project_Serialize, ProjectId } from '@/types/bindings';

    export interface ProjectItemProps {
        project: Project_Serialize;
        counters: ProjectStatusCounters;
        selected: boolean;
        hidden?: boolean;
        disabled?: boolean;
        pending?: boolean;
    }

    const props = withDefaults(defineProps<ProjectItemProps>(), {
        hidden: false,
        disabled: false,
        pending: false
    });

    const emit = defineEmits<{
        select: [id: ProjectId];
        edit: [id: ProjectId];
        configure: [id: ProjectId];
        remove: [id: ProjectId];
        redetect: [id: ProjectId];
        hide: [id: ProjectId];
        show: [id: ProjectId];
    }>();

    const popoverMenuRef = ref<InstanceType<typeof MaxPopoverMenu> | null>(null);

    const safeIcon = computed<string>(() =>
        isValidProjectIcon(props.project.icon) ? props.project.icon : 'mdi:folder'
    );

    const safeColor = computed<string>(() =>
        isValidProjectColor(props.project.color) ? props.project.color : '#14B8A6'
    );

    const menuItems = computed(() => [
        {
            label: 'Editar',
            icon: 'mdi:pencil-outline',
            actionKey: 'edit',
            action: () => handleEdit()
        },
        {
            label: 'Configurar',
            icon: 'lucide:settings',
            actionKey: 'configure',
            action: () => handleConfigure()
        },
        {
            label: 'Re-detectar identidade',
            icon: 'mdi:refresh',
            actionKey: 'redetect',
            action: () => handleRedetect()
        },
        {
            label: 'Remover do MaxTerminal',
            icon: 'mdi:trash-can-outline',
            actionKey: 'remove',
            danger: true,
            action: () => handleRemove()
        }
    ]);

    function toggleMenu(event?: MouseEvent): void {
        popoverMenuRef.value?.toggle(event);
    }

    function handleVisibilityToggle(event: MouseEvent): void {
        event.stopPropagation();
        if (props.disabled || props.pending) {
            return;
        }
        if (props.hidden) {
            emit('show', props.project.id);
        } else {
            emit('hide', props.project.id);
        }
    }

    function handleEdit(): void {
        popoverMenuRef.value?.hide();
        emit('edit', props.project.id);
    }

    function handleConfigure(): void {
        popoverMenuRef.value?.hide();
        emit('configure', props.project.id);
    }

    function handleRedetect(): void {
        popoverMenuRef.value?.hide();
        emit('redetect', props.project.id);
    }

    function handleRemove(): void {
        popoverMenuRef.value?.hide();
        emit('remove', props.project.id);
    }
</script>

<style lang="scss" scoped>
    @use '@/styles/params' as *;

    .project-item {
        position: relative;
        display: flex;
        align-items: stretch;
        gap: 0.25rem;
        border-radius: 0.5rem;
        border: 1px solid transparent;
        background: transparent;
        transition: background-color 120ms ease, border-color 120ms ease;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
        }
    }

    .project-item--selected {
        background: var(--mt-ui-surface-elevated, var(--background-50));
        border-color: var(--mt-project-color, var(--max-primary-500));
    }

    .project-item--missing {
        opacity: 0.82;
        border-color: var(--max-warning-600);
    }

    .project-item__main {
        flex: 1;
        min-width: 0;
        display: flex;
        align-items: flex-start;
        gap: 0.625rem;
        padding: 0.5rem 0.625rem;
        border: none;
        background: transparent;
        color: var(--mt-ui-text, var(--background-800));
        text-align: left;
        cursor: pointer;
        border-radius: 0.5rem;

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-item__avatar {
        width: 2rem;
        height: 2rem;
        border-radius: 0.5rem;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex-shrink: 0;
        background: color-mix(in srgb, var(--mt-project-color, #14B8A6) 15%, transparent);
        color: var(--mt-project-color, #14B8A6);
        border: 1px solid color-mix(in srgb, var(--mt-project-color, #14B8A6) 35%, transparent);
    }

    .project-item__content {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 0.1875rem;
    }

    .project-item__header {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        min-width: 0;
    }

    .project-item__title {
        font-size: 0.875rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .project-item__missing-badge {
        font-size: 0.6875rem;
        font-weight: 500;
        padding: 0.0625rem 0.375rem;
        border-radius: 9999px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--max-warning-400);
        border: 1px solid var(--max-warning-500);
        white-space: nowrap;
    }

    .project-item__path {
        font-size: 0.75rem;
        color: var(--mt-ui-text-muted, var(--background-400));
        font-family: var(--mt-font-mono);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .project-item__actions {
        position: relative;
        display: flex;
        align-items: center;
        gap: 0.25rem;
        padding-right: 0.375rem;
    }

    :deep(.project-item__visibility-btn) {
        padding: 0.1875rem 0.375rem !important;
        min-height: 1.5rem;
        font-size: 0.6875rem !important;
        font-weight: 500;
        color: var(--background-300) !important;
        background: transparent !important;
        border: 1px solid transparent !important;
        border-radius: 0.375rem;
        cursor: pointer;
        white-space: nowrap;

        &:hover:not(:disabled) {
            background: var(--background-600) !important;
            color: var(--background-50) !important;
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-item__menu-trigger {
        width: 1.75rem;
        height: 1.75rem;
        border-radius: 0.375rem;
        border: none;
        background: transparent;
        color: var(--mt-ui-text-muted, var(--background-400));
        display: inline-flex;
        align-items: center;
        justify-content: center;
        cursor: pointer;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
            color: var(--mt-ui-text-strong, var(--background-950));
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-item__menu {
        position: absolute;
        top: calc(100% + 0.25rem);
        right: 0.25rem;
        z-index: 30;
        min-width: 13rem;
        padding: 0.25rem;
        border-radius: 0.5rem;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        border: 1px solid var(--mt-ui-border, var(--background-100));
        box-shadow: 0 8px 24px rgb(0 0 0 / 45%);
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
    }

    .project-item__menu-btn {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        width: 100%;
        padding: 0.4375rem 0.625rem;
        border: none;
        border-radius: 0.375rem;
        background: transparent;
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.8125rem;
        text-align: left;
        cursor: pointer;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-item__menu-btn--danger {
        color: var(--max-danger-400);

        &:hover {
            background: color-mix(in srgb, var(--max-danger-500) 15%, var(--mt-ui-surface-hover, var(--background-75)));
        }
    }
</style>
