<template>
    <span class="highlighted-text">
        <template v-for="(segment, index) in safeSegments" :key="index">
            <mark v-if="segment.matched" class="highlighted-text__mark">{{ segment.text }}</mark>
            <template v-else>{{ segment.text }}</template>
        </template>
    </span>
</template>

<script setup lang="ts">
    import { toRef } from 'vue';
    import { useHighlightSegments } from '@/composables/search/useHighlightSegments';
    import type { HighlightSegment } from '@/types/bindings';

    export interface HighlightedTextProps {
        segments: readonly HighlightSegment[] | null | undefined;
    }

    const props = defineProps<HighlightedTextProps>();

    const { safeSegments } = useHighlightSegments(toRef(props, 'segments'));
</script>

<style lang="scss" scoped>
    .highlighted-text {
        white-space: pre-wrap;
        word-break: break-word;
    }

    .highlighted-text__mark {
        padding: 0 2px;
        border-radius: 3px;
        background: var(--mt-ui-highlight-bg, color-mix(in srgb, var(--max-primary-500) 24%, transparent));
        color: var(--mt-ui-highlight-text, var(--max-primary-300));
        font-weight: 600;
    }
</style>
