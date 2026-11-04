<template>
    <div class="param-row" :data-testid="`artisan-param-row-${index}`">
        <div class="param-row__topbar">
            <span class="param-row__badge" :class="modelValue.kind === 'argument' ? 'param-row__badge--arg' : 'param-row__badge--opt'">
                {{ modelValue.kind === 'argument' ? 'Argumento' : 'Opção (--flag)' }}
            </span>
            <div class="param-row__actions">
                <MaxIconButton icon="mdi:arrow-up" label="Mover parâmetro para cima" variant="text" severity="secondary" class="param-row__icon-btn" :disabled="index === 0" title="Mover para cima" @click="emit('move', -1)" />
                <MaxIconButton icon="mdi:arrow-down" label="Mover parâmetro para baixo" variant="text" severity="secondary" class="param-row__icon-btn" :disabled="index >= total - 1" title="Mover para baixo" @click="emit('move', 1)" />
                <MaxIconButton icon="mdi:trash-can-outline" label="Remover parâmetro" variant="text" severity="danger" class="param-row__icon-btn param-row__icon-btn--danger" title="Remover parâmetro" @click="emit('remove')" />
            </div>
        </div>

        <div class="param-row__grid">
            <label class="param-row__field">
                <span class="param-row__label">{{ modelValue.kind === 'option' ? 'Nome (--)' : 'Nome' }}</span>
                <input type="text" class="param-row__input" :data-field-path="`params[${index}].name`" :placeholder="modelValue.kind === 'option' ? 'queue' : 'user'" :value="modelValue.name" @input="onNameInput">
                <span v-if="nameError" class="param-row__error">{{ nameError }}</span>
            </label>

            <label v-if="modelValue.kind === 'option'" class="param-row__field param-row__field--short">
                <span class="param-row__label">Atalho (-)</span>
                <input type="text" maxlength="1" class="param-row__input" :data-field-path="`params[${index}].shortcut`" placeholder="Q" :value="modelValue.shortcut ?? ''" @input="onShortcutInput">
                <span v-if="shortcutError" class="param-row__error">{{ shortcutError }}</span>
            </label>

            <label class="param-row__field param-row__field--wide">
                <span class="param-row__label">Descrição</span>
                <input type="text" class="param-row__input" :data-field-path="`params[${index}].description`" placeholder="Descrição exibida no --help" :value="modelValue.description" @input="onDescriptionInput">
                <span v-if="descriptionError" class="param-row__error">{{ descriptionError }}</span>
            </label>

            <label class="param-row__field">
                <span class="param-row__label">Valor padrão</span>
                <input type="text" class="param-row__input" :data-field-path="`params[${index}].default`" placeholder="Opcional" :disabled="!canEditDefault" :value="modelValue.default ?? ''" @input="onDefaultInput">
                <span v-if="defaultError" class="param-row__error">{{ defaultError }}</span>
            </label>
        </div>

        <div class="param-row__toggles">
            <template v-if="modelValue.kind === 'argument'">
                <label class="param-row__checkbox">
                    <input type="checkbox" :checked="modelValue.optional" @change="onOptionalChange">
                    <span>Opcional (?)</span>
                </label>
                <label class="param-row__checkbox">
                    <input type="checkbox" :checked="modelValue.isArray" @change="onArrayChange">
                    <span>Array (*)</span>
                </label>
            </template>
            <template v-else>
                <label class="param-row__checkbox">
                    <input type="checkbox" :checked="modelValue.acceptsValue" @change="onAcceptsValueChange">
                    <span>Aceita valor (=)</span>
                </label>
                <label class="param-row__checkbox">
                    <input type="checkbox" :checked="modelValue.isArray" @change="onOptionArrayChange">
                    <span>Múltiplos valores (=*)</span>
                </label>
            </template>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxIconButton } from '@maxvue/max-components-ui';
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

    function onNameInput(ev: Event): void {
        const val = (ev.target as HTMLInputElement).value.replace(/^--+/, '');
        patchParam({ name: val });
    }

    function onShortcutInput(ev: Event): void {
        const val = (ev.target as HTMLInputElement).value.replace(/^-+/, '').trim();
        patchParam({ shortcut: val.length > 0 ? val : null });
    }

    function onDescriptionInput(ev: Event): void {
        const val = (ev.target as HTMLInputElement).value;
        patchParam({ description: val });
    }

    function onDefaultInput(ev: Event): void {
        const val = (ev.target as HTMLInputElement).value;
        patchParam({ default: val.length > 0 ? val : null });
    }

    function onOptionalChange(ev: Event): void {
        const checked = (ev.target as HTMLInputElement).checked;
        patchParam({
            optional: checked,
            default: checked ? (props.modelValue.default ?? null) : null
        });
    }

    function onArrayChange(ev: Event): void {
        const checked = (ev.target as HTMLInputElement).checked;
        patchParam({
            isArray: checked,
            default: checked ? null : (props.modelValue.default ?? null)
        });
    }

    function onAcceptsValueChange(ev: Event): void {
        const checked = (ev.target as HTMLInputElement).checked;
        patchParam({
            acceptsValue: checked,
            isArray: checked ? Boolean(props.modelValue.isArray) : false,
            default: checked ? (props.modelValue.default ?? null) : null
        });
    }

    function onOptionArrayChange(ev: Event): void {
        const checked = (ev.target as HTMLInputElement).checked;
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
