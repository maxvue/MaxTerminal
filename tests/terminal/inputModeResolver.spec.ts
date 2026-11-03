import { beforeEach, describe, expect, it } from 'vitest';
import type { TerminalContext, TerminalModes } from '@/types/terminal';
import {
    clearInputModeHintsForTests,
    registerInputModeHint,
    resolveInputMode,
    shouldClearOverrideOnModesChange
} from '@/terminal/input/inputModeResolver';

const baseCtx: TerminalContext = {
    paneId: 'pane-1',
    ptyId: 'pty-1',
    projectId: null,
    cwd: '/tmp',
    extra: Object.freeze({})
};

const normalModes: TerminalModes = {
    altScreen: false,
    mouseTracking: 'none',
    bracketedPaste: false,
    applicationCursorKeys: false
};

describe('InputModeResolver', () => {
    beforeEach(() => {
        clearInputModeHintsForTests();
    });

    it('força passthrough quando altScreen=true ou mouseTracking!=none acima de qualquer override ou dica', () => {
        registerInputModeHint(() => ({
            mode: 'rich',
            priority: 100,
            reason: 'Dica alta'
        }));

        const resAlt = resolveInputMode({
            context: baseCtx,
            modes: { ...normalModes, altScreen: true },
            override: 'rich',
            richInputEnabled: true
        });
        expect(resAlt).toMatchObject({
            mode: 'passthrough',
            forcedByTui: true,
            source: 'tui'
        });

        const resMouse = resolveInputMode({
            context: baseCtx,
            modes: { ...normalModes, mouseTracking: 'any' },
            override: 'rich',
            richInputEnabled: true
        });
        expect(resMouse).toMatchObject({
            mode: 'passthrough',
            forcedByTui: true,
            source: 'tui'
        });

        expect(
            shouldClearOverrideOnModesChange(normalModes, { ...normalModes, altScreen: true })
        ).toBe(true);
    });

    it('aplica precedência: override manual > hint de maior priority (empate vence último) > default', () => {
        registerInputModeHint(() => ({
            mode: 'passthrough',
            priority: 10,
            reason: 'Agente A'
        }));
        const subTie = registerInputModeHint(() => ({
            mode: 'rich',
            priority: 10,
            reason: 'Agente B (empate mais recente)'
        }));

        const resHint = resolveInputMode({
            context: baseCtx,
            modes: normalModes,
            override: null,
            richInputEnabled: false
        });
        expect(resHint).toEqual({
            mode: 'rich',
            forcedByTui: false,
            source: 'hint',
            reason: 'Agente B (empate mais recente)'
        });

        // Override manual vence hint.
        const resOverride = resolveInputMode({
            context: baseCtx,
            modes: normalModes,
            override: 'passthrough',
            richInputEnabled: true
        });
        expect(resOverride.mode).toBe('passthrough');
        expect(resOverride.source).toBe('override');

        subTie.dispose();
    });
});
