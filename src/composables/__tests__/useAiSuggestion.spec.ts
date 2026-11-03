import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as adapters from '@/composables/ai/adapters';
import { useAiSuggestion } from '@/composables/ai/useAiSuggestion';
import { aiIpc } from '@/ipc/ai';
import { DEFAULT_AI_SETTINGS, useAiStore } from '@/stores/ai';
import type {
    AiContext_Serialize,
    AiSelection_Serialize,
    CommandSuggestion_Serialize,
    RiskAssessment_Serialize
} from '@/types/bindings';

const sampleSelection: AiSelection_Serialize = {
    harnessId: 'claude-code',
    modelId: 'claude-sonnet-4-5',
    reasoningId: 'medium'
};

const sampleContext: AiContext_Serialize = {
    shell: 'zsh',
    cwd: '/home/user/app',
    projectName: 'app',
    projectStack: 'laravel',
    os: { family: 'linux', distro: 'CachyOS', kernel: '6.12' },
    recentCommands: [],
    outputTail: null
};

describe('useAiSuggestion (Fase 12 — Unidades 03 e 04)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('processa streaming started → delta → completed, permite editar com reclassificação de risco e inserir/executar', async () => {
        const aiStore = useAiStore();
        aiStore.settings = { ...DEFAULT_AI_SETTINGS, defaultSelection: sampleSelection };

        const safeRisk: RiskAssessment_Serialize = {
            level: 'safe',
            localLevel: 'safe',
            modelHint: null,
            reasons: [],
            requiresConfirmation: false,
            confirmToken: null
        };
        const suggestion: CommandSuggestion_Serialize = {
            requestId: 'req-1',
            command: 'git status -sb',
            explanation: 'Exibe o status resumido da branch atual.',
            risk: safeRisk,
            selection: sampleSelection,
            latencyMs: 180,
            contextSummary: sampleContext
        };

        vi.spyOn(aiIpc, 'suggestCommand').mockImplementationOnce(async (req, onEvent) => {
            onEvent({
                type: 'started',
                requestId: req.requestId,
                selection: sampleSelection,
                context: sampleContext
            });
            onEvent({
                type: 'delta',
                requestId: req.requestId,
                text: 'git status'
            });
            onEvent({
                type: 'completed',
                requestId: req.requestId,
                suggestion: {
                    ...suggestion,
                    requestId: req.requestId
                }
            });
            return req.requestId;
        });

        const sug = useAiSuggestion('pane-1');
        await sug.suggest('mostrar status do git');

        expect(sug.state.value?.status).toBe('completed');
        expect(sug.effectiveCommand.value).toBe('git status -sb');
        expect(sug.effectiveRisk.value?.level).toBe('safe');

        // Reclassificação ao editar para um comando destrutivo
        const destructiveRisk: RiskAssessment_Serialize = {
            level: 'destructive',
            localLevel: 'destructive',
            modelHint: null,
            reasons: [
                {
                    ruleId: 'rm-recursive-or-force',
                    message: 'Remove recursivamente arquivos com rm -rf',
                    segment: 'rm -rf build'
                }
            ],
            requiresConfirmation: true,
            confirmToken: 'rm -rf build'
        };
        vi.spyOn(aiIpc, 'classifyRisk').mockResolvedValue(destructiveRisk);

        await sug.updateEditedCommand('rm -rf build');
        expect(sug.effectiveCommand.value).toBe('rm -rf build');
        expect(sug.effectiveRisk.value?.level).toBe('destructive');

        // Impede execução direta sem confirmação por token
        await expect(sug.executeSafeOrCaution()).rejects.toThrow('classificado como destrutivo');
        await expect(sug.executeDestructiveConfirmed('token-errado')).rejects.toThrow(
            'O texto de confirmação não coincide'
        );

        const execSpy = vi.spyOn(adapters, 'executeCommandInPane').mockResolvedValue();
        await sug.executeDestructiveConfirmed('rm -rf build');
        expect(execSpy).toHaveBeenCalledWith('pane-1', 'rm -rf build');
        expect(sug.state.value).toBeNull();
    });

    it('suporta regeneração, cancelamento, inserção no RichInput e carregamento de preview de contexto', async () => {
        const sug = useAiSuggestion('pane-2');
        const aiStore = useAiStore();

        aiStore.setPaneSuggestion('pane-2', {
            requestId: 'req-prev',
            trigger: 'at',
            prompt: 'listar portas',
            selection: sampleSelection,
            status: 'completed',
            streamingText: 'ss -tulpn',
            suggestion: null,
            editedCommand: 'ss -tulpn',
            effectiveRisk: {
                level: 'safe',
                localLevel: 'safe',
                modelHint: null,
                reasons: [],
                requiresConfirmation: false,
                confirmToken: null
            },
            contextPreview: null,
            error: null
        });

        vi.spyOn(aiIpc, 'contextPreview').mockResolvedValueOnce(sampleContext);
        const ctx = await sug.loadContextPreview();
        expect(ctx.cwd).toBe('/home/user/app');

        const insertSpy = vi.spyOn(adapters, 'insertCommandIntoRichInput').mockReturnValue(true);
        expect(sug.insertIntoInput()).toBe(true);
        expect(insertSpy).toHaveBeenCalledWith('pane-2', 'ss -tulpn');
        expect(sug.state.value).toBeNull();

        // Regenerate, cancel, dismiss e execução safe
        aiStore.setPaneSuggestion('pane-2', {
            requestId: 'req-regen',
            trigger: 'question',
            prompt: 'ver memória',
            selection: sampleSelection,
            status: 'completed',
            streamingText: 'free -h',
            suggestion: null,
            editedCommand: 'free -h',
            effectiveRisk: {
                level: 'safe',
                localLevel: 'safe',
                modelHint: null,
                reasons: [],
                requiresConfirmation: false,
                confirmToken: null
            },
            contextPreview: null,
            error: null
        });

        vi.spyOn(aiIpc, 'suggestCommand').mockResolvedValueOnce('req-regen-2');
        expect(await sug.regenerate()).toBe('req-regen-2');

        vi.spyOn(aiIpc, 'cancel').mockResolvedValueOnce(true);
        expect(await sug.cancel()).toBe(true);
        expect(sug.state.value?.status).toBe('cancelled');

        aiStore.patchPaneSuggestion('pane-2', {
            status: 'completed',
            editedCommand: 'free -h'
        });
        vi.spyOn(aiIpc, 'classifyRisk').mockResolvedValueOnce({
            level: 'safe',
            localLevel: 'safe',
            modelHint: null,
            reasons: [],
            requiresConfirmation: false,
            confirmToken: null
        });
        const execSafeSpy = vi.spyOn(adapters, 'executeCommandInPane').mockResolvedValueOnce();
        await sug.executeSafeOrCaution();
        expect(execSafeSpy).toHaveBeenCalledWith('pane-2', 'free -h');

        aiStore.setPaneSuggestion('pane-2', {
            requestId: 'req-d',
            trigger: 'question',
            prompt: 'teste',
            selection: sampleSelection,
            status: 'failed',
            streamingText: '',
            suggestion: null,
            editedCommand: null,
            effectiveRisk: null,
            contextPreview: null,
            error: 'erro'
        });
        sug.dismiss();
        expect(sug.state.value).toBeNull();
    });
});
