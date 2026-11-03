import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import DestructiveConfirm from '@/components/ai/DestructiveConfirm.vue';
import type { RiskAssessment_Serialize } from '@/types/bindings';

const sampleRisk: RiskAssessment_Serialize = {
    level: 'destructive',
    localLevel: 'destructive',
    modelHint: null,
    reasons: [
        {
            ruleId: 'rm-recursive-or-force',
            message: 'Remoção recursiva forçada de diretório',
            segment: 'rm -rf dist'
        }
    ],
    requiresConfirmation: true,
    confirmToken: 'rm -rf dist'
};

describe('DestructiveConfirm.vue (Fase 12 — Unidade 03)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });
    it('mantém o botão de confirmação desabilitado até o usuário digitar exatamente o confirmToken', async () => {
        const wrapper = mount(DestructiveConfirm, {
            props: {
                open: true,
                command: 'rm -rf dist',
                risk: sampleRisk
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        expect(wrapper.find('[data-testid="destructive-command-preview"]').text()).toBe(
            'rm -rf dist'
        );
        expect(wrapper.find('[data-testid="expected-confirm-token"]').text()).toBe('rm -rf dist');

        const confirmBtn = wrapper.find('[data-testid="btn-confirm-destructive"]');
        expect((confirmBtn.element as HTMLButtonElement).disabled).toBe(true);

        const input = wrapper.find('[data-testid="confirm-token-input"]');
        await input.setValue('errado');
        expect((confirmBtn.element as HTMLButtonElement).disabled).toBe(true);

        await input.setValue('rm -rf dist');
        expect((confirmBtn.element as HTMLButtonElement).disabled).toBe(false);

        await confirmBtn.trigger('click');
        expect(wrapper.emitted('confirm')?.[0]).toEqual(['rm -rf dist']);

        await wrapper.find('[data-testid="btn-cancel-destructive"]').trigger('click');
        expect(wrapper.emitted('cancel')).toBeTruthy();
    });
});
