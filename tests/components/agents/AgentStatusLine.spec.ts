import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import AgentStatusLine from '@/components/agents/AgentStatusLine.vue';
import { useAgentsStore } from '@/stores/agents';
import type { AgentTelemetry_Serialize } from '@/types/bindings';

describe('AgentStatusLine.vue (Fase 13)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.useFakeTimers();
        vi.setSystemTime(new Date(1_700_000_100_000));
    });

    it('exibe "—" com tooltip de indisponibilidade quando os campos são nulos (CA-19)', () => {
        const store = useAgentsStore();
        const telemetry: AgentTelemetry_Serialize = {
            tabId: 'tab-null',
            paneId: 'pane-1',
            sessionId: 'sess-1',
            runId: 'run-1',
            revision: 1,
            agent: 'agy',
            sessionRef: null,
            model: null,
            context: null,
            quota5h: null,
            quotaWeekly: null,
            costUsd: null,
            availability: {
                model: {
                    availability: 'unverified',
                    reason: 'Indisponível para este agente',
                    checkedAtMs: 1_700_000_100_000
                }
            },
            agentActive: true,
            updatedAtMs: 1_700_000_100_000
        };
        store.telemetryByTab['tab-null'] = telemetry;

        const wrapper = mount(AgentStatusLine, {
            props: { tabId: 'tab-null' },
            global: { stubs: { MaxIcon: true } }
        });

        const modelItem = wrapper.find('.agent-status-line__item--model');
        expect(modelItem.text()).toContain('—');
        expect(modelItem.attributes('title')).toContain('Indisponível para este agente');

        const quota5hItem = wrapper.find('.agent-status-line__item--quota5h');
        expect(quota5hItem.text()).toContain('—');
    });

    it('distingue "Modelo configurado" de "Modelo" observado e marca campo > 60s como desatualizado (CA-19, CA-34)', () => {
        const store = useAgentsStore();
        const now = 1_700_000_100_000;
        const staleObservedAt = now - 75_000; // 75s atrás (> 60s)

        const telemetry: AgentTelemetry_Serialize = {
            tabId: 'tab-stale',
            paneId: 'pane-1',
            sessionId: 'sess-1',
            runId: 'run-1',
            revision: 2,
            agent: 'claude-code',
            sessionRef: 's-1',
            model: {
                value: { id: 'claude-sonnet-4-5', displayName: 'Sonnet 4.5' },
                source: 'configured',
                observedAtMs: now - 5_000,
                receivedAtMs: now - 5_000,
                quality: 'configured',
                expiresAtMs: null
            },
            context: {
                value: { usedTokens: 50_000, windowTokens: 200_000, percent: 25 },
                source: 'claudeStatusLine',
                observedAtMs: staleObservedAt,
                receivedAtMs: staleObservedAt,
                quality: 'reported',
                expiresAtMs: null
            },
            quota5h: {
                value: {
                    windowMinutes: 300,
                    usedPercent: 42,
                    resetsAtMs: now + 3_600_000
                },
                source: 'claudeStatusLine',
                observedAtMs: now - 10_000,
                receivedAtMs: now - 10_000,
                quality: 'estimated',
                expiresAtMs: null
            },
            quotaWeekly: null,
            costUsd: null,
            availability: {},
            agentActive: true,
            updatedAtMs: now
        };
        store.telemetryByTab['tab-stale'] = telemetry;

        const wrapper = mount(AgentStatusLine, {
            props: { tabId: 'tab-stale' },
            global: { stubs: { MaxIcon: true } }
        });

        const modelItem = wrapper.find('.agent-status-line__item--model');
        expect(modelItem.text()).toContain('Modelo configurado:');
        expect(modelItem.text()).toContain('Sonnet 4.5');
        expect(modelItem.find('.agent-status-line__stale').exists()).toBe(false);

        const contextItem = wrapper.find('.agent-status-line__item--context');
        expect(contextItem.text()).toContain('25%');
        expect(contextItem.find('.agent-status-line__stale').text()).toBe('desatualizado');
        expect(contextItem.attributes('title')).toContain('Observado há 75s');

        const quota5hItem = wrapper.find('.agent-status-line__item--quota5h');
        expect(quota5hItem.text()).toContain('~42%');
        expect(quota5hItem.attributes('title')).toContain('Estimado');
    });
});
