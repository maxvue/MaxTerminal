<template>
    <div v-if="modelValue" class="command-group-modal-backdrop" role="dialog" aria-modal="true" :aria-label="editor.isEditMode.value ? 'Editar Grupo de Comandos' : 'Criar Grupo de Comandos'" data-testid="command-group-modal" @keydown.esc.stop.prevent="onClose">
        <div class="command-group-modal">
            <header class="command-group-modal__header">
                <div class="command-group-modal__title-row">
                    <MaxIcon :icon="editor.draft.value.scope === 'local' ? 'lucide:layers' : 'lucide:globe'" />
                    <h2 class="command-group-modal__title">{{ editor.isEditMode.value ? 'Editar Grupo de Comandos' : 'Criar Grupo de Comandos' }}</h2>
                    <span class="command-group-modal__scope-pill" data-testid="modal-scope-pill">{{ editor.draft.value.scope === 'local' ? 'Deste projeto' : 'Global' }}</span>
                </div>
                <div class="command-group-modal__header-actions">
                    <MaxButton v-if="editor.isEditMode.value && editor.draft.value.scope === 'local'" label="Copiar para Global" icon="lucide:copy" variant="secondary" size="small" class="command-group-modal__copy-btn" data-testid="btn-copy-to-global" @click="editor.copyToScope('global', null)" />
                    <MaxButton v-if="editor.isEditMode.value && editor.draft.value.scope === 'global' && projectId" label="Copiar para este Projeto" icon="lucide:copy" variant="secondary" size="small" class="command-group-modal__copy-btn" data-testid="btn-copy-to-local" @click="editor.copyToScope('local', projectId)" />
                    <MaxIconButton icon="lucide:x" label="Fechar modal" variant="ghost" class="command-group-modal__close-btn" data-testid="btn-close-group-modal" @click="onClose" />
                </div>
            </header>

            <div class="sr-only" aria-live="polite" data-testid="reorder-live-region">{{ editor.reorderAnnouncement.value }}</div>

            <div v-if="editor.revisionConflictGroup.value" class="command-group-modal__conflict-banner" role="alert" data-testid="revision-conflict-banner">
                <div class="command-group-modal__conflict-text">
                    <strong>Conflito de revisão detectado (revisão {{ editor.revisionConflictGroup.value.revision }}).</strong>
                    <span>Outro fluxo atualizou este grupo enquanto você editava. Suas alterações locais foram preservadas.</span>
                </div>
                <div class="command-group-modal__conflict-actions">
                    <MaxButton label="Carregar versão mais recente em paralelo" variant="ghost" class="command-group-modal__btn command-group-modal__btn--ghost" data-testid="btn-conflict-load-latest" @click="editor.loadLatestOnConflict()" />
                    <MaxButton label="Forçar salvamento sobre a revisão atual" variant="warning" class="command-group-modal__btn command-group-modal__btn--warning" data-testid="btn-conflict-force-save" @click="onForceSaveOverConflict" />
                </div>
            </div>

            <div class="command-group-modal__body">
                <div class="command-group-modal__main-col">
                    <div class="command-group-modal__grid">
                        <label class="command-group-modal__field">
                            <span class="command-group-modal__label">Nome do Grupo</span>
                            <input v-model="editor.draft.value.name" type="text" class="command-group-modal__input" data-testid="input-group-name" placeholder="Ex.: Subir Ambiente Completo" @input="editor.markMutated()">
                            <span v-if="editor.fieldErrors.value['name']" class="command-group-modal__error" data-testid="error-group-name">{{ editor.fieldErrors.value['name'] }}</span>
                        </label>

                        <div class="command-group-modal__field">
                            <span class="command-group-modal__label">Escopo</span>
                            <div class="command-group-modal__segmented">
                                <button type="button" class="command-group-modal__seg-btn" :class="{ 'is-active': editor.draft.value.scope === 'local' }" data-testid="btn-scope-local" :disabled="editor.isEditMode.value || !projectId" @click="editor.setScope('local', projectId)">Local (Projeto)</button>
                                <button type="button" class="command-group-modal__seg-btn" :class="{ 'is-active': editor.draft.value.scope === 'global' }" data-testid="btn-scope-global" :disabled="editor.isEditMode.value" @click="editor.setScope('global', null)">Global</button>
                            </div>
                            <span v-if="editor.fieldErrors.value['projectId']" class="command-group-modal__error">{{ editor.fieldErrors.value['projectId'] }}</span>
                        </div>
                    </div>

                    <label class="command-group-modal__field">
                        <span class="command-group-modal__label">Descrição</span>
                        <input v-model="editor.draft.value.description" type="text" class="command-group-modal__input" data-testid="input-group-description" placeholder="Descrição curta do propósito deste grupo de comandos" @input="editor.markMutated()">
                        <span v-if="editor.fieldErrors.value['description']" class="command-group-modal__error">{{ editor.fieldErrors.value['description'] }}</span>
                    </label>

                    <div class="command-group-modal__grid">
                        <div class="command-group-modal__field">
                            <span class="command-group-modal__label">Modo de Execução Padrão</span>
                            <div class="command-group-modal__segmented">
                                <button type="button" class="command-group-modal__seg-btn" :class="{ 'is-active': editor.draft.value.mode === 'sequential' }" data-testid="btn-mode-sequential" @click="editor.setMode('sequential')">Sequencial na mesma aba</button>
                                <button type="button" class="command-group-modal__seg-btn" :class="{ 'is-active': editor.draft.value.mode === 'parallel' }" data-testid="btn-mode-parallel" @click="editor.setMode('parallel')">Paralelo em Background</button>
                                <button type="button" class="command-group-modal__seg-btn" :class="{ 'is-active': editor.draft.value.mode === 'multi_tabs' }" data-testid="btn-mode-multi-tabs" @click="editor.setMode('multi_tabs')">Multi-Abas</button>
                            </div>
                        </div>

                        <div v-if="editor.draft.value.mode === 'sequential'" class="command-group-modal__field" data-testid="sequential-separator-group">
                            <span class="command-group-modal__label">Política de Continuação Sequencial</span>
                            <div class="command-group-modal__segmented">
                                <button type="button" class="command-group-modal__seg-btn" :class="{ 'is-active': editor.draft.value.separator === 'and' }" data-testid="btn-sep-and" @click="editor.setSeparator('and')">&amp;&amp; (Parar na primeira falha)</button>
                                <button type="button" class="command-group-modal__seg-btn" :class="{ 'is-active': editor.draft.value.separator === 'semicolon' }" data-testid="btn-sep-semicolon" @click="editor.setSeparator('semicolon')">; (Continuar mesmo após falha)</button>
                            </div>
                        </div>
                    </div>

                    <p class="command-group-modal__isolation-note" data-testid="isolation-note">No modo Sequencial, itens sem override mantêm o estado do shell (<code>cd</code>, <code>export</code>), enquanto itens com override de diretório ou ambiente rodam em shell filho isolado (<code>env -C</code>).</p>

                    <div class="command-group-modal__grid">
                        <label class="command-group-modal__field">
                            <span class="command-group-modal__label">Diretório relativo padrão do grupo (opcional)</span>
                            <input v-model="editor.draft.value.cwd" type="text" class="command-group-modal__input command-group-modal__input--mono" data-testid="input-group-cwd" placeholder="Ex.: backend" @input="editor.markMutated()">
                            <span v-if="editor.fieldErrors.value['cwd']" class="command-group-modal__error" data-testid="error-group-cwd">{{ editor.fieldErrors.value['cwd'] }}</span>
                        </label>

                        <div class="command-group-modal__field">
                            <span class="command-group-modal__label">Etiquetas (Labels)</span>
                            <div class="command-group-modal__labels-box">
                                <span v-for="(lbl, idx) in editor.draft.value.labels" :key="lbl" class="command-group-modal__label-chip" :data-testid="`group-label-${idx}`">
                                    <span>#{{ lbl }}</span>
                                    <button type="button" class="command-group-modal__chip-remove" :aria-label="`Remover etiqueta ${lbl}`" @click="editor.removeLabel(idx)">×</button>
                                </span>
                                <input v-model="newLabelInput" type="text" class="command-group-modal__label-input" data-testid="input-add-label" placeholder="Adicionar label + Enter" @keydown.enter.prevent="onAddLabel">
                            </div>
                            <span v-if="editor.fieldErrors.value['labels']" class="command-group-modal__error">{{ editor.fieldErrors.value['labels'] }}</span>
                        </div>
                    </div>

                    <div v-if="editor.hasMidSequenceContinuousWarning.value" class="command-group-modal__warning-banner" role="alert" data-testid="continuous-mid-sequence-warning">
                        <MaxIcon icon="lucide:alert-triangle" />
                        <span>Este item está marcado como Execução Contínua e impedirá que os itens seguintes iniciem enquanto não for encerrado.</span>
                    </div>

                    <div class="command-group-modal__items-header">
                        <h3 class="command-group-modal__subtitle">Itens do Grupo ({{ editor.draft.value.items.length }})</h3>
                        <MaxButton label="Adicionar Item" icon="lucide:plus" variant="ghost" class="command-group-modal__btn command-group-modal__btn--ghost" data-testid="btn-add-group-item" @click="editor.addItem()" />
                    </div>

                    <div class="command-group-modal__items-list">
                        <CommandGroupItemRow v-for="(item, idx) in editor.draft.value.items" :key="item.localKey" :item="item" :index="idx" :total="editor.draft.value.items.length" :field-errors="editor.fieldErrors.value" @update:item="editor.updateItem(idx, $event)" @move-up="editor.moveItemUp" @move-down="editor.moveItemDown" @duplicate="editor.duplicateItem" @remove="editor.removeItem" />
                    </div>

                    <p v-if="editor.generalError.value" class="command-group-modal__error" role="alert" data-testid="modal-general-error">{{ editor.generalError.value }}</p>
                </div>

                <div class="command-group-modal__side-col">
                    <CommandGroupSuggestPanel :scope="editor.draft.value.scope" :project-id="editor.draft.value.projectId" :pane-id="null" :draft-revision="editor.draftRevision.value" :current-draft="editor.toInput()" :has-undo-snapshot="editor.hasUndoSnapshot.value" @apply-suggestion="editor.applySuggestion($event)" @undo-suggestion="editor.undoSuggestion()" @append-recurring-item="editor.appendRecurringCommand($event)" />
                </div>
            </div>

            <footer class="command-group-modal__footer">
                <div class="command-group-modal__footer-left">
                    <MaxButton v-if="editor.isEditMode.value" :label="editor.deleting.value ? 'Excluindo…' : 'Excluir grupo'" icon="lucide:trash-2" variant="danger" class="command-group-modal__btn command-group-modal__btn--danger" data-testid="btn-delete-group" :disabled="editor.deleting.value || editor.saving.value" :loading="editor.deleting.value" @click="onDelete" />
                </div>
                <div class="command-group-modal__footer-right">
                    <MaxButton label="Cancelar" variant="ghost" class="command-group-modal__btn command-group-modal__btn--ghost" data-testid="btn-cancel-group" @click="onClose" />
                    <MaxButton :label="editor.saving.value ? 'Salvando…' : 'Salvar grupo'" icon="lucide:check" variant="primary" class="command-group-modal__btn command-group-modal__btn--primary" data-testid="btn-save-group" :disabled="editor.saving.value" :loading="editor.saving.value" @click="onSave" />
                </div>
            </footer>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { ref, watch } from 'vue';
    import { MaxButton, MaxIcon, MaxIconButton } from '@maxvue/max-components-ui';
    import CommandGroupItemRow from '@/components/commandGroups/CommandGroupItemRow.vue';
    import CommandGroupSuggestPanel from '@/components/commandGroups/CommandGroupSuggestPanel.vue';
    import { useCommandGroupEditor } from '@/composables/commandGroups/useCommandGroupEditor';
    import type { CommandGroup, GroupScope } from '@/types/commandGroups';

    export interface CommandGroupModalProps {
        modelValue: boolean;
        group: CommandGroup | null;
        scope: GroupScope;
        projectId: string | null;
        initialName?: string;
    }

    export interface CommandGroupModalEmits {
        (e: 'update:modelValue', value: boolean): void;
        (e: 'saved', group: CommandGroup): void;
        (e: 'deleted', groupId: string): void;
    }

    const props = withDefaults(defineProps<CommandGroupModalProps>(), {
        initialName: ''
    });

    const emit = defineEmits<CommandGroupModalEmits>();

    const editor = useCommandGroupEditor({
        initialGroup: props.group,
        initialScope: props.scope,
        initialProjectId: props.projectId,
        initialName: props.initialName
    });

    const newLabelInput = ref<string>('');

    watch(
        () => [props.modelValue, props.group, props.scope, props.projectId, props.initialName] as const,
        ([open, group, scope, projectId, initialName]) => {
            if (open) {
                editor.resetFromGroup(group, scope, projectId, initialName ?? '');
            }
        }
    );

    function onAddLabel(): void {
        if (!newLabelInput.value.trim()) {
            return;
        }
        editor.addLabel(newLabelInput.value);
        newLabelInput.value = '';
    }

    function onClose(): void {
        emit('update:modelValue', false);
    }

    async function onSave(): Promise<void> {
        const saved = await editor.save();
        if (saved) {
            emit('saved', saved);
            emit('update:modelValue', false);
        }
    }

    async function onForceSaveOverConflict(): Promise<void> {
        const saved = await editor.forceSaveOverConflict();
        if (saved) {
            emit('saved', saved);
            emit('update:modelValue', false);
        }
    }

    async function onDelete(): Promise<void> {
        const groupId = editor.baseGroup.value?.id;
        if (!groupId) {
            return;
        }
        const ok = await editor.deleteCurrentGroup();
        if (ok) {
            emit('deleted', groupId);
            emit('update:modelValue', false);
        }
    }
</script>

<style lang="scss" scoped>
    .sr-only {
        position: absolute;
        width: 1px;
        height: 1px;
        padding: 0;
        margin: -1px;
        overflow: hidden;
        clip: rect(0, 0, 0, 0);
        white-space: nowrap;
        border: 0;
    }

    .command-group-modal-backdrop {
        position: fixed;
        inset: 0;
        z-index: 1200;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 1.25rem;
        background: var(--mt-ui-overlay, rgb(0 0 0 / 65%));
    }

    .command-group-modal {
        display: flex;
        flex-direction: column;
        width: min(78rem, 96vw);
        max-height: 92vh;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 0.5rem;
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-strong, var(--background-950));
        overflow: hidden;

        &__header {
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 0.75rem 1rem;
            border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
            background: var(--mt-ui-surface-panel, var(--background-25));
        }

        &__title-row {
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }

        &__title {
            margin: 0;
            font-size: 0.9375rem;
            font-weight: 600;
        }

        &__scope-pill {
            padding: 0.125rem 0.5rem;
            border-radius: 999px;
            background: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--max-primary-400);
            font-size: 0.6875rem;
            font-weight: 600;
        }

        &__header-actions {
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }

        &__copy-btn {
            display: inline-flex;
            align-items: center;
            gap: 0.375rem;
            padding: 0.25rem 0.625rem;
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 0.25rem;
            background: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--mt-ui-text-secondary, var(--background-600));
            font-size: 0.75rem;
            cursor: pointer;
        }

        &__close-btn {
            border: none;
            background: transparent;
            color: var(--mt-ui-text-secondary, var(--background-600));
            cursor: pointer;
            padding: 0.25rem;
        }

        &__conflict-banner {
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 1rem;
            padding: 0.625rem 1rem;
            background: var(--mt-ui-warning-subtle, color-mix(in srgb, var(--max-warning-500) 15%, transparent));
            border-bottom: 1px solid var(--max-warning-500);
            color: var(--max-warning-300);
            font-size: 0.7813rem;
        }

        &__conflict-text {
            display: flex;
            flex-direction: column;
            gap: 0.125rem;
        }

        &__conflict-actions {
            display: flex;
            gap: 0.5rem;
            flex-shrink: 0;
        }

        &__body {
            flex: 1;
            display: grid;
            grid-template-columns: 1.75fr 1fr;
            gap: 1rem;
            padding: 1rem;
            overflow-y: auto;
            min-height: 0;
        }

        &__main-col {
            display: flex;
            flex-direction: column;
            gap: 0.75rem;
            min-width: 0;
        }

        &__side-col {
            min-width: 0;
        }

        &__grid {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 0.75rem;
        }

        &__field {
            display: flex;
            flex-direction: column;
            gap: 0.25rem;
        }

        &__label {
            font-size: 0.75rem;
            font-weight: 600;
            color: var(--mt-ui-text-secondary, var(--background-600));
        }

        &__input {
            width: 100%;
            padding: 0.4375rem 0.625rem;
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 0.25rem;
            background: var(--mt-ui-surface-panel, var(--background-25));
            color: var(--mt-ui-text-strong, var(--background-950));
            font-size: 0.8125rem;
            outline: none;

            &--mono {
                font-family: var(--mt-font-mono);
            }
        }

        &__segmented {
            display: flex;
            gap: 0.25rem;
        }

        &__seg-btn {
            flex: 1;
            padding: 0.375rem 0.5rem;
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 0.25rem;
            background: var(--mt-ui-surface-panel, var(--background-25));
            color: var(--mt-ui-text-secondary, var(--background-600));
            font-size: 0.75rem;
            cursor: pointer;

            &.is-active {
                border-color: var(--max-primary-500);
                background: color-mix(in srgb, var(--max-primary-500) 18%, transparent);
                color: var(--max-primary-400);
                font-weight: 600;
            }

            &:disabled {
                opacity: 0.5;
                cursor: not-allowed;
            }
        }

        &__isolation-note {
            margin: 0;
            font-size: 0.75rem;
            color: var(--mt-ui-text-muted, var(--background-400));
        }

        &__labels-box {
            display: flex;
            align-items: center;
            flex-wrap: wrap;
            gap: 0.375rem;
            padding: 0.25rem 0.5rem;
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 0.25rem;
            background: var(--mt-ui-surface-panel, var(--background-25));
        }

        &__label-chip {
            display: inline-flex;
            align-items: center;
            gap: 0.25rem;
            padding: 0.125rem 0.4375rem;
            border-radius: 999px;
            background: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--max-primary-400);
            font-size: 0.7188rem;
        }

        &__chip-remove {
            border: none;
            background: transparent;
            color: var(--mt-ui-text-muted, var(--background-400));
            cursor: pointer;
            padding: 0;
        }

        &__label-input {
            flex: 1;
            min-width: 7rem;
            border: none;
            background: transparent;
            color: var(--mt-ui-text-strong, var(--background-950));
            font-size: 0.7813rem;
            outline: none;
        }

        &__warning-banner {
            display: flex;
            align-items: center;
            gap: 0.5rem;
            padding: 0.5rem 0.75rem;
            border: 1px solid var(--max-warning-500);
            border-radius: 0.375rem;
            background: var(--mt-ui-warning-subtle, color-mix(in srgb, var(--max-warning-500) 15%, transparent));
            color: var(--max-warning-400);
            font-size: 0.7813rem;
        }

        &__items-header {
            display: flex;
            align-items: center;
            justify-content: space-between;
        }

        &__subtitle {
            margin: 0;
            font-size: 0.8125rem;
            font-weight: 600;
        }

        &__items-list {
            display: flex;
            flex-direction: column;
            gap: 0.5rem;
        }

        &__error {
            margin: 0;
            font-size: 0.75rem;
            color: var(--max-danger-400);
        }

        &__footer {
            display: flex;
            align-items: center;
            justify-content: space-between;
            padding: 0.75rem 1rem;
            border-top: 1px solid var(--mt-ui-border, var(--background-100));
            background: var(--mt-ui-surface-panel, var(--background-25));
        }

        &__footer-left,
        &__footer-right {
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }

        &__btn {
            display: inline-flex;
            align-items: center;
            gap: 0.375rem;
            padding: 0.4375rem 0.875rem;
            border-radius: 0.3125rem;
            font-size: 0.8125rem;
            font-weight: 600;
            cursor: pointer;

            &--primary {
                border: 1px solid var(--max-primary-500);
                background: color-mix(in srgb, var(--max-primary-500) 18%, transparent);
                color: var(--max-primary-400);
            }

            &--warning {
                border: 1px solid var(--max-warning-500);
                background: var(--mt-ui-warning-subtle, color-mix(in srgb, var(--max-warning-500) 18%, transparent));
                color: var(--max-warning-300);
            }

            &--danger {
                border: 1px solid var(--max-danger-500);
                background: var(--mt-ui-danger-subtle, color-mix(in srgb, var(--max-danger-500) 18%, transparent));
                color: var(--max-danger-300);
            }

            &--ghost {
                border: 1px solid var(--mt-ui-border, var(--background-100));
                background: var(--mt-ui-surface-elevated, var(--background-50));
                color: var(--mt-ui-text-secondary, var(--background-600));
            }

            &:disabled {
                opacity: 0.5;
                cursor: not-allowed;
            }
        }
    }
</style>
