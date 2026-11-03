<template>
    <div v-if="open" class="destructive-confirm" role="dialog" aria-modal="true" aria-labelledby="destructive-confirm-title" data-testid="destructive-confirm-modal">
        <div class="destructive-confirm__backdrop" @click="onCancel" />
        <div class="destructive-confirm__panel">
            <header class="destructive-confirm__header">
                <div class="destructive-confirm__title-wrap">
                    <MaxIcon icon="lucide:skull" class="destructive-confirm__title-icon" />
                    <h3 id="destructive-confirm-title" class="destructive-confirm__title">Confirmar execução de comando destrutivo</h3>
                </div>
                <button type="button" class="destructive-confirm__close" aria-label="Fechar confirmação" @click="onCancel">
                    <MaxIcon icon="lucide:x" />
                </button>
            </header>

            <p class="destructive-confirm__warning">Este comando pode causar perda irreversível de dados, remoção recursiva ou alteração crítica do sistema.</p>

            <div class="destructive-confirm__command-box">
                <code class="destructive-confirm__command" data-testid="destructive-command-preview">{{ command }}</code>
            </div>

            <ul v-if="risk.reasons.length > 0" class="destructive-confirm__reasons" data-testid="destructive-reasons">
                <li v-for="(reason, idx) in risk.reasons" :key="`${reason.ruleId}-${idx}`" class="destructive-confirm__reason">
                    <span class="destructive-confirm__reason-msg">{{ reason.message }}</span>
                    <code class="destructive-confirm__reason-seg">{{ reason.segment }}</code>
                </li>
            </ul>

            <label class="destructive-confirm__field">
                <span class="destructive-confirm__field-label">Para liberar a execução, digite exatamente <strong class="destructive-confirm__token" data-testid="expected-confirm-token">{{ expectedToken }}</strong> abaixo:</span>
                <input v-model="typedToken" type="text" class="destructive-confirm__input" :placeholder="expectedToken" autocomplete="off" spellcheck="false" data-testid="confirm-token-input" @keydown.enter.prevent="onConfirm" @keydown.esc.prevent="onCancel">
            </label>

            <footer class="destructive-confirm__footer">
                <button type="button" class="destructive-confirm__btn is-secondary" data-testid="btn-cancel-destructive" @click="onCancel">Cancelar</button>
                <button type="button" class="destructive-confirm__btn is-danger" :disabled="!isConfirmed" data-testid="btn-confirm-destructive" @click="onConfirm">Executar comando destrutivo</button>
            </footer>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
    import type { RiskAssessment_Serialize } from '@/types/bindings';

    export interface DestructiveConfirmProps {
        open: boolean;
        command: string;
        risk: RiskAssessment_Serialize;
    }

    const props = defineProps<DestructiveConfirmProps>();

    const emit = defineEmits<{
        confirm: [typedToken: string];
        cancel: [];
    }>();

    const typedToken = ref<string>('');

    const expectedToken = computed<string>(() => {
        const token = props.risk.confirmToken?.trim();
        return token && token.length > 0 ? token : 'CONFIRMAR';
    });

    const isConfirmed = computed<boolean>(() => {
        return expectedToken.value.length > 0 && typedToken.value === expectedToken.value;
    });

    watch(
        () => [props.open, props.command, props.risk.confirmToken],
        () => {
            typedToken.value = '';
        }
    );

    function onConfirm(): void {
        if (!isConfirmed.value) {
            return;
        }
        emit('confirm', typedToken.value);
    }

    function onCancel(): void {
        typedToken.value = '';
        emit('cancel');
    }
</script>

<style lang="scss" scoped>
    .destructive-confirm {
        position: fixed;
        inset: 0;
        z-index: 1200;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 1rem;
    }

    .destructive-confirm__backdrop {
        position: absolute;
        inset: 0;
        background: rgb(0 0 0 / 72%);
    }

    .destructive-confirm__panel {
        position: relative;
        z-index: 1;
        width: 100%;
        max-width: 32rem;
        display: flex;
        flex-direction: column;
        gap: 0.875rem;
        padding: 1.125rem;
        border-radius: 0.5rem;
        border: 1px solid var(--max-danger-500);
        background: var(--background-925);
        color: var(--background-50);
    }

    .destructive-confirm__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
    }

    .destructive-confirm__title-wrap {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .destructive-confirm__title-icon {
        font-size: 1.125rem;
        color: var(--max-danger-400);
    }

    .destructive-confirm__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
        color: var(--max-danger-400);
    }

    .destructive-confirm__close {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.75rem;
        height: 1.75rem;
        border-radius: 0.25rem;
        border: 1px solid transparent;
        background: transparent;
        color: var(--background-300);
        cursor: pointer;

        &:hover {
            background: var(--background-800);
            color: var(--background-50);
        }
    }

    .destructive-confirm__warning {
        margin: 0;
        font-size: 0.8125rem;
        color: var(--background-200);
        line-height: 1.4;
    }

    .destructive-confirm__command-box {
        padding: 0.625rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-750);
        background: var(--background-950);
        overflow-x: auto;
    }

    .destructive-confirm__command {
        font-family: var(--mt-font-mono);
        font-size: 0.8125rem;
        color: var(--max-danger-300);
        white-space: pre-wrap;
        word-break: break-all;
    }

    .destructive-confirm__reasons {
        margin: 0;
        padding: 0;
        list-style: none;
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .destructive-confirm__reason {
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
        padding: 0.375rem 0.625rem;
        border-radius: 0.25rem;
        background: color-mix(in srgb, var(--max-danger-500) 12%, transparent);
        font-size: 0.75rem;
    }

    .destructive-confirm__reason-msg {
        color: var(--background-100);
        font-weight: 500;
    }

    .destructive-confirm__reason-seg {
        font-family: var(--mt-font-mono);
        font-size: 0.6875rem;
        color: var(--max-danger-300);
    }

    .destructive-confirm__field {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .destructive-confirm__field-label {
        font-size: 0.75rem;
        color: var(--background-200);
    }

    .destructive-confirm__token {
        font-family: var(--mt-font-mono);
        color: var(--max-danger-400);
    }

    .destructive-confirm__input {
        padding: 0.5rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-950);
        color: var(--background-50);
        font-family: var(--mt-font-mono);
        font-size: 0.8125rem;

        &:focus {
            outline: none;
            border-color: var(--max-danger-500);
        }
    }

    .destructive-confirm__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
    }

    .destructive-confirm__btn {
        padding: 0.4375rem 0.875rem;
        border-radius: 0.375rem;
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
        border: 1px solid transparent;

        &.is-secondary {
            border-color: var(--background-700);
            background: var(--background-850);
            color: var(--background-100);

            &:hover {
                background: var(--background-800);
            }
        }

        &.is-danger {
            border-color: var(--max-danger-500);
            background: var(--max-danger-600);
            color: var(--background-50);

            &:disabled {
                opacity: 0.45;
                cursor: not-allowed;
            }

            &:hover:not(:disabled) {
                background: var(--max-danger-500);
            }
        }
    }
</style>
