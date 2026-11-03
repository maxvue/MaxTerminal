import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { useQuickCommandsStore } from '@/stores/quickCommands';
import * as ipc from '@/ipc/quickCommands';
import type { ArtisanCatalog, ArtisanCommand, CommandCreatedPayload, ShortcutList } from '@/types/bindings';

vi.mock('@/ipc/quickCommands', () => ({
    quickCommandsIpc: {
        resolveRoot: vi.fn(),
        catalogGet: vi.fn(),
        catalogRefresh: vi.fn(),
        shortcutList: vi.fn(),
        validateForm: vi.fn(),
        preview: vi.fn(async () => ({
            path: 'app/Console/Commands/Test.php',
            signature: 'app:test',
            php: '<?php',
            report: { valid: true, issues: [] },
            consolePatch: null,
            manualSteps: [],
            revision: 'rev-1'
        })),
        create: vi.fn(),
        maxcodePrompt: vi.fn(),
        formSchema: vi.fn()
    },
    onCatalogUpdated: vi.fn(async () => () => {}),
    onDiscoveryFailed: vi.fn(async () => () => {}),
    onCommandCreated: vi.fn(async () => () => {})
}));

function sampleCmd(name: string): ArtisanCommand {
    return {
        name,
        description: `Comando ${name}`,
        options: [],
        arguments: [],
        aliases: []
    };
}

describe('useQuickCommandsStore', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('deduplica chamadas concorrentes (single-flight) a ensureCatalog', async () => {
        const store = useQuickCommandsStore();
        let resolvePromise!: (v: ArtisanCatalog) => void;
        vi.mocked(ipc.quickCommandsIpc.catalogGet).mockImplementation(
            () =>
                new Promise<ArtisanCatalog>((resolve) => {
                    resolvePromise = resolve;
                })
        );

        const p1 = store.ensureCatalog('/tmp/laravel');
        const p2 = store.ensureCatalog('/tmp/laravel');

        expect(ipc.quickCommandsIpc.catalogGet).toHaveBeenCalledTimes(1);

        resolvePromise({
            root: '/tmp/laravel',
            source: 'fresh',
            generatedAtMs: 1_700_000_000_000,
            stale: false,
            commands: [sampleCmd('migrate')],
            lastError: null
        });

        const [r1, r2] = await Promise.all([p1, p2]);
        expect(r1.commands).toHaveLength(1);
        expect(r2.commands).toHaveLength(1);
        expect(store.catalogFor('/tmp/laravel')?.commands[0]?.name).toBe('migrate');
    });

    it('recarrega catalogo via evento artisan://command-created', async () => {
        let commandCreatedCb: ((p: CommandCreatedPayload) => void) | null = null;
        vi.mocked(ipc.onCommandCreated).mockImplementation(async (cb) => {
            commandCreatedCb = cb;
            return () => {};
        });
        vi.mocked(ipc.quickCommandsIpc.catalogGet).mockResolvedValue({
            root: '/tmp/laravel',
            source: 'cache',
            generatedAtMs: 1_700_000_000_000,
            stale: false,
            commands: [sampleCmd('billing:sync'), sampleCmd('migrate')],
            lastError: null
        });

        const store = useQuickCommandsStore();
        await store.init();

        expect(commandCreatedCb).not.toBeNull();
        commandCreatedCb!({
            root: '/tmp/laravel',
            projectId: null,
            path: 'app/Console/Commands/SyncBillingCommand.php',
            className: 'SyncBillingCommand',
            signatureName: 'billing:sync',
            revision: 2,
            requestId: 'req-1'
        });

        await Promise.resolve();
        await Promise.resolve();
        expect(ipc.quickCommandsIpc.catalogGet).toHaveBeenCalledWith('/tmp/laravel');
    });

    it('carrega catalogo de atalhos universais e abre/fecha modal Artisan', async () => {
        const shortcutsMock: ShortcutList = {
            root: '/tmp/proj',
            managers: [],
            warnings: [],
            items: [
                {
                    id: 'test',
                    label: '/test',
                    description: 'Roda os testes do projeto',
                    line: 'pnpm test',
                    parts: [],
                    disabledReason: null,
                    requiresConfirmation: false,
                    evidenceRevision: 'rev-1'
                }
            ]
        };
        vi.mocked(ipc.quickCommandsIpc.shortcutList).mockResolvedValue(shortcutsMock);

        const store = useQuickCommandsStore();
        const loaded = await store.loadShortcuts('/tmp/proj');
        expect(loaded.items).toHaveLength(1);

        store.openCreateModal('/tmp/proj', { signatureName: 'reports:daily' });
        expect(store.modal.open).toBe(true);
        expect(store.modal.root).toBe('/tmp/proj');
        expect(store.modal.form.signatureName).toBe('reports:daily');

        store.closeModal();
        expect(store.modal.open).toBe(false);
    });
});
