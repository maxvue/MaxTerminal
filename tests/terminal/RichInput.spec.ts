import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import type { Terminal } from '@xterm/xterm';
import RichInput from '@/components/terminal/RichInput.vue';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { ptyClient } from '@/terminal/stream/ptyClient';
import { createMaxNavyTealTheme } from '@/terminal/theme/defaultTheme';
import { triggerRegistry } from '@/terminal/triggers/triggerRegistry';
import { DEFAULT_FLOW_CONFIG, DEFAULT_TERMINAL_PREFERENCES } from '@/types/terminal';
import { MockXtermTerminal } from './mocks/xtermMock';

describe('RichInput.vue', () => {
    beforeEach(() => {
        vi.useFakeTimers();
        setActivePinia(createPinia());
        terminalInstanceRegistry.disposeAll();
        triggerRegistry.clearForTests();
        vi.restoreAllMocks();
        vi.spyOn(ptyClient, 'write').mockResolvedValue();
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('navega no popup com setas/Home/End, atualiza aria-activedescendant, envia linha + \\r ao PTY, envia \\x03 em Ctrl+C vazio e navega no histórico', async () => {
        const store = useTerminalStore();
        store.register('pane-rich', 'pty-rich');

        const inst = terminalInstanceRegistry.getOrCreate({
            paneId: 'pane-rich',
            ptyId: 'pty-rich',
            prefs: DEFAULT_TERMINAL_PREFERENCES,
            theme: createMaxNavyTealTheme(),
            flow: DEFAULT_FLOW_CONFIG,
            createTerminal: () => new MockXtermTerminal() as unknown as Terminal,
            createWebglAddon: () => ({
                onContextLoss: () => ({ dispose: () => {} }),
                dispose: () => {}
            })
        });

        const sendLineSpy = vi.spyOn(inst, 'sendLine').mockResolvedValue();
        const sendInputSpy = vi.spyOn(inst, 'sendInput').mockResolvedValue();

        triggerRegistry.register({
            id: 'artisan',
            prefix: '>',
            label: 'Comandos Artisan',
            debounceMs: 0,
            isAvailable: () => true,
            provide: () => ({
                items: [
                    { id: 'migrate', label: 'migrate', kind: 'value' },
                    { id: 'disabled-item', label: 'indisponível', kind: 'value', disabled: true },
                    { id: 'seed', label: 'db:seed', kind: 'value' },
                    { id: 'create', label: 'Criar comando', kind: 'action', pinned: 'bottom' }
                ]
            }),
            onSelect: (item) => ({
                kind: 'submit',
                text: `php artisan ${item.label}`
            })
        });

        const wrapper = mount(RichInput, {
            props: { paneId: 'pane-rich' }
        });

        const textarea = wrapper.find('textarea.rich-textarea');

        // 1. Ctrl+C com campo vazio envia \x03 ao PTY.
        await textarea.trigger('keydown', { key: 'c', ctrlKey: true });
        expect(sendInputSpy).toHaveBeenCalledWith('\x03');

        // 2. Digita '>' para abrir o popup.
        await textarea.setValue('>');
        await vi.advanceTimersByTimeAsync(1);

        expect(textarea.attributes('aria-expanded')).toBe('true');
        expect(textarea.attributes('aria-activedescendant')).toContain('-item-0');

        // ArrowDown pula o item disabled (índice 1) e vai direto para o índice 2 ('db:seed').
        await textarea.trigger('keydown', { key: 'ArrowDown' });
        expect(textarea.attributes('aria-activedescendant')).toContain('-item-2');

        // Pressiona Enter para confirmar o item selecionado ('db:seed').
        await textarea.trigger('keydown', { key: 'Enter' });
        await Promise.resolve();
        expect(sendLineSpy).toHaveBeenCalledWith('php artisan db:seed', { isComposing: false });

        // 3. Digita comando normal e pressiona Enter.
        await textarea.setValue('git status');
        await textarea.trigger('keydown', { key: 'Enter' });
        await Promise.resolve();
        expect(sendLineSpy).toHaveBeenCalledWith('git status', { isComposing: false });
        expect((textarea.element as HTMLTextAreaElement).value).toBe('');

        // 4. ArrowUp recupera o último comando do histórico local.
        await textarea.trigger('keydown', { key: 'ArrowUp' });
        expect((textarea.element as HTMLTextAreaElement).value).toBe('git status');
        await textarea.trigger('keydown', { key: 'ArrowUp' });
        expect((textarea.element as HTMLTextAreaElement).value).toBe('php artisan db:seed');
        await textarea.trigger('keydown', { key: 'ArrowDown' });
        expect((textarea.element as HTMLTextAreaElement).value).toBe('git status');
        await textarea.trigger('keydown', { key: 'ArrowDown' });
        expect((textarea.element as HTMLTextAreaElement).value).toBe('');

        // 5. Testa Home, End, PageDown, PageUp, Escape e Tab no popup.
        await textarea.setValue('>');
        await vi.advanceTimersByTimeAsync(1);
        await textarea.trigger('keydown', { key: 'End' });
        expect(textarea.attributes('aria-activedescendant')).toContain('-item-3');
        await textarea.trigger('keydown', { key: 'Home' });
        expect(textarea.attributes('aria-activedescendant')).toContain('-item-0');
        await textarea.trigger('keydown', { key: 'PageDown' });
        await textarea.trigger('keydown', { key: 'PageUp' });
        await textarea.trigger('keydown', { key: 'Escape' });
        expect(textarea.attributes('aria-expanded')).toBe('false');

        // 6. Testa colagem (paste), Enter e InputModeToggle.
        await textarea.setValue('echo ');
        (textarea.element as HTMLTextAreaElement).selectionStart = 5;
        await textarea.trigger('paste', {
            clipboardData: {
                getData: () => 'hello'
            }
        });
        expect((textarea.element as HTMLTextAreaElement).value).toBe('echo hello');
        await textarea.trigger('keydown', { key: 'Enter' });
        await Promise.resolve();
        expect(sendLineSpy).toHaveBeenCalledWith('echo hello', { isComposing: false });
        await wrapper.find('button.input-mode-toggle').trigger('click');
        expect(wrapper.emitted('mode-toggle')).toHaveLength(1);
    });
});
