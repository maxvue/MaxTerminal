import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import RiskBadge from '@/components/ai/RiskBadge.vue';
import type { RiskAssessment_Serialize } from '@/types/bindings';

describe('RiskBadge.vue (Fase 12 — Unidade 03)', () => {
    it('renderiza classes e rótulos semânticos para níveis safe, caution e destructive', () => {
        const safeRisk: RiskAssessment_Serialize = {
            level: 'safe',
            localLevel: 'safe',
            modelHint: null,
            reasons: [],
            requiresConfirmation: false,
            confirmToken: null
        };
        const safeWrapper = mount(RiskBadge, {
            props: { risk: safeRisk },
            global: { stubs: { MaxIcon: true } }
        });
        expect(safeWrapper.classes()).toContain('is-safe');
        expect(safeWrapper.text()).toContain('Seguro');

        const cautionRisk: RiskAssessment_Serialize = {
            level: 'caution',
            localLevel: 'caution',
            modelHint: null,
            reasons: [
                {
                    ruleId: 'sudo-invocation',
                    message: 'Executa comando com privilégios elevados (sudo)',
                    segment: 'sudo systemctl restart nginx'
                }
            ],
            requiresConfirmation: false,
            confirmToken: null
        };
        const cautionWrapper = mount(RiskBadge, {
            props: { risk: cautionRisk },
            global: { stubs: { MaxIcon: true } }
        });
        expect(cautionWrapper.classes()).toContain('is-caution');
        expect(cautionWrapper.text()).toContain('Atenção');
        expect(cautionWrapper.text()).toContain('(1)');

        const destructiveRisk: RiskAssessment_Serialize = {
            level: 'destructive',
            localLevel: 'destructive',
            modelHint: null,
            reasons: [
                {
                    ruleId: 'rm-recursive-or-force',
                    message: 'Remoção recursiva forçada',
                    segment: 'rm -rf /tmp/data'
                }
            ],
            requiresConfirmation: true,
            confirmToken: 'rm -rf /tmp/data'
        };
        const destructiveWrapper = mount(RiskBadge, {
            props: { risk: destructiveRisk, compact: true },
            global: { stubs: { MaxIcon: true } }
        });
        expect(destructiveWrapper.classes()).toContain('is-destructive');
        expect(destructiveWrapper.classes()).toContain('is-compact');
        expect(destructiveWrapper.text()).toContain('Destrutivo');
    });
});
