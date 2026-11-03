<template>
    <div class="editor-selector">
        <div class="editor-selector__field">
            <label class="editor-selector__label" for="mt-editor-kind">Editor preferencial para links do terminal</label>
            <MaxInputSelect id="mt-editor-kind" :model-value="props.modelValue.kind" :options="selectOptions" option-label="label" option-value="value" option-name="label" :disabled="props.disabled" class="editor-selector__select" @change="onKindChange" />
        </div>
        <div v-if="props.modelValue.kind === 'custom'" class="editor-selector__custom">
            <div class="editor-selector__field">
                <label class="editor-selector__label" for="mt-editor-custom-argv">Comando e argumentos personalizados (use {file}, {line}, {column})</label>
                <MaxInputText id="mt-editor-custom-argv" :model-value="customArgvText" placeholder="Ex.: code --goto {file}:{line}:{column}" :disabled="props.disabled" class="editor-selector__input" @update:model-value="onCustomArgvChange" @change="onCustomArgvChange" />
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxInputSelect, MaxInputText } from '@maxvue/max-components-ui';
    import type {
        EditorAvailability_Serialize,
        EditorKind,
        EditorSettings_Serialize
    } from '@/types/bindings';

    export interface EditorSelectorProps {
        modelValue: EditorSettings_Serialize;
        editors?: EditorAvailability_Serialize[];
        disabled?: boolean;
    }

    const props = withDefaults(defineProps<EditorSelectorProps>(), {
        editors: () => [],
        disabled: false
    });

    const emit = defineEmits<{
        'update:modelValue': [settings: EditorSettings_Serialize];
    }>();

    interface EditorOptionItem {
        kind: EditorKind;
        label: string;
    }

    const editorOptions: EditorOptionItem[] = [
        { kind: 'vs_code', label: 'Visual Studio Code (code --goto)' },
        { kind: 'vs_code_insiders', label: 'VS Code Insiders (code-insiders --goto)' },
        { kind: 'max_code', label: 'MaxCode / VS Code URI (vscode://file)' },
        { kind: 'custom', label: 'Comando personalizado (argv)' }
    ];

    const customArgvText = computed<string>(() => (props.modelValue.customArgv ?? []).join(' '));

    const selectOptions = computed(() => {
        return editorOptions.map((item) => ({
            label: `${item.label}${availabilitySuffix(item.kind)}`,
            value: item.kind
        }));
    });

    function availabilitySuffix(kind: EditorKind): string {
        if (kind === 'custom') {
            return '';
        }
        const entry = props.editors.find((e) => e.kind === kind);
        if (!entry) {
            return '';
        }
        return entry.available ? ' — Disponível' : ' — Não detectado';
    }

    function onKindChange(valOrEv: unknown): void {
        const nextKind = (typeof valOrEv === 'string' ? valOrEv : (valOrEv as { target?: HTMLSelectElement })?.target?.value) as EditorKind;
        const defaultArgv =
            nextKind === 'custom' && props.modelValue.customArgv.length === 0
                ? ['code', '--goto', '{file}:{line}:{column}']
                : [...props.modelValue.customArgv];
        emit('update:modelValue', {
            kind: nextKind,
            customArgv: defaultArgv
        });
    }

    function onCustomArgvChange(valOrEv: unknown): void {
        const raw = typeof valOrEv === 'string' ? valOrEv : ((valOrEv as { target?: HTMLInputElement })?.target?.value ?? '');
        const tokens = raw
            .split(/\s+/)
            .map((s) => s.trim())
            .filter(Boolean);
        emit('update:modelValue', {
            ...props.modelValue,
            customArgv: tokens.length > 0 ? tokens : ['code', '{file}']
        });
    }
</script>

<style lang="scss" scoped>
    .editor-selector {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
    }

    .editor-selector__field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .editor-selector__label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--background-200);
    }

    .editor-selector__select,
    .editor-selector__input {
        height: 2.125rem;
        padding: 0 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-900);
        color: var(--background-100);
        font-size: 0.8125rem;

        &:disabled {
            opacity: 0.45;
            cursor: not-allowed;
        }
    }

    .editor-selector__custom {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }
</style>
