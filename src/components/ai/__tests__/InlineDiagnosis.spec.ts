import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import InlineDiagnosis from '@/components/ai/InlineDiagnosis.vue';
import * as adapters from '@/composables/ai/adapters';
import { aiIpc } from '@/ipc/ai';
import { DEFAULT_AI_SETTINGS, useAiStore } from '@/stores/ai';

describe('InlineDiagnosis.vue (Fase 12 — Unidade 05)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('exibe resumo do diagnóstico e sugestão corretiva ao lado do botão Inspecionar, permitindo inserir, aplicar e dispensar', async () => {
        const aiStore = useAiStore();
        aiStore.settings = { ...DEFAULT_AI_SETTINGS };
        aiStore.setBlockDiagnosis('block-err-1', {
            blockId: 'block-err-1',
            status: 'ready',
            diagnosis: {
                blockId: 'block-err-1',
                errorHash: 'hash-1',
                summary: 'Dependências ausentes na pasta node_modules.',
                fixCommand: 'npm install',
                fixRisk: {
                    level: 'safe',
                    localLevel: 'safe',
                    modelHint: null,
                    reasons: [],
                    requiresConfirmation: false,
                    confirmToken: null
                },
                fromCache: true,
                createdAt: '2026-11-01T12:00:00Z'
            },
            skipReason: null,
            error: null
        });

        const wrapper = mount(InlineDiagnosis, {
            props: {
                block: {
                    id: 'block-err-1',
                    paneId: 'pane-1',
                    status: 'error',
                    exitCode: 1,
                    command: 'npm test'
                },
                autoTrigger: false
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        expect(wrapper.find('[data-testid="diagnosis-summary"]').text()).toContain(
            'Dependências ausentes'
        );
        expect(wrapper.find('[data-testid="diagnosis-cache-badge"]').exists()).toBe(true);
        expect(wrapper.find('[data-testid="diagnosis-fix-command"]').text()).toBe('npm install');

        const insertSpy = vi.spyOn(adapters, 'insertCommandIntoRichInput').mockReturnValue(true);
        await wrapper.find('[data-testid="btn-insert-fix"]').trigger('click');
        expect(insertSpy).toHaveBeenCalledWith('pane-1', 'npm install');
        expect(wrapper.emitted('inserted')?.[0]).toEqual(['npm install']);

        vi.spyOn(aiIpc, 'classifyRisk').mockResolvedValueOnce({
            level: 'safe',
            localLevel: 'safe',
            modelHint: null,
            reasons: [],
            requiresConfirmation: false,
            confirmToken: null
        });
        const execSpy = vi.spyOn(adapters, 'executeCommandInPane').mockResolvedValue();
        await wrapper.find('[data-testid="btn-apply-fix"]').trigger('click');
        expect(execSpy).toHaveBeenCalledWith('pane-1', 'npm install');

        vi.spyOn(aiIpc, 'diagnosisDismiss').mockResolvedValueOnce();
        await wrapper.find('[data-testid="btn-dismiss-diagnosis"]').trigger('click');
        expect(wrapper.emitted('dismissed')?.[0]).toEqual(['block-err-1']);
    });
});
