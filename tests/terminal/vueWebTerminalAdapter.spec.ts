import { describe, expect, it, vi } from 'vitest';
import type { CommandBlockModel, TerminalContext } from '@/types/terminal';
import { createVueWebTerminalAdapter } from '@/terminal/blocks/vueWebTerminalAdapter';
import { ptyClient } from '@/terminal/stream/ptyClient';

const mockCtx: TerminalContext = {
    paneId: 'pane-vwt',
    ptyId: 'pty-vwt',
    projectId: null,
    cwd: '/home/user/app',
    extra: Object.freeze({})
};

describe('VueWebTerminalAdapter (ADR-04-01)', () => {
    it('importa vue-web-terminal real, projeta CommandBlockModel em texto puro (sem HTML), trunca a 4096 chars e não cria segundo leitor de PTY', async () => {
        const attachSpy = vi.spyOn(ptyClient, 'attach');
        const forwardedCommands: string[] = [];

        const adapter = createVueWebTerminalAdapter({
            paneId: 'pane-vwt',
            getContext: () => mockCtx,
            onInputIntent: async (cmd) => {
                forwardedCommands.push(cmd);
            }
        });

        expect(adapter.getLibraryMetadata()).toEqual({
            packageName: 'vue-web-terminal',
            available: true
        });
        expect(attachSpy).not.toHaveBeenCalled();

        const longCmd = `<script>alert(1)</script>${'a'.repeat(5000)}`;
        const block: CommandBlockModel = {
            id: 'blk-1',
            paneId: 'pane-vwt',
            command: longCmd,
            cwd: '/home/user/app',
            startedAt: 1000,
            finishedAt: 1250,
            exitCode: 0,
            status: 'success',
            promptLine: 10,
            outputStartLine: 11,
            outputEndLine: 15,
            meta: Object.freeze({})
        };

        const projected = adapter.projectBlock(block, '<b>Saída</b>');
        expect(projected.blockId).toBe('blk-1');
        expect(projected.class).toBe('success');
        expect(projected.durationMs).toBe(250);
        expect(projected.command.length).toBeLessThanOrEqual(4097);
        expect(projected.content.includes('<')).toBe(false);
        expect(projected.content.includes('>')).toBe(false);

        const list = adapter.projectBlocks([
            { ...block, id: 'b-run', status: 'running', finishedAt: null, exitCode: null },
            { ...block, id: 'b-err', status: 'error', exitCode: 2 },
            { ...block, id: 'b-abt', status: 'aborted', exitCode: null }
        ]);
        expect(list.map((e) => e.class)).toEqual(['info', 'error', 'warning']);

        let successCalled = false;
        await adapter.handleExecCmd('echo', 'echo teste', () => {
            successCalled = true;
        });
        expect(successCalled).toBe(true);
        expect(forwardedCommands).toEqual(['echo teste']);

        adapter.dispose();
        expect(adapter.projectBlocks([block])).toEqual([]);
    });
});
