import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';

describe('useTerminalStore', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        terminalInstanceRegistry.disposeAll();
    });

    it('registra instâncias, recalcula inputMode ao mudar modes/override, limpa override em altScreen e contabiliza webglCount', () => {
        const store = useTerminalStore();

        const s1 = store.register('pane-1', 'pty-1');
        const s2 = store.register('pane-2', 'pty-2');
        expect(s1.inputMode).toBe('rich');
        expect(s2.inputMode).toBe('rich');

        // Define override manual em pane-1.
        store.toggleInputMode('pane-1');
        expect(store.instance('pane-1')?.inputMode).toBe('passthrough');
        expect(store.instance('pane-1')?.inputModeOverride).toBe('passthrough');

        // Entrar em altScreen limpa o override e força passthrough.
        store.patch('pane-1', {
            modes: {
                altScreen: true,
                mouseTracking: 'none',
                bracketedPaste: false,
                applicationCursorKeys: false
            }
        });
        expect(store.instance('pane-1')?.inputMode).toBe('passthrough');
        expect(store.instance('pane-1')?.inputModeOverride).toBeNull();

        // Sair de altScreen restaura o modo padrão ('rich') já que o override foi limpo.
        store.patch('pane-1', {
            modes: {
                altScreen: false,
                mouseTracking: 'none',
                bracketedPaste: false,
                applicationCursorKeys: false
            }
        });
        expect(store.instance('pane-1')?.inputMode).toBe('rich');

        // webglCount conta apenas instâncias visíveis com renderer === 'webgl'.
        store.patch('pane-1', { renderer: 'webgl', visible: true });
        store.patch('pane-2', { renderer: 'webgl', visible: false });
        expect(store.webglCount).toBe(1);

        // Desabilitar richInputEnabled globalmente atualiza instâncias sem override.
        store.setPreferences({ richInputEnabled: false });
        expect(store.instance('pane-1')?.inputMode).toBe('passthrough');

        store.unregister('pane-1');
        expect(store.instance('pane-1')).toBeUndefined();
    });
});
