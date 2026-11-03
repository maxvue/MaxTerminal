<template>
    <MaxModal :visible="open" title="Confirmar execução de comando destrutivo" :no-button="true" max-width="36rem" class="destructive-confirm" @update:visible="onCancel">
        <div class="destructive-confirm__body" data-testid="destructive-confirm-modal">
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
        </div>

        <footer class="destructive-confirm__footer">
            <MaxButton type="button" variant="secondary" size="sm" data-testid="btn-cancel-destructive" @click="onCancel">
                <span>Cancelar</span>
            </MaxButton>
            <MaxButton type="button" variant="danger" size="sm" :disabled="!isConfirmed" data-testid="btn-confirm-destructive" @click="onConfirm">
                <span>Executar comando destrutivo</span>
            </MaxButton>
        </footer>
    </MaxModal>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import MaxModal from '@maxvue/max-components-ui/components/MaxModal';
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
        color: var(--max-danger-400);
        flex-shrink: 0;
    }

    .destructive-confirm__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
        color: var(--max-danger-400);
    }

    .destructive-confirm__close {
        color: var(--background-300);

        &:hover {
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
</style>
