import { describe, expect, it, vi } from 'vitest';
import { handleGlobalSearchKeydown } from '@/composables/search/useSearchShortcut';

function createKeyEvent(
    key: string,
    opts: { ctrlKey?: boolean; shiftKey?: boolean; altKey?: boolean; metaKey?: boolean } = {}
): KeyboardEvent {
    return new KeyboardEvent('keydown', {
        key,
        ctrlKey: opts.ctrlKey ?? false,
        shiftKey: opts.shiftKey ?? false,
        altKey: opts.altKey ?? false,
        metaKey: opts.metaKey ?? false,
        cancelable: true
    });
}

describe('useSearchShortcut / handleGlobalSearchKeydown (CA-11)', () => {
    it('abre a paleta com Ctrl+K quando fora de passthrough e com input rico vazio', () => {
        const onOpen = vi.fn();
        const event = createKeyEvent('k', { ctrlKey: true });

        const handled = handleGlobalSearchKeydown(
            event,
            {
                isTerminalPassthroughOrAltScreen: false,
                isRichInputNonEmptyFocused: false,
                isLocalSearchOpen: false,
                localSearchQuery: ''
            },
            onOpen
        );

        expect(handled).toBe(true);
        expect(event.defaultPrevented).toBe(true);
        expect(onOpen).toHaveBeenCalledTimes(1);
    });

    it('não intercepta Ctrl+K quando o input rico possui texto (preserva kill-line)', () => {
        const onOpen = vi.fn();
        const event = createKeyEvent('k', { ctrlKey: true });

        const handled = handleGlobalSearchKeydown(
            event,
            {
                isTerminalPassthroughOrAltScreen: false,
                isRichInputNonEmptyFocused: true,
                isLocalSearchOpen: false,
                localSearchQuery: ''
            },
            onOpen
        );

        expect(handled).toBe(false);
        expect(event.defaultPrevented).toBe(false);
        expect(onOpen).not.toHaveBeenCalled();
    });

    it('nunca intercepta Ctrl+K nem Ctrl+Shift+F quando o terminal está em passthrough / alt-screen (TUI ativa)', () => {
        const onOpen = vi.fn();
        const ctrlK = createKeyEvent('k', { ctrlKey: true });
        const ctrlShiftF = createKeyEvent('F', { ctrlKey: true, shiftKey: true });

        expect(
            handleGlobalSearchKeydown(
                ctrlK,
                {
                    isTerminalPassthroughOrAltScreen: true,
                    isRichInputNonEmptyFocused: false,
                    isLocalSearchOpen: false,
                    localSearchQuery: ''
                },
                onOpen
            )
        ).toBe(false);

        expect(
            handleGlobalSearchKeydown(
                ctrlShiftF,
                {
                    isTerminalPassthroughOrAltScreen: true,
                    isRichInputNonEmptyFocused: false,
                    isLocalSearchOpen: true,
                    localSearchQuery: 'erro'
                },
                onOpen
            )
        ).toBe(false);

        expect(onOpen).not.toHaveBeenCalled();
    });

    it('escalona Ctrl+Shift+F para a busca global levando o texto quando a barra de busca local já está aberta', () => {
        const onOpen = vi.fn();
        const closeLocal = vi.fn();
        const event = createKeyEvent('F', { ctrlKey: true, shiftKey: true });

        const handled = handleGlobalSearchKeydown(
            event,
            {
                isTerminalPassthroughOrAltScreen: false,
                isRichInputNonEmptyFocused: false,
                isLocalSearchOpen: true,
                localSearchQuery: 'SQLSTATE[HY000]',
                closeLocalSearch: closeLocal
            },
            onOpen
        );

        expect(handled).toBe(true);
        expect(closeLocal).toHaveBeenCalledTimes(1);
        expect(onOpen).toHaveBeenCalledWith('SQLSTATE[HY000]');
    });
});
