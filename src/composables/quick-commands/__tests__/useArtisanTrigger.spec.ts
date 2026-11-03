import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import {
    artisanTrigger,
    createArtisanArgsTriggerForHead
} from '@/composables/quick-commands/useArtisanTrigger';
import { quickCommandsIpc } from '@/ipc/quickCommands';
import { useQuickCommandsStore } from '@/stores/quickCommands';
import type { ArtisanCatalog, ArtisanRoot } from '@/types/bindings';
import type { TerminalContext, TriggerQuery } from '@/types/terminal';

vi.mock('@/ipc/quickCommands', () => ({
    quickCommandsIpc: {
        resolveRoot: vi.fn(),
        catalogGet: vi.fn(),
        catalogRefresh: vi.fn(),
        formSchema: vi.fn(),
        validateForm: vi.fn(),
        preview: vi.fn(),
        create: vi.fn(),
        maxcodePrompt: vi.fn(),
        shortcutList: vi.fn(),
        shortcutResolve: vi.fn()
    },
    onCatalogUpdated: vi.fn(async () => () => {}),
    onDiscoveryFailed: vi.fn(async () => () => {}),
    onCommandCreated: vi.fn(async () => () => {})
}));

const sampleRoot: ArtisanRoot = {
    root: '/srv/laravel-app',
    projectId: 'proj-1' as never,
    isWorktree: false,
    invoker: 'php artisan',
    invokerTokens: ['php', 'artisan'],
    runner: 'local',
    layout: 'modern',
    laravelVersion: '11.31.0'
};

const sampleCatalog: ArtisanCatalog = {
    root: '/srv/laravel-app',
    source: 'fresh',
    stale: false,
    generatedAtMs: 1700000000000,
    lastError: null,
    commands: [
        {
            name: 'make:model',
            description: 'Create a new Eloquent model class',
            arguments: [
                {
                    name: 'name',
                    description: 'The name of the model',
                    required: true,
                    isArray: false
                }
            ],
            options: [
                {
                    name: '--migration',
                    shortcut: '-m',
                    description: 'Create a new migration file for the model',
                    acceptValue: false,
                    isGlobal: false
                }
            ]
        },
        {
            name: 'migrate',
            description: 'Run the database migrations',
            arguments: [],
            options: [
                {
                    name: '--database',
                    shortcut: '-d',
                    description: 'The database connection to use',
                    acceptValue: true,
                    isValueRequired: false,
                    isGlobal: false
                },
                {
                    name: '--force',
                    description: 'Force the operation to run when in production',
                    acceptValue: false,
                    isGlobal: false
                },
                {
                    name: '--env',
                    description: 'The environment the command should run under',
                    acceptValue: true,
                    isGlobal: true
                }
            ]
        }
    ]
};

const ctx: TerminalContext = {
    paneId: 'pane-1' as never,
    ptyId: 'pty-1' as never,
    tabId: 'tab-1',
    projectId: 'proj-1' as never,
    cwd: '/srv/laravel-app',
    shell: 'zsh',
    inputMode: 'rich',
    extra: {}
};

function makeQuery(prefix: string, query: string, input?: string): TriggerQuery {
    const full = input ?? `${prefix}${query}`;
    return {
        prefix,
        query,
        input: full,
        cursor: full.length,
        stage: 0,
        stageState: null
    };
}

describe('useArtisanTrigger (Fase 14 — CA-10, CA-13 a CA-17)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        vi.mocked(quickCommandsIpc.resolveRoot).mockResolvedValue(sampleRoot);
        vi.mocked(quickCommandsIpc.catalogGet).mockResolvedValue(sampleCatalog);
    });

    it('faz passthrough para ">>" (Fase 15) e "> arquivo.txt" (redirecionamento shell — CA-17)', async () => {
        const store = useQuickCommandsStore();
        const trigger = artisanTrigger(store);
        const signal = new AbortController().signal;

        const doubleGt = await trigger.provide(makeQuery('>', '>deploy'), ctx, signal);
        expect(doubleGt.passthrough).toBe(true);

        const redirect = await trigger.provide(makeQuery('>', ' out.txt'), ctx, signal);
        expect(redirect.passthrough).toBe(true);
    });

    it('mantém a ação "Criar comando" fixada no rodapé mesmo quando nenhum comando corresponde (CA-16)', async () => {
        const store = useQuickCommandsStore();
        await store.ensureCatalog(sampleRoot.root);
        const trigger = artisanTrigger(store);
        const signal = new AbortController().signal;

        const res = await trigger.provide(makeQuery('>', 'reports:send-weekly'), ctx, signal);
        expect(res.items).toHaveLength(1);
        expect(res.items[0]?.id).toBe('artisan:create');
        expect(res.items[0]?.pinned).toBe('bottom');

        const outcome = trigger.onSelect(
            res.items[0]!,
            makeQuery('>', 'reports:send-weekly'),
            ctx
        );
        expect(outcome).toEqual({ kind: 'handled' });
        expect(store.modal.open).toBe(true);
        expect(store.modal.form.signatureName).toBe('reports:send-weekly');
        expect(store.modal.form.className).toBe('ReportsSendWeekly');
    });

    it('selecionar um comando Artisan substitui o input por "<invoker> <name> " sem enviar ao PTY (CA-13)', async () => {
        const store = useQuickCommandsStore();
        await store.ensureCatalog(sampleRoot.root);
        const trigger = artisanTrigger(store);
        const signal = new AbortController().signal;

        const res = await trigger.provide(makeQuery('>', 'migrate'), ctx, signal);
        const migrateItem = res.items.find((i) => i.label === 'migrate')!;
        const outcome = trigger.onSelect(migrateItem, makeQuery('>', 'migrate'), ctx);

        expect(outcome).toEqual({
            kind: 'replace-input',
            text: 'php artisan migrate ',
            cursor: 'php artisan migrate '.length,
            keepOpen: true
        });
    });

    it('autocompleta opções (--database= vs --force ) e exibe hint de argumento posicional (CA-14, CA-15)', async () => {
        const store = useQuickCommandsStore();
        await store.ensureCatalog(sampleRoot.root);
        const argsTrigger = createArtisanArgsTriggerForHead('php', store);
        const signal = new AbortController().signal;

        // 1. Flag com valor (--database)
        const optQuery = makeQuery('php', ' artisan migrate --dat', 'php artisan migrate --dat');
        const optRes = await argsTrigger.provide(optQuery, ctx, signal);
        expect(optRes.passthrough).toBeUndefined();
        const dbOpt = optRes.items.find((i) => i.label === '--database')!;
        expect(dbOpt).toBeDefined();

        const selectDb = argsTrigger.onSelect(dbOpt, optQuery, ctx);
        expect(selectDb).toEqual({
            kind: 'replace-input',
            text: 'php artisan migrate --database=',
            cursor: 'php artisan migrate --database='.length,
            keepOpen: false
        });

        // 2. Hint de argumento posicional em make:model
        const hintQuery = makeQuery('php', ' artisan make:model ', 'php artisan make:model ');
        const hintRes = await argsTrigger.provide(hintQuery, ctx, signal);
        expect(hintRes.items).toHaveLength(1);
        expect(hintRes.items[0]?.disabled).toBe(true);
        expect(hintRes.items[0]?.label).toBe('<name>');
        expect(hintRes.items[0]?.description).toContain('Obrigatório');
    });
});
