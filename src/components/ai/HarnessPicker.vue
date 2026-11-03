<template>
    <div class="harness-picker" data-testid="harness-picker">
        <div class="harness-picker__steps">
            <button type="button" class="harness-picker__step" :class="{ 'is-active': stage === 0 }" data-testid="step-harness" @click="goToStage(0)">
                <span>1. Harness</span>
                <MaxTag v-if="selectedHarness" size="sm" variant="primary" class="harness-picker__step-val">{{ selectedHarness.label }}</MaxTag>
            </button>
            <button type="button" class="harness-picker__step" :class="{ 'is-active': stage === 1 }" :disabled="!selectedHarness" data-testid="step-model" @click="goToStage(1)">
                <span>2. Modelo</span>
                <MaxTag v-if="selectedModel" size="sm" variant="primary" class="harness-picker__step-val">{{ selectedModel.label }}</MaxTag>
            </button>
            <button v-if="hasReasoningLevels" type="button" class="harness-picker__step" :class="{ 'is-active': stage === 2 }" :disabled="!selectedModel" data-testid="step-reasoning" @click="goToStage(2)">
                <span>3. Raciocínio</span>
                <MaxTag v-if="selectedReasoningId" size="sm" variant="primary" class="harness-picker__step-val">{{ selectedReasoningId }}</MaxTag>
            </button>
        </div>

        <div v-if="stage === 0" class="harness-picker__list" role="listbox" aria-label="Selecionar Harness" data-testid="stage-harnesses">
            <button v-for="harness in harnesses" :key="harness.id" type="button" role="option" class="harness-picker__option" :class="{ 'is-selected': selectedHarness?.id === harness.id }" :aria-selected="selectedHarness?.id === harness.id" :data-testid="`harness-opt-${harness.id}`" @click="onPickHarness(harness)">
                <MaxIcon :icon="harness.icon || 'lucide:bot'" :size="1" class="harness-picker__option-icon" />
                <div class="harness-picker__option-info">
                    <span class="harness-picker__option-label">{{ harness.label }}</span>
                    <MaxTag size="sm" variant="secondary" class="harness-picker__option-meta">{{ harness.models.length }} modelo(s)</MaxTag>
                </div>
            </button>
        </div>

        <div v-else-if="stage === 1" class="harness-picker__list" role="listbox" aria-label="Selecionar Modelo" data-testid="stage-models">
            <p v-if="availableModels.length === 0" class="harness-picker__empty" data-testid="empty-models">Nenhum modelo carregado para {{ selectedHarness?.label }}.</p>
            <button v-for="model in availableModels" :key="model.id" type="button" role="option" class="harness-picker__option" :class="{ 'is-selected': selectedModel?.id === model.id }" :aria-selected="selectedModel?.id === model.id" :data-testid="`model-opt-${model.id}`" @click="onPickModel(model)">
                <MaxIcon icon="lucide:cpu" :size="1" class="harness-picker__option-icon" />
                <div class="harness-picker__option-info">
                    <span class="harness-picker__option-label">{{ model.label }}</span>
                    <span class="harness-picker__option-meta">{{ model.id }}</span>
                </div>
            </button>
        </div>

        <div v-else class="harness-picker__list" role="listbox" aria-label="Selecionar Nível de Raciocínio" data-testid="stage-reasoning">
            <button v-for="level in availableReasoningLevels" :key="level.id" type="button" role="option" class="harness-picker__option" :class="{ 'is-selected': selectedReasoningId === level.id }" :aria-selected="selectedReasoningId === level.id" :data-testid="`reasoning-opt-${level.id}`" @click="onPickReasoning(level)">
                <MaxIcon icon="lucide:brain-circuit" :size="1" class="harness-picker__option-icon" />
                <div class="harness-picker__option-info">
                    <span class="harness-picker__option-label">{{ level.label }}</span>
                    <span class="harness-picker__option-meta">{{ level.id }}</span>
                </div>
            </button>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
    import MaxTag from '@maxvue/max-components-ui/components/MaxTag';
    import { CANONICAL_HARNESSES_FALLBACK } from '@/stores/ai';
    import type {
        AiModel_Serialize,
        AiSelection_Serialize,
        Harness_Serialize,
        ReasoningLevel_Serialize
    } from '@/types/bindings';

    export interface HarnessPickerProps {
        harnesses?: Harness_Serialize[];
        modelValue?: AiSelection_Serialize | null;
    }

    const props = withDefaults(defineProps<HarnessPickerProps>(), {
        harnesses: () => [...CANONICAL_HARNESSES_FALLBACK],
        modelValue: null
    });

    const emit = defineEmits<{
        'update:modelValue': [selection: AiSelection_Serialize];
        select: [selection: AiSelection_Serialize];
    }>();

    const stage = ref<0 | 1 | 2>(0);
    const selectedHarnessId = ref<string | null>(props.modelValue?.harnessId ?? null);
    const selectedModelId = ref<string | null>(props.modelValue?.modelId ?? null);
    const selectedReasoningId = ref<string | null>(props.modelValue?.reasoningId ?? null);

    watch(
        () => props.modelValue,
        (next) => {
            selectedHarnessId.value = next?.harnessId ?? null;
            selectedModelId.value = next?.modelId ?? null;
            selectedReasoningId.value = next?.reasoningId ?? null;
        }
    );

    const selectedHarness = computed<Harness_Serialize | null>(() => {
        if (!selectedHarnessId.value) {
            return null;
        }
        return props.harnesses.find((h) => h.id === selectedHarnessId.value) ?? null;
    });

    const availableModels = computed<AiModel_Serialize[]>(() => {
        return selectedHarness.value?.models ?? [];
    });

    const selectedModel = computed<AiModel_Serialize | null>(() => {
        if (!selectedModelId.value) {
            return null;
        }
        return availableModels.value.find((m) => m.id === selectedModelId.value) ?? null;
    });

    const availableReasoningLevels = computed<ReasoningLevel_Serialize[]>(() => {
        const levels = selectedModel.value?.reasoningLevels ?? [];
        return [...levels].sort((a, b) => a.rank - b.rank);
    });

    const hasReasoningLevels = computed<boolean>(() => {
        return availableReasoningLevels.value.length > 0;
    });

    function goToStage(nextStage: 0 | 1 | 2): void {
        if (nextStage === 1 && !selectedHarness.value) {
            return;
        }
        if (nextStage === 2 && (!selectedModel.value || !hasReasoningLevels.value)) {
            return;
        }
        stage.value = nextStage;
    }

    function onPickHarness(harness: Harness_Serialize): void {
        selectedHarnessId.value = harness.id;
        selectedModelId.value = null;
        selectedReasoningId.value = null;
        stage.value = 1;
    }

    function onPickModel(model: AiModel_Serialize): void {
        if (!selectedHarness.value) {
            return;
        }
        selectedModelId.value = model.id;

        if (!model.reasoningLevels || model.reasoningLevels.length === 0) {
            selectedReasoningId.value = null;
            const selection: AiSelection_Serialize = {
                harnessId: selectedHarness.value.id,
                modelId: model.id,
                reasoningId: null
            };
            emit('update:modelValue', selection);
            emit('select', selection);
            return;
        }

        selectedReasoningId.value = model.defaultReasoning ?? model.reasoningLevels[0]?.id ?? null;
        stage.value = 2;
    }

    function onPickReasoning(level: ReasoningLevel_Serialize): void {
        if (!selectedHarness.value || !selectedModel.value) {
            return;
        }
        selectedReasoningId.value = level.id;
        const selection: AiSelection_Serialize = {
            harnessId: selectedHarness.value.id,
            modelId: selectedModel.value.id,
            reasoningId: level.id
        };
        emit('update:modelValue', selection);
        emit('select', selection);
    }
</script>

<style lang="scss" scoped>
    .harness-picker {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
    }

    .harness-picker__steps {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        flex-wrap: wrap;
    }

    .harness-picker__step {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.25rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-750);
        background: var(--background-900);
        color: var(--background-300);
        font-size: 0.75rem;
        cursor: pointer;

        &:disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }

        &.is-active {
            border-color: var(--max-primary-500);
            background: color-mix(in srgb, var(--max-primary-500) 15%, transparent);
            color: var(--background-50);
        }
    }

    .harness-picker__step-val {
        color: var(--max-primary-400);
        font-weight: 600;
    }

    .harness-picker__list {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(11rem, 1fr));
        gap: 0.375rem;
    }

    .harness-picker__empty {
        grid-column: 1 / -1;
        margin: 0;
        padding: 0.5rem;
        font-size: 0.75rem;
        color: var(--background-400);
    }

    .harness-picker__option {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.5rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-800);
        background: var(--background-925);
        color: var(--background-100);
        text-align: left;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
        }

        &.is-selected {
            border-color: var(--max-primary-500);
            background: color-mix(in srgb, var(--max-primary-500) 18%, transparent);
        }
    }

    .harness-picker__option-icon {
        font-size: 1rem;
        color: var(--max-primary-400);
        flex-shrink: 0;
    }

    .harness-picker__option-info {
        display: flex;
        flex-direction: column;
        gap: 0.0625rem;
        min-width: 0;
    }

    .harness-picker__option-label {
        font-size: 0.75rem;
        font-weight: 600;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .harness-picker__option-meta {
        font-size: 0.6875rem;
        color: var(--background-400);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }
</style>
