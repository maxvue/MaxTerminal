<template>
    <section class="ai-preferences" aria-label="Preferências de Inteligência Artificial" data-testid="ai-preferences">
        <header class="ai-preferences__header">
            <div>
                <h3 class="ai-preferences__title">Integração com MaxAiManager</h3>
                <p class="ai-preferences__subtitle">Comando por linguagem natural (@ / ?), diagnóstico rápido inline de falhas e consulta RAG.</p>
            </div>
            <MaxTag :value="aiStore.indicator.label" :severity="aiStatusSeverity" :class="`is-${aiStore.indicator.color}`" data-testid="ai-prefs-status-badge" />
        </header>

        <div class="ai-preferences__grid">
            <label class="ai-preferences__row">
                <div class="ai-preferences__row-info">
                    <span class="ai-preferences__row-title">Ativar integração com MaxAiManager</span>
                    <span class="ai-preferences__row-desc">Habilita os gatilhos @ e ?, diagnóstico rápido inline e geração assistida.</span>
                </div>
                <MaxInputCheckbox v-model="draft.enabled" data-testid="toggle-ai-enabled" />
            </label>

            <label class="ai-preferences__row">
                <div class="ai-preferences__row-info">
                    <span class="ai-preferences__row-title">URL do Gateway MaxAiManager</span>
                    <span class="ai-preferences__row-desc">Endereço HTTP local do gateway (ex.: http://127.0.0.1:8787).</span>
                </div>
                <MaxInputText v-model="baseUrlInput" placeholder="http://127.0.0.1:8787" data-testid="input-ai-base-url" />
            </label>

            <label class="ai-preferences__row">
                <div class="ai-preferences__row-info">
                    <span class="ai-preferences__row-title">Permitir host fora de loopback</span>
                    <span class="ai-preferences__row-desc">Autoriza conexão com um gateway MaxAiManager em outro host da rede local.</span>
                </div>
                <MaxInputCheckbox v-model="draft.allowRemote" data-testid="toggle-ai-allow-remote" />
            </label>
        </div>

        <div class="ai-preferences__panel">
            <div class="ai-preferences__panel-head">
                <div>
                    <h4 class="ai-preferences__panel-title">Modelo Padrão do Gatilho ?</h4>
                    <p class="ai-preferences__panel-desc">Utilizado automaticamente ao digitar ? no input rico.</p>
                </div>
                <span v-if="draft.defaultSelection" class="ai-preferences__current-sel" data-testid="current-default-selection">{{ draft.defaultSelection.harnessId }} / {{ draft.defaultSelection.modelId }}<template v-if="draft.defaultSelection.reasoningId"> ({{ draft.defaultSelection.reasoningId }})</template></span>
            </div>
            <HarnessPicker :harnesses="aiStore.harnesses" :model-value="draft.defaultSelection" @update:model-value="onSelectDefaultModel" />
        </div>

        <div class="ai-preferences__grid">
            <label class="ai-preferences__row">
                <div class="ai-preferences__row-info">
                    <span class="ai-preferences__row-title">Diagnóstico rápido inline automático</span>
                    <span class="ai-preferences__row-desc">Exibe causa resumida e sugestão de comando corretivo ao lado do botão Inspecionar.</span>
                </div>
                <MaxInputCheckbox v-model="draft.diagnosis.enabled" data-testid="toggle-diagnosis-enabled" />
            </label>

            <label class="ai-preferences__row">
                <div class="ai-preferences__row-info">
                    <span class="ai-preferences__row-title">Atraso de debounce do diagnóstico (ms)</span>
                    <span class="ai-preferences__row-desc">Tempo de espera após a falha do comando antes de consultar o gateway.</span>
                </div>
                <MaxInputNumber v-model="draft.diagnosis.debounceMs" :min="0" :max="10000" aria-label="Atraso de debounce do diagnóstico em milissegundos" class="is-narrow" data-testid="input-diagnosis-debounce" />
            </label>

            <label class="ai-preferences__row">
                <div class="ai-preferences__row-info">
                    <span class="ai-preferences__row-title">Quantidade de trechos RAG (topK)</span>
                    <span class="ai-preferences__row-desc">Número máximo de resultados retornados por consulta de documentação (1 a 20).</span>
                </div>
                <MaxInputNumber v-model="draft.rag.topK" :min="1" :max="20" aria-label="Quantidade de trechos RAG" class="is-narrow" data-testid="input-rag-topk" />
            </label>

            <label class="ai-preferences__row">
                <div class="ai-preferences__row-info">
                    <span class="ai-preferences__row-title">Incluir cauda de saída no contexto</span>
                    <span class="ai-preferences__row-desc">Envia as últimas linhas sanitizadas do comando anterior nas traduções @ e ?.</span>
                </div>
                <MaxInputCheckbox v-model="draft.context.includeOutputTail" data-testid="toggle-include-output-tail" />
            </label>

            <label class="ai-preferences__row">
                <div class="ai-preferences__row-info">
                    <span class="ai-preferences__row-title">Salvar histórico sanitizado de prompts</span>
                    <span class="ai-preferences__row-desc">Armazena interações redigidas de segredos no SQLite local.</span>
                </div>
                <MaxInputCheckbox v-model="draft.saveHistory" data-testid="toggle-save-history" />
            </label>
        </div>

        <MaxAlert v-if="feedbackMsg" :message="feedbackMsg" :severity="feedbackSeverity" data-testid="ai-prefs-feedback" class="ai-preferences__feedback" />

        <footer class="ai-preferences__footer">
            <div class="ai-preferences__maintenance">
                <MaxButton type="button" label="Testar conexão" icon="lucide:activity" variant="outlined" data-testid="btn-test-health" @click="onTestHealth" />
                <MaxButton type="button" label="Atualizar catálogo" icon="lucide:refresh-cw" variant="outlined" data-testid="btn-refresh-catalog" @click="onRefreshCatalog" />
                <MaxButton type="button" label="Limpar cache" icon="lucide:eraser" variant="outlined" data-testid="btn-clear-cache" @click="onClearCache" />
                <MaxButton type="button" label="Limpar histórico" icon="lucide:trash-2" variant="outlined" data-testid="btn-clear-history" @click="onClearHistory" />
            </div>

            <MaxButton type="button" label="Salvar preferências" icon="lucide:check" data-testid="btn-save-ai-settings" @click="onSave" />
        </footer>
    </section>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import { MaxAlert, MaxButton, MaxInputCheckbox, MaxInputNumber, MaxInputText, MaxTag } from '@maxvue/max-components-ui';
    import HarnessPicker from '@/components/ai/HarnessPicker.vue';
    import { DEFAULT_AI_SETTINGS, useAiStore } from '@/stores/ai';
    import type { AiSelection_Serialize, AiSettings_Serialize } from '@/types/bindings';

    const emit = defineEmits<{
        saved: [settings: AiSettings_Serialize];
    }>();

    const aiStore = useAiStore();

    function cloneSettings(src: AiSettings_Serialize | null): AiSettings_Serialize {
        const base = src ?? DEFAULT_AI_SETTINGS;
        return {
            ...base,
            defaultSelection: base.defaultSelection ? { ...base.defaultSelection } : null,
            diagnosis: {
                ...base.diagnosis,
                selection: base.diagnosis.selection ? { ...base.diagnosis.selection } : null,
                ignoredExitCodes: [...base.diagnosis.ignoredExitCodes],
                disabledProjects: [...base.diagnosis.disabledProjects]
            },
            rag: {
                ...base.rag,
                stackSlugs: { ...base.rag.stackSlugs }
            },
            context: { ...base.context },
            timeouts: { ...base.timeouts }
        };
    }

    const draft = ref<AiSettings_Serialize>(cloneSettings(aiStore.settings));
    const feedbackMsg = ref<string | null>(null);
    const feedbackSeverity = ref<'info' | 'success' | 'warning' | 'danger'>('info');

    const aiStatusSeverity = computed<'success' | 'warning' | 'danger' | 'secondary'>(() => {
        switch (aiStore.indicator.color) {
            case 'success':
                return 'success';
            case 'warning':
                return 'warning';
            case 'danger':
                return 'danger';
            case 'neutral':
            default:
                return 'secondary';
        }
    });

    watch(
        () => aiStore.settings,
        (next) => {
            if (next) {
                draft.value = cloneSettings(next);
            }
        }
    );

    const baseUrlInput = computed<string>({
        get: () => draft.value.baseUrl ?? '',
        set: (val: string) => {
            draft.value.baseUrl = val.trim() === '' ? null : val.trim();
        }
    });

    function onSelectDefaultModel(selection: AiSelection_Serialize): void {
        draft.value.defaultSelection = selection;
    }

    function validateNumberLimits(): string | null {
        const debounce = draft.value.diagnosis.debounceMs;
        if (debounce === null || debounce === undefined || debounce === ('' as unknown) || Number.isNaN(Number(debounce))) {
            return 'O atraso de debounce do diagnóstico não pode ser vazio e deve ser um número inteiro entre 0 e 10000 ms.';
        }
        const debounceNum = Number(debounce);
        if (!Number.isInteger(debounceNum)) {
            return 'O atraso de debounce do diagnóstico deve ser um número inteiro (decimais não permitidos).';
        }
        if (debounceNum < 0 || debounceNum > 10000) {
            return 'O atraso de debounce do diagnóstico deve estar entre 0 e 10000 ms.';
        }

        const topK = draft.value.rag.topK;
        if (topK === null || topK === undefined || topK === ('' as unknown) || Number.isNaN(Number(topK))) {
            return 'A quantidade de trechos RAG (topK) não pode ser vazia e deve ser um número inteiro entre 1 e 20.';
        }
        const topKNum = Number(topK);
        if (!Number.isInteger(topKNum)) {
            return 'A quantidade de trechos RAG (topK) deve ser um número inteiro (decimais não permitidos).';
        }
        if (topKNum < 1 || topKNum > 20) {
            return 'A quantidade de trechos RAG (topK) deve estar entre 1 e 20.';
        }

        return null;
    }

    async function onSave(): Promise<void> {
        feedbackMsg.value = null;
        feedbackSeverity.value = 'info';

        const validationErr = validateNumberLimits();
        if (validationErr) {
            feedbackMsg.value = validationErr;
            feedbackSeverity.value = 'danger';
            return;
        }

        try {
            const saved = await aiStore.saveSettings(draft.value);
            feedbackMsg.value = 'Preferências de IA salvas com sucesso.';
            feedbackSeverity.value = 'success';
            emit('saved', saved);
        } catch (err: unknown) {
            feedbackMsg.value =
                err instanceof Error ? err.message : 'Falha ao salvar preferências de IA.';
            feedbackSeverity.value = 'danger';
        }
    }

    async function onTestHealth(): Promise<void> {
        feedbackMsg.value = null;
        try {
            const h = await aiStore.refreshHealth();
            feedbackMsg.value = `Status do gateway: ${h.status}${h.latencyMs !== null ? ` (${h.latencyMs} ms)` : ''}`;
        } catch (err: unknown) {
            feedbackMsg.value =
                err instanceof Error ? err.message : 'Falha ao testar conexão com o gateway.';
        }
    }

    async function onRefreshCatalog(): Promise<void> {
        feedbackMsg.value = null;
        try {
            const cat = await aiStore.loadCatalog(true);
            feedbackMsg.value = `Catálogo atualizado (${cat.harnesses.length} harnesses).`;
        } catch (err: unknown) {
            feedbackMsg.value =
                err instanceof Error ? err.message : 'Falha ao atualizar o catálogo.';
        }
    }

    async function onClearCache(): Promise<void> {
        feedbackMsg.value = null;
        try {
            const count = await aiStore.clearCache('all');
            feedbackMsg.value = `Cache limpo (${count} registro(s) removido(s)).`;
        } catch (err: unknown) {
            feedbackMsg.value =
                err instanceof Error ? err.message : 'Falha ao limpar o cache de IA.';
        }
    }

    async function onClearHistory(): Promise<void> {
        feedbackMsg.value = null;
        try {
            const count = await aiStore.clearHistory(null);
            feedbackMsg.value = `Histórico limpo (${count} registro(s) removido(s)).`;
        } catch (err: unknown) {
            feedbackMsg.value =
                err instanceof Error ? err.message : 'Falha ao limpar o histórico.';
        }
    }
</script>

<style lang="scss" scoped>
    .ai-preferences {
        display: flex;
        flex-direction: column;
        gap: 1rem;
    }

    .ai-preferences__header {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 1rem;
    }

    .ai-preferences__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .ai-preferences__subtitle {
        margin: 0.25rem 0 0;
        font-size: 0.75rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .ai-preferences__badge {
        padding: 0.1875rem 0.5rem;
        border-radius: 999px;
        font-size: 0.6875rem;
        font-weight: 600;

        &.is-success {
            background: color-mix(in srgb, var(--max-success-500) 18%, transparent);
            color: var(--max-success-400);
        }

        &.is-warning {
            background: color-mix(in srgb, var(--max-warning-500) 18%, transparent);
            color: var(--max-warning-400);
        }

        &.is-danger {
            background: color-mix(in srgb, var(--max-danger-500) 18%, transparent);
            color: var(--max-danger-400);
        }

        &.is-neutral {
            background: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--mt-ui-text-muted, var(--background-400));
        }
    }

    .ai-preferences__grid {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .ai-preferences__row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 1rem;
        padding: 0.625rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .ai-preferences__row-info {
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
    }

    .ai-preferences__row-title {
        font-size: 0.8125rem;
        font-weight: 500;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .ai-preferences__row-desc {
        font-size: 0.75rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .ai-preferences__input {
        width: 14rem;
        padding: 0.3125rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;

        &.is-narrow {
            width: 5.5rem;
        }
    }

    .ai-preferences__panel {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .ai-preferences__panel-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
    }

    .ai-preferences__panel-title {
        margin: 0;
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .ai-preferences__panel-desc {
        margin: 0.125rem 0 0;
        font-size: 0.75rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .ai-preferences__current-sel {
        padding: 0.125rem 0.5rem;
        border-radius: 0.25rem;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--max-primary-400);
        font-family: var(--mt-font-mono);
        font-size: 0.6875rem;
    }

    .ai-preferences__feedback {
        margin: 0;
        font-size: 0.75rem;
        color: var(--max-info-400);
    }

    .ai-preferences__footer {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .ai-preferences__maintenance {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        flex-wrap: wrap;
    }

    .ai-preferences__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.3125rem;
        padding: 0.375rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid transparent;
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &.is-secondary {
            border-color: var(--mt-ui-border, var(--background-100));
            background: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--mt-ui-text, var(--background-800));

            &:hover {
                background: var(--mt-ui-surface-hover, var(--background-75));
            }
        }

        &.is-primary {
            border-color: var(--max-primary-500);
            background: var(--max-primary-600);
            color: var(--max-primary-content, #fff);

            &:hover {
                background: var(--max-primary-500);
            }
        }
    }
</style>
