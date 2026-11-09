import { beforeEach, describe, expect, it, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import type { Terminal } from '@xterm/xterm';
import TerminalSearchBar from '@/components/terminal/TerminalSearchBar.vue';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { DEFAULT_FLOW_CONFIG, DEFAULT_TERMINAL_PREFERENCES } from '@/types/terminal';
import { createMaxNavyTealTheme } from '@/terminal/theme/defaultTheme';
import { MockXtermTerminal } from './mocks/xtermMock';

describe('TerminalSearchBar.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        terminalInstanceRegistry.disposeAll();
    });

    it('executa busca incremental, alterna toggles Aa/Palavra/.*, trata regex inválida e fecha com Escape', async () => {
        const store = useTerminalStore();
        store.register('pane-search', 'pty-search');
        store.openSearch('pane-search');

        const inst = terminalInstanceRegistry.getOrCreate({
            paneId: 'pane-search',
            ptyId: 'pty-search',
            prefs: DEFAULT_TERMINAL_PREFERENCES,
            theme: createMaxNavyTealTheme(),
            flow: DEFAULT_FLOW_CONFIG,
            createTerminal: () => new MockXtermTerminal() as unknown as Terminal,
            createWebglAddon: () => ({
                onContextLoss: () => ({ dispose: () => {} }),
                dispose: () => {}
            })
        });

        const findNextSpy = vi.spyOn(inst.searchAddon, 'findNext').mockReturnValue(true);
        const findPrevSpy = vi.spyOn(inst.searchAddon, 'findPrevious').mockReturnValue(true);

        const wrapper = mount(TerminalSearchBar, {
            props: { paneId: 'pane-search' }
        });

        const input = wrapper.find('.search-input input');
        await input.setValue('erro');
        expect(findNextSpy).toHaveBeenCalled();

        // Pressiona Enter (próximo) e Shift+Enter (anterior).
        await wrapper.trigger('keydown', { key: 'Enter' });
        expect(findNextSpy).toHaveBeenCalledTimes(2);

        await wrapper.trigger('keydown', { key: 'Enter', shiftKey: true });
        expect(findPrevSpy).toHaveBeenCalledTimes(1);

        // Ativa toggle de regex e digita regex inválida.
        const toggles = wrapper.findAll('.toggle-btn');
        await toggles[2]!.trigger('click'); // ".*"
        await input.setValue('[regex-invalida');
        expect(wrapper.find('.search-counter').text()).toBe('Expressão regular inválida');

        // Pressiona Escape -> fecha a barra de busca.
        await wrapper.trigger('keydown', { key: 'Escape' });
        expect(wrapper.emitted('close')).toHaveLength(1);
        expect(store.instance('pane-search')?.search.open).toBe(false);
    });
});
