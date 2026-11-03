import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { RESERVED_APP_SHORTCUTS, useAgentShortcuts } from '@/composables/useAgentShortcuts';
import { useAgentsStore } from '@/stores/agents';
import type { AgentInfo_Serialize } from '@/types/bindings';

function makeAgentInfo(
    kind: 'claude-code' | 'codex' | 'agy' | 'opencode',
    shortcut: string,
    installed: boolean
): AgentInfo_Serialize {
    return {
        descriptor: {
            kind,
            displayName: kind,
            icon: 'lucide:bot',
            colorToken: '--max-primary-500',
            binary: kind,
            processNames: [kind],
            versionArgs: ['--version'],
            versionRegex: '([0-9]+\\.[0-9]+\\.[0-9]+)',
            defaultArgs: [],
            extraArgs: [],
            modelFlag: '--model',
            env: {},
            shortcut,
            capabilities: {
                statuslineHook: true,
                nativeAttentionHooks: true,
                sessionFiles: true,
                oscNotifications: true
            }
        },
        installation: {
            kind,
            installed,
            path: installed ? `/usr/bin/${kind}` : null,
            version: installed ? '1.0.0' : null,
            reason: null,
            checkedAtMs: 1_700_000_000_000
        },
        enabled: true,
        capabilities: {
            agent: kind,
            binaryVersion: installed ? '1.0.0' : null,
            adapterRevision: 'rev-1',
            checkedAtMs: 1_700_000_000_000,
            features: {}
        }
    };
}

describe('useAgentShortcuts (Fase 13)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('abre agente instalado em 1 clique com Ctrl+Alt+C e abre diálogo com isolamento em Ctrl+Alt+Shift+C (CA-10)', () => {
        const store = useAgentsStore();
        store.agents = [
            makeAgentInfo('claude-code', 'Ctrl+Alt+C', true),
            makeAgentInfo('codex', 'Ctrl+Alt+X', false)
        ];

        const openAgentSpy = vi.spyOn(store, 'openAgent').mockResolvedValue({} as never);
        const openDialogSpy = vi.spyOn(store, 'openAgentDialog');

        const shortcuts = useAgentShortcuts({ projectId: 'proj-1' });

        const ev1 = new KeyboardEvent('keydown', {
            key: 'c',
            code: 'KeyC',
            ctrlKey: true,
            altKey: true,
            shiftKey: false,
            bubbles: true,
            cancelable: true
        });
        const handled1 = shortcuts.handleKeyEvent(ev1);
        expect(handled1).toBe(true);
        expect(ev1.defaultPrevented).toBe(true);
        expect(openAgentSpy).toHaveBeenCalledTimes(1);
        expect(openAgentSpy).toHaveBeenCalledWith({
            projectId: 'proj-1',
            agent: 'claude-code',
            isolation: { mode: 'none' },
            focus: true
        });

        // Mesmo KeyboardEvent repetido (ex.: xterm + window) não dispara segunda abertura
        shortcuts.handleKeyEvent(ev1);
        expect(openAgentSpy).toHaveBeenCalledTimes(1);

        // Com +Shift abre o AgentTabOpenDialog com isolamento marcado
        const evShift = new KeyboardEvent('keydown', {
            key: 'C',
            code: 'KeyC',
            ctrlKey: true,
            altKey: true,
            shiftKey: true,
            bubbles: true,
            cancelable: true
        });
        expect(shortcuts.handleKeyEvent(evShift)).toBe(true);
        expect(openDialogSpy).toHaveBeenCalledWith('proj-1', 'claude-code', true);

        shortcuts.dispose();
    });

    it('exibe toast "Agente não encontrado no PATH" quando o agente não está instalado e ignora repeat/IME (CA-10)', () => {
        const store = useAgentsStore();
        store.agents = [makeAgentInfo('codex', 'Ctrl+Alt+X', false)];
        const openAgentSpy = vi.spyOn(store, 'openAgent');

        const shortcuts = useAgentShortcuts({ projectId: 'proj-1' });

        const evRepeat = new KeyboardEvent('keydown', {
            key: 'x',
            code: 'KeyX',
            ctrlKey: true,
            altKey: true,
            repeat: true
        });
        expect(shortcuts.handleKeyEvent(evRepeat)).toBe(false);

        const evUninstalled = new KeyboardEvent('keydown', {
            key: 'x',
            code: 'KeyX',
            ctrlKey: true,
            altKey: true,
            cancelable: true
        });
        expect(shortcuts.handleKeyEvent(evUninstalled)).toBe(true);
        expect(openAgentSpy).not.toHaveBeenCalled();
        expect(shortcuts.lastToast.value?.title).toBe('Agente não encontrado no PATH');

        shortcuts.dispose();
    });

    it('reserva a tecla F5 na constante RESERVED_APP_SHORTCUTS e bloqueia acionamento de agentes por F5', () => {
        expect(RESERVED_APP_SHORTCUTS.has('F5')).toBe(true);

        const store = useAgentsStore();
        store.agents = [makeAgentInfo('claude-code', 'F5', true)];
        const openAgentSpy = vi.spyOn(store, 'openAgent');

        const shortcuts = useAgentShortcuts({ projectId: 'proj-1' });

        const evF5 = new KeyboardEvent('keydown', {
            key: 'F5',
            code: 'F5',
            bubbles: true,
            cancelable: true
        });

        const handled = shortcuts.handleKeyEvent(evF5);
        expect(handled).toBe(false);
        expect(openAgentSpy).not.toHaveBeenCalled();

        shortcuts.dispose();
    });
});
