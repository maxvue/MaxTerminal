<template>
    <div class="theme-swatch" :class="{ 'is-active': active, 'is-previewing': previewing }" :style="cardStyle" role="button" tabindex="0" :aria-pressed="active" :aria-label="`Selecionar tema ${summary.name}`" @click="emit('select', summary.id)" @keydown.enter.prevent="emit('select', summary.id)" @keydown.space.prevent="emit('select', summary.id)" @mouseenter="emit('hover', summary.id)" @mouseleave="emit('leave')" @focus="emit('hover', summary.id)" @blur="emit('leave')">
        <div class="theme-swatch__preview" :style="previewBoxStyle">
            <div class="theme-swatch__header">
                <span class="theme-swatch__dot" :style="{ backgroundColor: summary.accentSwatches[1] }" />
                <span class="theme-swatch__dot" :style="{ backgroundColor: summary.accentSwatches[3] }" />
                <span class="theme-swatch__dot" :style="{ backgroundColor: summary.accentSwatches[2] }" />
            </div>
            <div class="theme-swatch__sample">
                <span class="theme-swatch__prompt" :style="{ color: summary.accentSwatches[2] }">❯</span>
                <span class="theme-swatch__cmd" :style="{ color: summary.foreground }">maxterminal</span>
                <span class="theme-swatch__cursor" :style="{ backgroundColor: summary.cursor }" />
            </div>
            <div class="theme-swatch__bars">
                <span v-for="(swatch, idx) in summary.accentSwatches" :key="idx" class="theme-swatch__bar" :style="{ backgroundColor: swatch }" />
            </div>
        </div>
        <div class="theme-swatch__footer">
            <div class="theme-swatch__meta">
                <span class="theme-swatch__name">{{ summary.name }}</span>
                <MaxTag :value="polarityLabel" severity="secondary" :no-uppercase="true" class="theme-swatch__badge" />
            </div>
            <div class="theme-swatch__actions" @click.stop @keydown.enter.stop @keydown.space.stop>
                <MaxTag v-if="active" value="Ativo" severity="success" :no-uppercase="true" class="theme-swatch__active-tag" />
                <MaxIconButton icon="lucide:pencil" :disabled="summary.origin === 'builtin'" :title="editTitle" :aria-label="editAriaLabel" size="small" variant="text" class="theme-swatch__icon-btn" @click="onEditClick" />
                <MaxIconButton icon="lucide:copy" title="Duplicar tema" :aria-label="`Duplicar tema ${summary.name}`" size="small" variant="text" class="theme-swatch__icon-btn" @click="emit('duplicate', summary.id)" />
                <MaxIconButton v-if="summary.origin !== 'builtin'" icon="lucide:download" title="Exportar tema" :aria-label="`Exportar tema ${summary.name}`" size="small" variant="text" class="theme-swatch__icon-btn" @click="emit('export', summary.id)" />
                <MaxIconButton v-if="summary.origin !== 'builtin'" icon="lucide:trash-2" title="Excluir tema" :aria-label="`Excluir tema ${summary.name}`" size="small" variant="text" severity="danger" class="theme-swatch__icon-btn theme-swatch__icon-btn--danger" @click="emit('delete', summary.id)" />
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxIconButton, MaxTag } from '@maxvue/max-components-ui';
    import type { ThemeId, ThemeSummary } from '@/types/theme';

    export interface ThemeSwatchProps {
        summary: ThemeSummary;
        active: boolean;
        previewing?: boolean;
    }

    const props = withDefaults(defineProps<ThemeSwatchProps>(), {
        previewing: false
    });

    const emit = defineEmits<{
        select: [id: ThemeId];
        hover: [id: ThemeId];
        leave: [];
        edit: [id: ThemeId];
        duplicate: [id: ThemeId];
        delete: [id: ThemeId];
        export: [id: ThemeId];
    }>();

    const polarityLabel = computed(() =>
        props.summary.polarity === 'dark' ? 'Escuro' : 'Claro'
    );

    const editTitle = computed(() =>
        props.summary.origin === 'builtin'
            ? 'Tema nativo somente leitura — duplique para editar'
            : 'Editar tema'
    );

    const editAriaLabel = computed(() =>
        props.summary.origin === 'builtin'
            ? `Tema nativo ${props.summary.name} somente leitura — duplique para editar`
            : `Editar tema ${props.summary.name}`
    );

    function onEditClick(): void {
        if (props.summary.origin === 'builtin') {
            return;
        }
        emit('edit', props.summary.id);
    }

    const previewBoxStyle = computed(() => ({
        backgroundColor: props.summary.background,
        color: props.summary.foreground
    }));

    const cardStyle = computed(() => ({
        '--swatch-accent': props.summary.accentSwatches[4]
    }));
</script>

<style lang="scss" scoped>
    .theme-swatch {
        display: flex;
        flex-direction: column;
        border-radius: 0.5rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        overflow: hidden;
        cursor: pointer;
        transition: border-color 0.15s ease, transform 0.15s ease;

        &:hover,
        &.is-previewing {
            border-color: var(--max-primary-400);
        }

        &.is-active {
            border-color: var(--max-primary-500);
            box-shadow: 0 0 0 1px var(--max-primary-500);
        }
    }

    .theme-swatch__preview {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
        padding: 0.625rem;
        font-family: var(--mt-font-mono);
        font-size: 0.75rem;
    }

    .theme-swatch__header {
        display: flex;
        align-items: center;
        gap: 0.25rem;
    }

    .theme-swatch__dot {
        width: 0.4375rem;
        height: 0.4375rem;
        border-radius: 9999px;
    }

    .theme-swatch__sample {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.25rem 0;
    }

    .theme-swatch__prompt {
        font-weight: 700;
    }

    .theme-swatch__cursor {
        display: inline-block;
        width: 0.4375rem;
        height: 0.875rem;
    }

    .theme-swatch__bars {
        display: grid;
        grid-template-columns: repeat(8, 1fr);
        gap: 0.1875rem;
        height: 0.3125rem;
    }

    .theme-swatch__bar {
        border-radius: 0.125rem;
    }

    .theme-swatch__footer {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.5rem 0.625rem;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .theme-swatch__meta {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        min-width: 0;
    }

    .theme-swatch__name {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .theme-swatch__badge {
        padding: 0.0625rem 0.375rem;
        border-radius: 0.25rem;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.6875rem;
    }

    .theme-swatch__actions {
        display: flex;
        align-items: center;
        gap: 0.25rem;
    }

    .theme-swatch__active-tag {
        padding: 0.0625rem 0.375rem;
        border-radius: 0.25rem;
        background: color-mix(in srgb, var(--max-primary-500) 20%, transparent);
        color: var(--max-primary-400);
        font-size: 0.6875rem;
        font-weight: 600;
    }

    .theme-swatch__icon-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        border: none;
        border-radius: 0.25rem;
        background: transparent;
        color: var(--mt-ui-text-secondary, var(--background-600));
        cursor: pointer;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
            color: var(--mt-ui-text-strong, var(--background-950));
        }

        &--danger:hover {
            color: var(--max-danger-400);
        }
    }
</style>
