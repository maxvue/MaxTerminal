<template>
    <div :id="id" class="completion-item" :class="{ 'is-active': active, 'is-disabled': Boolean(item.disabled), 'is-action': item.kind === 'action' }" role="option" :aria-selected="active" :aria-disabled="Boolean(item.disabled)" @mouseenter="onMouseEnter" @mousedown.prevent="onSelectClick">
        <MaxIcon v-if="item.icon" :icon="item.icon" class="item-icon" />
        <div class="item-main">
            <span class="item-label">{{ item.label }}</span>
            <span v-if="item.description" class="item-description">{{ item.description }}</span>
        </div>
        <span v-if="item.detail" class="item-detail">{{ item.detail }}</span>
    </div>
</template>

<script setup lang="ts">
    import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
    import type { CompletionItem } from '@/types/terminal';

    export interface CompletionItemProps {
        item: CompletionItem;
        active: boolean;
        id: string;
    }

    const props = defineProps<CompletionItemProps>();

    const emit = defineEmits<{
        select: [item: CompletionItem];
        hover: [];
    }>();

    function onMouseEnter(): void {
        if (!props.item.disabled) {
            emit('hover');
        }
    }

    function onSelectClick(): void {
        if (!props.item.disabled) {
            emit('select', props.item);
        }
    }
</script>

<style lang="scss" scoped>
    .completion-item {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        min-height: 32px;
        padding: 4px 10px;
        border-radius: 4px;
        color: var(--background-100);
        font-size: 12px;
        cursor: pointer;
        user-select: none;

        &.is-active {
            background: var(--max-primary-700);
            color: var(--background-50);
        }

        &.is-disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }

        &.is-action {
            color: var(--max-primary-300);
            font-weight: 500;
        }
    }

    .item-icon {
        flex-shrink: 0;
        font-size: 15px;
        color: var(--max-primary-400);
    }

    .item-main {
        display: flex;
        align-items: baseline;
        gap: 8px;
        flex: 1;
        min-width: 0;
        overflow: hidden;
    }

    .item-label {
        font-family: var(--mt-font-mono);
        white-space: nowrap;
    }

    .item-description {
        color: var(--background-300);
        font-size: 11px;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .item-detail {
        flex-shrink: 0;
        padding: 1px 6px;
        background: var(--background-800);
        border: 1px solid var(--background-700);
        border-radius: 4px;
        color: var(--background-300);
        font-family: var(--mt-font-mono);
        font-size: 11px;
    }
</style>
