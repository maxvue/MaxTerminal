import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { artisanTrigger } from '@/composables/quick-commands/useArtisanTrigger';
import { commandGroupsIpc } from '@/ipc/commandGroups';
import { quickCommandsIpc } from '@/ipc/quickCommands';
import { useCommandGroupsStore } from '@/stores/commandGroups';
import { useQuickCommandsStore } from '@/stores/quickCommands';
import { CompletionSession } from '@/terminal/triggers/completionSession';
import {
    commandGroupsTrigger,
    matchCommandGroupsTrigger
} from '@/terminal/triggers/providers/commandGroupsTrigger';
import { createTriggerRegistry } from '@/terminal/triggers/triggerRegistry';
import type { CommandGroup, GroupRun } from '@/types/commandGroups';
import type { TerminalContext, TriggerQuery } from '@/types/terminal';

vi.mock('@/ipc/commandGroups', () => ({
    commandGroupsIpc: {
        list: vi.fn(),
        get: vi.fn(),
        create: vi.fn(),
        update: vi.fn(),
        remove: vi.fn(),
        reorder: vi.fn(),
        run: vi.fn(),
        cancelRun: vi.fn(),
        listRuns: vi.fn(),
        getRun: vi.fn(),
        attachItemOutput: vi.fn(),
        detachItemOutput: vi.fn(),
        suggest: vi.fn(),
        recurringCommands: vi.fn(),
        onGroupChanged: vi.fn(async () => () => {}),
        onRunStarted: vi.fn(async () => () => {}),
        onRunItemChanged: vi.fn(async () => () => {}),
        onRunFinished: vi.fn(async () => () => {})
    }
}));

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

function makeGroup(overrides: Partial<CommandGroup>): CommandGroup {
    const scope = overrides.scope ?? 'local';
    return {
        id: overrides.id ?? 'grp-1',
        scope,
        projectId: scope === 'local' ? (overrides.projectId ?? 'proj-1') : null,
        name: overrides.name ?? 'Grupo Padrão',
        description: overrides.description ?? '',
        labels: overrides.labels ?? [],
        mode: overrides.mode ?? 'sequential',
        separator: overrides.separator ?? 'and',
        cwd: overrides.cwd ?? null,
        env: overrides.env ?? {},
        sortOrder: overrides.sortOrder ?? 0,
        revision: overrides.revision ?? 1,
        createdAt: 1700000000000,
        updatedAt: 1700000000000,
        items: overrides.items ?? [
            {
                id: 'item-1',
                groupId: overrides.id ?? 'grp-1',
                position: 0,
                title: 'Dev',
                command: 'npm run dev',
                cwd: null,
                env: {},
                continuous: false
            }
        ]
    };
}

function makeCtx(overrides: Partial<TerminalContext> = {}): TerminalContext {
    return {
        paneId: overrides.paneId ?? 'pane-1',
        ptyId: overrides.ptyId ?? 'pty-1',
        projectId: overrides.projectId !== undefined ? overrides.projectId : 'proj-1',
        cwd: overrides.cwd ?? '/srv/app',
        inputMode: overrides.inputMode ?? 'rich',
        tabId: overrides.tabId ?? 'tab-1',
        shell: overrides.shell ?? 'zsh',
        extra: overrides.extra ?? {}
    };
}

function makeQuery(input: string, stage = 0, stageState: unknown = undefined): TriggerQuery {
    const query = input.startsWith('>>') ? input.slice(2) : input;
    return {
        prefix: '>>',
        query,
        input,
        cursor: input.length,
        stage,
        stageState
    };
}

describe('commandGroupsTrigger (Fase 15 — CA-11, CA-12, CA-13, CA-15, CA-25)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        vi.useFakeTimers();
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('desambigua estritamente >> vs >, escapes \\>>, espaço inicial, >>>dev -> >dev e passthrough (CA-12)', async () => {
        expect(matchCommandGroupsTrigger('>>', 'rich')).toEqual({ prefix: '>>', query: '' });
        expect(matchCommandGroupsTrigger('>>>dev', 'rich')).toEqual({
            prefix: '>>',
            query: '>dev'
        });
        expect(matchCommandGroupsTrigger(' >>', 'rich')).toBeNull();
        expect(matchCommandGroupsTrigger('\\>>', 'rich')).toBeNull();
        expect(matchCommandGroupsTrigger('>migrate', 'rich')).toBeNull();
        expect(matchCommandGroupsTrigger('>>dev', 'passthrough')).toBeNull();

        // Transição >> <-> > no TriggerRegistry + CompletionSession abortando AbortSignal anterior
        const registry = createTriggerRegistry();
        const quickStore = useQuickCommandsStore();
        const groupsStore = useCommandGroupsStore();

        const captured: {
            artisan: AbortSignal | null;
            groups: AbortSignal | null;
        } = {
            artisan: null,
            groups: null
        };

        const baseArtisan = artisanTrigger(quickStore);
        registry.register({
            ...baseArtisan,
            debounceMs: 0,
            isAvailable: () => true,
            provide: async (q, c, signal) => {
                captured.artisan = signal;
                return baseArtisan.provide(q, c, signal);
            }
        });

        const baseGroups = commandGroupsTrigger(groupsStore);
        registry.register({
            ...baseGroups,
            debounceMs: 0,
            provide: async (q, c, signal) => {
                captured.groups = signal;
                return baseGroups.provide(q, c, signal);
            }
        });

        vi.mocked(quickCommandsIpc.resolveRoot).mockResolvedValue({
            root: '/srv/app',
            projectId: 'proj-1',
            isWorktree: false,
            invoker: 'php artisan',
            invokerTokens: ['php', 'artisan'],
            runner: 'local',
            layout: 'modern',
            laravelVersion: '11.0.0'
        });
        vi.mocked(quickCommandsIpc.catalogGet).mockResolvedValue({
            root: '/srv/app',
            source: 'fresh',
            stale: false,
            generatedAtMs: 1700000000000,
            lastError: null,
            commands: [{ name: 'migrate', description: 'Run migrations', arguments: [], options: [] }]
        });
        vi.mocked(commandGroupsIpc.list).mockResolvedValue({
            projectId: 'proj-1',
            localGroups: [makeGroup({ id: 'g-1', name: 'Deploy Local' })],
            globalGroups: []
        });

        const session = new CompletionSession({
            paneId: 'pane-1',
            registry,
            getContext: () => makeCtx()
        });

        // 1. Digita '>' -> ativa Artisan
        session.updateInput('>', 1);
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().trigger?.id).toBe('artisan');
        expect(captured.artisan?.aborted).toBe(false);

        // 2. Digita segundo '>' formando '>>' -> aborta Artisan e ativa Grupos de Comandos
        session.updateInput('>>', 2);
        expect(captured.artisan?.aborted).toBe(true);
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().trigger?.id).toBe('command-groups');
        expect(captured.groups?.aborted).toBe(false);

        // 3. Apaga segundo '>' com Backspace voltando para '>' -> aborta Grupos e reativa Artisan
        session.updateInput('>', 1);
        expect(captured.groups?.aborted).toBe(true);
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().trigger?.id).toBe('artisan');

        session.dispose();
    });

    it('preserva estritamente a ordem "Deste projeto" -> "Globais" sem misturar escopos por score e suporta busca fuzzy por nome, descrição e labels (CA-13, CA-25)', async () => {
        const store = useCommandGroupsStore();
        const trigger = commandGroupsTrigger(store);

        // Grupo local tem match na descrição/label para "deploy", enquanto grupo global tem match exato de prefixo no nome "deploy" (score maior)
        const localWeakMatch = makeGroup({
            id: 'local-1',
            scope: 'local',
            projectId: 'proj-1',
            name: 'Publicar Aplicação',
            description: 'Rotina de deploy em homologação',
            labels: ['k8s', 'produção'],
            sortOrder: 0
        });
        const globalStrongMatch = makeGroup({
            id: 'global-1',
            scope: 'global',
            projectId: null,
            name: 'deploy',
            description: 'Deploy global padrão',
            labels: ['ci'],
            sortOrder: 0
        });

        vi.mocked(commandGroupsIpc.list).mockResolvedValue({
            projectId: 'proj-1',
            localGroups: [localWeakMatch],
            globalGroups: [globalStrongMatch]
        });

        const signal = new AbortController().signal;
        const result = await trigger.provide(makeQuery('>>deploy'), makeCtx(), signal);

        // Seções devem ser Deste projeto (order 0) -> Globais (order 1)
        expect(result.sections).toEqual([
            { id: 'local', label: 'Deste projeto', order: 0 },
            { id: 'global', label: 'Globais', order: 1 }
        ]);

        // Mesmo com score maior no global, o item local vem obrigatoriamente antes do global, e as 2 ações fixas por último
        expect(result.items.map((i) => i.id)).toEqual([
            'group:local:local-1',
            'group:global:global-1',
            'action:create-local',
            'action:create-global'
        ]);

        // Busca fuzzy por label Unicode NFC ("produção" escrita decomposta NFD na query)
        const nfdQuery = '>>produc\u0327a\u0303o';
        const labelResult = await trigger.provide(makeQuery(nfdQuery), makeCtx(), signal);
        expect(labelResult.items.map((i) => i.id)).toEqual([
            'group:local:local-1',
            'action:create-local',
            'action:create-global'
        ]);
    });

    it('mantém as duas opções finais de criação visíveis sempre, desabilitando a local quando não há projeto ativo e usando a query apenas como nome sugerido (CA-13)', async () => {
        const store = useCommandGroupsStore();
        const trigger = commandGroupsTrigger(store);

        vi.mocked(commandGroupsIpc.list).mockResolvedValue({
            projectId: null,
            localGroups: [],
            globalGroups: []
        });

        const signal = new AbortController().signal;
        const ctxNoProject = makeCtx({ projectId: null });
        const result = await trigger.provide(makeQuery('>>Meu Novo Grupo'), ctxNoProject, signal);

        expect(result.items).toHaveLength(2);
        const [createLocal, createGlobal] = result.items;
        expect(createLocal?.id).toBe('action:create-local');
        expect(createLocal?.pinned).toBe('bottom');
        expect(createLocal?.disabled).toBe(true);
        expect(createLocal?.disabledReason).toBe('Selecione um projeto para criar um grupo local');

        expect(createGlobal?.id).toBe('action:create-global');
        expect(createGlobal?.pinned).toBe('bottom');
        expect(createGlobal?.disabled).toBe(false);

        // Selecionar "Criar grupo de comandos global" abre modal preenchendo initialName (e nunca comando shell)
        const outcome = await trigger.onSelect(
            createGlobal!,
            makeQuery('>>Meu Novo Grupo'),
            ctxNoProject
        );
        expect(outcome).toEqual({ kind: 'handled' });
        expect(store.editorModalState).toEqual({
            open: true,
            group: null,
            scope: 'global',
            projectId: null,
            initialName: 'Meu Novo Grupo'
        });
    });

    it('abre Etapa 2 com Tab/-> listando os 3 modos de execução, Editar e Excluir, e evita lançamentos duplicados no Enter segurado (CA-15)', async () => {
        const store = useCommandGroupsStore();
        const trigger = commandGroupsTrigger(store);
        const grp = makeGroup({ id: 'grp-stage', name: 'Fullstack Dev', revision: 4 });

        vi.mocked(commandGroupsIpc.list).mockResolvedValue({
            projectId: 'proj-1',
            localGroups: [grp],
            globalGroups: []
        });

        const signal = new AbortController().signal;
        const stage0 = await trigger.provide(makeQuery('>>'), makeCtx(), signal);
        const groupItem = stage0.items[0]!;

        // Acionar ação secundária (Tab / ->) empilha Etapa 2
        const pushOutcome = await trigger.onSecondarySelect!(
            groupItem,
            makeQuery('>>'),
            makeCtx()
        );
        expect(pushOutcome).toEqual({
            kind: 'push-stage',
            stageState: { group: grp },
            initialQuery: ''
        });

        // Etapa 2 lista exatamente as 5 ações canônicas
        const stage1 = await trigger.provide(
            makeQuery('>>', 1, { group: grp }),
            makeCtx(),
            signal
        );
        expect(stage1.items.map((i) => i.label)).toEqual([
            'Executar em modo Sequencial',
            'Executar em modo Paralelo',
            'Executar em modo Multi-Abas',
            'Editar grupo',
            'Excluir grupo'
        ]);

        // Enter segurado em grupo na Etapa 1 envia apenas 1 chamada IPC enquanto pendente
        let resolveRun!: (r: GroupRun) => void;
        vi.mocked(commandGroupsIpc.run).mockImplementation(
            () =>
                new Promise<GroupRun>((res) => {
                    resolveRun = res;
                })
        );

        const sub1 = trigger.onSelect(groupItem, makeQuery('>>'), makeCtx());
        const sub2 = trigger.onSelect(groupItem, makeQuery('>>'), makeCtx());
        expect(commandGroupsIpc.run).toHaveBeenCalledTimes(1);

        resolveRun({
            id: 'run-ok',
            requestId: 'req-ok',
            groupId: grp.id,
            projectId: 'proj-1',
            originSessionId: 'sess-1',
            snapshot: {
                groupId: grp.id,
                groupRevision: grp.revision,
                scope: 'local',
                name: grp.name,
                description: '',
                labels: [],
                definedMode: 'sequential',
                effectiveMode: 'sequential',
                separator: 'and',
                cwd: null,
                env: {},
                projectRoot: '/srv/app',
                shellPath: '/bin/zsh',
                shellDialect: 'zsh',
                originTabId: 'tab-1',
                originPaneId: 'pane-1',
                originPtyId: 'pty-1',
                originSessionId: 'sess-1',
                originGeneration: 1
            },
            mode: 'sequential',
            status: 'running',
            counts: {
                total: 1,
                pending: 0,
                starting: 0,
                running: 1,
                cancelling: 0,
                succeeded: 0,
                failed: 0,
                skipped: 0,
                cancelled: 0,
                interrupted: 0
            },
            version: 1,
            lastExitCode: null,
            createdAt: 1700000000000,
            startedAt: 1700000000100,
            cancelRequestedAt: null,
            finishedAt: null,
            items: []
        });

        await Promise.all([sub1, sub2]);
        expect(commandGroupsIpc.run).toHaveBeenCalledTimes(1);
    });
});
