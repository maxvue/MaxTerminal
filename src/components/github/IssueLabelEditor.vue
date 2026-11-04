<template>
    <div class="issue-label-editor" aria-label="Editor de labels da issue">
        <div class="issue-label-editor__row">
            <label class="issue-label-editor__field">
                <span class="issue-label-editor__field-label">Estado MaxCode (exclusivo):</span>
                <select class="issue-label-editor__select" aria-label="Selecionar estado da issue" :value="currentStateLabel" :disabled="props.disabled" @change="onStateChange">
                    <option value="">(Sem estado)</option>
                    <option v-for="spec in stateSpecs" :key="spec.name" :value="spec.name">{{ spec.name }}</option>
                </select>
            </label>
            <label class="issue-label-editor__field">
                <span class="issue-label-editor__field-label">Encerramento (exclusivo):</span>
                <select class="issue-label-editor__select" aria-label="Selecionar label de encerramento" :value="currentClosureLabel" :disabled="props.disabled" @change="onClosureChange">
                    <option value="">(Nenhum)</option>
                    <option v-for="spec in closureSpecs" :key="spec.name" :value="spec.name">{{ spec.name }}</option>
                </select>
            </label>
        </div>
        <div class="issue-label-editor__categories" role="group" aria-label="Categorias MaxCode">
            <span class="issue-label-editor__field-label">Categorias:</span>
            <MaxInputCheckbox v-for="spec in categorySpecs" :key="spec.name" :model-value="selectedCategories.includes(spec.name)" :label="spec.name" :disabled="props.disabled" class="issue-label-editor__checkbox-label" @update:model-value="onToggleCategory(spec.name)" />
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxInputCheckbox } from '@maxvue/max-components-ui';
    import {
        CANONICAL_CATEGORY_LABELS,
        CANONICAL_CLOSURE_LABELS,
        CANONICAL_STATE_LABELS
    } from '@/constants/githubLabels';
    import type { LabelUpdatePatch, ProjectId, RepoLabel } from '@/types/bindings';

    export interface IssueLabelEditorProps {
        projectId: ProjectId;
        issueNumber: number;
        labels: RepoLabel[];
        disabled?: boolean;
    }

    const props = withDefaults(defineProps<IssueLabelEditorProps>(), {
        disabled: false
    });

    const emit = defineEmits<{
        'update-labels': [patch: LabelUpdatePatch];
    }>();

    const stateSpecs = CANONICAL_STATE_LABELS;
    const closureSpecs = CANONICAL_CLOSURE_LABELS;
    const categorySpecs = CANONICAL_CATEGORY_LABELS;

    const currentStateLabel = computed<string>(() => {
        const found = props.labels.find((l) => l.canonicalGroup === 'state');
        return found?.name ?? '';
    });

    const currentClosureLabel = computed<string>(() => {
        const found = props.labels.find((l) => l.canonicalGroup === 'closure');
        return found?.name ?? '';
    });

    const selectedCategories = computed<string[]>(() =>
        props.labels.filter((l) => l.canonicalGroup === 'category').map((l) => l.name)
    );

    function onStateChange(event: Event): void {
        const target = event.target as HTMLSelectElement;
        const val = target.value;
        emit('update-labels', {
            projectId: props.projectId,
            issueNumber: props.issueNumber,
            stateChange: val ? { op: 'set', value: val } : { op: 'clear' },
            closureChange: { op: 'keep' },
            addCategories: [],
            removeCategories: [],
            addCustom: [],
            removeCustom: [],
            clientMutationId: null
        });
    }

    function onClosureChange(event: Event): void {
        const target = event.target as HTMLSelectElement;
        const val = target.value;
        emit('update-labels', {
            projectId: props.projectId,
            issueNumber: props.issueNumber,
            stateChange: { op: 'keep' },
            closureChange: val ? { op: 'set', value: val } : { op: 'clear' },
            addCategories: [],
            removeCategories: [],
            addCustom: [],
            removeCustom: [],
            clientMutationId: null
        });
    }

    function onToggleCategory(categoryName: string): void {
        const has = selectedCategories.value.includes(categoryName);
        emit('update-labels', {
            projectId: props.projectId,
            issueNumber: props.issueNumber,
            stateChange: { op: 'keep' },
            closureChange: { op: 'keep' },
            addCategories: has ? [] : [categoryName],
            removeCategories: has ? [categoryName] : [],
            addCustom: [],
            removeCustom: [],
            clientMutationId: null
        });
    }
</script>

<style lang="scss" scoped>
    .issue-label-editor {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 0.625rem 0.875rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .issue-label-editor__row {
        display: flex;
        flex-wrap: wrap;
        gap: 0.75rem;
    }

    .issue-label-editor__field {
        display: flex;
        align-items: center;
        gap: 0.375rem;
    }

    .issue-label-editor__field-label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .issue-label-editor__select {
        height: 1.75rem;
        padding: 0 0.5rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
    }

    .issue-label-editor__categories {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.625rem;
    }

    .issue-label-editor__checkbox-label {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        font-size: 0.75rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
        cursor: pointer;
    }
</style>
