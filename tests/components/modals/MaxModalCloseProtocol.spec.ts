import { createApp, defineComponent } from 'vue';
import { flushPromises, mount } from '@vue/test-utils';
import { createMaxPinia } from '@maxvue/max-pinia';
import axios from 'axios';
import { createPinia, setActivePinia, type PiniaPlugin } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import AppearancePreferences from '@/components/preferences/appearance/AppearancePreferences.vue';
import HorizonModal from '@/components/horizon/HorizonModal.vue';
import ThemeEditorDialog from '@/components/preferences/appearance/ThemeEditorDialog.vue';
import { clearLastThemeSchemeSaveState, installThemeMaxPiniaAdapter } from '@/ipc/themeMaxPiniaAdapter';
import { useHorizonStore } from '@/stores/horizon';
import { useProjectsStore } from '@/stores/projects';
import { useThemeStore } from '@/stores/theme';
import { useThemeEditorStore } from '@/stores/themeEditor';
import type { HorizonSnapshot_Serialize, Project_Serialize } from '@/types/bindings';
import type { ResolvedTheme, ThemeDraftInput, ThemeSummary } from '@/types/theme';

vi.mock('@/components/terminal/TerminalView.vue', () => ({
    default: {
        name: 'TerminalView',
        props: ['paneId', 'ptyId', 'visible'],
        template: '<div class="mock-terminal-view" :data-pty-id="ptyId" :data-pane-id="paneId" />'
    }
}));

const customSummary: ThemeSummary = {
    id: 'meu-tema',
    name: 'Meu Tema Customizado',
    origin: 'custom',
    polarity: 'dark',
    background: '#0b1420',
    foreground: '#e2e8f0',
    cursor: '#14b8a6',
    accentSwatches: [
        '#14b8a6',
        '#10b981',
        '#3b82f6',
        '#f59e0b',
        '#ef4444',
        '#8b5cf6',
        '#06b6d4',
        '#ec4899'
    ],
    author: 'Equipe Max'
};

const sampleAppearance = {
    activeThemeId: 'meu-tema',
    preferredDarkThemeId: 'meu-tema',
    preferredLightThemeId: 'max-light',
    fontFamily: 'JetBrains Mono',
    fontSize: 13,
    lineHeight: 1.2,
    fontLigatures: true,
    cursorBlink: true,
    cursorStyleOverride: null,
    minimumContrastRatio: 1,
    projectOverrides: {}
};

const ipcMocks = vi.hoisted(() => ({
    themeList: vi.fn(async () => [customSummary]),
    themeGet: vi.fn(async () => null),
    themeAppearanceGet: vi.fn(async () => ({ ...sampleAppearance })),
    themeAppearanceUpdate: vi.fn(async (p: Record<string, unknown>) => ({ ...sampleAppearance, ...p })),
    themeSetActive: vi.fn(async () => ({})),
    themeCreate: vi.fn<(input: ThemeDraftInput) => Promise<ResolvedTheme>>(),
    themeUpdate: vi.fn<(id: string, input: ThemeDraftInput) => Promise<ResolvedTheme>>(),
    themeDelete: vi.fn(async () => null),
    onThemeChanged: vi.fn(async () => () => {}),
    onThemeLibraryChanged: vi.fn(async () => () => {})
}));

vi.mock('@/ipc/themes', () => ({
    themeList: ipcMocks.themeList,
    themeGet: ipcMocks.themeGet,
    themeAppearanceGet: ipcMocks.themeAppearanceGet,
    themeAppearanceUpdate: ipcMocks.themeAppearanceUpdate,
    themeSetActive: ipcMocks.themeSetActive,
    themeCreate: ipcMocks.themeCreate,
    themeUpdate: ipcMocks.themeUpdate,
    themeDelete: ipcMocks.themeDelete,
    onThemeChanged: ipcMocks.onThemeChanged,
    onThemeLibraryChanged: ipcMocks.onThemeLibraryChanged
}));

describe('Protocolo Assíncrono de Fechamento do MaxModal (Etapa 2)', () => {
    let pinia: ReturnType<typeof createPinia>;

    const sampleProject: Project_Serialize = {
        id: 'p1',
        title: 'Laravel Horizon Project',
        icon: 'lucide:code',
        color: '#ff2d20',
        path: '/app',
        rootId: null,
        sortOrder: 0,
        createdAt: 1700000000000,
        updatedAt: 1700000000000,
        stack: 'laravel',
        identitySource: 'auto',
        missing: false
    };

    function createMockSnapshot(
        overrides: Partial<HorizonSnapshot_Serialize> = {}
    ): HorizonSnapshot_Serialize {
        return {
            context: {
                projectId: 'p1',
                root: '/app',
                contextRevision: 'r1',
                scopeId: 's1'
            },
            availability: 'available',
            horizonVersion: '5.48.3',
            environment: 'local',
            queues: [{ connection: 'redis', queue: 'default' }],
            status: 'running',
            pending: 10,
            running: 2,
            failed: 0,
            sampledAtMs: 1700000000000,
            revision: 1,
            diagnostic: null,
            canStart: false,
            canStop: true,
            canClearPending: true,
            session: {
                ptyId: 'pty-100',
                paneId: 'pane-200',
                generation: 1
            },
            ...overrides
        };
    }

    beforeEach(() => {
        document.body.innerHTML = '';
        vi.clearAllMocks();
        clearLastThemeSchemeSaveState();

        const client = axios.create();
        installThemeMaxPiniaAdapter(client);
        pinia = createPinia();
        pinia.use(
            createMaxPinia({
                axios: client as unknown as NonNullable<NonNullable<Parameters<typeof createMaxPinia>[0]>['axios']>
            }) as unknown as PiniaPlugin
        );

        const app = createApp(defineComponent({ render: () => null }));
        app.use(pinia);
        setActivePinia(pinia);

        const projectsStore = useProjectsStore();
        projectsStore.projects = [sampleProject];
        projectsStore.selectedId = 'p1';
    });

    afterEach(() => {
        document.body.innerHTML = '';
    });

    describe('HorizonModal — Protocolo de Fechamento Assíncrono', () => {
        it('permite fechamento assíncrono retornando true quando o modal não está ocupado', async () => {
            const store = useHorizonStore();
            store.data = createMockSnapshot();
            store.isConfirmedCurrentActivation = true;
            store.isModalOpen = true;

            const wrapper = mount(HorizonModal, {
                attachTo: document.body,
                props: { modelValue: true },
                global: { plugins: [pinia] }
            });
            await flushPromises();

            const modals = wrapper.findAllComponents({ name: 'MaxModal' });
            expect(modals.length).toBeGreaterThan(0);
            const mainModal = modals[0]!;

            const beforeCloseFn = mainModal.props('beforeClose') as ((ev: { preventDefault: () => void }) => Promise<boolean>) | undefined;
            expect(beforeCloseFn).toBeDefined();

            const mockEv = { preventDefault: vi.fn() };
            const canClose = await beforeCloseFn!(mockEv);
            expect(canClose).toBe(true);
            expect(mockEv.preventDefault).not.toHaveBeenCalled();
        });

        it('bloqueia o fechamento assíncrono com preventDefault() e retorna false durante operação de limpeza', async () => {
            const store = useHorizonStore();
            store.data = createMockSnapshot();
            store.isConfirmedCurrentActivation = true;
            store.isModalOpen = true;

            const wrapper = mount(HorizonModal, {
                attachTo: document.body,
                props: { modelValue: true },
                global: { plugins: [pinia] }
            });
            await flushPromises();

            const modals = wrapper.findAllComponents({ name: 'MaxModal' });
            const mainModal = modals[0]!;
            const beforeCloseMain = mainModal.props('beforeClose') as ((ev?: { preventDefault: () => void }) => Promise<boolean>);

            // Simula store ocupada (isClearing = true -> isBusy = true)
            store.isClearing = true;
            await wrapper.vm.$nextTick();

            const mockEvMain = { preventDefault: vi.fn() };
            const mainResult = await beforeCloseMain(mockEvMain);
            expect(mainResult).toBe(false);
            expect(mockEvMain.preventDefault).toHaveBeenCalledTimes(1);

            // Simula conclusão da operação
            store.isClearing = false;
            await wrapper.vm.$nextTick();

            const mockEvAfter = { preventDefault: vi.fn() };
            const afterResult = await beforeCloseMain(mockEvAfter);
            expect(afterResult).toBe(true);
            expect(mockEvAfter.preventDefault).not.toHaveBeenCalled();
        });
    });

    describe('ThemeEditorDialog — Protocolo de Fechamento Assíncrono', () => {
        it('retorna true quando não está salvando e false com preventDefault() quando saving = true', async () => {
            const editorStore = useThemeEditorStore();
            editorStore.openCreate(null);

            const wrapper = mount(ThemeEditorDialog, {
                global: { plugins: [pinia] }
            });
            await flushPromises();

            const modal = wrapper.findComponent({ name: 'MaxModal' });
            expect(modal.exists()).toBe(true);
            const beforeClose = modal.props('beforeClose') as ((ev: { preventDefault: () => void }) => Promise<boolean>);

            // Estado livre (saving = false)
            const evFree = { preventDefault: vi.fn() };
            const resultFree = await beforeClose(evFree);
            expect(resultFree).toBe(true);
            expect(evFree.preventDefault).not.toHaveBeenCalled();
            expect(modal.props('dismissable')).toBe(true);
            expect(modal.props('closeOnEscape')).toBe(true);

            // Estado ocupado (saving = true)
            editorStore.saving = true;
            await wrapper.vm.$nextTick();

            const evBusy = { preventDefault: vi.fn() };
            const resultBusy = await beforeClose(evBusy);
            expect(resultBusy).toBe(false);
            expect(evBusy.preventDefault).toHaveBeenCalledTimes(1);
            expect(modal.props('dismissable')).toBe(false);
            expect(modal.props('closeOnEscape')).toBe(false);
        });
    });

    describe('AppearancePreferences — Modal de Confirmação de Exclusão', () => {
        it('retorna true quando deletingScheme = false e false com preventDefault() quando deletingScheme = true', async () => {
            ipcMocks.themeList.mockResolvedValue([customSummary]);
            let resolveDelete: () => void;
            const deletePromise = new Promise<void>((resolve) => {
                resolveDelete = resolve;
            });
            ipcMocks.themeDelete.mockImplementation(() => deletePromise as unknown as Promise<null>);

            const wrapper = mount(AppearancePreferences, {
                global: { plugins: [pinia] }
            });
            const themeStore = useThemeStore();
            await themeStore.init();
            await flushPromises();

            const picker = wrapper.findComponent({ name: 'ThemePicker' });
            picker.vm.$emit('delete', 'meu-tema');
            await wrapper.vm.$nextTick();

            const deleteModal = wrapper
                .findAllComponents({ name: 'MaxModal' })
                .find((m) => m.props('title') === 'Confirmar Exclusão de Tema')!;
            expect(deleteModal?.exists()).toBe(true);

            const beforeCloseFn = deleteModal.props('beforeClose') as (event: { preventDefault: () => void }) => Promise<boolean>;
            expect(typeof beforeCloseFn).toBe('function');

            // Antes de excluir (livre)
            const mockEvFree = { preventDefault: vi.fn() };
            expect(await beforeCloseFn(mockEvFree)).toBe(true);
            expect(mockEvFree.preventDefault).not.toHaveBeenCalled();

            // Dispara a exclusão para manter isDeleting ativo
            const confirmBtn = wrapper
                .findAll('button')
                .find((b) => b.text().includes('Confirmar exclusão'));
            expect(confirmBtn).toBeDefined();
            await confirmBtn!.trigger('click');
            await wrapper.vm.$nextTick();

            // Durante a exclusão, dismissable e closeOnEscape passam para false
            expect(deleteModal.props('dismissable')).toBe(false);
            expect(deleteModal.props('closeOnEscape')).toBe(false);

            // O hook beforeClose previne o evento e retorna false
            const mockEvDuring = { preventDefault: vi.fn() };
            const resultDuring = await beforeCloseFn(mockEvDuring);
            expect(resultDuring).toBe(false);
            expect(mockEvDuring.preventDefault).toHaveBeenCalledTimes(1);

            resolveDelete!();
            await flushPromises();
        });
    });
});
