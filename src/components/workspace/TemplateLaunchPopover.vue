<template>
    <div v-if="props.open" class="template-popover" role="dialog" aria-label="Iniciar Aba Template">
        <div class="template-popover__header">
            <span class="template-popover__title">Templates do Projeto</span>
            <MaxButton label="Novo Template" icon="lucide:plus" variant="ghost" size="small" class="template-popover__new-btn" @click="emit('create-template')" />
        </div>
        <div v-if="props.templates.length === 0" class="template-popover__empty">Nenhum template cadastrado neste projeto.</div>
        <ul v-else class="template-popover__list" role="menu">
            <li v-for="tpl in props.templates" :key="tpl.id" class="template-popover__item" role="none">
                <button type="button" class="template-popover__launch" role="menuitem" @click="emit('launch', tpl.id)">
                    <MaxIcon :icon="tpl.config.icon || 'lucide:terminal'" class="template-popover__icon" />
                    <div class="template-popover__info">
                        <div class="template-popover__name-row">
                            <span class="template-popover__name">{{ tpl.config.name }}</span>
                            <span v-if="tpl.isDefault" class="template-popover__badge template-popover__badge--default">Padrão</span>
                            <span v-if="tpl.config.continuous" class="template-popover__badge template-popover__badge--continuous">Contínuo</span>
                            <span v-if="tpl.config.addToWorkspace" class="template-popover__badge template-popover__badge--workspace">Auto</span>
                        </div>
                        <span v-if="tpl.config.initialCommand" class="template-popover__cmd">{{ tpl.config.initialCommand }}</span>
                    </div>
                </button>
                <div class="template-popover__actions">
                    <MaxIconButton v-if="!tpl.isDefault" icon="lucide:star" :label="`Definir ${tpl.config.name} como padrão`" variant="ghost" class="template-popover__action-btn" title="Definir como padrão" @click.stop="emit('set-default', tpl.id)" />
                    <MaxIconButton icon="lucide:pencil" :label="`Editar ${tpl.config.name}`" variant="ghost" class="template-popover__action-btn" title="Editar template" @click.stop="emit('edit-template', tpl)" />
                    <MaxIconButton v-if="!tpl.isDefault" icon="lucide:trash-2" :label="`Excluir ${tpl.config.name}`" variant="danger" class="template-popover__action-btn template-popover__action-btn--danger" title="Excluir template" @click.stop="emit('delete-template', tpl)" />
                </div>
            </li>
        </ul>
    </div>
</template>

<script setup lang="ts">
    import { MaxButton, MaxIcon, MaxIconButton } from '@maxvue/max-components-ui';
    import type { TabTemplate_Serialize, TemplateId } from '@/types/bindings';

    export interface TemplateLaunchPopoverProps {
        open: boolean;
        templates: TabTemplate_Serialize[];
    }

    const props = defineProps<TemplateLaunchPopoverProps>();

    const emit = defineEmits<{
        launch: [templateId: TemplateId];
        'create-template': [];
        'edit-template': [template: TabTemplate_Serialize];
        'delete-template': [template: TabTemplate_Serialize];
        'set-default': [templateId: TemplateId];
    }>();
</script>

<style lang="scss" scoped>
    .template-popover {
        position: absolute;
        top: calc(100% + 0.375rem);
        left: 0;
        z-index: 45;
        width: 22rem;
        background: var(--background-800);
        border: 1px solid var(--background-600);
        border-radius: 0.5rem;
        box-shadow: 0 10px 25px rgb(0 0 0 / 45%);
        padding: 0.5rem;
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .template-popover__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.25rem 0.375rem;
        border-bottom: 1px solid var(--background-700);
    }

    .template-popover__title {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--background-200);
    }

    .template-popover__new-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.1875rem 0.5rem;
        font-size: 0.75rem;
        font-weight: 600;
        border-radius: 0.3125rem;
        border: 1px solid var(--max-primary-500);
        background: rgb(20 184 166 / 15%);
        color: var(--max-primary-400);
        cursor: pointer;

        &:hover {
            background: rgb(20 184 166 / 25%);
        }
    }

    .template-popover__empty {
        padding: 0.75rem;
        font-size: 0.7813rem;
        color: var(--background-400);
        text-align: center;
    }

    .template-popover__list {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        max-height: 18rem;
        overflow-y: auto;
    }

    .template-popover__item {
        display: flex;
        align-items: center;
        gap: 0.25rem;
        border-radius: 0.375rem;
        background: var(--background-850, var(--background-900));
        padding: 0.25rem;
    }

    .template-popover__launch {
        flex: 1;
        min-width: 0;
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.3125rem 0.375rem;
        border: none;
        border-radius: 0.25rem;
        background: transparent;
        color: var(--background-100);
        text-align: left;
        cursor: pointer;

        &:hover {
            background: var(--background-700);
        }
    }

    .template-popover__icon {
        font-size: 1rem;
        color: var(--max-primary-400);
        flex-shrink: 0;
    }

    .template-popover__info {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
    }

    .template-popover__name-row {
        display: flex;
        align-items: center;
        gap: 0.3125rem;
        flex-wrap: wrap;
    }

    .template-popover__name {
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--background-50);
    }

    .template-popover__badge {
        font-size: 0.625rem;
        font-weight: 600;
        padding: 0.0625rem 0.375rem;
        border-radius: 999px;

        &--default {
            background: rgb(20 184 166 / 18%);
            color: var(--max-primary-400);
        }

        &--continuous {
            background: rgb(14 165 233 / 18%);
            color: var(--max-info-400);
        }

        &--workspace {
            background: var(--background-700);
            color: var(--background-200);
        }
    }

    .template-popover__cmd {
        font-size: 0.6875rem;
        font-family: var(--mt-font-mono);
        color: var(--background-400);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .template-popover__actions {
        display: flex;
        align-items: center;
        gap: 0.125rem;
        flex-shrink: 0;
    }

    .template-popover__action-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        border: none;
        border-radius: 0.25rem;
        background: transparent;
        color: var(--background-300);
        cursor: pointer;

        &:hover {
            background: var(--background-700);
            color: var(--background-50);
        }

        &--danger:hover {
            color: var(--max-danger-400);
        }
    }
</style>
