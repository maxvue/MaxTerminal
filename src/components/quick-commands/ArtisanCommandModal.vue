<template>
    <MaxModal v-if="open" :visible="open" title="Criar comando Artisan" :no-button="true" max-width="56rem" class="artisan-modal" data-testid="artisan-command-modal" @update:visible="handleClose">
        <template #header>
            <div class="artisan-modal__title-wrap">
                <MaxIcon icon="logos:laravel" />
                <h2 class="artisan-modal__title">Criar comando Artisan</h2>
                <span v-if="root" class="artisan-modal__root">{{ root }}</span>
            </div>
            <MaxIconButton icon="lucide:x" label="Fechar modal" variant="text" class="artisan-modal__close-btn" data-testid="btn-close-artisan-modal" @click="handleClose" />
        </template>

        <div v-if="store.modal.lastCreateResult" class="artisan-modal__created-banner" data-testid="artisan-created-banner">
            <span>Comando criado em <strong>{{ store.modal.lastCreateResult.path }}</strong>. Conclua os passos manuais listados ao lado antes de fechar.</span>
        </div>

        <div class="artisan-modal__columns">
            <div class="artisan-modal__form-col">
                <div class="artisan-modal__row">
                    <div class="artisan-modal__field">
                        <span class="artisan-modal__label">Nome da Classe</span>
                        <MaxInputText :model-value="form.className" data-field-path="className" data-testid="input-class-name" placeholder="SendWeeklyReport" aria-label="Nome da Classe" class="artisan-modal__input-text" @update:model-value="onClassNameChange" />
                        <span v-if="firstErrorMessageFor('className')" class="artisan-modal__error" data-testid="error-class-name">{{ firstErrorMessageFor('className') }}</span>
                    </div>

                    <div class="artisan-modal__field">
                        <span class="artisan-modal__label">Signature</span>
                        <MaxInputText :model-value="form.signatureName" data-field-path="signatureName" data-testid="input-signature-name" placeholder="reports:send-weekly" aria-label="Signature" class="artisan-modal__input-text" @update:model-value="onSignatureNameChange" />
                        <span v-if="firstErrorMessageFor('signatureName')" class="artisan-modal__error" data-testid="error-signature-name">{{ firstErrorMessageFor('signatureName') }}</span>
                    </div>
                </div>

                <div class="artisan-modal__field">
                    <span class="artisan-modal__label">Descrição</span>
                    <MaxInputText :model-value="form.description" data-field-path="description" data-testid="input-description" placeholder="Envia o relatório semanal por e-mail" aria-label="Descrição" class="artisan-modal__input-text" @update:model-value="onDescriptionChange" />
                    <span v-if="firstErrorMessageFor('description')" class="artisan-modal__error" data-testid="error-description">{{ firstErrorMessageFor('description') }}</span>
                </div>

                <ArtisanParamsBuilder :model-value="form.params ?? []" :issues="allIssues" @update:model-value="setParams" />

                <div class="artisan-modal__interfaces">
                    <MaxInputCheckbox :model-value="Boolean(form.isolatable)" label="Isolatable (--isolated)" data-testid="checkbox-isolatable" @update:model-value="(val: boolean) => setIsolatable(val)" />
                    <MaxInputCheckbox :model-value="Boolean(form.promptsForMissingInput)" label="PromptsForMissingInput" data-testid="checkbox-prompts" @update:model-value="(val: boolean) => setPromptsForMissingInput(val)" />
                </div>
                <p v-if="promptsWarning" class="artisan-modal__warning" data-testid="warning-prompts">{{ promptsWarning }}</p>

                <ArtisanSchedulePicker :model-value="form.schedule ?? null" :params="form.params ?? []" :issues="allIssues" @update:model-value="setSchedule" />

                <label class="artisan-modal__field">
                    <span class="artisan-modal__label">Objetivo / Regras do Comando</span>
                    <MaxInputTextArea :rows="6" class="artisan-modal__textarea" data-field-path="objective" data-testid="textarea-objective" placeholder="Descreva o objetivo e as regras de negócio do comando para gerar o PHPDoc, preencher com IA ou enviar ao MaxCode…" :model-value="form.objective ?? ''" @update:model-value="(val?: string) => { if (val !== undefined && val !== form.objective) setObjective(val); }" />
                    <span v-if="firstErrorMessageFor('objective')" class="artisan-modal__error">{{ firstErrorMessageFor('objective') }}</span>
                </label>

                <div v-if="generalIssues.length > 0" class="artisan-modal__general-errors">
                    <MaxAlert v-for="(iss, idx) in generalIssues" :key="idx" severity="danger" :message="iss.message" class="artisan-modal__error" />
                </div>
            </div>

            <div class="artisan-modal__preview-col">
                <ArtisanPhpPreview :preview="store.modal.preview" :loading="store.modal.previewLoading" />
            </div>
        </div>

        <footer class="artisan-modal__footer">
            <div class="artisan-modal__footer-left">
                <MaxButton :label="store.modal.aiLoading ? 'Gerando sugestão…' : 'Preencher com IA'" icon="mdi:sparkles" severity="secondary" class="artisan-modal__btn artisan-modal__btn--secondary" data-testid="btn-fill-ai" :disabled="store.modal.aiLoading || !canFillWithAi" :loading="store.modal.aiLoading" @click="handleFillWithAi" />
                <MaxButton v-if="hasAiSnapshot" label="Desfazer sugestão" icon="mdi:undo" variant="text" severity="secondary" class="artisan-modal__btn artisan-modal__btn--ghost" data-testid="btn-undo-ai" @click="store.undoAiSuggestion()" />
            </div>

            <div class="artisan-modal__footer-right">
                <MaxButton label="Cancelar" variant="text" severity="secondary" class="artisan-modal__btn artisan-modal__btn--ghost" data-testid="btn-cancel-modal" @click="handleClose" />
                <MaxButton label="Criar com MaxCode" icon="mdi:code-braces" severity="secondary" class="artisan-modal__btn artisan-modal__btn--secondary" data-testid="btn-create-maxcode" @click="handleCreateWithMaxCode" />
                <MaxButton :label="store.modal.creating ? 'Criando…' : 'Criar'" icon="mdi:check" class="artisan-modal__btn artisan-modal__btn--primary" data-testid="btn-create-artisan" :disabled="!canCreate" :loading="store.modal.creating" @click="handleCreate" />
            </div>
        </footer>
    </MaxModal>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import { MaxAlert, MaxButton, MaxIcon, MaxIconButton, MaxInputCheckbox, MaxInputText, MaxInputTextArea, MaxModal } from '@maxvue/max-components-ui';
    import ArtisanParamsBuilder from '@/components/quick-commands/ArtisanParamsBuilder.vue';
    import ArtisanPhpPreview from '@/components/quick-commands/ArtisanPhpPreview.vue';
    import ArtisanSchedulePicker from '@/components/quick-commands/ArtisanSchedulePicker.vue';
    import { useArtisanForm } from '@/composables/quick-commands/useArtisanForm';
    import { useQuickCommandsStore } from '@/stores/quickCommands';
    import type { FieldIssue } from '@/types/bindings';

    export interface ArtisanCommandModalProps {
        root: string;
        open: boolean;
    }

    export interface ArtisanCommandModalEmits {
        (e: 'close'): void;
    }

    defineProps<ArtisanCommandModalProps>();
    const emit = defineEmits<ArtisanCommandModalEmits>();

    const store = useQuickCommandsStore();
    const modalRootRef = ref<HTMLElement | null>(null);

    const {
        form,
        generalIssues,
        canCreate,
        hasAiSnapshot,
        setClassName,
        setSignatureName,
        setDescription,
        setParams,
        setIsolatable,
        setPromptsForMissingInput,
        setSchedule,
        setObjective,
        issuesFor,
        firstErrorMessageFor,
        focusFirstInvalidField
    } = useArtisanForm();

    const allIssues = computed<FieldIssue[]>(() => store.modal.report?.issues ?? []);

    const promptsWarning = computed<string | null>(() => {
        const list = issuesFor('promptsForMissingInput');
        const warn = list.find((i) => i.severity === 'warning');
        return warn ? warn.message : null;
    });

    const canFillWithAi = computed<boolean>(() => {
        return Boolean(
            (form.value.objective ?? '').trim() ||
                form.value.description.trim() ||
                form.value.signatureName.trim()
        );
    });

    function onClassNameChange(val?: string | number): void {
        setClassName(String(val ?? ''));
    }

    function onSignatureNameChange(val?: string | number): void {
        setSignatureName(String(val ?? ''));
    }

    function onDescriptionChange(val?: string | number): void {
        setDescription(String(val ?? ''));
    }

    function handleClose(): void {
        emit('close');
    }

    async function handleFillWithAi(): Promise<void> {
        await store.fillWithAi();
    }

    async function handleCreateWithMaxCode(): Promise<void> {
        await store.createWithMaxCode();
    }

    async function handleCreate(): Promise<void> {
        if (!canCreate.value) {
            focusFirstInvalidField(modalRootRef.value);
            return;
        }
        await store.create();
    }
</script>

<style lang="scss" scoped>
    .artisan-modal-backdrop {
        position: fixed;
        inset: 0;
        z-index: 1200;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 1.25rem;
        background: var(--mt-ui-overlay, rgb(0 0 0 / 65%));
    }

    .artisan-modal {
        display: flex;
        flex-direction: column;
        width: min(76rem, 96vw);
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

        &__title-wrap {
            display: flex;
            align-items: center;
            gap: 0.5rem;
            min-width: 0;
        }

        &__title {
            margin: 0;
            font-size: 0.9375rem;
            font-weight: 600;
            color: var(--mt-ui-text-strong, var(--background-950));
        }

        &__root {
            font-family: var(--mt-font-mono);
            font-size: 0.75rem;
            color: var(--mt-ui-text-muted, var(--background-400));
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }

        &__close-btn {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            width: 1.75rem;
            height: 1.75rem;
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 0.25rem;
            background: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--mt-ui-text-secondary, var(--background-600));
            cursor: pointer;

            &:hover {
                border-color: var(--max-primary-500);
                color: var(--mt-ui-text-strong, var(--background-950));
            }
        }

        &__created-banner {
            padding: 0.5rem 1rem;
            background: var(--mt-ui-surface-panel, var(--background-25));
            border-bottom: 1px solid var(--max-info-500);
            color: var(--max-info-400);
            font-size: 0.8125rem;
        }

        &__columns {
            display: grid;
            grid-template-columns: 1.15fr 0.85fr;
            gap: 1rem;
            padding: 1rem;
            flex: 1;
            min-height: 0;
            overflow: hidden;
        }

        &__form-col {
            display: flex;
            flex-direction: column;
            gap: 0.75rem;
            overflow-y: auto;
            padding-right: 0.25rem;
        }

        &__preview-col {
            display: flex;
            flex-direction: column;
            min-height: 0;
            overflow: hidden;
        }

        &__row {
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

        &__textarea {
            font-family: inherit;
            resize: vertical;
        }

        &__input,
        &__textarea {
            padding: 0.4375rem 0.625rem;
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 0.375rem;
            background: var(--mt-ui-surface-panel, var(--background-25));
            color: var(--mt-ui-text, var(--background-800));
            font-size: 0.8125rem;
            font-family: var(--mt-font-mono);

            &:focus {
                outline: none;
                border-color: var(--max-primary-500);
            }
        }

        &__error {
            margin: 0;
            font-size: 0.6875rem;
            color: var(--max-danger-400);
        }

        &__warning {
            margin: 0;
            font-size: 0.6875rem;
            color: var(--max-warning-400);
        }

        &__interfaces {
            display: flex;
            flex-wrap: wrap;
            gap: 1rem;
        }

        &__checkbox {
            display: inline-flex;
            align-items: center;
            gap: 0.375rem;
            font-size: 0.8125rem;
            color: var(--mt-ui-text-secondary, var(--background-600));
            cursor: pointer;
        }

        &__footer {
            display: flex;
            align-items: center;
            justify-content: space-between;
            flex-wrap: wrap;
            gap: 0.5rem;
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
            padding: 0.4375rem 0.75rem;
            border-radius: 0.375rem;
            font-size: 0.8125rem;
            font-weight: 500;
            cursor: pointer;

            &:disabled {
                opacity: 0.45;
                cursor: not-allowed;
            }

            &--primary {
                border: 1px solid var(--max-primary-500);
                background: var(--max-primary-600);
                color: var(--max-primary-content, #fff);
            }

            &--secondary {
                border: 1px solid var(--mt-ui-border, var(--background-100));
                background: var(--mt-ui-surface-elevated, var(--background-50));
                color: var(--mt-ui-text, var(--background-800));
            }

            &--ghost {
                border: 1px solid transparent;
                background: transparent;
                color: var(--mt-ui-text-secondary, var(--background-600));
            }
        }
    }
</style>
