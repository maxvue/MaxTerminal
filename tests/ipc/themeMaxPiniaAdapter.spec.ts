import axios from 'axios';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ResolvedTheme, ThemeDraftInput, ThemeSummary } from '@/types/theme';

const ipcMocks = vi.hoisted(() => ({
    themeList: vi.fn<() => Promise<ThemeSummary[]>>(),
    themeGet: vi.fn<(id: string) => Promise<ResolvedTheme>>(),
    themeCreate: vi.fn<(input: ThemeDraftInput) => Promise<ResolvedTheme>>(),
    themeUpdate: vi.fn<(id: string, input: ThemeDraftInput) => Promise<ResolvedTheme>>()
}));

vi.mock('@/ipc/themes', () => ({
    themeList: ipcMocks.themeList,
    themeGet: ipcMocks.themeGet,
    themeCreate: ipcMocks.themeCreate,
    themeUpdate: ipcMocks.themeUpdate
}));

import {
    THEME_CATALOG_VIRTUAL_ROUTE,
    THEME_SCHEME_VIRTUAL_ROUTE,
    getLastThemeSchemeSaveError,
    getLastThemeSchemeSaveResult,
    installThemeMaxPiniaAdapter
} from '@/ipc/themeMaxPiniaAdapter';

function makeDraftInput(name = 'Meu Tema Custom'): ThemeDraftInput {
    return {
        name,
        author: 'Max',
        foreground: '#e2e8f0',
        background: '#0b1420',
        cursor: '#14b8a6',
        cursorAccent: '#0b1420',
        cursorStyle: 'block',
        selectionBackground: '#14b8a652',
        selectionForeground: null,
        activeTabAccent: '#14b8a6',
        ansi: {
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
        },
        indexedColors: { 16: '#112233' }
    };
}

describe('themeMaxPiniaAdapter', () => {
    beforeEach(() => {
        vi.clearAllMocks();
    });

    it('intercepta GET /app/themes/catalog e delega para themeList()', async () => {
        const client = axios.create();
        installThemeMaxPiniaAdapter(client);
        // Chamada dupla valida idempotência de registro na mesma instância
        installThemeMaxPiniaAdapter(client);

        const fakeList: ThemeSummary[] = [
            {
                id: 'max-navy-teal',
                name: 'Max Navy Teal',
                origin: 'builtin',
                polarity: 'dark',
                author: 'Max',
                background: '#0b1420',
                foreground: '#e2e8f0',
                cursor: '#14b8a6',
                accentSwatches: [
                    '#0b1420',
                    '#f43f5e',
                    '#10b981',
                    '#f59e0b',
                    '#3b82f6',
                    '#a855f7',
                    '#06b6d4',
                    '#e2e8f0'
                ]
            }
        ];
        ipcMocks.themeList.mockResolvedValueOnce(fakeList);

        const response = await client.get<ThemeSummary[]>(THEME_CATALOG_VIRTUAL_ROUTE);
        expect(response.status).toBe(200);
        expect(response.data).toEqual(fakeList);
        expect(ipcMocks.themeList).toHaveBeenCalledTimes(1);
    });

    it('intercepta GET /app/themes/scheme com params.id ou query ?id= e retorna null quando vazio', async () => {
        const client = axios.create();
        installThemeMaxPiniaAdapter(client);

        const emptyRes = await client.get(THEME_SCHEME_VIRTUAL_ROUTE);
        expect(emptyRes.data).toBeNull();
        expect(ipcMocks.themeGet).not.toHaveBeenCalled();

        const fakeResolved = {
            scheme: { id: 'dracula', name: 'Dracula' }
        } as unknown as ResolvedTheme;
        ipcMocks.themeGet.mockResolvedValueOnce(fakeResolved);

        const byParam = await client.get<ResolvedTheme>(THEME_SCHEME_VIRTUAL_ROUTE, {
            params: { id: 'dracula' }
        });
        expect(byParam.data).toEqual(fakeResolved);
        expect(ipcMocks.themeGet).toHaveBeenCalledWith('dracula');
    });

    it('intercepta POST /app/themes/scheme para create e update e propaga erros mantendo registro', async () => {
        const client = axios.create();
        installThemeMaxPiniaAdapter(client);

        const draft = makeDraftInput('Tema Criado');
        const createdResolved = {
            scheme: { id: 'tema-criado', name: 'Tema Criado' }
        } as unknown as ResolvedTheme;
        ipcMocks.themeCreate.mockResolvedValueOnce(createdResolved);

        const createRes = await client.post<ResolvedTheme>(THEME_SCHEME_VIRTUAL_ROUTE, {
            kind: 'create',
            input: draft
        });
        expect(createRes.data).toEqual(createdResolved);
        expect(ipcMocks.themeCreate).toHaveBeenCalledWith(draft);
        expect(getLastThemeSchemeSaveResult()).toEqual(createdResolved);
        expect(getLastThemeSchemeSaveError()).toBeNull();

        const updatedResolved = {
            scheme: { id: 'tema-criado', name: 'Tema Atualizado' }
        } as unknown as ResolvedTheme;
        ipcMocks.themeUpdate.mockResolvedValueOnce(updatedResolved);

        const updateRes = await client.post<ResolvedTheme>(THEME_SCHEME_VIRTUAL_ROUTE, {
            kind: 'update',
            id: 'tema-criado',
            input: { ...draft, name: 'Tema Atualizado' }
        });
        expect(updateRes.data).toEqual(updatedResolved);
        expect(ipcMocks.themeUpdate).toHaveBeenCalledWith('tema-criado', {
            ...draft,
            name: 'Tema Atualizado'
        });

        const backendErr = new Error('Conflito de nome de tema');
        ipcMocks.themeCreate.mockRejectedValueOnce(backendErr);
        await expect(
            client.post(THEME_SCHEME_VIRTUAL_ROUTE, {
                kind: 'create',
                input: draft
            })
        ).rejects.toThrow('Conflito de nome de tema');
        expect(getLastThemeSchemeSaveError()).toBe(backendErr);
    });

    it('delega rotas não correspondentes ao adapter base sem interceptar subcaminhos falsos', async () => {
        const baseCalls: string[] = [];
        const client = axios.create({
            adapter: async (config) => {
                baseCalls.push(config.url ?? '');
                return {
                    data: { passthrough: true },
                    status: 200,
                    statusText: 'OK',
                    headers: {},
                    config
                };
            }
        });
        installThemeMaxPiniaAdapter(client);

        const res = await client.get('/app/themes/catalog/extra');
        expect(res.data).toEqual({ passthrough: true });
        expect(baseCalls).toEqual(['/app/themes/catalog/extra']);
        expect(ipcMocks.themeList).not.toHaveBeenCalled();
    });
});
