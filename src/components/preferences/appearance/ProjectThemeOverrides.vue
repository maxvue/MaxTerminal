<template>
    <div class="project-overrides">
        <p class="project-overrides__hint">
            Associe opcionalmente um esquema de cores específico para cada projeto. Ao selecionar o projeto na barra lateral, o MaxTerminal aplicará automaticamente o tema configurado.
        </p>

        <div v-if="projects.length === 0" class="project-overrides__empty">
            Nenhum projeto cadastrado até o momento.
        </div>

        <div v-else class="project-overrides__list">
            <div v-for="proj in projects" :key="proj.id" class="project-overrides__row">
                <div class="project-overrides__project">
                    <MaxIcon :icon="proj.icon" class="project-overrides__icon" />
                    <span class="project-overrides__title">{{ proj.title }}</span>
                </div>
                <select class="project-overrides__select" :value="overrides[proj.id] ?? ''" :disabled="disabled" :aria-label="`Tema para o projeto ${proj.title}`" @change="onProjectThemeChange(proj.id, $event)">
                    <option value="">Herdar tema global</option>
                    <option v-for="theme in themes" :key="theme.id" :value="theme.id">
                        {{ theme.name }} ({{ theme.polarity === 'dark' ? 'Escuro' : 'Claro' }})
                    </option>
                </select>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import type { Project_Serialize } from '@/types/bindings';
    import type { ThemeId, ThemeScope, ThemeSummary } from '@/types/theme';

    export interface ProjectThemeOverridesProps {
        projects: Project_Serialize[];
        themes: ThemeSummary[];
        overrides: Partial<Record<string, ThemeId>>;
        disabled?: boolean;
    }

    withDefaults(defineProps<ProjectThemeOverridesProps>(), {
        disabled: false
    });

    const emit = defineEmits<{
        change: [id: ThemeId, scope: ThemeScope];
    }>();

    function onProjectThemeChange(projectId: string, ev: Event): void {
        const val = (ev.target as HTMLSelectElement).value;
        if (!val) {
            emit('change', 'max-navy-teal', { kind: 'clear_project', projectId });
        } else {
            emit('change', val, { kind: 'project', projectId });
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
        color: var(--background-300);
    }

    .project-overrides__empty {
        padding: 0.75rem;
        border-radius: 0.375rem;
        background: var(--background-950);
        color: var(--background-400);
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
        background: var(--background-950);
        border: 1px solid var(--background-800);
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
        color: var(--background-100);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .project-overrides__select {
        height: 1.875rem;
        min-width: 13rem;
        padding: 0 0.5rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-900);
        color: var(--background-100);
        font-size: 0.75rem;
    }
</style>
