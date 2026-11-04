<template>
    <section v-if="props.items.length > 0" class="project-discovery" aria-label="Novas pastas encontradas">
        <div class="project-discovery__header">
            <button type="button" class="project-discovery__toggle" :aria-expanded="expanded ? 'true' : 'false'" @click="expanded = !expanded">
                <MaxIcon :icon="expanded ? 'mdi:chevron-down' : 'mdi:chevron-right'" />
                <span>Novas pastas encontradas ({{ props.items.length }})</span>
            </button>
            <MaxButton class="project-discovery__accept-all" @click="acceptAll">
                Adicionar todas
            </MaxButton>
        </div>

        <ul v-if="expanded" class="project-discovery__list">
            <li v-for="item in props.items" :key="item.path" class="project-discovery__item" :style="{ '--project-color': item.identity.color, '--mt-project-color': item.identity.color }">
                <span class="project-discovery__avatar" aria-hidden="true">
                    <MaxIcon :icon="item.identity.icon" />
                </span>
                <div class="project-discovery__info">
                    <span class="project-discovery__title">{{ item.suggestedTitle }}</span>
                    <span class="project-discovery__path" :title="item.path">{{ item.path }}</span>
                </div>
                <div class="project-discovery__actions">
                    <MaxButton severity="success" class="project-discovery__btn project-discovery__btn--accept" @click="emit('accept', [item.path])">
                        Adicionar
                    </MaxButton>
                    <MaxButton severity="secondary" class="project-discovery__btn project-discovery__btn--dismiss" @click="emit('dismiss', [item.path])">
                        Dispensar
                    </MaxButton>
                </div>
            </li>
        </ul>
    </section>
</template>

<script setup lang="ts">
    import { ref } from 'vue';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import type { DiscoveredFolder } from '@/types/bindings';

    export interface ProjectDiscoveryPanelProps {
        items: DiscoveredFolder[];
    }

    const props = defineProps<ProjectDiscoveryPanelProps>();

    const emit = defineEmits<{
        accept: [paths: string[]];
        dismiss: [paths: string[]];
    }>();

    const expanded = ref<boolean>(true);

    function acceptAll(): void {
        const allPaths = props.items.map((item) => item.path);
        if (allPaths.length > 0) {
            emit('accept', allPaths);
        }
    }
</script>

<style lang="scss" scoped>
    @use '@/styles/params' as *;

    .project-discovery {
        border: 1px solid var(--max-info-500);
        border-radius: 0.5rem;
        background: var(--mt-ui-surface-panel, var(--background-25));
        margin: 0.5rem;
        overflow: hidden;
    }

    .project-discovery__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.4375rem 0.625rem;
        background: var(--mt-ui-surface-hover, var(--background-75));
    }

    .project-discovery__toggle {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        border: none;
        background: transparent;
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-discovery__accept-all {
        border: 1px solid var(--max-primary-500);
        background: var(--max-primary-600);
        color: var(--max-primary-content);
        border-radius: 0.375rem;
        padding: 0.1875rem 0.5rem;
        font-size: 0.6875rem;
        font-weight: 600;
        cursor: pointer;

        &:hover {
            background: var(--max-primary-500);
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-discovery__list {
        list-style: none;
        margin: 0;
        padding: 0.375rem;
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        max-height: 11rem;
        overflow-y: auto;
    }

    .project-discovery__item {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.375rem 0.5rem;
        border-radius: 0.375rem;
        background: var(--mt-ui-surface, var(--background-0));
    }

    .project-discovery__avatar {
        width: 1.5rem;
        height: 1.5rem;
        border-radius: 0.375rem;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: color-mix(in srgb, var(--mt-project-color, #14B8A6) 15%, transparent);
        color: var(--mt-project-color, #14B8A6);
        flex-shrink: 0;
    }

    .project-discovery__info {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
    }

    .project-discovery__title {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .project-discovery__path {
        font-size: 0.6875rem;
        color: var(--mt-ui-text-muted, var(--background-400));
        font-family: var(--mt-font-mono);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .project-discovery__actions {
        display: flex;
        align-items: center;
        gap: 0.25rem;
        flex-shrink: 0;
    }

    .project-discovery__btn {
        border-radius: 0.25rem;
        padding: 0.1875rem 0.4375rem;
        font-size: 0.6875rem;
        font-weight: 500;
        cursor: pointer;

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-discovery__btn--accept {
        border: 1px solid var(--max-success-500);
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--max-success-400);

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
        }
    }

    .project-discovery__btn--dismiss {
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: transparent;
        color: var(--mt-ui-text-muted, var(--background-400));

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
        }
    }
</style>
