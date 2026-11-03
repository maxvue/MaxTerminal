/** Descritor de atalho de teclado do Motor de Terminal Híbrido. */
export interface KeybindingDescriptor {
    id:
        | 'terminal.copy'
        | 'terminal.paste'
        | 'terminal.search'
        | 'terminal.toggleInputMode'
        | 'richInput.interrupt'
        | 'search.global.open';
    key: string;
    ctrlKey: boolean;
    shiftKey: boolean;
    altKey: boolean;
}

/** Mapa imutável dos atalhos de teclado de escopo do terminal. */
export const TERMINAL_KEYBINDINGS: Readonly<Record<KeybindingDescriptor['id'], KeybindingDescriptor>> =
    Object.freeze({
        'terminal.copy': {
            id: 'terminal.copy',
            key: 'C',
            ctrlKey: true,
            shiftKey: true,
            altKey: false
        },
        'terminal.paste': {
            id: 'terminal.paste',
            key: 'V',
            ctrlKey: true,
            shiftKey: true,
            altKey: false
        },
        'terminal.search': {
            id: 'terminal.search',
            key: 'F',
            ctrlKey: true,
            shiftKey: true,
            altKey: false
        },
        'terminal.toggleInputMode': {
            id: 'terminal.toggleInputMode',
            key: 'I',
            ctrlKey: true,
            shiftKey: true,
            altKey: false
        },
        'richInput.interrupt': {
            id: 'richInput.interrupt',
            key: 'c',
            ctrlKey: true,
            shiftKey: false,
            altKey: false
        },
        'search.global.open': {
            id: 'search.global.open',
            key: 'k',
            ctrlKey: true,
            shiftKey: false,
            altKey: false
        }
    });

function matchesKey(event: KeyboardEvent, expectedUpper: string): boolean {
    return typeof event.key === 'string' && event.key.toUpperCase() === expectedUpper.toUpperCase();
}

/** Verifica se o evento corresponde ao atalho `Ctrl+Shift+C` (copiar seleção do terminal). */
export function isCopyShortcut(event: KeyboardEvent): boolean {
    return (
        event.ctrlKey &&
        event.shiftKey &&
        !event.altKey &&
        !event.metaKey &&
        matchesKey(event, 'C')
    );
}

/** Verifica se o evento corresponde ao atalho `Ctrl+Shift+V` (colar no terminal). */
export function isPasteShortcut(event: KeyboardEvent): boolean {
    return (
        event.ctrlKey &&
        event.shiftKey &&
        !event.altKey &&
        !event.metaKey &&
        matchesKey(event, 'V')
    );
}

/** Verifica se o evento corresponde ao atalho `Ctrl+Shift+F` (abrir barra de busca). */
export function isSearchShortcut(event: KeyboardEvent): boolean {
    return (
        event.ctrlKey &&
        event.shiftKey &&
        !event.altKey &&
        !event.metaKey &&
        matchesKey(event, 'F')
    );
}

/** Verifica se o evento corresponde ao atalho `Ctrl+Shift+I` (alternar modo de entrada). */
export function isToggleInputModeShortcut(event: KeyboardEvent): boolean {
    return (
        event.ctrlKey &&
        event.shiftKey &&
        !event.altKey &&
        !event.metaKey &&
        matchesKey(event, 'I')
    );
}

/** Verifica se o evento corresponde ao atalho `Ctrl+C` sem `Shift`/`Alt` (interrupção SIGINT). */
export function isInterruptShortcut(event: KeyboardEvent): boolean {
    return (
        event.ctrlKey &&
        !event.shiftKey &&
        !event.altKey &&
        !event.metaKey &&
        matchesKey(event, 'C')
    );
}

/** Verifica se o evento corresponde ao atalho `Ctrl+K` sem `Shift`/`Alt` (abrir busca global). */
export function isGlobalSearchShortcut(event: KeyboardEvent): boolean {
    return (
        event.ctrlKey &&
        !event.shiftKey &&
        !event.altKey &&
        !event.metaKey &&
        matchesKey(event, 'K')
    );
}
