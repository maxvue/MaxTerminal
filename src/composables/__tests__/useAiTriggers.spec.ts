import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { registerAiTriggers, useAiTriggers } from '@/composables/ai/useAiTriggers';
import { aiIpc } from '@/ipc/ai';
import { DEFAULT_AI_SETTINGS, useAiStore } from '@/stores/ai';
import { useTerminalStore } from '@/stores/terminal';
import { useWindowStore } from '@/stores/window';
import { createTriggerRegistry } from '@/terminal/triggers/triggerRegistry';
import type { TerminalContext, TriggerQuery } from '@/types/terminal';

function makeContext(paneId = 'pane-1'): TerminalContext {
    return {
        paneId,
        ptyId: 'pty-1',
        projectId: 'proj-1',
        cwd: '/home/user/project',
        extra: {}
    };
}

function makeQuery(
    prefix: string,
    query: string,
    stage = 0,
    stageState: unknown = undefined
): TriggerQuery {
    const input = `${prefix}${query}`;
    return {
        prefix,
        query,
        input,
        cursor: input.length,
        stage,
        stageState
    };
}

describe('useAiTriggers & providers `@` / `?` (Fase 12 — Unidades 03 e 04)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('registra de forma idempotente os gatilhos @ e ? no TriggerRegistry e respeita inputMode === rich', () => {
        const registry = createTriggerRegistry();
        const sub = useAiTriggers(registry);

        const atDef = registry.getById('ai-at');
        const qDef = registry.getById('ai-question');
        expect(atDef).toBeDefined();
        expect(qDef).toBeDefined();

        const terminalStore = useTerminalStore();
        terminalStore.register('pane-1', 'pty-1');
        expect(atDef?.isAvailable(makeContext('pane-1'))).toBe(true);

        terminalStore.patch('pane-1', {
            modes: {
                altScreen: true,
                mouseTracking: 'none',
                bracketedPaste: false,
                applicationCursorKeys: false
            }
        });
        expect(atDef?.isAvailable(makeContext('pane-1'))).toBe(false);

        sub.unregister();
        expect(registry.getById('ai-at')).toBeUndefined();
    });

    it('navega pelos 3 estágios em cascata do gatilho @ (Harness → Modelo → Raciocínio) e abre o prompt de IA', async () => {
        const registry = createTriggerRegistry();
        registerAiTriggers(registry);
        const atDef = registry.getById('ai-at')!;

        const aiStore = useAiStore();
        aiStore.settings = { ...DEFAULT_AI_SETTINGS, enabled: true };
        aiStore.catalog = {
            configRevision: 1,
            capabilities: {
                catalog: { status: 'validated' },
                generation: { status: 'validated' },
                reasoning: { status: 'validated' },
                jsonMode: { status: 'validated' },
                sse: { status: 'validated' },
                ragSearch: { status: 'validated' },
                ragList: { status: 'validated' },
                health: { status: 'validated' }
            },
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
                                { id: 'high', label: 'Alto', rank: 2 }
                            ]
                        },
                        {
                            id: 'claude-haiku',
                            label: 'Claude Haiku',
                            defaultReasoning: null,
                            contextWindow: 100000,
                            reasoningLevels: []
                        }
                    ]
                }
            ]
        };

        const ctx = makeContext('pane-1');

        // Estágio 0: Harnesses
        const stage0 = await atDef.provide(
            makeQuery('@', 'claude', 0),
            ctx,
            new AbortController().signal
        );
        expect(stage0.stageLabel).toContain('1/3');
        const harnessItem = stage0.items.find((i) => i.id === 'harness:claude-code')!;
        expect(harnessItem).toBeDefined();

        const out0 = await atDef.onSelect(harnessItem, makeQuery('@', '', 0), ctx);
        expect(out0.kind).toBe('push-stage');
        if (out0.kind !== 'push-stage') return;

        // Estágio 1: Modelos
        const stage1 = await atDef.provide(
            makeQuery('@', '', 1, out0.stageState),
            ctx,
            new AbortController().signal
        );
        expect(stage1.stageLabel).toContain('2/3');
        const sonnetItem = stage1.items.find((i) => i.id === 'model:claude-sonnet-4-5')!;
        expect(sonnetItem).toBeDefined();

        const out1 = await atDef.onSelect(
            sonnetItem,
            makeQuery('@', '', 1, out0.stageState),
            ctx
        );
        expect(out1.kind).toBe('push-stage');
        if (out1.kind !== 'push-stage') return;

        // Estágio 2: Níveis de Raciocínio
        const stage2 = await atDef.provide(
            makeQuery('@', '', 2, out1.stageState),
            ctx,
            new AbortController().signal
        );
        expect(stage2.stageLabel).toContain('3/3');
        const highItem = stage2.items.find((i) => i.id === 'reasoning:high')!;
        expect(highItem).toBeDefined();

        const out2 = await atDef.onSelect(
            highItem,
            makeQuery('@', '', 2, out1.stageState),
            ctx
        );
        expect(out2).toEqual({ kind: 'replace-input', text: '' });
        expect(aiStore.activeModeForPane('pane-1')?.selection).toEqual({
            harnessId: 'claude-code',
            modelId: 'claude-sonnet-4-5',
            reasoningId: 'high'
        });
    });

    it('trata o gatilho ? com e sem seleção padrão configurada e permite abrir consulta RAG', async () => {
        const registry = createTriggerRegistry();
        registerAiTriggers(registry);
        const qDef = registry.getById('ai-question')!;
        const aiStore = useAiStore();
        const windowStore = useWindowStore();
        const ctx = makeContext('pane-1');

        // Sem seleção padrão
        aiStore.settings = { ...DEFAULT_AI_SETTINGS, defaultSelection: null };
        const noDefault = await qDef.provide(
            makeQuery('?', 'migrations', 0),
            ctx,
            new AbortController().signal
        );
        expect(noDefault.items.some((i) => i.id === 'action:configure-default')).toBe(true);

        const cfgItem = noDefault.items.find((i) => i.id === 'action:configure-default')!;
        await qDef.onSelect(cfgItem, makeQuery('?', '', 0), ctx);
        expect(windowStore.preferencesOpen).toBe(true);
        expect(windowStore.preferencesSection).toBe('ai');

        // Com seleção padrão e consulta RAG
        aiStore.settings = {
            ...DEFAULT_AI_SETTINGS,
            defaultSelection: {
                harnessId: 'codex',
                modelId: 'gpt-5-codex',
                reasoningId: 'high'
            }
        };
        const withDefault = await qDef.provide(
            makeQuery('?', 'listar containers', 0),
            ctx,
            new AbortController().signal
        );
        const ragItem = withDefault.items.find((i) => i.id === 'action:rag')!;
        await qDef.onSelect(ragItem, makeQuery('?', 'eloquent', 0), ctx);
        expect(aiStore.activeModeForPane('pane-1')?.kind).toBe('rag');
        expect(aiStore.activeModeForPane('pane-1')?.initialPrompt).toBe('eloquent');

        // Disparo direto de sugestão via onSubmitWithoutSelection com streaming completo
        vi.spyOn(aiIpc, 'suggestCommand').mockImplementationOnce(async (req, onEvent) => {
            onEvent({
                type: 'started',
                requestId: req.requestId,
                selection: aiStore.defaultSelection!,
                context: {
                    shell: 'zsh',
                    cwd: '/tmp',
                    projectName: null,
                    projectStack: null,
                    os: { family: 'linux', distro: 'CachyOS', kernel: '6.12' },
                    recentCommands: [],
                    outputTail: null
                }
            });
            onEvent({ type: 'delta', requestId: req.requestId, text: 'df -h' });
            onEvent({
                type: 'completed',
                requestId: req.requestId,
                suggestion: {
                    requestId: req.requestId,
                    command: 'df -h',
                    explanation: 'Mostra espaço em disco',
                    risk: {
                        level: 'safe',
                        localLevel: 'safe',
                        modelHint: null,
                        reasons: [],
                        requiresConfirmation: false,
                        confirmToken: null
                    },
                    selection: aiStore.defaultSelection!,
                    contextSummary: {
                        shell: 'zsh',
                        cwd: '/tmp',
                        projectName: null,
                        projectStack: null,
                        os: { family: 'linux', distro: 'CachyOS', kernel: '6.12' },
                        recentCommands: [],
                        outputTail: null
                    },
                    latencyMs: 50
                }
            });
            onEvent({
                type: 'failed',
                requestId: req.requestId,
                error: { code: 'ai.gateway_offline', message: 'Offline', details: null }
            });
            onEvent({ type: 'cancelled', requestId: req.requestId });
            return 'req-q';
        });
        await qDef.onSubmitWithoutSelection!(makeQuery('?', 'ver uso de disco', 0), ctx);
        expect(aiStore.suggestionForPane('pane-1')?.status).toBe('cancelled');

        // Ações adicionais de ? e @ (switch-to-at, suggest vazio, haiku sem raciocínio, IA desativada)
        const switchItem = noDefault.items.find((i) => i.id === 'action:switch-to-at')!;
        const swOut = await qDef.onSelect(switchItem, makeQuery('?', '', 0), ctx);
        expect(swOut).toEqual({ kind: 'replace-input', text: '@', keepOpen: true });

        const suggestItem = withDefault.items.find((i) => i.id === 'action:suggest')!;
        await qDef.onSelect(suggestItem, makeQuery('?', '', 0), ctx);
        expect(aiStore.activeModeForPane('pane-1')?.kind).toBe('suggest');

        await qDef.onSelect(suggestItem, makeQuery('?', 'listar', 0), ctx);
        await qDef.onSubmitWithoutSelection!(makeQuery('?', '', 0), ctx);

        const atDef = registry.getById('ai-at')!;
        await atDef.onSelect(
            { id: 'action:open-settings', label: 'Cfg', kind: 'action' },
            makeQuery('@', '', 0),
            ctx
        );
        vi.spyOn(aiStore, 'loadCatalog').mockResolvedValueOnce(aiStore.catalog!);
        await atDef.onSelect(
            { id: 'action:refresh-catalog', label: 'Refresh', kind: 'action' },
            makeQuery('@', '', 0),
            ctx
        );

        // Modelo sem níveis de raciocínio no estágio 1 do @
        aiStore.catalog = {
            configRevision: 1,
            capabilities: {
                catalog: { status: 'validated' },
                generation: { status: 'validated' },
                reasoning: { status: 'validated' },
                jsonMode: { status: 'validated' },
                sse: { status: 'validated' },
                ragSearch: { status: 'validated' },
                ragList: { status: 'validated' },
                health: { status: 'validated' }
            },
            source: 'gateway',
            fetchedAt: '2026-11-01T12:00:00Z',
            harnesses: [
                {
                    id: 'claude-code',
                    label: 'Claude Code',
                    icon: 'simple-icons:anthropic',
                    models: [
                        {
                            id: 'claude-haiku',
                            label: 'Claude Haiku',
                            defaultReasoning: null,
                            contextWindow: 100000,
                            reasoningLevels: []
                        }
                    ]
                }
            ]
        };
        const haikuStage1 = await atDef.provide(
            makeQuery('@', '', 1, { harness: aiStore.catalog.harnesses[0] }),
            ctx,
            new AbortController().signal
        );
        await atDef.onSelect(
            haikuStage1.items[0]!,
            makeQuery('@', '', 1, { harness: aiStore.catalog.harnesses[0] }),
            ctx
        );
        expect(aiStore.activeModeForPane('pane-1')?.selection?.reasoningId).toBeNull();

        // Quando IA está desativada nas configurações
        aiStore.settings = { ...DEFAULT_AI_SETTINGS, enabled: false };
        const disabledAt = await atDef.provide(
            makeQuery('@', '', 0),
            ctx,
            new AbortController().signal
        );
        expect(disabledAt.stageLabel).toContain('Desativada');
        const disabledQ = await qDef.provide(
            makeQuery('?', '', 0),
            ctx,
            new AbortController().signal
        );
        expect(disabledQ.stageLabel).toContain('Desativada');
    });
});
