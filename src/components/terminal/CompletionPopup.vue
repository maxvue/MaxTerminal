<template>
    <div :id="listboxId" class="completion-popup" role="listbox" :aria-label="title">
        <div class="popup-header">
            <span class="popup-title">{{ title }}</span>
            <span v-if="stageLabel" class="popup-stage">{{ stageLabel }}</span>
        </div>
        <div v-if="loading && items.length === 0" class="popup-state popup-loading" role="status">
            Carregando sugestões...
        </div>
        <div v-else-if="error" class="popup-state popup-error" role="alert">
            {{ error }}
        </div>
        <div v-else-if="items.length === 0" class="popup-state popup-empty" role="status">
            {{ emptyMessage }}
        </div>
        <div v-else ref="scrollContainerRef" class="popup-list" @scroll="onListScroll">
            <div v-if="isVirtualized" class="virtual-spacer" :style="{ height: `${totalVirtualHeight}px` }">
                <div class="virtual-window" :style="{ transform: `translateY(${virtualOffsetY}px)` }">
                    <div v-for="entry in virtualVisibleEntries" :key="entry.item.id" class="popup-entry">
                        <slot name="item" :item="entry.item" :active="entry.index === activeIndex">
                            <CompletionItemView :id="itemId(entry.index)" :item="entry.item" :active="entry.index === activeIndex" @hover="emit('hover', entry.index)" @select="emit('select', $event)" />
                        </slot>
                    </div>
                </div>
            </div>
            <template v-else>
                <template v-for="group in groupedRegularEntries" :key="group.sectionId">
                    <div v-if="group.sectionLabel" class="popup-section-header">
                        {{ group.sectionLabel }}
                    </div>
                    <div v-for="entry in group.entries" :key="entry.item.id" class="popup-entry">
                        <slot name="item" :item="entry.item" :active="entry.index === activeIndex">
                            <CompletionItemView :id="itemId(entry.index)" :item="entry.item" :active="entry.index === activeIndex" @hover="emit('hover', entry.index)" @select="emit('select', $event)" />
                        </slot>
                    </div>
                </template>
            </template>
        </div>
        <div v-if="!isVirtualized && pinnedEntries.length > 0" class="popup-pinned-footer">
            <div v-for="entry in pinnedEntries" :key="entry.item.id" class="popup-entry">
                <slot name="item" :item="entry.item" :active="entry.index === activeIndex">
                    <CompletionItemView :id="itemId(entry.index)" :item="entry.item" :active="entry.index === activeIndex" @hover="emit('hover', entry.index)" @select="emit('select', $event)" />
                </slot>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import type { CompletionItem, CompletionSection } from '@/types/terminal';
    import CompletionItemView from './CompletionItem.vue';

    export interface CompletionPopupProps {
        listboxId?: string;
        items: CompletionItem[];
        sections?: CompletionSection[];
        activeIndex: number;
        loading: boolean;
        error: string | null;
        title: string;
        stageLabel?: string | undefined;
        emptyMessage?: string | undefined;
    }

    const props = withDefaults(defineProps<CompletionPopupProps>(), {
        listboxId: 'mt-completion-listbox',
        sections: () => [],
        stageLabel: undefined,
        emptyMessage: 'Nenhum resultado encontrado'
    });

    const emit = defineEmits<{
        select: [item: CompletionItem];
        hover: [index: number];
    }>();

    const VIRTUAL_THRESHOLD = 200;
    const ROW_HEIGHT = 32;
    const VIEWPORT_ROWS = 10;
    const BUFFER_ROWS = 4;

    const scrollContainerRef = ref<HTMLElement | null>(null);
    const scrollTop = ref<number>(0);

    const isVirtualized = computed<boolean>(() => props.items.length > VIRTUAL_THRESHOLD);
    const totalVirtualHeight = computed<number>(() => props.items.length * ROW_HEIGHT);

    const virtualStartIndex = computed<number>(() => {
        if (!isVirtualized.value) {
            return 0;
        }
        const rawStart = Math.floor(scrollTop.value / ROW_HEIGHT) - BUFFER_ROWS;
        return Math.max(0, Math.min(rawStart, Math.max(0, props.items.length - VIEWPORT_ROWS)));
    });

    const virtualEndIndex = computed<number>(() => {
        if (!isVirtualized.value) {
            return props.items.length;
        }
        return Math.min(props.items.length, virtualStartIndex.value + VIEWPORT_ROWS + BUFFER_ROWS * 2);
    });

    const virtualOffsetY = computed<number>(() => virtualStartIndex.value * ROW_HEIGHT);

    const virtualVisibleEntries = computed(() => {
        const slice: Array<{ item: CompletionItem; index: number }> = [];
        for (let i = virtualStartIndex.value; i < virtualEndIndex.value; i++) {
            const it = props.items[i];
            if (it) {
                slice.push({ item: it, index: i });
            }
        }
        return slice;
    });

    watch(
        () => props.activeIndex,
        (nextIdx) => {
            if (!isVirtualized.value || nextIdx < 0) {
                return;
            }
            const itemTop = nextIdx * ROW_HEIGHT;
            const itemBottom = itemTop + ROW_HEIGHT;
            const viewHeight = VIEWPORT_ROWS * ROW_HEIGHT;
            if (itemTop < scrollTop.value) {
                scrollTop.value = itemTop;
            } else if (itemBottom > scrollTop.value + viewHeight) {
                scrollTop.value = itemBottom - viewHeight;
            }
        }
    );

    const sectionLabelMap = computed(() => {
        const map = new Map<string, string>();
        for (const sec of props.sections ?? []) {
            map.set(sec.id, sec.label);
        }
        return map;
    });

    const groupedRegularEntries = computed(() => {
        const groups: Array<{
            sectionId: string;
            sectionLabel: string | null;
            entries: Array<{ item: CompletionItem; index: number }>;
        }> = [];

        for (let i = 0; i < props.items.length; i++) {
            const item = props.items[i]!;
            if (item.pinned === 'bottom') {
                continue;
            }
            const secId = item.section ?? '__default__';
            const secLabel = item.section ? (sectionLabelMap.value.get(item.section) ?? item.section) : null;
            const lastGroup = groups[groups.length - 1];
            if (!lastGroup || lastGroup.sectionId !== secId) {
                groups.push({
                    sectionId: secId,
                    sectionLabel: secLabel,
                    entries: [{ item, index: i }]
                });
            } else {
                lastGroup.entries.push({ item, index: i });
            }
        }

        return groups;
    });

    const pinnedEntries = computed(() => {
        const list: Array<{ item: CompletionItem; index: number }> = [];
        for (let i = 0; i < props.items.length; i++) {
            const item = props.items[i]!;
            if (item.pinned === 'bottom') {
                list.push({ item, index: i });
            }
        }
        return list;
    });

    function itemId(index: number): string {
        return `${props.listboxId}-item-${index}`;
    }

    function onListScroll(event: Event): void {
        const target = event.target as HTMLElement | null;
        if (target) {
            scrollTop.value = target.scrollTop;
        }
    }
</script>

<style lang="scss" scoped>
    .completion-popup {
        display: flex;
        flex-direction: column;
        max-height: 340px;
        background: var(--background-900);
        border: 1px solid var(--background-700);
        border-radius: 8px;
        box-shadow: 0 8px 24px rgb(0 0 0 / 45%);
        overflow: hidden;
    }

    .popup-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 6px 10px;
        background: var(--background-800);
        border-bottom: 1px solid var(--background-700);
        font-size: 11px;
    }

    .popup-title {
        font-weight: 600;
        color: var(--background-100);
    }

    .popup-stage {
        padding: 1px 6px;
        background: var(--background-900);
        border: 1px solid var(--max-primary-600);
        border-radius: 4px;
        color: var(--max-primary-300);
        font-family: var(--mt-font-mono);
    }

    .popup-state {
        padding: 12px;
        font-size: 12px;
        text-align: center;
        color: var(--background-300);
    }

    .popup-error {
        color: var(--max-danger-400);
    }

    .popup-list {
        flex: 1;
        overflow-y: auto;
        padding: 4px;
    }

    .popup-section-header {
        padding: 4px 8px 2px;
        color: var(--background-400);
        font-size: 10px;
        font-weight: 600;
        text-transform: uppercase;
        letter-spacing: 0.04em;
    }

    .popup-pinned-footer {
        padding: 4px;
        background: var(--background-950);
        border-top: 1px solid var(--background-700);
    }

    .virtual-spacer {
        position: relative;
        width: 100%;
    }

    .virtual-window {
        position: absolute;
        top: 0;
        left: 0;
        right: 0;
    }
</style>
