<template>
    <div v-if="state" class="ai-suggestion-block" :class="[`is-${state.status}`]" data-testid="ai-suggestion-block">
        <header class="ai-suggestion-block__header">
            <div class="ai-suggestion-block__title-wrap">
                <MaxIcon :icon="state.status === 'streaming' ? 'lucide:loader-circle' : 'lucide:sparkles'" :size="0.875" class="ai-suggestion-block__icon" :class="{ 'is-spin': state.status === 'streaming' }" />
                <span class="ai-suggestion-block__title">{{ titleText }}</span>
                <span v-if="selectionBadge" class="ai-suggestion-block__model" data-testid="suggestion-model-badge">{{ selectionBadge }}</span>
            </div>

            <div class="ai-suggestion-block__meta">
                <RiskBadge v-if="effectiveRisk" :risk="effectiveRisk" />
                <MaxIconButton icon="lucide:eye" :size="0.75" class="ai-suggestion-block__icon-btn" title="Ver contexto sanitizado enviado" aria-label="Ver contexto enviado" data-testid="btn-toggle-context" @click="onToggleContext" />
                <MaxIconButton icon="lucide:x" :size="0.75" class="ai-suggestion-block__icon-btn" title="Fechar sugestão" aria-label="Fechar sugestão" data-testid="btn-dismiss-suggestion" @click="onDismiss" />
            </div>
        </header>

        <p class="ai-suggestion-block__prompt" data-testid="suggestion-prompt">“{{ state.prompt }}”</p>

        <div v-if="state.status === 'streaming'" class="ai-suggestion-block__streaming" data-testid="suggestion-streaming">
            <code class="ai-suggestion-block__stream-code">{{ state.streamingText || 'Gerando sugestão de comando...' }}</code>
        </div>

        <div v-else-if="state.status === 'failed'" class="ai-suggestion-block__error" role="alert" data-testid="suggestion-error">
            <MaxIcon icon="lucide:circle-alert" :size="0.875" />
            <span>{{ state.error || 'Falha ao gerar comando.' }}</span>
        </div>

        <div v-else-if="state.status === 'completed'" class="ai-suggestion-block__body">
            <label class="ai-suggestion-block__cmd-field">
                <span class="ai-suggestion-block__cmd-label">Comando sugerido (editável antes de executar):</span>
                <input :value="effectiveCommand" type="text" class="ai-suggestion-block__cmd-input" spellcheck="false" autocomplete="off" data-testid="suggestion-command-input" @input="onCommandInput">
            </label>

            <p v-if="state.suggestion?.explanation" class="ai-suggestion-block__explanation" data-testid="suggestion-explanation">{{ state.suggestion.explanation }}</p>

            <ul v-if="effectiveRisk && effectiveRisk.reasons.length > 0" class="ai-suggestion-block__reasons" data-testid="suggestion-risk-reasons">
                <li v-for="(reason, idx) in effectiveRisk.reasons" :key="`${reason.ruleId}-${idx}`" class="ai-suggestion-block__reason">{{ reason.message }} ({{ reason.segment }})</li>
            </ul>
        </div>

        <AiContextPreview v-if="showContext && state.contextPreview" :context="state.contextPreview" />

        <div v-if="cautionConfirmOpen && effectiveRisk?.level === 'caution'" class="ai-suggestion-block__caution-popover" role="alertdialog" aria-label="Confirmar comando com atenção" data-testid="caution-confirm-popover">
            <span class="ai-suggestion-block__caution-text">Este comando requer atenção antes de executar. Deseja prosseguir?</span>
            <div class="ai-suggestion-block__caution-actions">
                <MaxButton type="button" severity="secondary" size="sm" data-testid="btn-cancel-caution" @click="cautionConfirmOpen = false">Cancelar</MaxButton>
                <MaxButton type="button" severity="warning" size="sm" data-testid="btn-confirm-caution" @click="onConfirmCaution">Confirmar execução</MaxButton>
            </div>
        </div>

        <footer class="ai-suggestion-block__actions">
            <MaxButton v-if="state.status === 'streaming'" type="button" severity="secondary" size="sm" icon="lucide:square" data-testid="btn-cancel-stream" @click="onCancel">
                <span>Cancelar</span>
            </MaxButton>

            <template v-else>
                <MaxButton type="button" severity="secondary" size="sm" icon="lucide:refresh-cw" data-testid="btn-regenerate" @click="onRegenerate">
                    <span>Regenerar</span>
                </MaxButton>

                <MaxButton v-if="state.status === 'completed'" type="button" severity="secondary" size="sm" icon="lucide:corner-down-left" data-testid="btn-insert-input" @click="onInsert">
                    <span>Inserir no input</span>
                </MaxButton>

                <MaxButton v-if="state.status === 'completed' && executeSeverity" type="button" :severity="executeSeverity" size="sm" :disabled="reclassifying || !effectiveCommand.trim()" :icon="executeBtnIcon" data-testid="btn-execute-command" @click="onExecuteClick">
                    <span>{{ executeBtnLabel }}</span>
                </MaxButton>
                <MaxButton v-else-if="state.status === 'completed'" type="button" size="sm" :disabled="reclassifying || !effectiveCommand.trim()" :icon="executeBtnIcon" data-testid="btn-execute-command" @click="onExecuteClick">
                    <span>{{ executeBtnLabel }}</span>
                </MaxButton>
            </template>
        </footer>

        <DestructiveConfirm v-if="effectiveRisk" :open="destructiveModalOpen" :command="effectiveCommand" :risk="effectiveRisk" @confirm="onConfirmDestructive" @cancel="destructiveModalOpen = false" />
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import MaxIconButton from '@maxvue/max-components-ui/components/MaxIconButton';
    import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
    import AiContextPreview from '@/components/ai/AiContextPreview.vue';
    import DestructiveConfirm from '@/components/ai/DestructiveConfirm.vue';
    import RiskBadge from '@/components/ai/RiskBadge.vue';
    import { useAiSuggestion } from '@/composables/ai/useAiSuggestion';
    import type { PaneId } from '@/types/bindings';

    export interface AiSuggestionBlockProps {
        paneId: PaneId;
    }

    const props = defineProps<AiSuggestionBlockProps>();

    const emit = defineEmits<{
        inserted: [command: string];
        executed: [command: string];
    }>();

    const {
        state,
        effectiveCommand,
        effectiveRisk,
        reclassifying,
        regenerate,
        cancel,
        updateEditedCommand,
        insertIntoInput,
        executeSafeOrCaution,
        executeDestructiveConfirmed,
        loadContextPreview,
        dismiss
    } = useAiSuggestion(() => props.paneId);

    const showContext = ref<boolean>(false);
    const cautionConfirmOpen = ref<boolean>(false);
    const destructiveModalOpen = ref<boolean>(false);

    const titleText = computed<string>(() => {
        switch (state.value?.status) {
            case 'streaming':
                return 'Gerando comando...';
            case 'failed':
                return 'Falha na sugestão de IA';
            case 'cancelled':
                return 'Sugestão cancelada';
            case 'completed':
            default:
                return 'Sugestão de Comando Shell';
        }
    });

    const selectionBadge = computed<string | null>(() => {
        const sel = state.value?.selection ?? state.value?.suggestion?.selection;
        if (!sel) {
            return null;
        }
        return sel.reasoningId
            ? `${sel.harnessId} · ${sel.modelId} (${sel.reasoningId})`
            : `${sel.harnessId} · ${sel.modelId}`;
    });

    const executeSeverity = computed<'warning' | 'danger' | null>(() => {
        const level = effectiveRisk.value?.level ?? 'safe';
        if (level === 'destructive') {
            return 'danger';
        }
        if (level === 'caution') {
            return 'warning';
        }
        return null;
    });

    const executeBtnIcon = computed<string>(() => {
        const level = effectiveRisk.value?.level ?? 'safe';
        if (level === 'destructive') {
            return 'lucide:skull';
        }
        if (level === 'caution') {
            return 'lucide:triangle-alert';
        }
        return 'lucide:play';
    });

    const executeBtnLabel = computed<string>(() => {
        const level = effectiveRisk.value?.level ?? 'safe';
        if (level === 'destructive') {
            return 'Revisar e Executar...';
        }
        if (level === 'caution') {
            return 'Executar com Atenção';
        }
        return 'Executar';
    });

    async function onCommandInput(ev: Event): Promise<void> {
        const nextVal = (ev.target as HTMLInputElement).value;
        cautionConfirmOpen.value = false;
        await updateEditedCommand(nextVal);
    }

    async function onToggleContext(): Promise<void> {
        showContext.value = !showContext.value;
        if (showContext.value && !state.value?.contextPreview) {
            await loadContextPreview().catch(() => {});
        }
    }

    function onInsert(): void {
        const cmd = effectiveCommand.value;
        if (insertIntoInput()) {
            emit('inserted', cmd);
        }
    }

    async function onExecuteClick(): Promise<void> {
        const cmd = effectiveCommand.value.trim();
        if (!cmd) {
            return;
        }
        const risk = await updateEditedCommand(cmd);
        const level = risk?.level ?? effectiveRisk.value?.level ?? 'safe';

        if (level === 'destructive' || risk?.requiresConfirmation) {
            cautionConfirmOpen.value = false;
            destructiveModalOpen.value = true;
            return;
        }

        if (level === 'caution') {
            cautionConfirmOpen.value = true;
            return;
        }

        await executeSafeOrCaution();
        emit('executed', cmd);
    }

    async function onConfirmCaution(): Promise<void> {
        const cmd = effectiveCommand.value.trim();
        cautionConfirmOpen.value = false;
        await executeSafeOrCaution();
        emit('executed', cmd);
    }

    async function onConfirmDestructive(typedToken: string): Promise<void> {
        const cmd = effectiveCommand.value.trim();
        destructiveModalOpen.value = false;
        await executeDestructiveConfirmed(typedToken);
        emit('executed', cmd);
    }

    async function onRegenerate(): Promise<void> {
        cautionConfirmOpen.value = false;
        destructiveModalOpen.value = false;
        await regenerate().catch(() => {});
    }

    async function onCancel(): Promise<void> {
        await cancel();
    }

    function onDismiss(): void {
        cautionConfirmOpen.value = false;
        destructiveModalOpen.value = false;
        dismiss();
    }
</script>

<style lang="scss" scoped>
    .ai-suggestion-block {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 0.625rem 0.75rem;
        border-radius: 0.5rem;
        border: 1px solid var(--background-750);
        background: var(--background-925);
        color: var(--background-100);
        font-size: 0.75rem;
    }

    .ai-suggestion-block__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
    }

    .ai-suggestion-block__title-wrap {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        min-width: 0;
    }

    .ai-suggestion-block__icon {
        color: var(--max-primary-400);
        flex-shrink: 0;
    }

    .ai-suggestion-block__title {
        font-weight: 600;
        color: var(--background-50);
    }

    .ai-suggestion-block__model {
        padding: 0.0625rem 0.375rem;
        border-radius: 0.25rem;
        background: var(--background-850);
        color: var(--background-300);
        font-family: var(--mt-font-mono);
        font-size: 0.6875rem;
    }

    .ai-suggestion-block__meta {
        display: flex;
        align-items: center;
        gap: 0.375rem;
    }

    .ai-suggestion-block__icon-btn {
        color: var(--background-300);

        &:hover {
            color: var(--background-50);
        }
    }

    .ai-suggestion-block__prompt {
        margin: 0;
        color: var(--background-300);
        font-style: italic;
    }

    .ai-suggestion-block__streaming {
        padding: 0.5rem;
        border-radius: 0.375rem;
        background: var(--background-950);
    }

    .ai-suggestion-block__stream-code {
        font-family: var(--mt-font-mono);
        color: var(--max-primary-300);
        white-space: pre-wrap;
    }

    .ai-suggestion-block__error {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.5rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-danger-500);
        background: color-mix(in srgb, var(--max-danger-500) 14%, transparent);
        color: var(--max-danger-300);
    }

    .ai-suggestion-block__body {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .ai-suggestion-block__cmd-field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .ai-suggestion-block__cmd-label {
        font-size: 0.6875rem;
        color: var(--background-400);
    }

    .ai-suggestion-block__cmd-input {
        padding: 0.4375rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-950);
        color: var(--background-50);
        font-family: var(--mt-font-mono);
        font-size: 0.8125rem;

        &:focus {
            outline: none;
            border-color: var(--max-primary-500);
        }
    }

    .ai-suggestion-block__explanation {
        margin: 0;
        color: var(--background-200);
        line-height: 1.4;
    }

    .ai-suggestion-block__reasons {
        margin: 0;
        padding-left: 1rem;
        color: var(--max-warning-300);
        font-size: 0.6875rem;
    }

    .ai-suggestion-block__caution-popover {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 0.5rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-warning-500);
        background: color-mix(in srgb, var(--max-warning-500) 14%, transparent);
    }

    .ai-suggestion-block__caution-text {
        color: var(--max-warning-300);
        font-weight: 500;
    }

    .ai-suggestion-block__caution-actions {
        display: flex;
        align-items: center;
        gap: 0.375rem;
    }

    .ai-suggestion-block__actions {
        display: flex;
        align-items: center;
        justify-content: flex-end;
        gap: 0.375rem;
        flex-wrap: wrap;
    }
</style>
