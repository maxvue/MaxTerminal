<template>
    <div class="params-builder" data-testid="artisan-params-builder">
        <div class="params-builder__header">
            <span class="params-builder__title">Argumentos e Flags (--options)</span>
            <div class="params-builder__buttons">
                <MaxButton label="Adicionar argumento" icon="mdi:plus" severity="secondary" size="small" class="params-builder__add-btn" data-testid="add-argument-btn" @click="addArgument" />
                <MaxButton label="Adicionar opção" icon="mdi:plus" severity="secondary" size="small" class="params-builder__add-btn" data-testid="add-option-btn" @click="addOption" />
            </div>
        </div>

        <p v-if="modelValue.length === 0" class="params-builder__empty">
            Nenhum argumento ou opção configurado.
        </p>

        <div v-else class="params-builder__list">
            <ArtisanParamRow v-for="(param, idx) in modelValue" :key="idx" :model-value="param" :index="idx" :total="modelValue.length" :issues="issues" @update:model-value="(val) => updateAt(idx, val)" @remove="removeAt(idx)" @move="(delta) => moveAt(idx, delta)" />
        </div>
    </div>
</template>

<script setup lang="ts">
    import { MaxButton } from '@maxvue/max-components-ui';
    import ArtisanParamRow from '@/components/quick-commands/ArtisanParamRow.vue';
    import type { ArtisanParam, FieldIssue } from '@/types/bindings';

    export interface ArtisanParamsBuilderProps {
        modelValue: ArtisanParam[];
        issues: FieldIssue[];
    }

    export interface ArtisanParamsBuilderEmits {
        (e: 'update:modelValue', value: ArtisanParam[]): void;
    }

    const props = defineProps<ArtisanParamsBuilderProps>();
    const emit = defineEmits<ArtisanParamsBuilderEmits>();

    function addArgument(): void {
        const next: ArtisanParam = {
            kind: 'argument',
            name: '',
            description: '',
            optional: false,
            isArray: false,
            default: null,
            shortcut: null,
            acceptsValue: false
        };
        emit('update:modelValue', [...props.modelValue, next]);
    }

    function addOption(): void {
        const next: ArtisanParam = {
            kind: 'option',
            name: '',
            description: '',
            optional: false,
            isArray: false,
            default: null,
            shortcut: null,
            acceptsValue: false
        };
        emit('update:modelValue', [...props.modelValue, next]);
    }

    function updateAt(index: number, updated: ArtisanParam): void {
        const copy = [...props.modelValue];
        copy[index] = updated;
        emit('update:modelValue', copy);
    }

    function removeAt(index: number): void {
        const copy = props.modelValue.filter((_, idx) => idx !== index);
        emit('update:modelValue', copy);
    }

    function moveAt(index: number, delta: -1 | 1): void {
        const target = index + delta;
        if (target < 0 || target >= props.modelValue.length) {
            return;
        }
        const copy = [...props.modelValue];
        const current = copy[index];
        const swapped = copy[target];
        if (!current || !swapped) {
            return;
        }
        copy[index] = swapped;
        copy[target] = current;
        emit('update:modelValue', copy);
    }
</script>

<style lang="scss" scoped>
    .params-builder {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;

        &__header {
            display: flex;
            align-items: center;
            justify-content: space-between;
            flex-wrap: wrap;
            gap: 0.5rem;
        }

        &__title {
            font-size: 0.8125rem;
            font-weight: 600;
            color: var(--background-100);
        }

        &__buttons {
            display: flex;
            align-items: center;
            gap: 0.375rem;
        }

        &__add-btn {
            display: inline-flex;
            align-items: center;
            gap: 0.25rem;
            padding: 0.25rem 0.5rem;
            border: 1px solid var(--background-700);
            border-radius: 0.25rem;
            background: var(--background-800);
            color: var(--background-100);
            font-size: 0.75rem;
            cursor: pointer;

            &:hover {
                border-color: var(--max-primary-500);
                color: var(--background-50);
            }
        }

        &__empty {
            margin: 0;
            padding: 0.625rem;
            border: 1px dashed var(--background-700);
            border-radius: 0.375rem;
            color: var(--background-400);
            font-size: 0.75rem;
            text-align: center;
        }

        &__list {
            display: flex;
            flex-direction: column;
            gap: 0.5rem;
        }
    }
</style>
