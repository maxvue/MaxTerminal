import { Channel } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { mockIPC } from '@tauri-apps/api/mocks';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
    createAiRequestId,
    executeCommandInPane,
    insertCommandIntoRichInput,
    registerRichInputInsertHandler,
    resolvePaneProjectInfo
} from '@/composables/ai/adapters';
import { aiIpc } from '@/ipc/ai';
import * as ptyIpc from '@/ipc/pty';
import {
    CANONICAL_HARNESSES_FALLBACK,
    DEFAULT_AI_SETTINGS,
    useAiStore
} from '@/stores/ai';
import { useProjectsStore } from '@/stores/projects';
import { useTabsStore } from '@/stores/tabs';
import { useTerminalStore } from '@/stores/terminal';
import {
    commands,
    type AiContext_Serialize,
    type AiHistoryPage_Serialize,
    type AiSettings_Serialize,
    type GatewayCapabilities,
    type GatewayHealth_Serialize,
    type HarnessCatalog_Serialize,
    type RagSlug_Serialize,
    type RiskAssessment_Serialize
} from '@/types/bindings';

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn()
}));

const eventListeners = new Map<string, (ev: { payload: unknown }) => void>();

function emitTauriEvent(name: string, payload: unknown): void {
    eventListeners.get(name)?.({ payload });
}

const sampleCapabilities: GatewayCapabilities = {
    catalog: { status: 'validated' },
    generation: { status: 'validated' },
    reasoning: { status: 'validated' },
    jsonMode: { status: 'validated' },
    sse: { status: 'validated' },
    ragSearch: { status: 'validated' },
    ragList: { status: 'validated' },
    health: { status: 'validated' }
};

function makeHealth(
    status: GatewayHealth_Serialize['status'] = 'online',
    configRevision = 3
): GatewayHealth_Serialize {
    return {
        status,
        version: '1.2.0',
        latencyMs: 12,
        baseUrl: 'http://127.0.0.1:8787',
        checkedAt: '2026-11-01T12:00:00Z',
        configRevision,
        capabilities: sampleCapabilities
    };
}

function makeCatalog(configRevision = 2): HarnessCatalog_Serialize {
    return {
        configRevision,
        capabilities: sampleCapabilities,
        source: 'gateway',
        fetchedAt: '2026-11-01T12:00:00Z',
        harnesses: [
            {
                id: 'claude-code',
                label: 'Claude Code',
                icon: 'simple-icons:anthropic',
                models: [
                    {
                        id: 'claude-sonnet-4-5',
                        label: 'Claude Sonnet 4.5',
                        defaultReasoning: 'medium',
                        contextWindow: 200000,
                        reasoningLevels: [
                            { id: 'low', label: 'Baixo', rank: 1 },
                            { id: 'medium', label: 'Médio', rank: 2 },
                            { id: 'high', label: 'Alto', rank: 3 }
                        ]
                    }
                ]
            }
        ]
    };
}

describe('useAiStore, aiIpc & adapters (Fase 12)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
        eventListeners.clear();

        vi.mocked(listen).mockImplementation(async (eventName, handler) => {
            eventListeners.set(
                String(eventName),
                handler as (ev: { payload: unknown }) => void
            );
            return () => {
                eventListeners.delete(String(eventName));
            };
        });
    });

    it('inicializa useAiStore, hidrata configurações/saúde/catálogo/slugs e processa eventos ai://* com guarda de revisão', async () => {
        const settings: AiSettings_Serialize = {
            ...DEFAULT_AI_SETTINGS,
            revision: 3,
            defaultSelection: {
                harnessId: 'claude-code',
                modelId: 'claude-sonnet-4-5',
                reasoningId: 'medium'
            }
        };
        const health = makeHealth('online', 3);
        const catalog = makeCatalog(4);
        const slugs: RagSlug_Serialize[] = [
            { slug: 'laravel', title: 'Laravel 12', documents: 120 }
        ];

        vi.spyOn(commands, 'aiSettingsGet').mockResolvedValueOnce({ status: 'ok', data: settings });
        vi.spyOn(commands, 'aiHealth').mockResolvedValueOnce({ status: 'ok', data: health });
        vi.spyOn(commands, 'aiCatalogGet').mockResolvedValueOnce({ status: 'ok', data: catalog });
        vi.spyOn(commands, 'aiRagListSlugs').mockResolvedValueOnce({ status: 'ok', data: slugs });

        const store = useAiStore();
        expect(store.harnesses).toHaveLength(CANONICAL_HARNESSES_FALLBACK.length);

        await store.init();
        await store.init(); // idempotente

        expect(store.initialized).toBe(true);
        expect(store.status).toBe('online');
        expect(store.isOnline).toBe(true);
        expect(store.isEnabled).toBe(true);
        expect(store.indicator).toEqual({ color: 'success', label: 'MaxAiManager Online' });
        expect(store.defaultSelection?.modelId).toBe('claude-sonnet-4-5');
        expect(store.harnesses).toHaveLength(1);

        // Evento de status
        emitTauriEvent('ai://status-changed', makeHealth('degraded', 4));
        expect(store.status).toBe('degraded');
        expect(store.indicator.color).toBe('warning');

        emitTauriEvent('ai://status-changed', makeHealth('offline', 5));
        expect(store.status).toBe('offline');
        expect(store.indicator.color).toBe('danger');

        // Evento de catálogo com configRevision menor deve ser ignorado
        emitTauriEvent('ai://catalog-updated', makeCatalog(2));
        expect(store.catalog?.configRevision).toBe(4);

        // Evento de catálogo com configRevision maior deve atualizar
        emitTauriEvent('ai://catalog-updated', makeCatalog(5));
        expect(store.catalog?.configRevision).toBe(5);

        // Evento de settings com revisão menor ignorado e revisão maior aplicado
        emitTauriEvent('ai://settings-changed', { ...settings, revision: 1, enabled: false });
        expect(store.isEnabled).toBe(true);

        emitTauriEvent('ai://settings-changed', { ...settings, revision: 6, enabled: false });
        expect(store.isEnabled).toBe(false);
        expect(store.status).toBe('disabled');
        expect(store.indicator.label).toBe('IA Desativada');

        emitTauriEvent('ai://settings-changed', {
            ...settings,
            revision: 7,
            enabled: true,
            baseUrl: ''
        });
        expect(store.status).toBe('unconfigured');
        expect(store.indicator.label).toBe('IA Não Configurada');

        // Evento pane://exited limpa o estado efêmero do painel
        store.openPanePrompt('pane-1', {
            kind: 'suggest',
            trigger: 'at',
            selection: settings.defaultSelection
        });
        store.setPaneSuggestion('pane-1', {
            requestId: 'req-1',
            trigger: 'at',
            prompt: 'listar arquivos',
            selection: settings.defaultSelection,
            status: 'completed',
            streamingText: 'ls -la',
            suggestion: null,
            editedCommand: 'ls -la',
            effectiveRisk: null,
            contextPreview: null,
            error: null
        });
        expect(store.suggestionForPane('pane-1')?.editedCommand).toBe('ls -la');
        expect(store.activeModeForPane('pane-1')?.kind).toBe('suggest');

        emitTauriEvent('pane://exited', {
            revision: 1,
            payload: { pane_id: 'pane-1' }
        });
        expect(store.suggestionForPane('pane-1')).toBeNull();
        expect(store.activeModeForPane('pane-1')).toBeNull();

        store.dispose();
        expect(store.initialized).toBe(false);
    });

    it('executa ações de histórico, cache, catálogo forçado e wrappers diretos de aiIpc', async () => {
        const store = useAiStore();

        const historyPage: AiHistoryPage_Serialize = {
            items: [
                {
                    id: 'req-10',
                    kind: 'suggest',
                    trigger: 'question',
                    projectId: 'proj-1',
                    tabId: 'tab-1',
                    paneId: 'pane-1',
                    sessionId: 'sess-1',
                    blockId: null,
                    harnessId: 'claude-code',
                    modelId: 'claude-sonnet-4-5',
                    reasoningId: 'medium',
                    ragSlug: null,
                    schemaId: null,
                    promptSanitized: 'listar portas',
                    responseSanitized: 'ss -tulpn',
                    riskLevel: 'safe',
                    status: 'completed',
                    errorCode: null,
                    latencyMs: 320,
                    createdAt: '2026-11-01T12:00:00Z'
                }
            ],
            nextBeforeId: null
        };

        vi.spyOn(commands, 'aiHistoryList').mockResolvedValueOnce({
            status: 'ok',
            data: historyPage
        });
        const items = await store.loadHistory({ limit: 20 });
        expect(items).toHaveLength(1);
        expect(store.history[0]?.responseSanitized).toBe('ss -tulpn');

        vi.spyOn(commands, 'aiHistoryClear').mockResolvedValueOnce({ status: 'ok', data: 1 });
        const removed = await store.clearHistory(null);
        expect(removed).toBe(1);
        expect(store.history).toHaveLength(0);

        vi.spyOn(commands, 'aiCacheClear').mockResolvedValueOnce({ status: 'ok', data: 4 });
        expect(await store.clearCache('all')).toBe(4);

        vi.spyOn(commands, 'aiCatalogRefresh').mockResolvedValueOnce({
            status: 'ok',
            data: makeCatalog(9)
        });
        await store.loadCatalog(true);
        expect(store.catalog?.configRevision).toBe(9);

        const updatedSettings = { ...DEFAULT_AI_SETTINGS, revision: 10, allowRemote: true };
        vi.spyOn(commands, 'aiSettingsUpdate').mockResolvedValueOnce({
            status: 'ok',
            data: updatedSettings
        });
        await store.saveSettings(updatedSettings);
        expect(store.settings?.allowRemote).toBe(true);

        // Wrappers diretos do aiIpc
        const fakeCtx: AiContext_Serialize = {
            shell: 'zsh',
            cwd: '/home/user/app',
            projectName: 'app',
            projectStack: 'laravel',
            os: { family: 'linux', distro: 'CachyOS', kernel: '6.12' },
            recentCommands: [],
            outputTail: null
        };
        vi.spyOn(commands, 'aiContextPreview').mockResolvedValueOnce({
            status: 'ok',
            data: fakeCtx
        });
        expect((await aiIpc.contextPreview('pane-1')).shell).toBe('zsh');

        const fakeRisk: RiskAssessment_Serialize = {
            level: 'safe',
            localLevel: 'safe',
            modelHint: null,
            reasons: [],
            requiresConfirmation: false,
            confirmToken: null
        };
        vi.spyOn(commands, 'aiClassifyRisk').mockResolvedValueOnce({
            status: 'ok',
            data: fakeRisk
        });
        expect((await aiIpc.classifyRisk('ls')).level).toBe('safe');

        vi.spyOn(commands, 'aiDiagnosisDismiss').mockResolvedValueOnce({
            status: 'ok',
            data: null
        });
        await aiIpc.diagnosisDismiss('block-1');

        vi.spyOn(commands, 'aiCancel').mockResolvedValueOnce({ status: 'ok', data: true });
        expect(await aiIpc.cancel('req-1')).toBe(true);

        vi.spyOn(commands, 'aiDiagnoseBlock').mockResolvedValueOnce({
            status: 'ok',
            data: { status: 'skipped', reason: 'disabled' }
        });
        expect((await aiIpc.diagnoseBlock({ requestId: 'r1', blockId: 'b1' })).status).toBe(
            'skipped'
        );

        vi.spyOn(commands, 'aiRagSearch').mockResolvedValueOnce({
            status: 'ok',
            data: { slug: 'laravel', query: 'q', results: [], latencyMs: 10 }
        });
        expect(
            (
                await aiIpc.ragSearch({
                    requestId: 'r2',
                    slug: 'laravel',
                    query: 'q',
                    topK: 5,
                    paneId: null
                })
            ).slug
        ).toBe('laravel');

        vi.spyOn(commands, 'aiGenerateStructured').mockResolvedValueOnce({
            status: 'ok',
            data: { requestId: 'r3', value: { ok: true }, repaired: false, latencyMs: 15 }
        });
        expect(
            (
                await aiIpc.generateStructured({
                    requestId: 'r3',
                    schemaId: 's1',
                    schema: {},
                    prompt: 'p',
                    system: null,
                    selection: null,
                    paneId: null,
                    projectId: null
                })
            ).requestId
        ).toBe('r3');

        vi.spyOn(commands, 'aiHealth').mockResolvedValueOnce({
            status: 'ok',
            data: makeHealth('online', 10)
        });
        await store.refreshHealth();

        vi.spyOn(commands, 'aiSettingsGet').mockResolvedValueOnce({
            status: 'ok',
            data: updatedSettings
        });
        await store.loadSettings();

        vi.spyOn(commands, 'aiRagListSlugs').mockResolvedValueOnce({
            status: 'ok',
            data: [{ slug: 'vue', title: 'Vue', documents: 10 }]
        });
        await store.loadRagSlugs();

        store.resetForTests();
        expect(store.settings).toBeNull();

        mockIPC(() => null);
        vi.spyOn(commands, 'aiSuggestCommand').mockImplementationOnce(async (_req, ch) => {
            expect(ch).toBeInstanceOf(Channel);
            ch.onmessage?.({
                type: 'cancelled',
                requestId: 'req-chan'
            });
            return { status: 'ok', data: 'req-chan' };
        });
        const received: string[] = [];
        const id = await aiIpc.suggestCommand(
            {
                requestId: 'req-chan',
                paneId: 'pane-1',
                trigger: 'question',
                selection: null,
                prompt: 'teste',
                regenerateFrom: null
            },
            (ev) => {
                received.push(ev.type);
            }
        );
        expect(id).toBe('req-chan');
        expect(received).toEqual(['cancelled']);
    });

    it('exercita adaptadores de inserção no RichInput, execução em PTY e resolução de projeto', async () => {
        const inserted: string[] = [];
        const unregister = registerRichInputInsertHandler('pane-a', (cmd) => {
            inserted.push(cmd);
        });

        expect(insertCommandIntoRichInput('pane-a', '  echo "oi"\n  ')).toBe(true);
        expect(inserted).toEqual(['echo "oi"']);
        expect(insertCommandIntoRichInput('pane-a', '   ')).toBe(false);

        unregister();
        expect(insertCommandIntoRichInput('pane-inexistente', 'ls')).toBe(false);

        const terminalStore = useTerminalStore();
        terminalStore.register('pane-a', 'pty-a');
        const ptyWriteSpy = vi.spyOn(ptyIpc, 'ptyWrite').mockResolvedValue(undefined);

        await executeCommandInPane('pane-a', 'git status');
        expect(ptyWriteSpy).toHaveBeenCalledWith('pty-a', {
            kind: 'text',
            text: 'git status\n'
        });

        const projectsStore = useProjectsStore();
        const tabsStore = useTabsStore();
        projectsStore.projects = [
            {
                id: 'proj-1',
                title: 'App Laravel',
                path: '/tmp/app',
                icon: 'logos:laravel',
                color: '#ff2d20',
                stack: 'laravel',
                rootId: null,
                identitySource: 'auto',
                sortOrder: 0,
                missing: false,
                createdAt: 0,
                updatedAt: 0
            }
        ];
        tabsStore.byId = {
            'tab-1': {
                id: 'tab-1',
                projectId: 'proj-1',
                templateId: null,
                title: 'Terminal',
                icon: 'lucide:terminal',
                position: 0,
                origin: 'manual',
                continuous: false,
                lifecycle: 'ready',
                revision: 1,
                layoutRevision: 1,
                activePaneId: 'pane-a',
                primaryPaneId: 'pane-a',
                initialDispatch: 'none',
                tabSessionId: 'sess-1',
                layout: { kind: 'leaf', paneId: 'pane-a' },
                panes: [
                    {
                        id: 'pane-a',
                        ptyId: 'pty-a',
                        generation: 1,
                        cwd: '/tmp/app',
                        state: 'live',
                        exit: null,
                        error: null
                    }
                ]
            }
        };

        expect(resolvePaneProjectInfo('pane-a')).toEqual({
            projectId: 'proj-1',
            stack: 'laravel'
        });
        expect(createAiRequestId('test').length).toBeGreaterThan(4);
    });
});
