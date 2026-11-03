import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import type {
    BridgeError,
    BridgeProjectSyncedEvent,
    BridgeSettings_Serialize,
    BridgeStatusSnapshot_Serialize,
    BridgeTerminalOpenedEvent,
    BugCycleProgressPayload_Serialize,
    InspectDispatchResult_Serialize
} from '@/types/bindings';

const mockStatus = vi.fn();
const mockGetSettings = vi.fn();
const mockUpdateSettings = vi.fn();
const mockReconnectNow = vi.fn();
const mockInspectCommandError = vi.fn();
const mockNotifyProjectSelected = vi.fn();
const mockOpenFileInMaxCode = vi.fn();
const mockIssueProtocolSupport = vi.fn();
const mockSendIssueCommand = vi.fn();
const mockBugsCyclePreview = vi.fn();
const mockBugsCycleStart = vi.fn();

let statusListener: ((s: BridgeStatusSnapshot_Serialize) => void) | null = null;
let projectSyncedListener: ((ev: BridgeProjectSyncedEvent) => void) | null = null;
let terminalOpenedListener: ((ev: BridgeTerminalOpenedEvent) => void) | null = null;
let bugCycleListener: ((ev: BugCycleProgressPayload_Serialize) => void) | null = null;
let errorListener: ((err: BridgeError) => void) | null = null;

vi.mock('@/ipc/bridge', () => ({
    bridgeIpc: {
        status: (...args: unknown[]) => mockStatus(...args),
        getSettings: (...args: unknown[]) => mockGetSettings(...args),
        updateSettings: (...args: unknown[]) => mockUpdateSettings(...args),
        reconnectNow: (...args: unknown[]) => mockReconnectNow(...args),
        inspectCommandError: (...args: unknown[]) => mockInspectCommandError(...args),
        inspectBlock: (...args: unknown[]) => mockInspectCommandError(...args),
        notifyProjectSelected: (...args: unknown[]) => mockNotifyProjectSelected(...args),
        openFileInMaxCode: (...args: unknown[]) => mockOpenFileInMaxCode(...args),
        issueProtocolSupport: (...args: unknown[]) => mockIssueProtocolSupport(...args),
        sendIssueCommand: (...args: unknown[]) => mockSendIssueCommand(...args),
        bugsCyclePreview: (...args: unknown[]) => mockBugsCyclePreview(...args),
        bugsCycleStart: (...args: unknown[]) => mockBugsCycleStart(...args)
    },
    onBridgeStatusChanged: vi.fn((cb: (s: BridgeStatusSnapshot_Serialize) => void) => {
        statusListener = cb;
        return Promise.resolve(() => {
            statusListener = null;
        });
    }),
    onBridgeProjectSynced: vi.fn((cb: (ev: BridgeProjectSyncedEvent) => void) => {
        projectSyncedListener = cb;
        return Promise.resolve(() => {
            projectSyncedListener = null;
        });
    }),
    onBridgeTerminalOpened: vi.fn((cb: (ev: BridgeTerminalOpenedEvent) => void) => {
        terminalOpenedListener = cb;
        return Promise.resolve(() => {
            terminalOpenedListener = null;
        });
    }),
    onBridgeBugCycleProgress: vi.fn((cb: (ev: BugCycleProgressPayload_Serialize) => void) => {
        bugCycleListener = cb;
        return Promise.resolve(() => {
            bugCycleListener = null;
        });
    }),
    onBridgeError: vi.fn((cb: (err: BridgeError) => void) => {
        errorListener = cb;
        return Promise.resolve(() => {
            errorListener = null;
        });
    })
}));

import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';

function makeStatus(
    overrides: Partial<BridgeStatusSnapshot_Serialize> = {}
): BridgeStatusSnapshot_Serialize {
    return {
        state: 'disconnected',
        serverPort: 45123,
        peerInstanceId: null,
        peerPid: null,
        peerVersion: null,
        peerCapabilities: [],
        lastRttMs: null,
        lastSeenMs: null,
        lastError: null,
        ...overrides
    };
}

function makeSettings(overrides: Partial<BridgeSettings_Serialize> = {}): BridgeSettings_Serialize {
    return {
        enabled: true,
        autoSyncActiveProject: true,
        autoRegisterUnknownProject: true,
        focusWindowOnIncomingCommand: true,
        inspectFallbackMode: 'uri_scheme',
        preferredPort: null,
        ...overrides
    };
}

describe('useMaxCodeBridgeStore', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        statusListener = null;
        projectSyncedListener = null;
        terminalOpenedListener = null;
        bugCycleListener = null;
        errorListener = null;

        mockStatus.mockResolvedValue(makeStatus());
        mockGetSettings.mockResolvedValue(makeSettings());
        mockIssueProtocolSupport.mockResolvedValue({
            issuesProtocolV1: false,
            bugsCycleV1: false
        });
    });

    it('inicializa em modo single-flight e atualiza indicator conforme o estado', async () => {
        const store = useMaxCodeBridgeStore();

        const [p1, p2] = [store.init(), store.init()];
        await Promise.all([p1, p2]);

        expect(mockStatus).toHaveBeenCalledTimes(1);
        expect(store.initialized).toBe(true);
        expect(store.indicator).toEqual({
            color: 'info',
            label: 'Aguardando MaxCode'
        });

        statusListener?.(
            makeStatus({
                state: 'connected',
                peerInstanceId: 'mc-1',
                peerVersion: '1.4.0',
                peerCapabilities: ['inspect_v1', 'issues_protocol_v1', 'bugs_cycle_v1']
            })
        );

        expect(store.isConnected).toBe(true);
        expect(store.indicator).toEqual({
            color: 'success',
            label: 'MaxCode conectado'
        });
        expect(store.issueSupport).toEqual({
            issuesProtocolV1: true,
            bugsCycleV1: true
        });

        statusListener?.(
            makeStatus({
                state: 'disconnected',
                lastError: 'Falha no handshake'
            })
        );
        expect(store.indicator).toEqual({
            color: 'danger',
            label: 'Ponte indisponível'
        });

        statusListener?.(makeStatus({ state: 'disabled' }));
        expect(store.indicator).toEqual({
            color: 'neutral',
            label: 'Ponte desativada'
        });
    });

    it('coalesce chamadas concorrentes de inspectCommandError para o mesmo bloco', async () => {
        const store = useMaxCodeBridgeStore();
        await store.init();

        let resolveInspect!: (val: InspectDispatchResult_Serialize) => void;
        mockInspectCommandError.mockReturnValue(
            new Promise<InspectDispatchResult_Serialize>((res) => {
                resolveInspect = res;
            })
        );

        const c1 = store.inspectCommandError('blk-1');
        const c2 = store.inspectCommandError('blk-1');

        expect(store.isInspecting('blk-1')).toBe(true);
        expect(mockInspectCommandError).toHaveBeenCalledTimes(1);

        resolveInspect({
            transport: 'loopback',
            requestId: 'req-1',
            conversationId: 'conv-99',
            formattedPrompt: 'Ao executar o comando "cargo test" tive o seguinte erro: "falha"'
        });

        const [r1, r2] = await Promise.all([c1, c2]);
        expect(r1.requestId).toBe('req-1');
        expect(r2.requestId).toBe('req-1');
        expect(store.isInspecting('blk-1')).toBe(false);
    });

    it('atualiza settings e faz rollback se o backend retornar erro', async () => {
        const store = useMaxCodeBridgeStore();
        await store.init();

        mockUpdateSettings.mockResolvedValueOnce(
            makeSettings({ autoSyncActiveProject: false })
        );
        await store.updateSettings({ autoSyncActiveProject: false });
        expect(store.settings.autoSyncActiveProject).toBe(false);
        expect(store.syncEnabled).toBe(false);

        mockUpdateSettings.mockRejectedValueOnce(new Error('Falha ao salvar'));
        await expect(store.updateSettings({ enabled: false })).rejects.toThrow('Falha ao salvar');
        expect(store.settings.enabled).toBe(true);
    });

    it('processa eventos de projeto sincronizado, terminal aberto, progresso de ciclo e erro', async () => {
        const store = useMaxCodeBridgeStore();
        await store.init();

        projectSyncedListener?.({
            projectId: 'proj-1',
            projectPath: '/home/user/app',
            created: false
        });
        expect(store.lastSyncedProject?.projectId).toBe('proj-1');

        terminalOpenedListener?.({
            projectId: 'proj-1',
            tabId: 'tab-1',
            issueNumber: 42
        });
        expect(store.lastOpenedTerminal?.issueNumber).toBe(42);

        bugCycleListener?.({
            cycleId: 'cyc-1',
            projectPath: '/home/user/app',
            overallStatus: 'running',
            currentIssueNumber: 42,
            stage: 'planning',
            stepStatus: 'in_progress',
            completedCount: 1,
            totalCount: 3,
            updatedLabel: 'Planned',
            message: 'Analisando issue #42'
        });
        expect(store.activeBugCycles['cyc-1']?.completedCount).toBe(1);

        errorListener?.({
            code: 'PEER_DISCONNECTED',
            message: 'Extensão desconectada'
        });
        expect(store.lastError).toContain('PEER_DISCONNECTED');

        store.dispose();
        expect(store.initialized).toBe(false);
    });

    it('executa reconnectNow, notifyProjectSelected, openFileInMaxCode e ações de issues', async () => {
        const store = useMaxCodeBridgeStore();
        await store.init();

        mockReconnectNow.mockResolvedValueOnce(makeStatus({ state: 'connected' }));
        const snap = await store.reconnectNow();
        expect(snap.state).toBe('connected');

        mockNotifyProjectSelected.mockResolvedValueOnce(true);
        const synced = await store.notifyProjectSelected('/home/user/app', 'App', 'main');
        expect(synced).toBe(true);

        mockOpenFileInMaxCode.mockResolvedValueOnce(true);
        const opened = await store.openFileInMaxCode({
            filePath: '/home/user/app/src/main.rs',
            line: 10,
            column: 2,
            projectPath: '/home/user/app',
            focusEditor: true
        });
        expect(opened).toBe(true);

        mockSendIssueCommand.mockResolvedValueOnce({
            transport: 'loopback',
            ack: {
                requestId: 'r-1',
                status: 'accepted',
                conversationId: 'c-1',
                worktreePath: null,
                error: null
            }
        });
        const issueRes = await store.sendIssueCommand({
            kind: 'plan',
            projectPath: '/home/user/app',
            repoFullName: 'org/app',
            issueNumber: 7,
            issueTitle: 'Bug crítico',
            issueUrl: null,
            labels: ['Bug'],
            stage: 'bug_plan',
            focusEditor: true
        });
        expect(issueRes.transport).toBe('loopback');

        mockBugsCyclePreview.mockResolvedValueOnce({
            requestId: 'r-2',
            cycleId: 'cyc-2',
            accepted: true,
            queuedIssues: [7],
            skippedIssues: [],
            error: null
        });
        const preview = await store.previewBugsCycle({
            cycleId: 'cyc-2',
            projectPath: '/home/user/app',
            repoFullName: 'org/app',
            issueNumbers: [7],
            filters: {
                includeLabels: ['Bug'],
                excludeLabels: [],
                explicitIssueNumbers: [7],
                maxIssues: null
            },
            dryRun: true
        });
        expect(preview.accepted).toBe(true);

        mockBugsCycleStart.mockResolvedValueOnce({
            requestId: 'r-3',
            cycleId: 'cyc-2',
            accepted: true,
            queuedIssues: [7],
            skippedIssues: [],
            error: null
        });
        const started = await store.startBugsCycle({
            cycleId: 'cyc-2',
            projectPath: '/home/user/app',
            repoFullName: 'org/app',
            issueNumbers: [7],
            filters: {
                includeLabels: ['Bug'],
                excludeLabels: [],
                explicitIssueNumbers: [7],
                maxIssues: null
            },
            dryRun: false
        });
        expect(started.accepted).toBe(true);
    });
});
