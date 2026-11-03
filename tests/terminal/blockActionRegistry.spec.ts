import { beforeEach, describe, expect, it } from 'vitest';
import type { Terminal } from '@xterm/xterm';
import type { CommandBlockModel, TerminalContext } from '@/types/terminal';
import {
    executeBlockAction,
    extractRenderedBlockOutput,
    getBlockActionsFor,
    registerBlockAction,
    resetBlockActionsForTests
} from '@/terminal/blocks/blockActionRegistry';
import { MockMarker, MockXtermTerminal } from './mocks/xtermMock';

const dummyCtx: TerminalContext = {
    paneId: 'pane-act',
    ptyId: 'pty-act',
    projectId: null,
    cwd: '/tmp',
    extra: Object.freeze({})
};

describe('BlockActionRegistry & extractRenderedBlockOutput', () => {
    beforeEach(() => {
        resetBlockActionsForTests();
    });

    it('extrai texto renderizado do buffer.normal concatenando linhas isWrapped sem quebra artificial e detecta trim parcial', () => {
        const mockTerm = new MockXtermTerminal();
        mockTerm.normalBuffer.setLines([
            { text: '$ echo longo' },
            { text: 'parte1_', isWrapped: false },
            { text: 'parte2_continua', isWrapped: true },
            { text: 'segunda linha real', isWrapped: false }
        ]);

        const block: CommandBlockModel = {
            id: 'blk-out',
            paneId: 'pane-act',
            command: 'echo longo',
            cwd: '/tmp',
            startedAt: 100,
            finishedAt: 200,
            exitCode: 0,
            status: 'success',
            promptLine: 0,
            outputStartLine: 1,
            outputEndLine: 3,
            meta: Object.freeze({})
        };

        const startMarker = new MockMarker(1, 1);
        const endMarker = new MockMarker(2, 3);

        const full = extractRenderedBlockOutput(mockTerm as unknown as Terminal, block, {
            promptMarker: new MockMarker(0, 0),
            outputStartMarker: startMarker,
            outputEndMarker: endMarker,
            partialTrimmed: false
        });

        expect(full).toEqual({
            text: 'parte1_parte2_continua\nsegunda linha real',
            partial: false,
            available: true
        });

        // Simula descarte do startMarker por trim do scrollback, mantendo endMarker válido.
        startMarker.dispose();
        const partial = extractRenderedBlockOutput(mockTerm as unknown as Terminal, block, {
            promptMarker: null,
            outputStartMarker: startMarker,
            outputEndMarker: endMarker,
            partialTrimmed: true
        });
        expect(partial.partial).toBe(true);
        expect(partial.available).toBe(true);
    });

    it('filtra ações por when(block), ordena por order e permite registrar/remover ações de fases futuras', async () => {
        const blockErr: CommandBlockModel = {
            id: 'blk-err',
            paneId: 'pane-act',
            command: 'php artisan migrate',
            cwd: '/tmp',
            startedAt: 100,
            finishedAt: 300,
            exitCode: 1,
            status: 'error',
            promptLine: 0,
            outputStartLine: 1,
            outputEndLine: 2,
            meta: Object.freeze({})
        };

        const executed: string[] = [];
        const sub = registerBlockAction({
            id: 'maxcode.inspect',
            label: 'Inspecionar',
            icon: 'mdi:magnify-scan',
            order: 5,
            when: (b) => b.status === 'error',
            run: (b) => {
                executed.push(b.id);
            }
        });

        const actions = getBlockActionsFor(blockErr);
        expect(actions.map((a) => a.id)).toEqual([
            'maxcode.inspect',
            'copy-command',
            'copy-output'
        ]);

        await executeBlockAction('maxcode.inspect', blockErr, dummyCtx);
        expect(executed).toEqual(['blk-err']);

        // Testa ações embutidas copy-command e copy-output e tratamento de falha com MtTerminalError.
        await executeBlockAction('copy-command', blockErr, dummyCtx);
        await executeBlockAction('copy-output', blockErr, dummyCtx);

        const failSub = registerBlockAction({
            id: 'fail.action',
            label: 'Falhar',
            icon: 'mdi:alert',
            order: 99,
            when: () => true,
            run: () => {
                throw new Error('Falha simulada');
            }
        });
        await expect(executeBlockAction('fail.action', blockErr, dummyCtx)).rejects.toThrow(
            'Falha simulada'
        );
        failSub.dispose();

        sub.dispose();
        expect(getBlockActionsFor(blockErr).map((a) => a.id)).toEqual([
            'copy-command',
            'copy-output'
        ]);
    });
});
