<template>
    <div class="tab-bar" role="tablist" aria-label="Abas do projeto">
        <div class="tab-bar__tabs">
            <slot name="pinned" />
            <TabBarItem v-for="tab in props.tabs" :key="tab.id" :tab="tab" :active="tab.id === props.activeTabId" :dragging="draggingTabId === tab.id" @select="emit('select-tab', $event)" @close="emit('close-tab', $event)" @drag-start="onDragStart" @drag-over="onDragOver" @drop="onDrop" @drag-end="onDragEnd">
                <template #indicators="{ tab: itemTab }">
                    <slot name="tab-indicators" :tab="itemTab" />
                </template>
            </TabBarItem>
        </div>
        <div class="tab-bar__controls">
            <button type="button" class="tab-bar__btn tab-bar__btn--primary" title="Adicionar nova aba (Template Padrão)" aria-label="Adicionar nova aba" @click="emit('add-default-tab')">
                <MaxIcon icon="lucide:plus" />
                <span>Adicionar nova aba</span>
            </button>
            <div class="tab-bar__template-anchor">
                <button type="button" class="tab-bar__btn" title="Iniciar Aba Template" aria-label="Iniciar Aba Template" :aria-expanded="templatePopoverOpen" @click="toggleTemplatePopover">
                    <MaxIcon icon="lucide:layers" />
                    <span>Iniciar Aba Template</span>
                </button>
                <TemplateLaunchPopover :open="templatePopoverOpen" :templates="props.templates" @launch="onLaunchTemplate" @create-template="onCreateTemplate" @edit-template="onEditTemplate" @delete-template="emit('delete-template', $event)" @set-default="emit('set-default-template', $event)" />
            </div>
            <template v-if="props.activeTabId">
                <button type="button" class="tab-bar__btn" title="Dividir painel verticalmente (lado a lado)" aria-label="Dividir painel verticalmente" :disabled="!props.canSplitActivePane" @click="emit('split-active-pane', 'vertical')">
                    <MaxIcon icon="lucide:columns-2" />
                </button>
                <button type="button" class="tab-bar__btn" title="Dividir painel horizontalmente (acima/abaixo)" aria-label="Dividir painel horizontalmente" :disabled="!props.canSplitActivePane" @click="emit('split-active-pane', 'horizontal')">
                    <MaxIcon icon="lucide:rows-2" />
                </button>
                <button type="button" class="tab-bar__btn" title="Iniciar nova Sessão limpa nesta aba" aria-label="Iniciar nova Sessão" @click="emit('new-session')">
                    <MaxIcon icon="lucide:rotate-ccw" />
                    <span>Iniciar nova Sessão</span>
                </button>
                <button type="button" class="tab-bar__btn" title="Histórico de Sessões da aba" aria-label="Histórico de Sessões" @click="emit('open-session-history')">
                    <MaxIcon icon="lucide:history" />
                    <span>Sessões</span>
                </button>
            </template>
            <slot name="bar-extra" />
        </div>
    </div>
</template>

<script setup lang="ts">
    import { ref } from 'vue';
    import type { SplitDirection, TabId, TabTemplate_Serialize, TabView_Serialize, TemplateId } from '@/types/bindings';
    import TabBarItem from './TabBarItem.vue';
    import TemplateLaunchPopover from './TemplateLaunchPopover.vue';

    export interface TabBarProps {
        tabs: TabView_Serialize[];
        activeTabId: TabId | null;
        templates: TabTemplate_Serialize[];
        canSplitActivePane?: boolean;
    }

    const props = withDefaults(defineProps<TabBarProps>(), {
        canSplitActivePane: true
    });

    const emit = defineEmits<{
        'select-tab': [tabId: TabId];
        'close-tab': [tabId: TabId];
        'reorder-tabs': [orderedTabIds: TabId[]];
        'add-default-tab': [];
        'launch-template': [templateId: TemplateId];
        'create-template': [];
        'edit-template': [template: TabTemplate_Serialize];
        'delete-template': [template: TabTemplate_Serialize];
        'set-default-template': [templateId: TemplateId];
        'split-active-pane': [direction: SplitDirection];
        'new-session': [];
        'open-session-history': [];
    }>();

    const templatePopoverOpen = ref<boolean>(false);
    const draggingTabId = ref<TabId | null>(null);

    function toggleTemplatePopover(): void {
        templatePopoverOpen.value = !templatePopoverOpen.value;
    }

    function onLaunchTemplate(templateId: TemplateId): void {
        templatePopoverOpen.value = false;
        emit('launch-template', templateId);
    }

    function onCreateTemplate(): void {
        templatePopoverOpen.value = false;
        emit('create-template');
    }

    function onEditTemplate(template: TabTemplate_Serialize): void {
        templatePopoverOpen.value = false;
        emit('edit-template', template);
    }

    function onDragStart(tabId: TabId): void {
        draggingTabId.value = tabId;
    }

    function onDragOver(_targetTabId: TabId): void {
        // Permite drop sobre outra aba da mesma lista.
    }

    function onDrop(targetTabId: TabId): void {
        const sourceId = draggingTabId.value;
        draggingTabId.value = null;
        if (!sourceId || sourceId === targetTabId) {
            return;
        }
        const ids = props.tabs.map((t) => t.id);
        const fromIdx = ids.indexOf(sourceId);
        const toIdx = ids.indexOf(targetTabId);
        if (fromIdx === -1 || toIdx === -1) {
            return;
        }
        ids.splice(fromIdx, 1);
        ids.splice(toIdx, 0, sourceId);
        emit('reorder-tabs', ids);
    }

    function onDragEnd(): void {
        draggingTabId.value = null;
    }
</script>

<style lang="scss" scoped>
    .tab-bar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.375rem 0.75rem 0;
        background: var(--background-800);
        border-bottom: 1px solid var(--background-700);
        min-height: 2.5rem;
    }

    .tab-bar__tabs {
        display: flex;
        align-items: flex-end;
        gap: 0.25rem;
        overflow-x: auto;
        min-width: 0;
        flex: 1;
    }

    .tab-bar__controls {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        padding-bottom: 0.3125rem;
        flex-shrink: 0;
    }

    .tab-bar__template-anchor {
        position: relative;
    }

    .tab-bar__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.3125rem;
        height: 1.75rem;
        padding: 0 0.5625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--background-600);
        background: var(--background-850, var(--background-900));
        color: var(--background-200);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
        white-space: nowrap;

        &:disabled {
            opacity: 0.45;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            background: var(--background-700);
            color: var(--background-50);
        }

        &--primary {
            border-color: var(--max-primary-500);
            background: rgb(20 184 166 / 15%);
            color: var(--max-primary-400);

            &:hover:not(:disabled) {
                background: rgb(20 184 166 / 25%);
            }
        }
    }
</style>
