import { createAiRequestId } from '@/composables/ai/adapters';
import { aiIpc } from '@/ipc/ai';
import { useAiStore } from '@/stores/ai';
import { useTerminalStore } from '@/stores/terminal';
import { useWindowStore } from '@/stores/window';
import { fuzzyFilter } from '@/terminal/triggers/fuzzy';
import type {
    AiModel_Serialize,
    AiSelection_Serialize,
    Harness_Serialize,
    ReasoningLevel_Serialize
} from '@/types/bindings';
import type {
    CompletionItem,
    CompletionResult,
    TerminalContext,
    TriggerDefinition,
    TriggerQuery,
    TriggerSelectOutcome
} from '@/types/terminal';

export interface AtStage1State {
    harness: Harness_Serialize;
}

export interface AtStage2State {
    harness: Harness_Serialize;
    model: AiModel_Serialize;
}

function isRichInputMode(ctx: TerminalContext): boolean {
    const terminalStore = useTerminalStore();
    const inst = terminalStore.instance(ctx.paneId);
    return !inst || inst.inputMode === 'rich';
}

function buildSettingsDisabledResult(): CompletionResult {
    return {
        stageLabel: 'Inteligência Artificial Desativada',
        emptyMessage: 'A integração com o MaxAiManager está desativada.',
        items: [
            {
                id: 'action:open-settings',
                label: 'Ativar Inteligência Artificial nas Preferências...',
                description: 'Abre a seção de Inteligência Artificial nas Preferências',
                icon: 'lucide:settings',
                kind: 'action',
                pinned: 'bottom'
            }
        ]
    };
}

/**
 * Cria a definição do gatilho `@` (seleção em cascata 3 estágios: Harness → Modelo → Raciocínio).
 */
export function createAtTriggerDefinition(): TriggerDefinition {
    return {
        id: 'ai-at',
        prefix: '@',
        label: 'Selecionar Modelo de IA (@)',
        debounceMs: 40,

        isAvailable(ctx: TerminalContext): boolean {
            return isRichInputMode(ctx);
        },

        provide(q: TriggerQuery): CompletionResult {
            const aiStore = useAiStore();
            if (aiStore.settings && !aiStore.settings.enabled) {
                return buildSettingsDisabledResult();
            }

            // Estágio 0: Selecionar Harness
            if (q.stage === 0) {
                const rawItems: CompletionItem[] = aiStore.harnesses.map((harness) => ({
                    id: `harness:${harness.id}`,
                    label: harness.label,
                    description:
                        harness.models.length > 0
                            ? `${harness.models.length} modelo(s) disponível(is)`
                            : 'Catálogo de modelos pendente',
                    icon: harness.icon || 'lucide:bot',
                    detail: harness.id,
                    kind: 'value',
                    payload: harness
                }));

                const filtered = fuzzyFilter(rawItems, q.query, (item) =>
                    `${item.label} ${item.detail ?? ''}`
                );

                const footerItems: CompletionItem[] = [
                    {
                        id: 'action:refresh-catalog',
                        label: 'Atualizar catálogo de modelos',
                        description: 'Reconsulta /v1/harnesses no MaxAiManager',
                        icon: 'lucide:refresh-cw',
                        kind: 'action',
                        pinned: 'bottom'
                    },
                    {
                        id: 'action:open-settings',
                        label: 'Configurar Inteligência Artificial...',
                        description: 'Abre as preferências do MaxAiManager',
                        icon: 'lucide:settings',
                        kind: 'action',
                        pinned: 'bottom'
                    }
                ];

                return {
                    stageLabel: '1/3 · Selecionar Harness',
                    emptyMessage: 'Nenhum harness encontrado',
                    items: [...filtered, ...footerItems]
                };
            }

            // Estágio 1: Selecionar Modelo do Harness
            if (q.stage === 1) {
                const state = q.stageState as AtStage1State | undefined;
                const latestHarness =
                    aiStore.harnesses.find((h) => h.id === state?.harness.id) ?? state?.harness;

                if (!latestHarness) {
                    return {
                        stageLabel: '2/3 · Selecionar Modelo',
                        emptyMessage: 'Harness não encontrado',
                        items: []
                    };
                }

                const modelItems: CompletionItem[] = latestHarness.models.map((model) => ({
                    id: `model:${model.id}`,
                    label: model.label,
                    description:
                        model.reasoningLevels.length > 0
                            ? `Raciocínio: ${model.reasoningLevels.map((r) => r.label).join(', ')}`
                            : 'Sem nível de raciocínio adicional',
                    icon: latestHarness.icon || 'lucide:cpu',
                    detail: model.contextWindow
                        ? `${Math.round(model.contextWindow / 1000)}k ctx`
                        : model.id,
                    kind: 'value',
                    payload: model
                }));

                const filtered = fuzzyFilter(modelItems, q.query, (item) =>
                    `${item.label} ${item.detail ?? ''}`
                );

                const extraActions: CompletionItem[] =
                    latestHarness.models.length === 0
                        ? [
                            {
                                id: 'action:refresh-catalog',
                                label: 'Carregar modelos do MaxAiManager',
                                description: 'Sincroniza a lista de modelos disponíveis para este harness',
                                icon: 'lucide:refresh-cw',
                                kind: 'action',
                                pinned: 'bottom'
                            }
                        ]
                        : [];

                return {
                    stageLabel: `2/3 · Modelo (${latestHarness.label})`,
                    emptyMessage: 'Nenhum modelo disponível para este harness',
                    items: [...filtered, ...extraActions]
                };
            }

            // Estágio 2: Selecionar Nível de Raciocínio
            const state2 = q.stageState as AtStage2State | undefined;
            const model = state2?.model;
            const harness = state2?.harness;
            if (!model || !harness) {
                return {
                    stageLabel: '3/3 · Nível de Raciocínio',
                    emptyMessage: 'Modelo não selecionado',
                    items: []
                };
            }

            const sortedLevels = [...model.reasoningLevels].sort((a, b) => a.rank - b.rank);
            const levelItems: CompletionItem[] = sortedLevels.map((level) => ({
                id: `reasoning:${level.id}`,
                label: level.label,
                description:
                    model.defaultReasoning === level.id
                        ? 'Nível padrão recomendado para este modelo'
                        : `Nível de raciocínio (${level.id})`,
                icon: 'lucide:brain-circuit',
                detail: model.defaultReasoning === level.id ? 'Padrão' : level.id,
                kind: 'value',
                payload: level
            }));

            const filtered = fuzzyFilter(levelItems, q.query, (item) =>
                `${item.label} ${item.detail ?? ''}`
            );

            return {
                stageLabel: `3/3 · Raciocínio (${harness.label} / ${model.label})`,
                emptyMessage: 'Nenhum nível de raciocínio correspondente',
                items: filtered
            };
        },

        async onSelect(
            item: CompletionItem,
            q: TriggerQuery,
            ctx: TerminalContext
        ): Promise<TriggerSelectOutcome> {
            const aiStore = useAiStore();
            const windowStore = useWindowStore();

            if (item.id === 'action:open-settings') {
                windowStore.openPreferences('ai');
                return { kind: 'replace-input', text: '' };
            }

            if (item.id === 'action:refresh-catalog') {
                await aiStore.loadCatalog(true).catch(() => {});
                return {
                    kind: 'replace-input',
                    text: q.prefix,
                    keepOpen: true
                };
            }

            if (q.stage === 0) {
                const harness = item.payload as Harness_Serialize;
                if (harness.models.length === 0) {
                    await aiStore.loadCatalog(true).catch(() => {});
                }
                const refreshed =
                    aiStore.harnesses.find((h) => h.id === harness.id) ?? harness;
                return {
                    kind: 'push-stage',
                    stageState: { harness: refreshed } satisfies AtStage1State,
                    initialQuery: ''
                };
            }

            if (q.stage === 1) {
                const state1 = q.stageState as AtStage1State;
                const model = item.payload as AiModel_Serialize;

                // Quando o modelo não possui níveis de raciocínio, pula o estágio 2 diretamente para o prompt
                if (!model.reasoningLevels || model.reasoningLevels.length === 0) {
                    const selection: AiSelection_Serialize = {
                        harnessId: state1.harness.id,
                        modelId: model.id,
                        reasoningId: null
                    };
                    aiStore.openPanePrompt(ctx.paneId, {
                        kind: 'suggest',
                        trigger: 'at',
                        selection
                    });
                    return { kind: 'replace-input', text: '' };
                }

                return {
                    kind: 'push-stage',
                    stageState: {
                        harness: state1.harness,
                        model
                    } satisfies AtStage2State,
                    initialQuery: ''
                };
            }

            const state2 = q.stageState as AtStage2State;
            const level = item.payload as ReasoningLevel_Serialize;
            const selection: AiSelection_Serialize = {
                harnessId: state2.harness.id,
                modelId: state2.model.id,
                reasoningId: level.id
            };

            aiStore.openPanePrompt(ctx.paneId, {
                kind: 'suggest',
                trigger: 'at',
                selection
            });
            return { kind: 'replace-input', text: '' };
        }
    };
}

/**
 * Dispara uma sugestão direta no painel `paneId` usando o gatilho `?` e a seleção padrão.
 */
async function triggerDirectQuestionSuggest(
    paneId: string,
    prompt: string,
    selection: AiSelection_Serialize | null
): Promise<void> {
    const aiStore = useAiStore();
    const requestId = createAiRequestId('ai-sug');

    aiStore.setPaneSuggestion(paneId, {
        requestId,
        trigger: 'question',
        prompt,
        selection,
        status: 'streaming',
        streamingText: '',
        suggestion: null,
        editedCommand: null,
        effectiveRisk: null,
        contextPreview: null,
        error: null
    });

    try {
        await aiIpc.suggestCommand(
            {
                requestId,
                paneId,
                trigger: 'question',
                selection,
                prompt,
                regenerateFrom: null
            },
            (ev) => {
                if (ev.type === 'started') {
                    aiStore.patchPaneSuggestion(paneId, {
                        requestId: ev.requestId,
                        selection: ev.selection,
                        contextPreview: ev.context
                    });
                } else if (ev.type === 'delta') {
                    const current = aiStore.suggestionForPane(paneId);
                    aiStore.patchPaneSuggestion(paneId, {
                        streamingText: (current?.streamingText ?? '') + ev.text
                    });
                } else if (ev.type === 'completed') {
                    aiStore.patchPaneSuggestion(paneId, {
                        status: 'completed',
                        suggestion: ev.suggestion,
                        editedCommand: ev.suggestion.command,
                        effectiveRisk: ev.suggestion.risk,
                        contextPreview: ev.suggestion.contextSummary,
                        error: null
                    });
                } else if (ev.type === 'failed') {
                    aiStore.patchPaneSuggestion(paneId, {
                        status: 'failed',
                        error: ev.error.message
                    });
                } else if (ev.type === 'cancelled') {
                    aiStore.patchPaneSuggestion(paneId, {
                        status: 'cancelled'
                    });
                }
            }
        );
    } catch (err: unknown) {
        const message = err instanceof Error ? err.message : 'Falha ao solicitar comando à IA';
        aiStore.patchPaneSuggestion(paneId, {
            status: 'failed',
            error: message
        });
    }
}

/**
 * Cria a definição do gatilho `?` (usa seleção padrão configurada ou consulta RAG).
 */
export function createQuestionTriggerDefinition(): TriggerDefinition {
    return {
        id: 'ai-question',
        prefix: '?',
        label: 'Comando por Linguagem Natural (?)',
        debounceMs: 40,

        isAvailable(ctx: TerminalContext): boolean {
            return isRichInputMode(ctx);
        },

        provide(q: TriggerQuery): CompletionResult {
            const aiStore = useAiStore();
            if (aiStore.settings && !aiStore.settings.enabled) {
                return buildSettingsDisabledResult();
            }

            const defSel = aiStore.defaultSelection;
            const trimmedQuery = q.query.trim();

            if (!defSel) {
                return {
                    stageLabel: 'Modelo Padrão Não Configurado (?)',
                    emptyMessage: 'Configure um modelo padrão ou use @ para escolher',
                    items: [
                        {
                            id: 'action:configure-default',
                            label: 'Definir Modelo Padrão nas Preferências de IA...',
                            description: 'Configura o Harness, Modelo e Nível de Raciocínio padrão para o gatilho ?',
                            icon: 'lucide:sliders-horizontal',
                            kind: 'action'
                        },
                        {
                            id: 'action:switch-to-at',
                            label: 'Escolher modelo agora com @',
                            description: 'Abre o seletor em cascata de Harness → Modelo → Raciocínio',
                            icon: 'lucide:at-sign',
                            kind: 'action'
                        },
                        {
                            id: 'action:rag',
                            label: trimmedQuery
                                ? `Consultar documentação (RAG): "${trimmedQuery}"`
                                : 'Consultar documentação indexada (RAG)',
                            description: 'Pesquisa trechos relevantes na documentação da stack no MaxAiManager',
                            icon: 'lucide:book-open',
                            kind: 'action'
                        }
                    ]
                };
            }

            const badgeText = defSel.reasoningId
                ? `${defSel.harnessId} · ${defSel.modelId} (${defSel.reasoningId})`
                : `${defSel.harnessId} · ${defSel.modelId}`;

            const items: CompletionItem[] = [
                {
                    id: 'action:suggest',
                    label: trimmedQuery
                        ? `Gerar comando Shell: "${trimmedQuery}"`
                        : 'Descrever comando em português (NL → Shell)',
                    description: `Tradução via modelo padrão: ${badgeText}`,
                    icon: 'lucide:sparkles',
                    detail: badgeText,
                    kind: 'action'
                },
                {
                    id: 'action:rag',
                    label: trimmedQuery
                        ? `Consultar documentação (RAG): "${trimmedQuery}"`
                        : 'Consultar documentação indexada (RAG)',
                    description: 'Busca em /v1/rags/{slug}/search direto do terminal',
                    icon: 'lucide:book-open',
                    detail: 'RAG',
                    kind: 'action'
                },
                {
                    id: 'action:open-settings',
                    label: 'Alterar modelo padrão de IA...',
                    description: 'Abre as preferências do MaxAiManager',
                    icon: 'lucide:settings',
                    kind: 'action',
                    pinned: 'bottom'
                }
            ];

            return {
                stageLabel: `IA Padrão · ${badgeText}`,
                emptyMessage: 'Digite o que deseja executar em português',
                items
            };
        },

        async onSelect(
            item: CompletionItem,
            q: TriggerQuery,
            ctx: TerminalContext
        ): Promise<TriggerSelectOutcome> {
            const aiStore = useAiStore();
            const windowStore = useWindowStore();
            const trimmedQuery = q.query.trim();

            if (item.id === 'action:open-settings' || item.id === 'action:configure-default') {
                windowStore.openPreferences('ai');
                return { kind: 'replace-input', text: '' };
            }

            if (item.id === 'action:switch-to-at') {
                return {
                    kind: 'replace-input',
                    text: '@',
                    keepOpen: true
                };
            }

            if (item.id === 'action:rag') {
                const ragPrompt = trimmedQuery.replace(/^\/rag\s*/i, '').trim();
                aiStore.openPanePrompt(ctx.paneId, {
                    kind: 'rag',
                    trigger: 'question',
                    selection: aiStore.defaultSelection,
                    initialPrompt: ragPrompt
                });
                return { kind: 'replace-input', text: '' };
            }

            // action:suggest
            const defSel = aiStore.defaultSelection;
            if (!defSel) {
                windowStore.openPreferences('ai');
                return { kind: 'replace-input', text: '' };
            }

            if (trimmedQuery.length > 0) {
                aiStore.closePanePrompt(ctx.paneId);
                void triggerDirectQuestionSuggest(ctx.paneId, trimmedQuery, defSel);
                return { kind: 'replace-input', text: '' };
            }

            aiStore.openPanePrompt(ctx.paneId, {
                kind: 'suggest',
                trigger: 'question',
                selection: defSel
            });
            return { kind: 'replace-input', text: '' };
        },

        async onSubmitWithoutSelection(
            q: TriggerQuery,
            ctx: TerminalContext
        ): Promise<TriggerSelectOutcome> {
            const aiStore = useAiStore();
            const windowStore = useWindowStore();
            const trimmedQuery = q.query.trim();
            const defSel = aiStore.defaultSelection;

            if (!defSel) {
                windowStore.openPreferences('ai');
                return { kind: 'replace-input', text: '' };
            }

            if (trimmedQuery.length > 0) {
                aiStore.closePanePrompt(ctx.paneId);
                void triggerDirectQuestionSuggest(ctx.paneId, trimmedQuery, defSel);
                return { kind: 'replace-input', text: '' };
            }

            aiStore.openPanePrompt(ctx.paneId, {
                kind: 'suggest',
                trigger: 'question',
                selection: defSel
            });
            return { kind: 'replace-input', text: '' };
        }
    };
}
