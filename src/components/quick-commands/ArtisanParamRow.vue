<template>
    <div class="param-row" :data-testid="`artisan-param-row-${index}`">
        <div class="param-row__topbar">
            <MaxTag :value="modelValue.kind === 'argument' ? 'Argumento' : 'Opção (--flag)'" :severity="modelValue.kind === 'argument' ? 'info' : 'secondary'" size="sm" class="param-row__badge" />
            <div class="param-row__actions">
                <MaxIconButton icon="mdi:arrow-up" aria-label="Mover parâmetro para cima" class="param-row__icon-btn" :disabled="index === 0" title="Mover para cima" @click="emit('move', -1)" />
                <MaxIconButton icon="mdi:arrow-down" aria-label="Mover parâmetro para baixo" class="param-row__icon-btn" :disabled="index >= total - 1" title="Mover para baixo" @click="emit('move', 1)" />
                <MaxIconButton icon="mdi:trash-can-outline" aria-label="Remover parâmetro" class="param-row__icon-btn param-row__icon-btn--danger" title="Remover parâmetro" @click="emit('remove')" />
            </div>
        </div>

        <div class="param-row__grid">
            <div class="param-row__field">
                <span class="param-row__label">{{ modelValue.kind === 'option' ? 'Nome (--)' : 'Nome' }}</span>
                <MaxInputText :model-value="modelValue.name" :placeholder="modelValue.kind === 'option' ? 'queue' : 'user'" :data-field-path="`params[${index}].name`" aria-label="Nome do parâmetro" class="param-row__input" @update:model-value="onNameChange" />
                <span v-if="nameError" class="param-row__error">{{ nameError }}</span>
            </div>

            <div v-if="modelValue.kind === 'option'" class="param-row__field param-row__field--short">
                <span class="param-row__label">Atalho (-)</span>
                <MaxInputText :model-value="modelValue.shortcut ?? ''" placeholder="Q" maxlength="1" :data-field-path="`params[${index}].shortcut`" aria-label="Atalho" class="param-row__input" @update:model-value="onShortcutChange" />
                <span v-if="shortcutError" class="param-row__error">{{ shortcutError }}</span>
            </div>

            <div class="param-row__field param-row__field--wide">
                <span class="param-row__label">Descrição</span>
                <MaxInputText :model-value="modelValue.description" placeholder="Descrição exibida no --help" :data-field-path="`params[${index}].description`" aria-label="Descrição do parâmetro" class="param-row__input" @update:model-value="onDescriptionChange" />
                <span v-if="descriptionError" class="param-row__error">{{ descriptionError }}</span>
            </div>

            <div class="param-row__field">
                <span class="param-row__label">Valor padrão</span>
                <MaxInputText :model-value="modelValue.default ?? ''" placeholder="Opcional" :disabled="!canEditDefault" :data-field-path="`params[${index}].default`" aria-label="Valor padrão do parâmetro" class="param-row__input" @update:model-value="onDefaultChange" />
                <span v-if="defaultError" class="param-row__error">{{ defaultError }}</span>
            </div>
        </div>

        <div class="param-row__toggles">
            <template v-if="modelValue.kind === 'argument'">
                <MaxInputCheckbox :model-value="Boolean(modelValue.optional)" label="Opcional (?)" class="param-row__checkbox" @update:model-value="onOptionalChange" />
                <MaxInputCheckbox :model-value="Boolean(modelValue.isArray)" label="Array (*)" class="param-row__checkbox" @update:model-value="onArrayChange" />
            </template>
            <template v-else>
                <MaxInputCheckbox :model-value="Boolean(modelValue.acceptsValue)" label="Aceita valor (=)" class="param-row__checkbox" @update:model-value="onAcceptsValueChange" />
                <MaxInputCheckbox :model-value="Boolean(modelValue.isArray)" label="Múltiplos valores (=*)" class="param-row__checkbox" @update:model-value="onOptionArrayChange" />
            </template>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxIconButton, MaxInputCheckbox, MaxInputText, MaxTag } from '@maxvue/max-components-ui';
    import type { ArtisanParam, FieldIssue } from '@/types/bindings';

    export interface ArtisanParamRowProps {
        modelValue: ArtisanParam;
        index: number;
        total: number;
        issues: FieldIssue[];
    }

    export interface ArtisanParamRowEmits {
        (e: 'update:modelValue', value: ArtisanParam): void;
        (e: 'remove'): void;
        (e: 'move', delta: -1 | 1): void;
    }

    const props = defineProps<ArtisanParamRowProps>();
    const emit = defineEmits<ArtisanParamRowEmits>();

    const canEditDefault = computed<boolean>(() => {
        if (props.modelValue.isArray) {
            return false;
        }
        if (props.modelValue.kind === 'argument') {
            return Boolean(props.modelValue.optional);
        }
        return Boolean(props.modelValue.acceptsValue);
    });

    function issueMessageFor(subField: string): string | null {
        const target = `params[${props.index}].${subField}`;
        const found = props.issues.find((i) => i.path === target && i.severity === 'error');
        return found ? found.message : null;
    }

    const nameError = computed(() => issueMessageFor('name'));
    const shortcutError = computed(() => issueMessageFor('shortcut'));
    const descriptionError = computed(() => issueMessageFor('description'));
    const defaultError = computed(() => issueMessageFor('default'));

    function patchParam(patch: Partial<ArtisanParam>): void {
        emit('update:modelValue', {
            ...props.modelValue,
            ...patch
        });
    }

    function onNameChange(val?: string | number): void {
        const cleaned = String(val ?? '').replace(/^--+/, '');
        patchParam({ name: cleaned });
    }

    function onShortcutChange(val?: string | number): void {
        const cleaned = String(val ?? '').replace(/^-+/, '').trim();
        patchParam({ shortcut: cleaned.length > 0 ? cleaned : null });
    }

    function onDescriptionChange(val?: string | number): void {
        patchParam({ description: String(val ?? '') });
    }

    function onDefaultChange(val?: string | number): void {
        const str = String(val ?? '');
        patchParam({ default: str.length > 0 ? str : null });
    }

    function extractChecked(valOrEv: unknown): boolean {
        if (typeof valOrEv === 'boolean') {
            return valOrEv;
        }
        if (valOrEv && typeof valOrEv === 'object' && 'target' in valOrEv) {
            return Boolean((valOrEv as { target: HTMLInputElement }).target?.checked);
        }
        return Boolean(valOrEv);
    }

    function onOptionalChange(valOrEv: unknown): void {
        const checked = extractChecked(valOrEv);
        patchParam({
            optional: checked,
            default: checked ? (props.modelValue.default ?? null) : null
        });
    }

    function onArrayChange(valOrEv: unknown): void {
        const checked = extractChecked(valOrEv);
        patchParam({
            isArray: checked,
            default: checked ? null : (props.modelValue.default ?? null)
        });
    }

    function onAcceptsValueChange(valOrEv: unknown): void {
        const checked = extractChecked(valOrEv);
        patchParam({
            acceptsValue: checked,
            isArray: checked ? Boolean(props.modelValue.isArray) : false,
            default: checked ? (props.modelValue.default ?? null) : null
        });
    }

    function onOptionArrayChange(valOrEv: unknown): void {
        const checked = extractChecked(valOrEv);
        patchParam({
            isArray: checked,
            acceptsValue: checked ? true : Boolean(props.modelValue.acceptsValue),
            default: checked ? null : (props.modelValue.default ?? null)
        });
    }
</script>

<style lang="scss" scoped>
    .param-row {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 0.625rem;
        border: 1px solid var(--background-700);
        border-radius: 0.375rem;
        background: var(--background-900);

        &__topbar {
            display: flex;
            align-items: center;
            justify-content: space-between;
        }

        &__badge {
            font-size: 0.6875rem;
            font-weight: 600;
            padding: 0.125rem 0.5rem;
            border-radius: 0.25rem;

            &--arg {
                background: var(--background-800);
                color: var(--max-primary-400);
            }

            &--opt {
                background: var(--background-800);
                color: var(--max-info-400);
            }
        }

        &__actions {
            display: flex;
            align-items: center;
            gap: 0.25rem;
        }

        &__icon-btn {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            width: 1.5rem;
            height: 1.5rem;
            border: 1px solid var(--background-700);
            border-radius: 0.25rem;
            background: var(--background-800);
            color: var(--background-200);
            cursor: pointer;

            &:disabled {
                opacity: 0.4;
                cursor: not-allowed;
            }

            &--danger:hover:not(:disabled) {
                border-color: var(--max-danger-500);
                color: var(--max-danger-400);
            }
        }

        &__grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(8rem, 1fr));
            gap: 0.5rem;
        }

        &__field {
            display: flex;
            flex-direction: column;
            gap: 0.25rem;

            &--short {
                max-width: 5.5rem;
            }

            &--wide {
                grid-column: span 2;
            }
        }

        &__label {
            font-size: 0.6875rem;
            color: var(--background-300);
        }

        &__input {
            padding: 0.3125rem 0.5rem;
            border: 1px solid var(--background-700);
            border-radius: 0.25rem;
            background: var(--background-950);
            color: var(--background-50);
            font-size: 0.75rem;
            font-family: var(--mt-font-mono);

            &:disabled {
                opacity: 0.5;
            }
        }

        &__error {
            font-size: 0.6875rem;
            color: var(--max-danger-400);
        }

        &__toggles {
            display: flex;
            flex-wrap: wrap;
            gap: 0.75rem;
        }

        &__checkbox {
            display: inline-flex;
            align-items: center;
            gap: 0.375rem;
            font-size: 0.75rem;
            color: var(--background-200);
            cursor: pointer;
        }
    }
</style>
