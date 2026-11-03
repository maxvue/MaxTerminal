<template>
    <div class="theme-picker">
        <div class="theme-picker__toolbar">
            <div class="theme-picker__filters" role="tablist" aria-label="Filtrar temas por polaridade">
                <button type="button" class="theme-picker__filter-btn" :class="{ 'is-active': filter === 'all' }" @click="filter = 'all'">
                    Todos ({{ themes.length }})
                </button>
                <button type="button" class="theme-picker__filter-btn" :class="{ 'is-active': filter === 'dark' }" @click="filter = 'dark'">
                    Escuros ({{ darkCount }})
                </button>
                <button type="button" class="theme-picker__filter-btn" :class="{ 'is-active': filter === 'light' }" @click="filter = 'light'">
                    Claros ({{ lightCount }})
                </button>
            </div>
            <input v-model="searchQuery" type="search" class="theme-picker__search" placeholder="Filtrar temas pelo nome..." aria-label="Filtrar temas pelo nome">
        </div>

        <div class="theme-picker__grid">
            <ThemeSwatch v-for="item in filteredThemes" :key="item.id" :summary="item" :active="item.id === activeThemeId" :previewing="item.id === previewingId" @select="emit('select', $event)" @hover="onHover" @leave="onLeave" @delete="emit('delete', $event)" @export="emit('export', $event)" />
        </div>

        <p v-if="filteredThemes.length === 0" class="theme-picker__empty">
            Nenhum esquema de cores corresponde ao filtro informado.
        </p>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import ThemeSwatch from '@/components/preferences/appearance/ThemeSwatch.vue';
    import { useThemePreview } from '@/composables/useThemePreview';
    import type { ThemeId, ThemeSummary } from '@/types/theme';

    export interface ThemePickerProps {
        themes: ThemeSummary[];
        activeThemeId: ThemeId;
    }

    const props = defineProps<ThemePickerProps>();

    const emit = defineEmits<{
        select: [id: ThemeId];
        delete: [id: ThemeId];
        export: [id: ThemeId];
    }>();

    const filter = ref<'all' | 'dark' | 'light'>('all');
    const searchQuery = ref('');
    const { previewingId, previewOnHover, stopPreview } = useThemePreview();

    const darkCount = computed(
        () => props.themes.filter((t) => t.polarity === 'dark').length
    );

    const lightCount = computed(
        () => props.themes.filter((t) => t.polarity === 'light').length
    );

    const filteredThemes = computed(() => {
        const q = searchQuery.value.trim().toLowerCase();
        return props.themes.filter((item) => {
            if (filter.value !== 'all' && item.polarity !== filter.value) {
                return false;
            }
            if (q && !item.name.toLowerCase().includes(q) && !item.id.toLowerCase().includes(q)) {
                return false;
            }
            return true;
        });
    });

    function onHover(id: ThemeId): void {
        previewOnHover(id);
    }

    function onLeave(): void {
        stopPreview();
    }
</script>

<style lang="scss" scoped>
    .theme-picker {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
    }

    .theme-picker__toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        flex-wrap: wrap;
    }

    .theme-picker__filters {
        display: inline-flex;
        gap: 0.25rem;
        padding: 0.1875rem;
        border-radius: 0.375rem;
        background: var(--mt-ui-surface, var(--background-0));
        border: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .theme-picker__filter-btn {
        padding: 0.25rem 0.625rem;
        border: none;
        border-radius: 0.25rem;
        background: transparent;
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.75rem;
        font-weight: 500;
        cursor: pointer;

        &:hover {
            color: var(--mt-ui-text-strong, var(--background-950));
        }

        &.is-active {
            background: var(--max-primary-500);
            color: var(--max-primary-content);
            font-weight: 600;
        }
    }

    .theme-picker__search {
        height: 2rem;
        min-width: 14rem;
        padding: 0 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.8125rem;
    }

    .theme-picker__grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(13.5rem, 1fr));
        gap: 0.625rem;
    }

    .theme-picker__empty {
        margin: 0;
        padding: 1rem;
        text-align: center;
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.8125rem;
    }
</style>
