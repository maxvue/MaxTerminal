<template>
    <div class="shortcut-recorder">
        <div class="shortcut-recorder__row">
            <div ref="captureBoxRef" class="shortcut-recorder__box" :class="{ 'is-recording': recording }" tabindex="0" role="button" aria-label="Combinação de teclas do atalho global" @keydown="onKeyDown" @blur="cancelRecording">
                <span v-if="recording" class="shortcut-recorder__prompt">Pressione a combinação desejada (Esc para cancelar)…</span>
                <span v-else class="shortcut-recorder__value">{{ props.modelValue }}</span>
            </div>
            <button v-if="!recording" type="button" class="shortcut-recorder__btn" :disabled="props.disabled" @click="beginCapture">
                Gravar atalho
            </button>
            <button v-else type="button" class="shortcut-recorder__btn shortcut-recorder__btn--secondary" @click="cancelRecording">
                Cancelar
            </button>
            <button type="button" class="shortcut-recorder__btn shortcut-recorder__btn--secondary" :disabled="props.disabled || props.modelValue === DEFAULT_ACCELERATOR" @click="resetDefault">
                Padrão
            </button>
        </div>
        <p v-if="validationError" class="shortcut-recorder__error" role="alert">{{ validationError }}</p>
    </div>
</template>

<script setup lang="ts">
    import { nextTick, ref } from 'vue';
    import { useShortcutRecorder } from '@/composables/useShortcutRecorder';

    export interface ShortcutRecorderProps {
        modelValue: string;
        disabled?: boolean;
    }

    const DEFAULT_ACCELERATOR = 'Ctrl+Alt+Backquote';

    const props = withDefaults(defineProps<ShortcutRecorderProps>(), {
        disabled: false
    });

    const emit = defineEmits<{
        'update:modelValue': [accelerator: string];
    }>();

    const captureBoxRef = ref<HTMLElement | null>(null);
    const { recording, validationError, startRecording, cancelRecording, handleKeyDown } =
        useShortcutRecorder();

    function beginCapture(): void {
        if (props.disabled) {
            return;
        }
        startRecording();
        void nextTick(() => {
            captureBoxRef.value?.focus();
        });
    }

    function onKeyDown(event: KeyboardEvent): void {
        if (!recording.value) {
            return;
        }
        const res = handleKeyDown(event);
        if (res && res.accelerator) {
            emit('update:modelValue', res.accelerator);
        }
    }

    function resetDefault(): void {
        cancelRecording();
        emit('update:modelValue', DEFAULT_ACCELERATOR);
    }
</script>

<style lang="scss" scoped>
    .shortcut-recorder {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .shortcut-recorder__row {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .shortcut-recorder__box {
        flex: 1;
        display: flex;
        align-items: center;
        min-height: 2.125rem;
        padding: 0 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-900);
        font-family: var(--mt-font-mono);
        font-size: 0.8125rem;
        color: var(--background-100);
        cursor: pointer;

        &.is-recording {
            border-color: var(--max-primary-500);
            background: color-mix(in srgb, var(--max-primary-500) 12%, var(--background-900));
        }
    }

    .shortcut-recorder__prompt {
        color: var(--max-primary-400);
        font-family: var(--mt-font-sans);
    }

    .shortcut-recorder__value {
        font-weight: 600;
    }

    .shortcut-recorder__btn {
        height: 2.125rem;
        padding: 0 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-primary-500);
        background: color-mix(in srgb, var(--max-primary-500) 16%, transparent);
        color: var(--max-primary-400);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.45;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            background: color-mix(in srgb, var(--max-primary-500) 26%, transparent);
        }

        &--secondary {
            border-color: var(--background-600);
            background: var(--background-800);
            color: var(--background-200);

            &:hover:not(:disabled) {
                background: var(--background-700);
            }
        }
    }

    .shortcut-recorder__error {
        margin: 0;
        font-size: 0.75rem;
        color: var(--max-danger-400);
    }
</style>
