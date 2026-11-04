import axios, {
    type AxiosAdapter,
    type AxiosInstance,
    type AxiosResponse,
    type InternalAxiosRequestConfig
} from 'axios';
import { themeCreate, themeGet, themeList, themeUpdate } from '@/ipc/themes';
import type { ResolvedTheme, ThemeDraftInput, ThemeSummary } from '@/types/theme';

export const THEME_CATALOG_VIRTUAL_ROUTE = '/app/themes/catalog';
export const THEME_SCHEME_VIRTUAL_ROUTE = '/app/themes/scheme';

export interface ThemeSchemeSaveRequest {
    kind: 'create' | 'update';
    id?: string | null;
    input: ThemeDraftInput;
}

const installedInstances = new WeakSet<AxiosInstance>();
let lastSchemeSaveError: unknown = null;
let lastSchemeSaveResult: ResolvedTheme | null = null;

export function getLastThemeSchemeSaveError(): unknown {
    return lastSchemeSaveError;
}

export function getLastThemeSchemeSaveResult(): ResolvedTheme | null {
    return lastSchemeSaveResult;
}

export function clearLastThemeSchemeSaveState(): void {
    lastSchemeSaveError = null;
    lastSchemeSaveResult = null;
}

function parsePayload(raw: unknown): Record<string, unknown> {
    if (!raw) {
        return {};
    }
    if (typeof raw === 'string') {
        try {
            const parsed: unknown = JSON.parse(raw);
            return parsed && typeof parsed === 'object' ? (parsed as Record<string, unknown>) : {};
        } catch {
            return {};
        }
    }
    if (typeof raw === 'object') {
        return raw as Record<string, unknown>;
    }
    return {};
}

function extractRoutePathname(url?: unknown): string {
    if (!url || typeof url !== 'string') {
        return '';
    }
    try {
        return new URL(url, 'http://127.0.0.1').pathname;
    } catch {
        return url.split('?')[0] ?? '';
    }
}

function extractQueryParam(url: unknown, key: string): string | null {
    if (!url || typeof url !== 'string') {
        return null;
    }
    try {
        const parsed = new URL(url, 'http://127.0.0.1');
        const val = parsed.searchParams.get(key);
        return val && val.trim().length > 0 ? val.trim() : null;
    } catch {
        return null;
    }
}

function extractThemeIdFromConfig(config: InternalAxiosRequestConfig): string | null {
    const params = config.params as Record<string, unknown> | undefined;
    if (params && typeof params.id === 'string' && params.id.trim().length > 0) {
        return params.id.trim();
    }
    return extractQueryParam(config.url, 'id');
}

function resolveFallbackAdapter(current: AxiosInstance['defaults']['adapter']): AxiosAdapter {
    if (typeof current === 'function') {
        return current;
    }
    const resolved = axios.getAdapter(current ?? ['xhr', 'http', 'fetch']);
    return resolved;
}

function isThemeDraftInput(value: unknown): value is ThemeDraftInput {
    if (!value || typeof value !== 'object') {
        return false;
    }
    const candidate = value as Record<string, unknown>;
    return (
        typeof candidate.name === 'string' &&
        typeof candidate.foreground === 'string' &&
        typeof candidate.background === 'string' &&
        typeof candidate.cursor === 'string' &&
        typeof candidate.cursorAccent === 'string' &&
        typeof candidate.selectionBackground === 'string' &&
        typeof candidate.ansi === 'object' &&
        candidate.ansi !== null
    );
}

/**
 * Registra o adapter virtual do Axios para que `MaxPinia` e `useRefCachedApi`
 * consultem e persistam temas nativos via Tauri IPC sem abrir sockets HTTP locais.
 */
export function installThemeMaxPiniaAdapter(
    instance: AxiosInstance = axios
): void {
    if (installedInstances.has(instance)) {
        return;
    }

    const baseAdapter = resolveFallbackAdapter(instance.defaults.adapter);

    instance.defaults.adapter = async (
        config: InternalAxiosRequestConfig
    ): Promise<AxiosResponse> => {
        const pathname = extractRoutePathname(config.url);
        const method = (config.method ?? 'get').toLowerCase();

        if (pathname === THEME_CATALOG_VIRTUAL_ROUTE && method === 'get') {
            const list: ThemeSummary[] = await themeList();
            return {
                data: list,
                status: 200,
                statusText: 'OK',
                headers: {},
                config
            };
        }

        if (pathname === THEME_SCHEME_VIRTUAL_ROUTE && method === 'get') {
            const id = extractThemeIdFromConfig(config);
            if (!id) {
                return {
                    data: null,
                    status: 200,
                    statusText: 'OK',
                    headers: {},
                    config
                };
            }
            const resolved: ResolvedTheme = await themeGet(id);
            return {
                data: resolved,
                status: 200,
                statusText: 'OK',
                headers: {},
                config
            };
        }

        if (pathname === THEME_SCHEME_VIRTUAL_ROUTE && (method === 'post' || method === 'put')) {
            clearLastThemeSchemeSaveState();
            try {
                const payload = parsePayload(config.data);
                const rawInput = payload.input ?? payload.draft ?? payload;
                if (!isThemeDraftInput(rawInput)) {
                    throw new Error('Payload de tema inválido para persistência via MaxPinia.');
                }
                const rawKind = typeof payload.kind === 'string' ? payload.kind : null;
                const targetId =
                    typeof payload.id === 'string' && payload.id.trim().length > 0
                        ? payload.id.trim()
                        : extractThemeIdFromConfig(config);

                const saved: ResolvedTheme =
                    rawKind === 'update' || (rawKind !== 'create' && targetId)
                        ? await themeUpdate(targetId ?? '', rawInput)
                        : await themeCreate(rawInput);

                lastSchemeSaveResult = saved;
                return {
                    data: saved,
                    status: 200,
                    statusText: 'OK',
                    headers: {},
                    config
                };
            } catch (err) {
                lastSchemeSaveError = err;
                throw err;
            }
        }

        return baseAdapter(config);
    };

    installedInstances.add(instance);
}
