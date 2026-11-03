<template>
    <div class="project-overrides">
        <p class="project-overrides__hint">
            Associe opcionalmente um esquema de cores específico para cada projeto. Ao selecionar o projeto na barra lateral, o MaxTerminal aplicará automaticamente o tema configurado.
        </p>

        <div v-if="props.projects.length === 0" class="project-overrides__empty">
            Nenhum projeto cadastrado até o momento.
        </div>

        <div v-else class="project-overrides__list">
            <div v-for="proj in props.projects" :key="proj.id" class="project-overrides__row">
                <div class="project-overrides__project">
                    <MaxIcon :icon="proj.icon" class="project-overrides__icon" />
                    <span class="project-overrides__title">{{ proj.title }}</span>
                </div>
                <MaxInputSelect :model-value="props.overrides[proj.id] ?? ''" :options="themeOptions" option-label="label" option-value="value" option-name="label" :disabled="props.disabled" :aria-label="`Tema para o projeto ${proj.title}`" class="project-overrides__select" @change="onProjectThemeChange(proj.id, $event)" />
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import MaxInputSelect from '@maxvue/max-components-ui/components/MaxInputSelect';
    import type { Project_Serialize } from '@/types/bindings';
    import type { ThemeId, ThemeScope, ThemeSummary } from '@/types/theme';

    export interface ProjectThemeOverridesProps {
        projects: Project_Serialize[];
        themes: ThemeSummary[];
        overrides: Partial<Record<string, ThemeId>>;
        disabled?: boolean;
    }

    const props = withDefaults(defineProps<ProjectThemeOverridesProps>(), {
        disabled: false
    });

    const emit = defineEmits<{
        change: [id: ThemeId, scope: ThemeScope];
    }>();

    const themeOptions = computed(() => [
        { label: 'Herdar tema global', value: '' },
        ...props.themes.map((t) => ({
            label: `${t.name} (${t.polarity === 'dark' ? 'Escuro' : 'Claro'})`,
            value: t.id
        }))
    ]);

    function onProjectThemeChange(projectId: string, val: unknown): void {
        const themeId = typeof val === 'string' ? val : '';
        if (!themeId) {
            emit('change', 'max-navy-teal', { kind: 'clear_project', projectId });
        } else {
            emit('change', themeId, { kind: 'project', projectId });
        }
    }
</script>

<style lang="scss" scoped>
    .project-overrides {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
    }

    .project-overrides__hint {
        margin: 0;
        font-size: 0.75rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .project-overrides__empty {
        padding: 0.75rem;
        border-radius: 0.375rem;
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.8125rem;
    }

    .project-overrides__list {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .project-overrides__row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 0.5rem 0.625rem;
        border-radius: 0.375rem;
        background: var(--mt-ui-surface, var(--background-0));
        border: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .project-overrides__project {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        min-width: 0;
    }

    .project-overrides__icon {
        color: var(--max-primary-400);
    }

    .project-overrides__title {
        font-size: 0.8125rem;
        font-weight: 500;
        color: var(--mt-ui-text-strong, var(--background-950));
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .project-overrides__select {
        min-width: 13rem;
    }
</style>
