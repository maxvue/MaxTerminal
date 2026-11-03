import { beforeEach, describe, expect, it, vi } from 'vitest';
import { defineComponent, h } from 'vue';
import { mount } from '@vue/test-utils';
import { usePageReloadShortcut } from '@/composables/usePageReloadShortcut';

describe('usePageReloadShortcut composable', () => {
    let reloadMock = vi.fn<() => void>();

    beforeEach(() => {
        reloadMock = vi.fn<() => void>();
        vi.clearAllMocks();
    });

    it('recarrega o documento chamando reloadFn uma única vez ao receber F5 simples', () => {
        const { dispose } = usePageReloadShortcut({ reloadFn: reloadMock });

        const event = new KeyboardEvent('keydown', {
            key: 'F5',
            code: 'F5',
            bubbles: true,
            cancelable: true
        });

        window.dispatchEvent(event);

        expect(event.defaultPrevented).toBe(true);
        expect(reloadMock).toHaveBeenCalledTimes(1);

        dispose();
    });

    it('ignora eventos de repeat contínuo da tecla F5', () => {
        const { dispose } = usePageReloadShortcut({ reloadFn: reloadMock });

        const repeatEvent = new KeyboardEvent('keydown', {
            key: 'F5',
            code: 'F5',
            repeat: true,
            bubbles: true,
            cancelable: true
        });

        window.dispatchEvent(repeatEvent);

        expect(repeatEvent.defaultPrevented).toBe(true);
        expect(reloadMock).not.toHaveBeenCalled();

        dispose();
    });

    it('impede múltiplos reloads subsequentes por meio da guarda reloadInitiated', () => {
        const { dispose } = usePageReloadShortcut({ reloadFn: reloadMock });

        const ev1 = new KeyboardEvent('keydown', {
            key: 'F5',
            code: 'F5',
            bubbles: true,
            cancelable: true
        });
        window.dispatchEvent(ev1);
        expect(reloadMock).toHaveBeenCalledTimes(1);

        const ev2 = new KeyboardEvent('keydown', {
            key: 'F5',
            code: 'F5',
            bubbles: true,
            cancelable: true
        });
        window.dispatchEvent(ev2);
        expect(reloadMock).toHaveBeenCalledTimes(1);

        dispose();
    });

    it('não consome o evento nem dispara reload quando F5 possui modificadores (Ctrl, Alt, Shift, Meta)', () => {
        const { dispose } = usePageReloadShortcut({ reloadFn: reloadMock });

        const modifiers = [
            { ctrlKey: true },
            { altKey: true },
            { shiftKey: true },
            { metaKey: true }
        ];

        for (const mod of modifiers) {
            const ev = new KeyboardEvent('keydown', {
                key: 'F5',
                code: 'F5',
                bubbles: true,
                cancelable: true,
                ...mod
            });
            window.dispatchEvent(ev);
            expect(ev.defaultPrevented).toBe(false);
        }

        expect(reloadMock).not.toHaveBeenCalled();
        dispose();
    });

    it('ignora quando isComposing === true (composição IME)', () => {
        const { dispose } = usePageReloadShortcut({ reloadFn: reloadMock });

        const ev = new KeyboardEvent('keydown', {
            key: 'F5',
            code: 'F5',
            bubbles: true,
            cancelable: true
        });
        Object.defineProperty(ev, 'isComposing', { value: true });

        window.dispatchEvent(ev);

        expect(ev.defaultPrevented).toBe(false);
        expect(reloadMock).not.toHaveBeenCalled();
        dispose();
    });

    it('ignora outras teclas sem interferir no comportamento padrão', () => {
        const { dispose } = usePageReloadShortcut({ reloadFn: reloadMock });

        const keys = ['F4', 'F6', 'Enter', 'KeyA', 'Tab', 'Escape'];
        for (const key of keys) {
            const ev = new KeyboardEvent('keydown', {
                key,
                code: key,
                bubbles: true,
                cancelable: true
            });
            window.dispatchEvent(ev);
            expect(ev.defaultPrevented).toBe(false);
        }

        expect(reloadMock).not.toHaveBeenCalled();
        dispose();
    });

    it('intercepta na fase de captura antes que o evento chegue a inputs, terminais ou modais', () => {
        const { dispose } = usePageReloadShortcut({ reloadFn: reloadMock });

        const container = document.createElement('div');
        container.setAttribute('role', 'dialog');
        const input = document.createElement('textarea');
        container.appendChild(input);
        document.body.appendChild(container);

        const childHandler = vi.fn();
        input.addEventListener('keydown', childHandler);

        const ev = new KeyboardEvent('keydown', {
            key: 'F5',
            code: 'F5',
            bubbles: true,
            cancelable: true
        });

        input.dispatchEvent(ev);

        expect(reloadMock).toHaveBeenCalledTimes(1);
        expect(ev.defaultPrevented).toBe(true);
        // Como o listener da raiz na captura chamou stopPropagation, o listener do filho não deve disparar
        expect(childHandler).not.toHaveBeenCalled();

        dispose();
        document.body.removeChild(container);
    });

    it('descarta automaticamente o listener quando o componente Vue é desmontado', () => {
        const HostComponent = defineComponent({
            setup() {
                usePageReloadShortcut({ reloadFn: reloadMock });
                return () => h('div', 'host');
            }
        });

        const wrapper = mount(HostComponent);
        wrapper.unmount();

        const ev = new KeyboardEvent('keydown', {
            key: 'F5',
            code: 'F5',
            bubbles: true,
            cancelable: true
        });
        window.dispatchEvent(ev);

        expect(reloadMock).not.toHaveBeenCalled();
    });
});
