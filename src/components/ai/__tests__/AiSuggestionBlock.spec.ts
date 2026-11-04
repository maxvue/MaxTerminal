import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import AiSuggestionBlock from '@/components/ai/AiSuggestionBlock.vue';
import * as adapters from '@/composables/ai/adapters';
import { aiIpc } from '@/ipc/ai';
import { useAiStore } from '@/stores/ai';
import type { AiContext_Serialize, RiskAssessment_Serialize } from '@/types/bindings';

const sampleContext: AiContext_Serialize = {
    shell: 'zsh',
    cwd: '/home/user/app',
    projectName: 'app',
    projectStack: 'laravel',
    os: { family: 'linux', distro: 'CachyOS', kernel: '6.12' },
    recentCommands: [{ command: 'php artisan migrate', exitCode: 0, cwd: '/home/user/app' }],
    outputTail: 'DONE'
};

describe('AiSuggestionBlock.vue (Fase 12 — Unidades 03 e 04)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('exibe comando sugerido, permite alternar preview de contexto, editar comando, inserir no input e executar comando seguro/destrutivo', async () => {
        const aiStore = useAiStore();
        const safeRisk: RiskAssessment_Serialize = {
            level: 'safe',
            localLevel: 'safe',
            modelHint: null,
            reasons: [],
            requiresConfirmation: false,
            confirmToken: null
        };

        aiStore.setPaneSuggestion('pane-1', {
            requestId: 'req-1',
            trigger: 'at',
            prompt: 'executar migrations',
            selection: {
                harnessId: 'claude-code',
                modelId: 'claude-sonnet-4-5',
                reasoningId: 'medium'
            },
            status: 'completed',
            streamingText: 'php artisan migrate',
            suggestion: {
                requestId: 'req-1',
                command: 'php artisan migrate',
                explanation: 'Executa as migrations pendentes.',
                risk: safeRisk,
                selection: {
                    harnessId: 'claude-code',
                    modelId: 'claude-sonnet-4-5',
                    reasoningId: 'medium'
                },
                contextSummary: sampleContext,
                latencyMs: 140
            },
            editedCommand: 'php artisan migrate',
            effectiveRisk: safeRisk,
            contextPreview: sampleContext,
            error: null
        });

        const wrapper = mount(AiSuggestionBlock, {
            props: { paneId: 'pane-1' },
            global: { stubs: { MaxIcon: true } }
        });

        expect(wrapper.find('[data-testid="suggestion-prompt"]').text()).toContain(
            'executar migrations'
        );
        expect(wrapper.find('[data-testid="suggestion-explanation"]').text()).toContain(
            'migrations pendentes'
        );

        // Alterna preview de contexto sanitizado
        await wrapper.find('[data-testid="btn-toggle-context"]').trigger('click');
        expect(wrapper.find('[data-testid="ai-context-preview"]').exists()).toBe(true);

        // Quando editado para destrutivo, abre modal DestructiveConfirm ao clicar em executar
        const destructiveRisk: RiskAssessment_Serialize = {
            level: 'destructive',
            localLevel: 'destructive',
            modelHint: null,
            reasons: [
                {
                    ruleId: 'artisan-destructive',
                    message: 'Apaga todas as tabelas do banco',
                    segment: 'migrate:fresh'
                }
            ],
            requiresConfirmation: true,
            confirmToken: 'migrate:fresh'
        };
        vi.spyOn(aiIpc, 'classifyRisk').mockResolvedValue(destructiveRisk);

        const cmdInput = wrapper.find('[data-testid="suggestion-command-input"]');
        await cmdInput.setValue('php artisan migrate:fresh');

        await wrapper.find('[data-testid="btn-execute-command"]').trigger('click');
        expect(wrapper.find('[data-testid="destructive-confirm-modal"]').exists()).toBe(true);

        const execSpy = vi.spyOn(adapters, 'executeCommandInPane').mockResolvedValue();
        const tokenInput = wrapper.find('[data-testid="confirm-token-input"] input').exists()
            ? wrapper.find('[data-testid="confirm-token-input"] input')
            : wrapper.find('[data-testid="confirm-token-input"]');
        await tokenInput.setValue('migrate:fresh');
        await wrapper.find('[data-testid="btn-confirm-destructive"]').trigger('click');

        expect(execSpy).toHaveBeenCalledWith('pane-1', 'php artisan migrate:fresh');
    });
});
