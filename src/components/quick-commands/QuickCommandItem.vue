<template>
    <div class="qc-item" :class="{ 'qc-item--active': active, 'qc-item--disabled': item.disabled, 'qc-item--action': item.kind === 'action' }" data-testid="quick-command-item">
        <span class="qc-item__icon">
            <MaxIcon :icon="item.icon ?? 'mdi:console-line'" />
        </span>
        <div class="qc-item__main">
            <div class="qc-item__header">
                <span class="qc-item__label">
                    <template v-for="(seg, idx) in labelSegments" :key="idx">
                        <mark v-if="seg.highlighted" class="qc-item__mark">{{ seg.text }}</mark>
                        <span v-else>{{ seg.text }}</span>
                    </template>
                </span>
                <span v-if="shortcutBadge" class="qc-item__badge qc-item__badge--shortcut">{{ shortcutBadge }}</span>
                <span v-if="acceptsValueBadge" class="qc-item__badge qc-item__badge--value">aceita valor</span>
                <span v-if="isGlobalBadge" class="qc-item__badge qc-item__badge--global">global</span>
                <span v-if="item.detail && !shortcutBadge" class="qc-item__detail">{{ item.detail }}</span>
            </div>
            <p v-if="item.description" class="qc-item__description">{{ item.description }}</p>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import type { CompletionItem } from '@/types/terminal';

    export interface QuickCommandItemProps {
        item: CompletionItem;
        active: boolean;
        highlightIndices?: number[] | undefined;
    }

    const props = defineProps<QuickCommandItemProps>();

    interface LabelSegment {
        text: string;
        highlighted: boolean;
    }

    const effectiveIndices = computed<Set<number>>(() => {
        if (props.highlightIndices && props.highlightIndices.length > 0) {
            return new Set(props.highlightIndices);
        }
        const payload = props.item.payload as { highlightIndices?: number[] } | undefined;
        if (payload?.highlightIndices && Array.isArray(payload.highlightIndices)) {
            return new Set(payload.highlightIndices);
        }
        return new Set<number>();
    });

    const labelSegments = computed<LabelSegment[]>(() => {
        const text = props.item.label ?? '';
        const idxSet = effectiveIndices.value;
        if (idxSet.size === 0 || text.length === 0) {
            return [{ text, highlighted: false }];
        }

        const out: LabelSegment[] = [];
        let currentText = '';
        let currentHighlighted = idxSet.has(0);

        for (let i = 0; i < text.length; i += 1) {
            const hl = idxSet.has(i);
            if (hl !== currentHighlighted && currentText.length > 0) {
                out.push({ text: currentText, highlighted: currentHighlighted });
                currentText = '';
            }
            currentHighlighted = hl;
            currentText += text[i] ?? '';
        }
        if (currentText.length > 0) {
            out.push({ text: currentText, highlighted: currentHighlighted });
        }
        return out;
    });

    const payloadMeta = computed(() => {
        return (props.item.payload ?? {}) as {
            acceptValue?: boolean | null;
            isGlobal?: boolean;
            shortcut?: string | null;
        };
    });

    const acceptsValueBadge = computed<boolean>(() => payloadMeta.value.acceptValue === true);
    const isGlobalBadge = computed<boolean>(() => payloadMeta.value.isGlobal === true);
    const shortcutBadge = computed<string | null>(() => {
        const sc = payloadMeta.value.shortcut;
        if (!sc) {
            return null;
        }
        return sc.startsWith('-') ? sc : `-${sc}`;
    });
</script>

<style lang="scss" scoped>
    .qc-item {
        display: flex;
        align-items: flex-start;
        gap: 0.5rem;
        padding: 0.375rem 0.625rem;
        border-radius: 0.375rem;
        color: var(--background-100);
        transition: background-color 120ms ease;

        &--active {
            background: var(--background-800);
        }

        &--disabled {
            opacity: 0.55;
        }

        &--action {
            color: var(--max-primary-400);
            border-top: 1px solid var(--background-800);
        }

        &__icon {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            margin-top: 0.125rem;
            color: var(--max-primary-400);
            flex-shrink: 0;
        }

        &__main {
            display: flex;
            flex-direction: column;
            gap: 0.125rem;
            min-width: 0;
            flex: 1;
        }

        &__header {
            display: flex;
            align-items: center;
            flex-wrap: wrap;
            gap: 0.375rem;
        }

        &__label {
            font-family: var(--mt-font-mono);
            font-size: 0.8125rem;
            font-weight: 600;
            color: var(--background-50);
        }

        &__mark {
            background: transparent;
            color: var(--max-primary-400);
            font-weight: 700;
            text-decoration: underline;
        }

        &__badge {
            font-size: 0.6875rem;
            padding: 0.0625rem 0.375rem;
            border-radius: 0.25rem;
            font-family: var(--mt-font-mono);

            &--shortcut {
                background: var(--background-800);
                color: var(--max-info-400);
            }

            &--value {
                background: var(--background-800);
                color: var(--max-primary-400);
            }

            &--global {
                background: var(--background-800);
                color: var(--max-warning-400);
            }
        }

        &__detail {
            font-size: 0.6875rem;
            color: var(--background-400);
            font-family: var(--mt-font-mono);
        }

        &__description {
            margin: 0;
            font-size: 0.75rem;
            color: var(--background-300);
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }
    }
</style>
