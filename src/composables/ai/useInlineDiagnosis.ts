import {
    computed,
    getCurrentInstance,
    onBeforeUnmount,
    toValue,
    watch,
    type ComputedRef,
    type MaybeRefOrGetter
} from 'vue';
import {
    createAiRequestId,
    executeCommandInPane,
    insertCommandIntoRichInput,
    resolvePaneProjectInfo
} from '@/composables/ai/adapters';
import { useIpcError } from '@/composables/useIpcError';
import { aiIpc } from '@/ipc/ai';
import { useAiStore, type BlockDiagnosisState } from '@/stores/ai';
import { useTerminalStore } from '@/stores/terminal';
import type {
    CommandBlockId,
    InlineDiagnosis_Serialize,
    PaneId
} from '@/types/bindings';

export interface DiagnosableBlockLike {
    id: CommandBlockId;
    paneId?: PaneId | null;
    status: string;
    exitCode: number | null;
    command?: string | null;
}

const activePaneDebounceTimers = new Map<PaneId, ReturnType<typeof setTimeout>>();

export interface UseInlineDiagnosisReturn {
    state: ComputedRef<BlockDiagnosisState | null>;
    diagnosis: ComputedRef<InlineDiagnosis_Serialize | null>;
    loading: ComputedRef<boolean>;
    visible: ComputedRef<boolean>;
    eligible: ComputedRef<boolean>;
    diagnoseNow: () => Promise<InlineDiagnosis_Serialize | null>;
    dismiss: () => Promise<void>;
    insertFix: () => boolean;
    executeSafeOrCautionFix: () => Promise<void>;
    executeDestructiveFix: (typedToken: string) => Promise<void>;
}

/**
 * Composable de Diagnóstico Rápido Inline (`CA-19`..`CA-25`) exibido ao lado do botão "Inspecionar"
 * quando um bloco de comando falha (`exit != 0` e não ignorado como `130`).
 */
export function useInlineDiagnosis(
    blockSource: MaybeRefOrGetter<DiagnosableBlockLike>,
    options: { autoTrigger?: boolean } = {}
): UseInlineDiagnosisReturn {
    const aiStore = useAiStore();
    const terminalStore = useTerminalStore();
    const ipcError = useIpcError();

    const autoTrigger = options.autoTrigger ?? true;
    const block = computed<DiagnosableBlockLike>(() => toValue(blockSource));

    const state = computed<BlockDiagnosisState | null>(() =>
        aiStore.diagnosisForBlock(block.value.id)
    );

    const diagnosis = computed<InlineDiagnosis_Serialize | null>(
        () => state.value?.diagnosis ?? null
    );

    const loading = computed<boolean>(() => state.value?.status === 'pending');

    const eligible = computed<boolean>(() => {
        const b = block.value;
        if (!b || !b.id) {
            return false;
        }
        if (b.exitCode === null || b.exitCode === 0) {
            return false;
        }
        const ignoredCodes = aiStore.settings?.diagnosis.ignoredExitCodes ?? [130];
        if (ignoredCodes.includes(b.exitCode)) {
            return false;
        }
        if (b.status !== 'error' && b.status !== 'failed') {
            return false;
        }
        if (aiStore.settings && (!aiStore.settings.enabled || !aiStore.settings.diagnosis.enabled)) {
            return false;
        }
        if (b.paneId) {
            const inst = terminalStore.instance(b.paneId);
            if (inst && (inst.modes.altScreen || inst.inputMode === 'passthrough')) {
                return false;
            }
            const { projectId } = resolvePaneProjectInfo(b.paneId);
            const disabledProjects = aiStore.settings?.diagnosis.disabledProjects ?? [];
            if (projectId && disabledProjects.includes(projectId)) {
                return false;
            }
        }
        return true;
    });

    const visible = computed<boolean>(() => {
        if (!eligible.value) {
            return false;
        }
        const st = state.value?.status;
        if (st === 'dismissed' || st === 'skipped') {
            return false;
        }
        return Boolean(st === 'pending' || st === 'ready' || st === 'failed');
    });

    async function diagnoseNow(): Promise<InlineDiagnosis_Serialize | null> {
        const b = block.value;
        if (!eligible.value) {
            return null;
        }

        const existing = aiStore.diagnosisForBlock(b.id);
        if (existing?.status === 'ready' && existing.diagnosis) {
            return existing.diagnosis;
        }
        if (existing?.status === 'dismissed') {
            return null;
        }

        aiStore.setBlockDiagnosis(b.id, {
            blockId: b.id,
            status: 'pending',
            diagnosis: null,
            skipReason: null,
            error: null
        });

        try {
            const outcome = await aiIpc.diagnoseBlock({
                requestId: createAiRequestId('ai-diag'),
                blockId: b.id
            });

            if (outcome.status === 'ready') {
                aiStore.setBlockDiagnosis(b.id, {
                    blockId: b.id,
                    status: 'ready',
                    diagnosis: outcome.diagnosis,
                    skipReason: null,
                    error: null
                });
                return outcome.diagnosis;
            }

            aiStore.setBlockDiagnosis(b.id, {
                blockId: b.id,
                status: 'skipped',
                diagnosis: null,
                skipReason: outcome.reason,
                error: null
            });
            return null;
        } catch (err: unknown) {
            aiStore.setBlockDiagnosis(b.id, {
                blockId: b.id,
                status: 'failed',
                diagnosis: null,
                skipReason: null,
                error: ipcError.message(err)
            });
            return null;
        }
    }

    let localTimer: ReturnType<typeof setTimeout> | null = null;

    function clearPendingTimer(): void {
        if (localTimer !== null) {
            clearTimeout(localTimer);
            localTimer = null;
        }
        const pid = block.value.paneId;
        if (pid && activePaneDebounceTimers.get(pid) === localTimer) {
            activePaneDebounceTimers.delete(pid);
        }
    }

    if (autoTrigger) {
        watch(
            () => [block.value.id, block.value.status, block.value.exitCode, eligible.value] as const,
            ([id, _status, _exitCode, isEligible]) => {
                clearPendingTimer();
                if (!isEligible || !id) {
                    return;
                }
                const current = aiStore.diagnosisForBlock(id);
                if (current) {
                    return;
                }

                const pid = block.value.paneId;
                if (pid) {
                    const prevPaneTimer = activePaneDebounceTimers.get(pid);
                    if (prevPaneTimer) {
                        clearTimeout(prevPaneTimer);
                        activePaneDebounceTimers.delete(pid);
                    }
                }

                const debounceMs = aiStore.settings?.diagnosis.debounceMs ?? 800;
                const timer = setTimeout(() => {
                    localTimer = null;
                    if (pid && activePaneDebounceTimers.get(pid) === timer) {
                        activePaneDebounceTimers.delete(pid);
                    }
                    void diagnoseNow();
                }, debounceMs);

                localTimer = timer;
                if (pid) {
                    activePaneDebounceTimers.set(pid, timer);
                }
            },
            { immediate: true }
        );
    }

    if (getCurrentInstance()) {
        onBeforeUnmount(() => {
            clearPendingTimer();
        });
    }

    async function dismiss(): Promise<void> {
        clearPendingTimer();
        const id = block.value.id;
        aiStore.setBlockDiagnosis(id, {
            blockId: id,
            status: 'dismissed',
            diagnosis: null,
            skipReason: null,
            error: null
        });
        try {
            await aiIpc.diagnosisDismiss(id);
        } catch {
            // Mantém dispensado localmente mesmo se o bloco ainda não estava no cache.
        }
    }

    function insertFix(): boolean {
        const fix = diagnosis.value?.fixCommand?.trim();
        const pid = block.value.paneId;
        if (!fix || !pid) {
            return false;
        }
        return insertCommandIntoRichInput(pid, fix);
    }

    async function executeSafeOrCautionFix(): Promise<void> {
        const diag = diagnosis.value;
        const fix = diag?.fixCommand?.trim();
        const pid = block.value.paneId;
        if (!fix || !pid) {
            return;
        }

        const risk = diag?.fixRisk ?? (await aiIpc.classifyRisk(fix));
        if (risk.level === 'destructive' || risk.requiresConfirmation) {
            throw new Error(
                'A correção sugerida é destrutiva e exige confirmação com token.'
            );
        }

        await executeCommandInPane(pid, fix);
    }

    async function executeDestructiveFix(typedToken: string): Promise<void> {
        const diag = diagnosis.value;
        const fix = diag?.fixCommand?.trim();
        const pid = block.value.paneId;
        if (!fix || !pid) {
            return;
        }

        const risk = diag?.fixRisk ?? (await aiIpc.classifyRisk(fix));
        const expected = risk.confirmToken ?? '';
        if (!expected || typedToken !== expected) {
            throw new Error(`Token de confirmação inválido (esperado "${expected}").`);
        }

        await executeCommandInPane(pid, fix);
    }

    return {
        state,
        diagnosis,
        loading,
        visible,
        eligible,
        diagnoseNow,
        dismiss,
        insertFix,
        executeSafeOrCautionFix,
        executeDestructiveFix
    };
}
