import axios from 'axios';
import { createApp, defineComponent } from 'vue';
import { flushPromises, mount } from '@vue/test-utils';
import { createMaxPinia } from '@maxvue/max-pinia';
import { createPinia, setActivePinia, type PiniaPlugin } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ThemeEditorDialog from '@/components/preferences/appearance/ThemeEditorDialog.vue';
import {
    clearLastThemeSchemeSaveState,
    installThemeMaxPiniaAdapter
} from '@/ipc/themeMaxPiniaAdapter';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import { useThemeEditorStore } from '@/stores/themeEditor';
import type { ResolvedTheme, ThemeDraftInput } from '@/types/theme';

const ipcMocks = vi.hoisted(() => ({
    themeList: vi.fn(async () => []),
    themeGet: vi.fn(async () => null),
    themeCreate: vi.fn<(input: ThemeDraftInput) => Promise<ResolvedTheme>>(),
    themeUpdate: vi.fn<(id: string, input: ThemeDraftInput) => Promise<ResolvedTheme>>()
}));

vi.mock('@/ipc/themes', () => ({
    themeList: ipcMocks.themeList,
    themeGet: ipcMocks.themeGet,
    themeCreate: ipcMocks.themeCreate,
    themeUpdate: ipcMocks.themeUpdate
}));

function makeFakeResolved(id: string, name: string): ResolvedTheme {
    const ansi = {
        black: '#0b1420',
        red: '#f43f5e',
        green: '#10b981',
        yellow: '#f59e0b',
        blue: '#3b82f6',
        magenta: '#a855f7',
        cyan: '#06b6d4',
        white: '#e2e8f0',
        brightBlack: '#475569',
        brightRed: '#fb7185',
        brightGreen: '#34d399',
        brightYellow: '#fbbf24',
        brightBlue: '#60a5fa',
        brightMagenta: '#c084fc',
        brightCyan: '#22d3ee',
        brightWhite: '#f8fafc'
    };
    return {
        scheme: {
            id,
            name,
            origin: 'custom',
            polarity: 'dark',
            foreground: '#e2e8f0',
            background: '#0b1420',
            cursor: '#14b8a6',
            cursorAccent: '#0b1420',
            cursorStyle: 'block',
            selectionBackground: '#14b8a652',
            selectionForeground: null,
            activeTabAccent: '#14b8a6',
            ansi,
            indexedColors: { 16: '#112233', 200: '#abcdef' },
            metadata: {
                author: 'Equipe Max',
                sourceFormat: null,
                sourceFilename: null,
                sha256: 'a'.repeat(64),
                createdAt: 100,
                updatedAt: 200
            }
        },
        xterm: {
            foreground: '#e2e8f0',
            background: '#0b1420',
            cursor: '#14b8a6',
            cursorAccent: '#0b1420',
            selectionBackground: '#14b8a652',
            selectionForeground: null,
            selectionInactiveBackground: '#14b8a629',
            ...ansi,
            extendedAnsi: null
        },
        uiTokens: FALLBACK_MAX_NAVY_TEAL_TOKENS
    };
}

type MaxPiniaAxios = NonNullable<NonNullable<Parameters<typeof createMaxPinia>[0]>['axios']>;

describe('ThemeEditorDialog.vue', () => {
    let pinia: ReturnType<typeof createPinia>;

    beforeEach(() => {
        vi.clearAllMocks();
        clearLastThemeSchemeSaveState();
        const client = axios.create();
        installThemeMaxPiniaAdapter(client);
        pinia = createPinia();
        pinia.use(
            createMaxPinia({
                axios: client as unknown as MaxPiniaAxios
            }) as unknown as PiniaPlugin
        );
        const app = createApp(defineComponent({ render: () => null }));
        app.use(pinia);
        setActivePinia(pinia);
    });

    it('abre em modo criação, edita cores e opacidade de seleção e salva via MaxPinia', async () => {
        const editorStore = useThemeEditorStore();
        editorStore.openCreate(null);

        const wrapper = mount(ThemeEditorDialog, {
            global: {
                plugins: [pinia]
            }
        });
        expect(wrapper.text()).toContain('Criar Novo Tema Customizado');
        expect(wrapper.text()).toContain('Paleta ANSI de 16 Cores');

        const nameInput = wrapper.find('#theme-editor-name input').exists()
            ? wrapper.find('#theme-editor-name input')
            : wrapper.find('#theme-editor-name');
        await nameInput.setValue('Tema Nebulosa');
        editorStore.data.draft.name = 'Tema Nebulosa';
        await wrapper.vm.$nextTick();

        const saved = makeFakeResolved('tema-nebulosa', 'Tema Nebulosa');
        ipcMocks.themeCreate.mockResolvedValueOnce(saved);

        const buttons = wrapper.findAll('button');
        const submitBtn = buttons.find((b) => b.text().includes('Criar tema'));
        expect(submitBtn?.exists()).toBe(true);
        await submitBtn!.trigger('click');
        await flushPromises();

        expect(ipcMocks.themeCreate).toHaveBeenCalledTimes(1);
        expect(ipcMocks.themeCreate).toHaveBeenCalledWith(
            expect.objectContaining({
                name: 'Tema Nebulosa'
            })
        );
        expect(wrapper.emitted('saved')?.[0]).toEqual([saved]);
    });

    it('exibe erro inline e mantém o modal aberto quando o backend rejeita o salvamento', async () => {
        const editorStore = useThemeEditorStore();
        const existing = makeFakeResolved('tema-nebulosa', 'Tema Nebulosa');
        editorStore.openEdit(existing);

        const wrapper = mount(ThemeEditorDialog, {
            global: {
                plugins: [pinia]
            }
        });
        expect(wrapper.text()).toContain('Editar Tema: Tema Nebulosa');
        expect(wrapper.text()).toContain('2 cores indexadas (16–255) preservadas');

        ipcMocks.themeUpdate.mockRejectedValueOnce(
            new Error('Contraste insuficiente entre texto e fundo (2.10:1)')
        );

        const buttons = wrapper.findAll('button');
        const submitBtn = buttons.find((b) => b.text().includes('Salvar alterações'));
        await submitBtn!.trigger('click');
        await flushPromises();

        expect(editorStore.isOpen).toBe(true);
        expect(wrapper.text()).toContain(
            'Contraste insuficiente entre texto e fundo (2.10:1)'
        );
    });
});
