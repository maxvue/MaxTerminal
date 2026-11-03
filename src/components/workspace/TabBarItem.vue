<template>
    <div class="tab-bar-item" :class="{ 'is-active': props.active, 'is-dragging': props.dragging }" role="tab" :aria-selected="props.active" tabindex="0" draggable="true" @click="emit('select', props.tab.id)" @keydown.enter.prevent="emit('select', props.tab.id)" @keydown.space.prevent="emit('select', props.tab.id)" @dragstart="emit('drag-start', props.tab.id)" @dragover.prevent="emit('drag-over', props.tab.id)" @drop.prevent="emit('drop', props.tab.id)" @dragend="emit('drag-end')">
        <MaxIcon :icon="props.tab.icon || 'lucide:terminal'" class="tab-bar-item__icon" aria-hidden="true" />
        <span class="tab-bar-item__title" :title="props.tab.title">{{ props.tab.title }}</span>
        <span v-if="paneCount > 1" class="tab-bar-item__panes-badge" :title="`${paneCount} painéis divididos`">{{ paneCount }}</span>
        <TabStatusIndicator :status="effectiveStatus" :seen="props.active" :show-label="false" size="sm" />
        <slot name="indicators" :tab="props.tab" />
        <MaxIconButton icon="lucide:x" :label="`Fechar aba ${props.tab.title}`" variant="ghost" class="tab-bar-item__close" title="Fechar aba" @click.stop="emit('close', props.tab.id)" />
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxIcon, MaxIconButton } from '@maxvue/max-components-ui';
    import TabStatusIndicator from '@/components/shell/TabStatusIndicator.vue';
    import { useTabStatusStore } from '@/stores/tabStatus';
    import type { TabId, TabStatus, TabView_Serialize } from '@/types/bindings';

    export interface TabBarItemProps {
        tab: TabView_Serialize;
        active: boolean;
        dragging?: boolean;
    }

    const props = withDefaults(defineProps<TabBarItemProps>(), {
        dragging: false
    });

    const emit = defineEmits<{
        select: [tabId: TabId];
        close: [tabId: TabId];
        'drag-start': [tabId: TabId];
        'drag-over': [tabId: TabId];
        drop: [tabId: TabId];
        'drag-end': [];
    }>();

    const tabStatusStore = useTabStatusStore();

    const paneCount = computed<number>(() => props.tab.panes.length);

    const effectiveStatus = computed<TabStatus>(() => {
        const snap = tabStatusStore.snapshotOf(props.tab.id);
        if (snap) {
            return snap.status;
        }
        return props.tab.continuous ? 'continuous' : 'idle';
    });
</script>

<style lang="scss" scoped>
    .tab-bar-item {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        height: 2.125rem;
        padding: 0 0.625rem;
        border-radius: 0.375rem 0.375rem 0 0;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-bottom: none;
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.7813rem;
        font-weight: 500;
        cursor: pointer;
        user-select: none;
        transition: background-color 120ms ease, color 120ms ease, border-color 120ms ease;
        max-width: 14rem;
        flex-shrink: 0;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
            color: var(--mt-ui-text, var(--background-800));
        }

        &.is-active {
            background: var(--mt-ui-surface, var(--background-0));
            color: var(--mt-ui-text-strong, var(--background-950));
            border-color: var(--max-primary-500);
            font-weight: 600;
        }

        &.is-dragging {
            opacity: 0.55;
        }
    }

    .tab-bar-item__icon {
        font-size: 0.9375rem;
        color: var(--max-primary-400);
        flex-shrink: 0;
    }

    .tab-bar-item__title {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        min-width: 2.5rem;
    }

    .tab-bar-item__panes-badge {
        padding: 0 0.3125rem;
        border-radius: 999px;
        font-size: 0.65rem;
        font-weight: 600;
        background: var(--mt-ui-surface-hover, var(--background-75));
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .tab-bar-item__close {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.125rem;
        height: 1.125rem;
        padding: 0;
        border: none;
        border-radius: 0.25rem;
        background: transparent;
        color: var(--mt-ui-text-muted, var(--background-400));
        cursor: pointer;
        flex-shrink: 0;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
            color: var(--max-danger-400);
        }
    }

    .tab-bar-item__close-icon {
        font-size: 0.75rem;
    }
</style>
