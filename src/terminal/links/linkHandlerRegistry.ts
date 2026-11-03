import { invoke } from '@tauri-apps/api/core';
import type { ILinkHandler } from '@xterm/xterm';
import type { Disposable, TerminalContext, TerminalLinkHandler } from '@/types/terminal';

const customHandlers: TerminalLinkHandler[] = [];

/**
 * Valida se uma URI é estritamente `http:` ou `https:` via `new URL()`.
 * Rejeita `javascript:`, `file:`, `data:`, `vbscript:`, etc.
 */
export function isSafeHttpUrl(rawUri: string): boolean {
    if (typeof rawUri !== 'string' || rawUri.trim().length === 0) {
        return false;
    }
    try {
        const parsed = new URL(rawUri);
        return parsed.protocol === 'http:' || parsed.protocol === 'https:';
    } catch {
        return false;
    }
}

/**
 * Verifica se o clique de ativação possui o modificador exigido (`Ctrl+Clique` ou `Meta+Clique`).
 */
export function hasActivationModifier(event: MouseEvent): boolean {
    return Boolean(event.ctrlKey || event.metaKey);
}

async function openExternalHttpUrl(uri: string): Promise<void> {
    const parsed = new URL(uri);
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
        return;
    }
    const normalized = parsed.toString();
    try {
        await invoke('plugin:opener|open_url', { url: normalized });
    } catch {
        if (typeof window !== 'undefined' && typeof window.open === 'function') {
            window.open(normalized, '_blank', 'noopener,noreferrer');
        }
    }
}

const builtinHttpHandler: TerminalLinkHandler = {
    id: 'builtin.http',
    priority: 0,
    match: /https?:\/\/[^\s"'<>]+/i,
    accepts(uri: string): boolean {
        return isSafeHttpUrl(uri);
    },
    async activate(_event: MouseEvent, uri: string): Promise<void> {
        if (!isSafeHttpUrl(uri)) {
            return;
        }
        await openExternalHttpUrl(uri);
    }
};

function getSortedHandlers(): TerminalLinkHandler[] {
    const all = [...customHandlers, builtinHttpHandler];
    return all.sort((a, b) => b.priority - a.priority);
}

/**
 * Registra um manipulador customizado de links do terminal (ex.: Fase 09 `arquivo:linha`).
 */
export function registerLinkHandler(handler: TerminalLinkHandler): Disposable {
    const existingIdx = customHandlers.findIndex((h) => h.id === handler.id);
    if (existingIdx !== -1) {
        customHandlers.splice(existingIdx, 1);
    }
    customHandlers.push(handler);

    return {
        dispose(): void {
            const idx = customHandlers.indexOf(handler);
            if (idx !== -1) {
                customHandlers.splice(idx, 1);
            }
        }
    };
}

/** Lista todos os handlers registrados ordenados por `priority` decrescente. */
export function getRegisteredLinkHandlers(): readonly TerminalLinkHandler[] {
    return getSortedHandlers();
}

/**
 * Despacha a ativação de um link clicado no terminal se o modificador `Ctrl`/`Meta` estiver ativo.
 */
export async function dispatchTerminalLink(
    event: MouseEvent,
    uri: string,
    ctx: TerminalContext
): Promise<boolean> {
    if (!hasActivationModifier(event)) {
        return false;
    }

    const handlers = getSortedHandlers();
    for (const handler of handlers) {
        if (handler.accepts(uri)) {
            await handler.activate(event, uri, ctx);
            return true;
        }
    }
    return false;
}

/**
 * Cria o `ILinkHandler` seguro para escapes OSC 8 (`Terminal.options.linkHandler`).
 */
export function createOsc8LinkHandler(
    getContext: () => TerminalContext,
    onHoverChange?: (uri: string | null) => void
): ILinkHandler {
    return {
        activate(event: MouseEvent, text: string): void {
            void dispatchTerminalLink(event, text, getContext());
        },
        hover(_event: MouseEvent, text: string): void {
            if (isSafeHttpUrl(text) || customHandlers.some((h) => h.accepts(text))) {
                onHoverChange?.(text);
            }
        },
        leave(): void {
            onHoverChange?.(null);
        },
        allowNonHttpProtocols: false
    };
}

/** Remove handlers customizados registrados (exclusivo para testes). */
export function clearLinkHandlersForTests(): void {
    customHandlers.length = 0;
}
