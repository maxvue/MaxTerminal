<template>
    <MaxEmptyDiv class="project-empty-state" icon="mdi:folder-plus-outline" :icon-size="1.5">
        <template #default>
            <div class="project-empty-state__icon-box">
                <MaxIcon icon="mdi:folder-plus-outline" size="1.5" class="project-empty-state__icon" />
            </div>
            <h3 class="project-empty-state__title">Nenhum projeto cadastrado</h3>
            <p class="project-empty-state__desc">Adicione um projeto individual ou cadastre uma pasta de projetos para descobrir subpastas automaticamente.</p>
            <div class="project-empty-state__actions">
                <MaxButton class="project-empty-state__btn project-empty-state__btn--primary" @click="emit('add-project')">
                    Adicionar projeto
                </MaxButton>
                <MaxButton severity="secondary" class="project-empty-state__btn project-empty-state__btn--secondary" @click="emit('add-root')">
                    Adicionar pasta de projetos
                </MaxButton>
            </div>
        </template>
    </MaxEmptyDiv>
</template>

<script setup lang="ts">
    import { MaxButton, MaxEmptyDiv, MaxIcon } from '@maxvue/max-components-ui';

    const emit = defineEmits<{
        'add-project': [];
        'add-root': [];
    }>();
</script>

<style lang="scss" scoped>
    @use '@/styles/params' as *;

    .project-empty-state {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        text-align: center;
        padding: 1.5rem 1rem;
        gap: 0.625rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .project-empty-state__icon-box {
        width: 2.75rem;
        height: 2.75rem;
        border-radius: 0.75rem;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--max-primary-400);
        border: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .project-empty-state__title {
        margin: 0;
        font-size: 0.875rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .project-empty-state__desc {
        margin: 0;
        font-size: 0.75rem;
        line-height: 1.4;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .project-empty-state__actions {
        display: flex;
        flex-direction: column;
        width: 100%;
        gap: 0.375rem;
        margin-top: 0.25rem;
    }

    .project-empty-state__btn {
        width: 100%;
        padding: 0.4375rem 0.75rem;
        border-radius: 0.375rem;
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-empty-state__btn--primary {
        background: var(--max-primary-600);
        color: var(--max-primary-content);
        border: 1px solid var(--max-primary-500);

        &:hover {
            background: var(--max-primary-500);
        }
    }

    .project-empty-state__btn--secondary {
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        border: 1px solid var(--mt-ui-border, var(--background-100));

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
        }
    }
</style>
