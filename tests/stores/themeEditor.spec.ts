import axios, { type AxiosInstance } from 'axios';
import { createApp, defineComponent } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia, type PiniaPlugin } from 'pinia';
import { createMaxPinia } from '@maxvue/max-pinia';
import {
    clearLastThemeSchemeSaveState,
    installThemeMaxPiniaAdapter
} from '@/ipc/themeMaxPiniaAdapter';
import { useThemeEditorStore } from '@/stores/themeEditor';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import type { ResolvedTheme, ThemeDraftInput } from '@/types/theme';

type MaxPiniaAxios = NonNullable<NonNullable<Parameters<typeof createMaxPinia>[0]>['axios']>;

function setupTestPinia(axiosInstance?: AxiosInstance) {
    const client = axiosInstance ?? axios.create();
    installThemeMaxPiniaAdapter(client);
    const pinia = createPinia();
    pinia.use(
        createMaxPinia({
            axios: client as unknown as MaxPiniaAxios
        }) as unknown as PiniaPlugin
    );
    const app = createApp(defineComponent({ render: () => null }));
    app.use(pinia);
    setActivePinia(pinia);
    return { pinia, app, client };
}

const ipcMocks = vi.hoisted(() => ({
    themeCreate: vi.fn<(input: ThemeDraftInput) => Promise<ResolvedTheme>>(),
    themeUpdate: vi.fn<(id: string, input: ThemeDraftInput) => Promise<ResolvedTheme>>(),
    themeGet: vi.fn<(id: string) => Promise<ResolvedTheme>>()
}));

vi.mock('@/ipc/themes', () => ({
    themeCreate: ipcMocks.themeCreate,
    themeUpdate: ipcMocks.themeUpdate,
    themeGet: ipcMocks.themeGet,
    themeList: vi.fn(async () => [])
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
            indexedColors: {},
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

describe('useThemeEditorStore - Validações de MaxPinia e Concorrência', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        clearLastThemeSchemeSaveState();
    });

    it('falha explicitamente ao submeter caso o plugin MaxPinia não esteja instalado', async () => {
        const pinia = createPinia();
        setActivePinia(pinia);

        const store = useThemeEditorStore();
        store.openCreate(null);
        store.data.draft.name = 'Tema Sem Plugin';

        await expect(store.submitDraft()).rejects.toThrow(
            'Plugin MaxPinia não está disponível para persistência do editor de temas.'
        );
        expect(store.errorMessage).toContain(
            'Plugin MaxPinia não está disponível para persistência do editor de temas.'
        );
    });

    it('bloqueia submissão concorrente se saving já estiver em andamento', async () => {
        setupTestPinia();

        const store = useThemeEditorStore();
        store.openCreate(null);
        store.data.draft.name = 'Tema Concorrente';

        // Simula salvamento em andamento
        store.saving = true;

        await expect(store.submitDraft()).rejects.toThrow(
            'Uma operação de salvamento de tema já está em andamento.'
        );
    });

    it('persiste via MaxPinia e limpa recibos anteriores, não aceitando recibos obsoletos', async () => {
        setupTestPinia();

        const store = useThemeEditorStore();
        store.openCreate(null);
        store.data.draft.name = 'Novo Tema';

        const fakeSaved = makeFakeResolved('novo-tema', 'Novo Tema');
        ipcMocks.themeCreate.mockResolvedValueOnce(fakeSaved);

        const saved = await store.submitDraft();
        expect(saved).toEqual(fakeSaved);
        expect(store.isOpen).toBe(false);
        expect(store.lastSavedResolved).toEqual(fakeSaved);
        expect(ipcMocks.themeCreate).toHaveBeenCalledTimes(1);
    });

    it('propaga erro lançado pelo IPC no adapter e mantém o rascunho intacto', async () => {
        setupTestPinia();

        const store = useThemeEditorStore();
        store.openCreate(null);
        store.data.draft.name = 'Tema Conflito';

        ipcMocks.themeCreate.mockRejectedValueOnce(
            new Error('Já existe um tema cadastrado com o nome informado.')
        );

        await expect(store.submitDraft()).rejects.toThrow(
            'Já existe um tema cadastrado com o nome informado.'
        );
        expect(store.errorMessage).toBe(
            'Já existe um tema cadastrado com o nome informado.'
        );
        expect(store.isOpen).toBe(true);
        expect(store.data.draft.name).toBe('Tema Conflito');
    });
});
