import { computed, ref, toValue, type ComputedRef, type MaybeRefOrGetter, type Ref } from 'vue';
import {
    createAiRequestId,
    executeCommandInPane,
    insertCommandIntoRichInput
} from '@/composables/ai/adapters';
import { useIpcError } from '@/composables/useIpcError';
import { aiIpc } from '@/ipc/ai';
import {
    useAiStore,
    type PaneAiPromptMode,
    type PaneSuggestionState
} from '@/stores/ai';
import type {
    AiContext_Serialize,
    AiRequestId,
    AiSelection_Serialize,
    AiTrigger,
    PaneId,
    RiskAssessment_Serialize
} from '@/types/bindings';

export interface SuggestCommandOptions {
    trigger?: AiTrigger;
    selection?: AiSelection_Serialize | null;
    regenerateFrom?: AiRequestId | null;
}

export interface UseAiSuggestionReturn {
    state: ComputedRef<PaneSuggestionState | null>;
    activeMode: ComputedRef<PaneAiPromptMode | null>;
    isStreaming: ComputedRef<boolean>;
    effectiveCommand: ComputedRef<string>;
    effectiveRisk: ComputedRef<RiskAssessment_Serialize | null>;
    reclassifying: Ref<boolean>;
    contextLoading: Ref<boolean>;
    suggest: (prompt: string, opts?: SuggestCommandOptions) => Promise<AiRequestId>;
    regenerate: () => Promise<AiRequestId | null>;
    cancel: () => Promise<boolean>;
    updateEditedCommand: (nextCommand: string) => Promise<RiskAssessment_Serialize | null>;
    insertIntoInput: () => boolean;
    executeSafeOrCaution: () => Promise<void>;
    executeDestructiveConfirmed: (typedToken: string) => Promise<void>;
    loadContextPreview: () => Promise<AiContext_Serialize>;
    dismiss: () => void;
}

/**
 * Composable que gerencia o ciclo de vida completo de sugestão NL → Shell (`@` e `?`) para um painel:
 * streaming incremental, reclassificação determinística de risco ao editar o comando,
 * inserção no `RichInput`, execução segura (`safe` / `caution` / `destructive`) e preview de contexto.
 */
export function useAiSuggestion(paneIdSource: MaybeRefOrGetter<PaneId>): UseAiSuggestionReturn {
    const aiStore = useAiStore();
    const ipcError = useIpcError();

    const reclassifying = ref<boolean>(false);
    const contextLoading = ref<boolean>(false);
    let classifySeq = 0;

    const paneId = computed<PaneId>(() => toValue(paneIdSource));

    const state = computed<PaneSuggestionState | null>(() =>
        aiStore.suggestionForPane(paneId.value)
    );

    const activeMode = computed<PaneAiPromptMode | null>(() =>
        aiStore.activeModeForPane(paneId.value)
    );

    const isStreaming = computed<boolean>(() => state.value?.status === 'streaming');

    const effectiveCommand = computed<string>(() => {
        const s = state.value;
        if (!s) {
            return '';
        }
        if (s.editedCommand !== null) {
            return s.editedCommand;
        }
        return s.suggestion?.command ?? '';
    });

    const effectiveRisk = computed<RiskAssessment_Serialize | null>(() => {
        const s = state.value;
        if (!s) {
            return null;
        }
        return s.effectiveRisk ?? s.suggestion?.risk ?? null;
    });

    async function suggest(
        prompt: string,
        opts: SuggestCommandOptions = {}
    ): Promise<AiRequestId> {
        const trimmed = prompt.trim();
        if (!trimmed) {
            throw new Error('Digite uma instrução em português para gerar o comando.');
        }

        const pid = paneId.value;
        const current = aiStore.suggestionForPane(pid);
        if (current?.status === 'streaming' && current.requestId) {
            void aiIpc.cancel(current.requestId).catch(() => {});
        }

        const trigger: AiTrigger =
            opts.trigger ?? aiStore.activeModeForPane(pid)?.trigger ?? 'question';
        const selection: AiSelection_Serialize | null =
            opts.selection !== undefined
                ? opts.selection
                : (aiStore.activeModeForPane(pid)?.selection ??
                    aiStore.lastSelectionByPane[pid] ??
                    aiStore.defaultSelection);

        const requestId = createAiRequestId('ai-sug');

        aiStore.setPaneSuggestion(pid, {
            requestId,
            trigger,
            prompt: trimmed,
            selection,
            status: 'streaming',
            streamingText: '',
            suggestion: null,
            editedCommand: null,
            effectiveRisk: null,
            contextPreview: null,
            error: null
        });

        aiStore.closePanePrompt(pid);

        try {
            const confirmedId = await aiIpc.suggestCommand(
                {
                    requestId,
                    paneId: pid,
                    trigger,
                    selection,
                    prompt: trimmed,
                    regenerateFrom: opts.regenerateFrom ?? null
                },
                (ev) => {
                    const snap = aiStore.suggestionForPane(pid);
                    if (!snap || snap.requestId !== requestId) {
                        return;
                    }

                    if (ev.type === 'started') {
                        aiStore.patchPaneSuggestion(pid, {
                            selection: ev.selection,
                            contextPreview: ev.context
                        });
                    } else if (ev.type === 'delta') {
                        aiStore.patchPaneSuggestion(pid, {
                            streamingText: snap.streamingText + ev.text
                        });
                    } else if (ev.type === 'completed') {
                        aiStore.patchPaneSuggestion(pid, {
                            status: 'completed',
                            suggestion: ev.suggestion,
                            editedCommand: ev.suggestion.command,
                            effectiveRisk: ev.suggestion.risk,
                            contextPreview: ev.suggestion.contextSummary,
                            error: null
                        });
                    } else if (ev.type === 'failed') {
                        aiStore.patchPaneSuggestion(pid, {
                            status: 'failed',
                            error: ev.error.message
                        });
                    } else if (ev.type === 'cancelled') {
                        aiStore.patchPaneSuggestion(pid, {
                            status: 'cancelled'
                        });
                    }
                }
            );
            return confirmedId;
        } catch (err: unknown) {
            const msg = ipcError.message(err);
            aiStore.patchPaneSuggestion(pid, {
                status: 'failed',
                error: msg
            });
            throw err;
        }
    }

    async function regenerate(): Promise<AiRequestId | null> {
        const current = state.value;
        if (!current || !current.prompt) {
            return null;
        }
        return suggest(current.prompt, {
            trigger: current.trigger,
            selection: current.selection,
            regenerateFrom: current.requestId
        });
    }

    async function cancel(): Promise<boolean> {
        const pid = paneId.value;
        const current = aiStore.suggestionForPane(pid);
        if (!current || !current.requestId) {
            return false;
        }
        const reqId = current.requestId;
        aiStore.patchPaneSuggestion(pid, {
            status: 'cancelled'
        });
        try {
            return await aiIpc.cancel(reqId);
        } catch {
            return false;
        }
    }

    async function updateEditedCommand(
        nextCommand: string
    ): Promise<RiskAssessment_Serialize | null> {
        const pid = paneId.value;
        const current = aiStore.suggestionForPane(pid);
        if (!current) {
            return null;
        }

        const sanitizedSingleLine = nextCommand.replace(/[\r\n]+/g, ' ');
        aiStore.patchPaneSuggestion(pid, {
            editedCommand: sanitizedSingleLine
        });

        const seq = ++classifySeq;
        reclassifying.value = true;
        try {
            const risk = await aiIpc.classifyRisk(sanitizedSingleLine);
            if (seq === classifySeq) {
                aiStore.patchPaneSuggestion(pid, {
                    effectiveRisk: risk
                });
            }
            return risk;
        } catch {
            return current.effectiveRisk;
        } finally {
            if (seq === classifySeq) {
                reclassifying.value = false;
            }
        }
    }

    function insertIntoInput(): boolean {
        const pid = paneId.value;
        const cmd = effectiveCommand.value.trim();
        if (!cmd) {
            return false;
        }
        const ok = insertCommandIntoRichInput(pid, cmd);
        if (ok) {
            aiStore.clearPaneSuggestion(pid);
        }
        return ok;
    }

    async function executeSafeOrCaution(): Promise<void> {
        const pid = paneId.value;
        const cmd = effectiveCommand.value.trim();
        if (!cmd) {
            return;
        }

        // Revalida o risco caso o comando tenha sido editado para impedir bypass de comando destrutivo
        const currentRisk = await aiIpc.classifyRisk(cmd);
        aiStore.patchPaneSuggestion(pid, {
            effectiveRisk: currentRisk
        });

        if (currentRisk.level === 'destructive' || currentRisk.requiresConfirmation) {
            throw new Error(
                'Este comando foi classificado como destrutivo e exige confirmação explícita com token.'
            );
        }

        await executeCommandInPane(pid, cmd);
        aiStore.clearPaneSuggestion(pid);
    }

    async function executeDestructiveConfirmed(typedToken: string): Promise<void> {
        const pid = paneId.value;
        const cmd = effectiveCommand.value.trim();
        if (!cmd) {
            return;
        }

        const currentRisk = effectiveRisk.value ?? (await aiIpc.classifyRisk(cmd));
        const expectedToken = currentRisk.confirmToken ?? '';

        if (!expectedToken || typedToken !== expectedToken) {
            throw new Error(
                `O texto de confirmação não coincide com "${expectedToken}".`
            );
        }

        await executeCommandInPane(pid, cmd);
        aiStore.clearPaneSuggestion(pid);
    }

    async function loadContextPreview(): Promise<AiContext_Serialize> {
        const pid = paneId.value;
        contextLoading.value = true;
        try {
            const ctx = await aiIpc.contextPreview(pid);
            if (aiStore.suggestionForPane(pid)) {
                aiStore.patchPaneSuggestion(pid, {
                    contextPreview: ctx
                });
            }
            return ctx;
        } finally {
            contextLoading.value = false;
        }
    }

    function dismiss(): void {
        const pid = paneId.value;
        const current = aiStore.suggestionForPane(pid);
        if (current?.status === 'streaming' && current.requestId) {
            void aiIpc.cancel(current.requestId).catch(() => {});
        }
        aiStore.clearPaneSuggestion(pid);
        aiStore.closePanePrompt(pid);
    }

    return {
        state,
        activeMode,
        isStreaming,
        effectiveCommand,
        effectiveRisk,
        reclassifying,
        contextLoading,
        suggest,
        regenerate,
        cancel,
        updateEditedCommand,
        insertIntoInput,
        executeSafeOrCaution,
        executeDestructiveConfirmed,
        loadContextPreview,
        dismiss
    };
}
