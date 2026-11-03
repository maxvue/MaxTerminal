import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { agentsIpc } from '@/ipc/agents';
import { useAgentsStore } from '@/stores/agents';
import type {
    AgentCloseInspection_Serialize,
    AgentTelemetry_Serialize,
    AttentionSnapshot_Serialize
} from '@/types/bindings';

const eventListeners: Record<string, Array<(payload: unknown) => void>> = {};

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn((eventName: string, cb: (ev: { payload: unknown }) => void) => {
        if (!eventListeners[eventName]) {
            eventListeners[eventName] = [];
        }
        eventListeners[eventName]!.push((payload) => cb({ payload }));
        return Promise.resolve(() => {
            eventListeners[eventName] = (eventListeners[eventName] ?? []).filter(
                (fn) => fn !== cb
            );
        });
    })
}));

function emitTauriEvent(name: string, payload: unknown): void {
    for (const cb of eventListeners[name] ?? []) {
        cb(payload);
    }
}

function makeTelemetry(
    tabId: string,
    runId: string,
    revision: number
): AgentTelemetry_Serialize {
    return {
        tabId,
        paneId: 'pane-1',
        sessionId: 'session-1',
        runId,
        revision,
        agent: 'claude-code',
        sessionRef: 'sess-ref-1',
        model: {
            value: { id: 'claude-opus-4-6', displayName: 'Opus 4.6' },
            source: 'claudeStatusLine',
            observedAtMs: 1_700_000_000_000,
            receivedAtMs: 1_700_000_000_000,
            quality: 'reported',
            expiresAtMs: null
        },
        context: null,
        quota5h: null,
        quotaWeekly: null,
        costUsd: null,
        availability: {},
        agentActive: true,
        updatedAtMs: 1_700_000_000_000
    };
}

describe('useAgentsStore (Fase 13)', () => {
    beforeEach(() => {
        for (const key of Object.keys(eventListeners)) {
            delete eventListeners[key];
        }
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('bufferiza eventos durante hidratação de init() e preserva maior revisão por runId', async () => {
        let resolveTelemetryList!: (items: AgentTelemetry_Serialize[]) => void;
        const telemetryListPromise = new Promise<AgentTelemetry_Serialize[]>((resolve) => {
            resolveTelemetryList = resolve;
        });

        vi.spyOn(agentsIpc, 'list').mockResolvedValue([]);
        vi.spyOn(agentsIpc, 'settingsGet').mockResolvedValue({
            shortcutsEnabled: true,
            shortcuts: {},
            isolation: {
                openIsolatedByDefault: false,
                defaultLocation: 'project',
                branchPrefix: 'agent'
            },
            attention: {
                enabled: true,
                onTurnComplete: 'ignore',
                silenceMs: 1800,
                notify: true
            },
            telemetry: {
                enabled: true,
                staleAfterS: 60,
                readSessionFiles: true
            },
            inject: {},
            enabled: {}
        });
        vi.spyOn(agentsIpc, 'telemetryList').mockReturnValue(telemetryListPromise);
        vi.spyOn(agentsIpc, 'worktreePendingList').mockResolvedValue([]);

        const store = useAgentsStore();
        const initTask = store.init();

        // Aguarda registro dos listeners antes da resolução do snapshot
        await Promise.resolve();
        await Promise.resolve();

        // Evento de revisão 3 chega enquanto telemetryList(revisão 2) ainda está em voo
        emitTauriEvent('agent://telemetry', makeTelemetry('tab-1', 'run-1', 3));

        resolveTelemetryList([makeTelemetry('tab-1', 'run-1', 2)]);
        await initTask;

        expect(store.telemetryForTab('tab-1')?.revision).toBe(3);

        // Evento atrasado de revisão inferior é ignorado
        await store.applyTelemetry(makeTelemetry('tab-1', 'run-1', 1));
        expect(store.telemetryForTab('tab-1')?.revision).toBe(3);
    });

    it('reconsulta telemetryGet quando recebe evento de runId diferente e respeita tombstone de tab://closed', async () => {
        vi.spyOn(agentsIpc, 'list').mockResolvedValue([]);
        vi.spyOn(agentsIpc, 'settingsGet').mockResolvedValue({
            shortcutsEnabled: true,
            shortcuts: {},
            isolation: {
                openIsolatedByDefault: false,
                defaultLocation: 'project',
                branchPrefix: 'agent'
            },
            attention: {
                enabled: true,
                onTurnComplete: 'ignore',
                silenceMs: 1800,
                notify: true
            },
            telemetry: {
                enabled: true,
                staleAfterS: 60,
                readSessionFiles: true
            },
            inject: {},
            enabled: {}
        });
        vi.spyOn(agentsIpc, 'telemetryList').mockResolvedValue([
            makeTelemetry('tab-1', 'run-current', 5)
        ]);
        vi.spyOn(agentsIpc, 'worktreePendingList').mockResolvedValue([]);

        const telemetryGetSpy = vi
            .spyOn(agentsIpc, 'telemetryGet')
            .mockResolvedValue(makeTelemetry('tab-1', 'run-current', 5));

        const store = useAgentsStore();
        await store.init();

        // Evento atrasado de run antigo dispara reconsulta e não sobrescreve run-current
        await store.applyTelemetry(makeTelemetry('tab-1', 'run-stale-old', 99));
        expect(telemetryGetSpy).toHaveBeenCalledWith('tab-1');
        expect(store.telemetryForTab('tab-1')?.runId).toBe('run-current');

        // Fechamento da aba instala tombstone e bloqueia eventos tardios
        emitTauriEvent('tab://closed', { payload: { tab_id: 'tab-1', project_id: 'p-1', revision: 1 } });
        expect(store.telemetryForTab('tab-1')).toBeNull();

        await store.applyTelemetry(makeTelemetry('tab-1', 'run-current', 6));
        expect(store.telemetryForTab('tab-1')).toBeNull();
    });

    it('gerencia atenção e pendências de worktree pós-fechamento', async () => {
        vi.spyOn(agentsIpc, 'list').mockResolvedValue([]);
        vi.spyOn(agentsIpc, 'settingsGet').mockResolvedValue({
            shortcutsEnabled: true,
            shortcuts: {},
            isolation: {
                openIsolatedByDefault: false,
                defaultLocation: 'project',
                branchPrefix: 'agent'
            },
            attention: {
                enabled: true,
                onTurnComplete: 'ignore',
                silenceMs: 1800,
                notify: true
            },
            telemetry: {
                enabled: true,
                staleAfterS: 60,
                readSessionFiles: true
            },
            inject: {},
            enabled: {}
        });
        vi.spyOn(agentsIpc, 'telemetryList').mockResolvedValue([]);
        vi.spyOn(agentsIpc, 'worktreePendingList').mockResolvedValue([]);

        const store = useAgentsStore();
        await store.init();

        const attentionSnap: AttentionSnapshot_Serialize = {
            tabId: 'tab-2',
            paneId: 'pane-2',
            projectId: 'proj-1',
            sessionId: 'sess-2',
            runId: 'run-2',
            agent: 'claude-code',
            revision: 1,
            acknowledged: false,
            state: {
                state: 'awaiting',
                kind: 'awaitingApproval',
                episode: 1,
                sinceMs: 1_700_000_000_000,
                source: 'nativeHook',
                message: 'Aguardando aprovação'
            }
        };

        await store.applyAttention(attentionSnap);
        expect(store.attentionForTab('tab-2')?.state.state).toBe('awaiting');

        const inspection: AgentCloseInspection_Serialize = {
            link: {
                id: 10,
                tabId: 'tab-2',
                projectId: 'proj-1',
                agent: 'claude-code',
                worktreePath: '/tmp/wt-1',
                branch: 'agent/claude-code/wt-1',
                baseRef: 'abc1234',
                createdByApp: true,
                openedAtMs: 1000,
                tabClosedAtMs: 2000,
                resolution: null,
                resolvedAtMs: null
            },
            exists: true,
            dirty: true,
            hasIgnoredFiles: false,
            inUse: false,
            aheadOfBase: 1,
            branchMerged: false,
            changedFiles: ['M src/main.ts'],
            changedFilesTruncated: false,
            inspectionComplete: true,
            requiresConfirmation: true,
            allowed: ['keep'],
            inspectionToken: 'tok-123'
        };

        emitTauriEvent('agent://worktree-close-pending', inspection);
        expect(store.pendingWorktrees).toHaveLength(1);
        expect(store.activeCloseDialogInspection?.link.id).toBe(10);
    });
});
