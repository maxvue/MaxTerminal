<template>
    <MaxModal :visible="props.open" :title="modalTitle" :no-button="true" max-width="36rem" class="template-modal" :dismissable="!props.saving" :close-on-escape="!props.saving" @update:visible="emit('close')">
        <form class="template-modal__form" @submit.prevent="handleSubmit">
            <div v-if="localError || props.errorMessage" class="template-modal__error" role="alert">{{ localError || props.errorMessage }}</div>
            <div class="template-modal__row">
                <label class="template-modal__field template-modal__field--grow">
                    <span class="template-modal__label">Nome do Template *</span>
                    <input v-model="name" type="text" class="template-modal__input" maxlength="64" required placeholder="Ex.: Servidor Dev">
                </label>
                <label class="template-modal__field">
                    <span class="template-modal__label">Ícone (Iconify)</span>
                    <input v-model="icon" type="text" class="template-modal__input" maxlength="64" placeholder="lucide:terminal">
                </label>
            </div>
            <label class="template-modal__field">
                <span class="template-modal__label">Pasta personalizada (opcional)</span>
                <input v-model="cwd" type="text" class="template-modal__input" placeholder="Relativa à raiz do projeto ou caminho absoluto">
            </label>
            <label class="template-modal__field">
                <span class="template-modal__label">Comando Inicial (opcional)</span>
                <input v-model="initialCommand" type="text" class="template-modal__input template-modal__input--mono" placeholder="Ex.: npm run dev">
            </label>
            <div class="template-modal__checkboxes">
                <label class="template-modal__checkbox">
                    <input v-model="addToWorkspace" type="checkbox">
                    <span>Adicionar ao Workspace (abrir ao iniciar)</span>
                </label>
                <label class="template-modal__checkbox">
                    <input v-model="continuous" type="checkbox">
                    <span>Execução Contínua (daemon / watcher)</span>
                </label>
                <label class="template-modal__checkbox">
                    <input v-model="makeDefault" type="checkbox">
                    <span>Definir como Template Padrão do projeto</span>
                </label>
            </div>
            <div class="template-modal__env-section">
                <div class="template-modal__env-header">
                    <span class="template-modal__label">Variáveis de Ambiente</span>
                    <MaxButton label="Adicionar variável" icon="lucide:plus" variant="ghost" size="small" class="template-modal__add-env" @click="addEnvRow" />
                </div>
                <div v-for="(row, idx) in envRows" :key="idx" class="template-modal__env-row">
                    <input v-model="row.key" type="text" class="template-modal__input template-modal__input--mono" placeholder="NOME_VAR">
                    <select v-model="row.kind" class="template-modal__select">
                        <option value="literal">Valor literal</option>
                        <option value="inherited">Herdar do sistema</option>
                    </select>
                    <input v-model="row.value" type="text" class="template-modal__input template-modal__input--mono" :placeholder="row.kind === 'inherited' ? 'VAR_HOST' : 'valor'">
                    <MaxIconButton icon="lucide:trash-2" label="Remover variável" variant="danger" class="template-modal__remove-env" @click="removeEnvRow(idx)" />
                </div>
            </div>
            <footer class="template-modal__footer">
                <MaxButton label="Cancelar" variant="ghost" class="template-modal__btn template-modal__btn--ghost" @click="emit('close')" />
                <MaxButton type="submit" :label="props.saving ? 'Salvando...' : 'Salvar Template'" variant="primary" class="template-modal__btn template-modal__btn--primary" :disabled="props.saving" :loading="props.saving" />
            </footer>
        </form>
    </MaxModal>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import { MaxButton, MaxIconButton, MaxModal } from '@maxvue/max-components-ui';
    import type { EnvValue, TabTemplate_Serialize, TabTemplateInput_Deserialize } from '@/types/bindings';

    interface EnvRowItem {
        key: string;
        kind: 'literal' | 'inherited';
        value: string;
    }

    export interface TemplateEditorModalProps {
        open: boolean;
        template: TabTemplate_Serialize | null;
        saving?: boolean;
        errorMessage?: string | null;
    }

    const props = withDefaults(defineProps<TemplateEditorModalProps>(), {
        saving: false,
        errorMessage: null
    });

    const emit = defineEmits<{
        close: [];
        save: [input: TabTemplateInput_Deserialize];
    }>();

    const name = ref<string>('');
    const icon = ref<string>('lucide:terminal');
    const cwd = ref<string>('');
    const initialCommand = ref<string>('');
    const addToWorkspace = ref<boolean>(false);
    const continuous = ref<boolean>(false);
    const makeDefault = ref<boolean>(false);
    const envRows = ref<EnvRowItem[]>([]);
    const localError = ref<string | null>(null);

    const modalTitle = computed<string>(() =>
        props.template ? `Editar Template: ${props.template.config.name}` : 'Novo Template de Aba'
    );

    watch(
        () => [props.open, props.template] as const,
        ([isOpen, tpl]) => {
            if (!isOpen) {
                return;
            }
            localError.value = null;
            if (tpl) {
                name.value = tpl.config.name;
                icon.value = tpl.config.icon || 'lucide:terminal';
                cwd.value = tpl.config.cwd ?? '';
                initialCommand.value = tpl.config.initialCommand ?? '';
                addToWorkspace.value = tpl.config.addToWorkspace;
                continuous.value = tpl.config.continuous;
                makeDefault.value = tpl.isDefault;
                envRows.value = Object.entries(tpl.config.envVars).map(([k, v]) => ({
                    key: k,
                    kind: v.kind,
                    value: v.kind === 'literal' ? v.value : v.source
                }));
            } else {
                name.value = '';
                icon.value = 'lucide:terminal';
                cwd.value = '';
                initialCommand.value = '';
                addToWorkspace.value = false;
                continuous.value = false;
                makeDefault.value = false;
                envRows.value = [];
            }
        },
        { immediate: true }
    );

    function addEnvRow(): void {
        envRows.value.push({
            key: '',
            kind: 'literal',
            value: ''
        });
    }

    function removeEnvRow(idx: number): void {
        envRows.value.splice(idx, 1);
    }

    function handleSubmit(): void {
        localError.value = null;
        const trimmedName = name.value.trim();
        if (!trimmedName) {
            localError.value = 'Informe o nome do template.';
            return;
        }

        const envVars: Record<string, EnvValue> = {};
        for (const row of envRows.value) {
            const k = row.key.trim();
            if (!k) {
                continue;
            }
            if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(k)) {
                localError.value = `Nome de variável inválido: '${k}'.`;
                return;
            }
            if (row.kind === 'inherited') {
                envVars[k] = {
                    kind: 'inherited',
                    source: row.value.trim() || k
                };
            } else {
                envVars[k] = {
                    kind: 'literal',
                    value: row.value
                };
            }
        }

        emit('save', {
            name: trimmedName,
            icon: icon.value.trim() || 'lucide:terminal',
            cwd: cwd.value.trim() ? cwd.value.trim() : null,
            initialCommand: initialCommand.value.trim() ? initialCommand.value.trim() : null,
            addToWorkspace: addToWorkspace.value,
            continuous: continuous.value,
            environment: {
                kind: 'shell',
                agentId: null
            },
            envVars,
            makeDefault: makeDefault.value
        });
    }
</script>

<style lang="scss" scoped>
    .template-modal-backdrop {
        position: fixed;
        inset: 0;
        z-index: 90;
        background: rgb(0 0 0 / 60%);
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 1rem;
    }

    .template-modal {
        width: 100%;
        max-width: 34rem;
        background: var(--background-800);
        border: 1px solid var(--background-600);
        border-radius: 0.625rem;
        box-shadow: 0 20px 45px rgb(0 0 0 / 55%);
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .template-modal__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.875rem 1.125rem;
        border-bottom: 1px solid var(--background-700);
    }

    .template-modal__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
        color: var(--background-50);
    }

    .template-modal__close {
        border: none;
        background: transparent;
        color: var(--background-300);
        cursor: pointer;
        padding: 0.25rem;
        border-radius: 0.25rem;

        &:hover {
            background: var(--background-700);
            color: var(--background-50);
        }
    }

    .template-modal__form {
        padding: 1rem 1.125rem;
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
    }

    .template-modal__error {
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        background: rgb(239 68 68 / 15%);
        border: 1px solid var(--max-danger-500);
        color: var(--max-danger-300);
        font-size: 0.7813rem;
    }

    .template-modal__row {
        display: flex;
        gap: 0.75rem;
    }

    .template-modal__field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;

        &--grow {
            flex: 1;
        }
    }

    .template-modal__label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--background-200);
    }

    .template-modal__input,
    .template-modal__select {
        padding: 0.4375rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-600);
        background: var(--background-900);
        color: var(--background-50);
        font-size: 0.8125rem;

        &--mono {
            font-family: var(--mt-font-mono);
        }
    }

    .template-modal__checkboxes {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .template-modal__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.8125rem;
        color: var(--background-200);
        cursor: pointer;
    }

    .template-modal__env-section {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .template-modal__env-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
    }

    .template-modal__add-env {
        border: none;
        background: transparent;
        color: var(--max-primary-400);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
    }

    .template-modal__env-row {
        display: grid;
        grid-template-columns: 1fr auto 1fr auto;
        gap: 0.375rem;
        align-items: center;
    }

    .template-modal__remove-env {
        border: none;
        background: transparent;
        color: var(--background-400);
        cursor: pointer;
        padding: 0.25rem;

        &:hover {
            color: var(--max-danger-400);
        }
    }

    .template-modal__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        padding-top: 0.5rem;
        border-top: 1px solid var(--background-700);
    }

    .template-modal__btn {
        padding: 0.4375rem 0.875rem;
        border-radius: 0.375rem;
        font-size: 0.8125rem;
        font-weight: 600;
        cursor: pointer;

        &--ghost {
            border: 1px solid var(--background-600);
            background: transparent;
            color: var(--background-200);
        }

        &--primary {
            border: 1px solid var(--max-primary-500);
            background: var(--max-primary-500);
            color: var(--background-950);
        }
    }
</style>
