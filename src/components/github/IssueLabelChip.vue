<template>
    <span class="issue-label-chip" :class="{ 'issue-label-chip--canonical': Boolean(props.label.canonicalGroup), 'issue-label-chip--compact': props.compact }" :style="chipStyle" :title="chipTooltip">
        <span v-if="groupIndicator" class="issue-label-chip__dot" :aria-label="groupIndicator" />
        <span class="issue-label-chip__name">{{ props.label.name }}</span>
    </span>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { computeContrastTextColor, normalizeHexColor } from '@/constants/githubLabels';
    import type { RepoLabel } from '@/types/bindings';

    export interface IssueLabelChipProps {
        label: RepoLabel;
        compact?: boolean;
    }

    const props = withDefaults(defineProps<IssueLabelChipProps>(), {
        compact: false
    });

    const normalizedHex = computed<string>(() => normalizeHexColor(props.label.colorHex));
    const textColor = computed<string>(() => computeContrastTextColor(normalizedHex.value));

    const chipStyle = computed<Record<string, string>>(() => ({
        backgroundColor: `#${normalizedHex.value}`,
        color: textColor.value
    }));

    const groupIndicator = computed<string | null>(() => {
        if (props.label.canonicalGroup === 'state') {
            return 'Estado';
        }
        if (props.label.canonicalGroup === 'closure') {
            return 'Encerramento';
        }
        if (props.label.canonicalGroup === 'category') {
            return 'Categoria';
        }
        return null;
    });

    const chipTooltip = computed<string>(() => {
        if (groupIndicator.value) {
            return `${props.label.name} (${groupIndicator.value})`;
        }
        return props.label.name;
    });
</script>

<style lang="scss" scoped>
    .issue-label-chip {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.125rem 0.5rem;
        border-radius: 9999px;
        font-size: 0.6875rem;
        font-weight: 600;
        line-height: 1.25;
        white-space: nowrap;
        border: 1px solid rgb(255 255 255 / 12%);

        &--compact {
            padding: 0.0625rem 0.375rem;
            font-size: 0.625rem;
        }
    }

    .issue-label-chip__dot {
        width: 0.375rem;
        height: 0.375rem;
        border-radius: 50%;
        background: currentcolor;
        opacity: 0.75;
    }

    .issue-label-chip__name {
        overflow: hidden;
        text-overflow: ellipsis;
        max-width: 12rem;
    }
</style>
