import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useViewportTriggers } from '@/composables/terminal/useViewportTriggers';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import type { TerminalInstance } from '@/terminal/engine/terminalInstance';

describe('useViewportTriggers (R3 - Viewport Trigger Interception)', () => {
    let keyHandler: ((event: KeyboardEvent) => boolean) | null = null;
    let getSpy: ReturnType<typeof vi.spyOn> | null = null;

    beforeEach(() => {
        setActivePinia(createPinia());
        vi.useFakeTimers();

        keyHandler = null;
        const fakeInstance = {
            registerKeyHandler: vi.fn((handler: (event: KeyboardEvent) => boolean) => {
                keyHandler = handler;
                return {
                    dispose: () => {
                        keyHandler = null;
                    }
                };
            })
        } as unknown as TerminalInstance;

        getSpy = vi.spyOn(terminalInstanceRegistry, 'get').mockReturnValue(fakeInstance);
    });

    afterEach(() => {
        getSpy?.mockRestore();
        vi.useRealTimers();
    });

    function makeKeyEvent(key: string, modifiers: Partial<KeyboardEvent> = {}): KeyboardEvent {
        return {
            key,
            ctrlKey: false,
            altKey: false,
            metaKey: false,
            shiftKey: false,
            preventDefault: vi.fn(),
            stopPropagation: vi.fn(),
            ...modifiers
        } as unknown as KeyboardEvent;
    }

    it('registra o handler na instância do terminal ao dar attach', () => {
        const onTrigger = vi.fn();
        const triggers = useViewportTriggers({ paneId: 'pane-1', onTrigger });
        const sub = triggers.attach();

        expect(sub).not.toBeNull();
        expect(keyHandler).not.toBeNull();

        sub?.dispose();
    });

    it('intercepta @, ? e / quando o terminal estiver em modo rich e sem altScreen', () => {
        const terminalStore = useTerminalStore();
        terminalStore.register('pane-1', 'pty-1');

        const onTrigger = vi.fn();
        const triggers = useViewportTriggers({ paneId: 'pane-1', onTrigger });
        const sub = triggers.attach();

        const eventAt = makeKeyEvent('@');
        const handledAt = keyHandler!(eventAt);
        expect(handledAt).toBe(false);
        expect(eventAt.preventDefault).toHaveBeenCalled();
        expect(onTrigger).toHaveBeenCalledWith('@');

        const eventQuestion = makeKeyEvent('?');
        const handledQuestion = keyHandler!(eventQuestion);
        expect(handledQuestion).toBe(false);
        expect(eventQuestion.preventDefault).toHaveBeenCalled();
        expect(onTrigger).toHaveBeenCalledWith('?');

        const eventSlash = makeKeyEvent('/');
        const handledSlash = keyHandler!(eventSlash);
        expect(handledSlash).toBe(false);
        expect(eventSlash.preventDefault).toHaveBeenCalled();
        expect(onTrigger).toHaveBeenCalledWith('/');

        sub?.dispose();
    });

    it('não intercepta se altScreen estiver ativo', () => {
        const terminalStore = useTerminalStore();
        terminalStore.register('pane-1', 'pty-1');
        terminalStore.patch('pane-1', {
            modes: {
                altScreen: true,
                mouseTracking: 'none',
                bracketedPaste: false,
                applicationCursorKeys: false
            }
        });

        const onTrigger = vi.fn();
        const triggers = useViewportTriggers({ paneId: 'pane-1', onTrigger });
        const sub = triggers.attach();

        const eventSlash = makeKeyEvent('/');
        const handled = keyHandler!(eventSlash);
        expect(handled).toBe(true);
        expect(eventSlash.preventDefault).not.toHaveBeenCalled();
        expect(onTrigger).not.toHaveBeenCalled();

        sub?.dispose();
    });

    it('não intercepta quando Ctrl ou Alt estiverem pressionados', () => {
        const terminalStore = useTerminalStore();
        terminalStore.register('pane-1', 'pty-1');

        const onTrigger = vi.fn();
        const triggers = useViewportTriggers({ paneId: 'pane-1', onTrigger });
        const sub = triggers.attach();

        const eventCtrlSlash = makeKeyEvent('/', { ctrlKey: true });
        const handled = keyHandler!(eventCtrlSlash);
        expect(handled).toBe(true);
        expect(eventCtrlSlash.preventDefault).not.toHaveBeenCalled();
        expect(onTrigger).not.toHaveBeenCalled();

        sub?.dispose();
    });

    it('distingue > e >> através de sequência rápida', () => {
        const terminalStore = useTerminalStore();
        terminalStore.register('pane-1', 'pty-1');

        const onTrigger = vi.fn();
        const triggers = useViewportTriggers({ paneId: 'pane-1', onTrigger });
        const sub = triggers.attach();

        // Primeiro >
        const event1 = makeKeyEvent('>');
        keyHandler!(event1);
        expect(onTrigger).not.toHaveBeenCalled();

        // Segundo > antes do timeout
        const event2 = makeKeyEvent('>');
        keyHandler!(event2);
        expect(onTrigger).toHaveBeenCalledWith('>>');

        // Um único > após o timer expirar
        const eventSingle = makeKeyEvent('>');
        keyHandler!(eventSingle);
        vi.advanceTimersByTime(250);
        expect(onTrigger).toHaveBeenCalledWith('>');

        sub?.dispose();
    });
});
