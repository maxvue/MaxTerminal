<template>
    <div v-if="visible" class="inline-diagnosis" :class="[`is-${state?.status ?? 'pending'}`]" data-testid="inline-diagnosis">
        <div v-if="loading" class="inline-diagnosis__loading" data-testid="diagnosis-loading">
            <MaxIcon icon="lucide:loader-circle" :size="0.8125" class="inline-diagnosis__icon" />
            <span>Diagnosticando falha com IA...</span>
        </div>

        <div v-else-if="state?.status === 'failed'" class="inline-diagnosis__error" data-testid="diagnosis-error">
            <MaxIcon icon="lucide:circle-alert" :size="0.8125" class="inline-diagnosis__icon" />
            <span>{{ state.error || 'Falha no diagnóstico rápido.' }}</span>
            <MaxIconButton icon="lucide:x" :size="0.75" class="inline-diagnosis__dismiss" title="Dispensar diagnóstico" aria-label="Dispensar diagnóstico" data-testid="btn-dismiss-diagnosis" @click="onDismiss" />
        </div>

        <div v-else-if="diagnosis" class="inline-diagnosis__content" data-testid="diagnosis-ready">
            <div class="inline-diagnosis__main">
                <MaxIcon icon="lucide:sparkles" :size="0.8125" class="inline-diagnosis__icon" />
                <span class="inline-diagnosis__summary" data-testid="diagnosis-summary">{{ diagnosis.summary }}</span>
                <span v-if="diagnosis.fromCache" class="inline-diagnosis__cache-tag" data-testid="diagnosis-cache-badge">cache</span>
            </div>

            <div v-if="diagnosis.fixCommand" class="inline-diagnosis__fix">
                <code class="inline-diagnosis__fix-cmd" data-testid="diagnosis-fix-command">{{ diagnosis.fixCommand }}</code>
                <RiskBadge v-if="diagnosis.fixRisk" :risk="diagnosis.fixRisk" compact />

                <MaxButton type="button" variant="secondary" size="sm" icon="lucide:corner-down-left" title="Inserir comando corretivo no input" data-testid="btn-insert-fix" @click="onInsertFix">
                    <span>Inserir</span>
                </MaxButton>

                <MaxButton type="button" :variant="applyBtnVariant" size="sm" icon="lucide:play" data-testid="btn-apply-fix" @click="onApplyFixClick">
                    <span>{{ applyBtnLabel }}</span>
                </MaxButton>
            </div>

            <MaxIconButton icon="lucide:x" :size="0.75" class="inline-diagnosis__dismiss" title="Dispensar diagnóstico" aria-label="Dispensar diagnóstico" data-testid="btn-dismiss-diagnosis" @click="onDismiss" />
        </div>

        <div v-if="cautionOpen && diagnosis?.fixRisk?.level === 'caution'" class="inline-diagnosis__caution" role="alertdialog" data-testid="diagnosis-caution-popover">
            <span>Confirmar execução da correção sugerida?</span>
            <div class="inline-diagnosis__caution-actions">
                <MaxButton type="button" variant="secondary" size="sm" data-testid="btn-cancel-caution-fix" @click="cautionOpen = false">Cancelar</MaxButton>
                <MaxButton type="button" variant="warning" size="sm" data-testid="btn-confirm-caution-fix" @click="onConfirmCautionFix">Confirmar</MaxButton>
            </div>
        </div>

        <DestructiveConfirm v-if="diagnosis?.fixCommand && diagnosis?.fixRisk" :open="destructiveOpen" :command="diagnosis.fixCommand" :risk="diagnosis.fixRisk" @confirm="onConfirmDestructiveFix" @cancel="destructiveOpen = false" />
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import MaxIconButton from '@maxvue/max-components-ui/components/MaxIconButton';
    import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
    import DestructiveConfirm from '@/components/ai/DestructiveConfirm.vue';
    import RiskBadge from '@/components/ai/RiskBadge.vue';
    import {
        useInlineDiagnosis,
        type DiagnosableBlockLike
    } from '@/composables/ai/useInlineDiagnosis';

    export interface InlineDiagnosisProps {
        block: DiagnosableBlockLike;
        autoTrigger?: boolean;
    }

    const props = withDefaults(defineProps<InlineDiagnosisProps>(), {
        autoTrigger: true
    });

    const emit = defineEmits<{
        inserted: [command: string];
        applied: [command: string];
        dismissed: [blockId: string];
    }>();

    const {
        state,
        diagnosis,
        loading,
        visible,
        dismiss,
        insertFix,
        executeSafeOrCautionFix,
        executeDestructiveFix
    } = useInlineDiagnosis(() => props.block, { autoTrigger: props.autoTrigger });

    const cautionOpen = ref<boolean>(false);
    const destructiveOpen = ref<boolean>(false);

    const applyBtnVariant = computed<'primary' | 'warning' | 'danger'>(() => {
        const level = diagnosis.value?.fixRisk?.level ?? 'safe';
        if (level === 'destructive') {
            return 'danger';
        }
        if (level === 'caution') {
            return 'warning';
        }
        return 'primary';
    });

    const applyBtnLabel = computed<string>(() => {
        const level = diagnosis.value?.fixRisk?.level ?? 'safe';
        if (level === 'destructive') {
            return 'Revisar correção...';
        }
        return 'Aplicar correção';
    });

    function onInsertFix(): void {
        const cmd = diagnosis.value?.fixCommand ?? '';
        if (insertFix()) {
            emit('inserted', cmd);
        }
    }

    async function onApplyFixClick(): Promise<void> {
        const diag = diagnosis.value;
        if (!diag?.fixCommand) {
            return;
        }
        const level = diag.fixRisk?.level ?? 'safe';

        if (level === 'destructive' || diag.fixRisk?.requiresConfirmation) {
            cautionOpen.value = false;
            destructiveOpen.value = true;
            return;
        }

        if (level === 'caution') {
            cautionOpen.value = true;
            return;
        }

        await executeSafeOrCautionFix();
        emit('applied', diag.fixCommand);
    }

    async function onConfirmCautionFix(): Promise<void> {
        const cmd = diagnosis.value?.fixCommand ?? '';
        cautionOpen.value = false;
        await executeSafeOrCautionFix();
        emit('applied', cmd);
    }

    async function onConfirmDestructiveFix(typedToken: string): Promise<void> {
        const cmd = diagnosis.value?.fixCommand ?? '';
        destructiveOpen.value = false;
        await executeDestructiveFix(typedToken);
        emit('applied', cmd);
    }

    async function onDismiss(): Promise<void> {
        cautionOpen.value = false;
        destructiveOpen.value = false;
        await dismiss();
        emit('dismissed', props.block.id);
    }
</script>

<style lang="scss" scoped>
    .inline-diagnosis {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
        padding: 0.375rem 0.625rem;
        border-top: 1px solid var(--background-750);
        background: color-mix(in srgb, var(--max-warning-500) 8%, var(--background-900));
        font-size: 0.6875rem;
        color: var(--background-100);
    }

    .inline-diagnosis__loading,
    .inline-diagnosis__error,
    .inline-diagnosis__content {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .inline-diagnosis__main {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        min-width: 0;
        flex: 1;
    }

    .inline-diagnosis__icon {
        color: var(--max-warning-400);
        flex-shrink: 0;
    }

    .inline-diagnosis__summary {
        color: var(--background-100);
        font-weight: 500;
    }

    .inline-diagnosis__cache-tag {
        padding: 0.0625rem 0.3125rem;
        border-radius: 999px;
        background: var(--background-800);
        color: var(--background-400);
        font-family: var(--mt-font-mono);
        font-size: 0.625rem;
    }

    .inline-diagnosis__fix {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        flex-wrap: wrap;
    }

    .inline-diagnosis__fix-cmd {
        padding: 0.125rem 0.375rem;
        border-radius: 0.25rem;
        border: 1px solid var(--background-700);
        background: var(--background-950);
        color: var(--max-primary-300);
        font-family: var(--mt-font-mono);
    }

    .inline-diagnosis__dismiss {
        color: var(--background-400);

        &:hover {
            color: var(--background-100);
        }
    }

    .inline-diagnosis__caution {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.375rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--max-warning-500);
        background: color-mix(in srgb, var(--max-warning-500) 15%, transparent);
    }

    .inline-diagnosis__caution-actions {
        display: flex;
        align-items: center;
        gap: 0.25rem;
    }
</style>
