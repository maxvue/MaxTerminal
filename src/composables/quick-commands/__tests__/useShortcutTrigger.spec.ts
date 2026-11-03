import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { shortcutTrigger } from '@/composables/quick-commands/useShortcutTrigger';
import { quickCommandsIpc } from '@/ipc/quickCommands';
import { useQuickCommandsStore } from '@/stores/quickCommands';
import type { ResolvedShortcut, ShortcutList } from '@/types/bindings';
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

const installShortcut: ResolvedShortcut = {
    id: 'install',
    label: '/install',
    description: 'Instalar dependências do projeto',
    line: 'composer install && npm install',
    parts: [],
    disabledReason: null,
    requiresConfirmation: false,
    evidenceRevision: 'rev-1'
};

const deployShortcut: ResolvedShortcut = {
    id: 'deploy',
    label: '/deploy',
    description: 'Executar script de publicação/deploy do projeto',
    line: 'npm run deploy',
    parts: [],
    disabledReason: null,
    requiresConfirmation: true,
    evidenceRevision: 'rev-1'
};

const sampleList: ShortcutList = {
    root: '/srv/app',
    managers: [],
    warnings: [],
    items: [installShortcut, deployShortcut]
};

const ctx: TerminalContext = {
    paneId: 'pane-1' as never,
    ptyId: 'pty-1' as never,
    tabId: 'tab-1',
    projectId: 'proj-1' as never,
    cwd: '/srv/app',
    shell: 'zsh',
    inputMode: 'rich',
    extra: {}
};

function makeQuery(query: string): TriggerQuery {
    const input = `/${query}`;
    return {
        prefix: '/',
        query,
        input,
        cursor: input.length,
        stage: 0,
        stageState: null
    };
}

describe('useShortcutTrigger (Fase 14 — CA-31, CA-32, CA-40)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        vi.mocked(quickCommandsIpc.shortcutList).mockResolvedValue(sampleList);
    });

    it('faz passthrough para caminhos Unix (/usr/bin/env) e termos não correspondentes (CA-32)', async () => {
        const store = useQuickCommandsStore();
        const trigger = shortcutTrigger(store);
        const signal = new AbortController().signal;

        const pathRes = await trigger.provide(makeQuery('usr/bin/env'), ctx, signal);
        expect(pathRes.passthrough).toBe(true);

        const unknownRes = await trigger.provide(makeQuery('foo'), ctx, signal);
        expect(unknownRes.passthrough).toBe(true);
    });

    it('insere /install diretamente no input sem exigir confirmação quando evidenceRevision coincide (CA-31)', async () => {
        vi.mocked(quickCommandsIpc.shortcutResolve).mockResolvedValue(installShortcut);
        const store = useQuickCommandsStore();
        const trigger = shortcutTrigger(store);
        const signal = new AbortController().signal;

        const res = await trigger.provide(makeQuery('inst'), ctx, signal);
        expect(res.items).toHaveLength(1);

        const outcome = await trigger.onSelect(res.items[0]!, makeQuery('inst'), ctx);
        expect(outcome).toEqual({
            kind: 'replace-input',
            text: 'composer install && npm install',
            cursor: 'composer install && npm install'.length
        });
    });

    it('solicita confirmação para /deploy e cancela se o usuário recusar (CA-31)', async () => {
        vi.mocked(quickCommandsIpc.shortcutResolve).mockResolvedValue(deployShortcut);
        const store = useQuickCommandsStore();
        const trigger = shortcutTrigger(store);
        const signal = new AbortController().signal;

        const res = await trigger.provide(makeQuery('dep'), ctx, signal);
        const deployItem = res.items[0]!;

        const selectPromise = trigger.onSelect(deployItem, makeQuery('dep'), ctx);
        await vi.waitFor(() => {
            expect(store.confirm?.open).toBe(true);
        });
        expect(store.confirm?.line).toBe('npm run deploy');

        store.resolveConfirm(false);
        const outcome = await selectPromise;
        expect(outcome).toEqual({ kind: 'close' });
    });

    it('bloqueia inserção e recarrega atalhos se evidenceRevision mudou entre listagem e seleção (CA-40)', async () => {
        vi.mocked(quickCommandsIpc.shortcutResolve).mockResolvedValue({
            ...installShortcut,
            evidenceRevision: 'rev-2-changed'
        });
        const store = useQuickCommandsStore();
        const trigger = shortcutTrigger(store);
        const signal = new AbortController().signal;

        const res = await trigger.provide(makeQuery('inst'), ctx, signal);
        const outcome = await trigger.onSelect(res.items[0]!, makeQuery('inst'), ctx);

        expect(outcome).toEqual({ kind: 'handled' });
        expect(quickCommandsIpc.shortcutList).toHaveBeenCalledTimes(2);
    });
});
