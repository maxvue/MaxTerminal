import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
    CommandBlockDto_Serialize,
    CommandFinished_Serialize,
    CommandStarted_Serialize,
    ErrorContext_Serialize
} from '@/types/bindings';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
    invoke: (...args: unknown[]) => invokeMock(...args),
    Channel: class {}
}));

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn(() => Promise.resolve(vi.fn()))
}));

import { useShellHistoryStore } from '@/stores/shellHistory';

const SESSION_ID = '01960000-0000-7000-8000-000000000401';
const PTY_ID = '01960000-0000-7000-8000-000000000301';

function makeStarted(seq: number, blockId: string): CommandStarted_Serialize {
    return {
        binding: {
            ptyId: PTY_ID,
            sessionId: SESSION_ID,
            tabId: '01960000-0000-7000-8000-000000000101',
            paneId: '01960000-0000-7000-8000-000000000201'
        },
        execution: {
            blockId,
            commandSeq: seq,
            startedAt: 1730000000000 + seq * 100,
            commandPreview: `echo ${seq}`,
            source: 'osc133'
        },
        commandText: `echo ${seq}`,
        commandTextQuality: 'exact',
        cwd: '/home/user',
        startPos: { ptySeq: seq, byteOffset: seq * 10 },
        outputStartPos: { ptySeq: seq, byteOffset: seq * 10 + 5 },
        confidence: 'exact',
        persistence: 'persisted'
    };
}

function makeFinished(seq: number, blockId: string): CommandFinished_Serialize {
    return {
        binding: {
            ptyId: PTY_ID,
            sessionId: SESSION_ID,
            tabId: '01960000-0000-7000-8000-000000000101',
            paneId: '01960000-0000-7000-8000-000000000201'
        },
        blockId,
        commandSeq: seq,
        commandText: `echo ${seq}`,
        commandTextQuality: 'exact',
        cwd: '/home/user',
        startedAt: 1730000000000 + seq * 100,
        endedAt: 1730000000050 + seq * 100,
        durationMs: 50,
        status: 'succeeded',
        endReason: 'exit_zero',
        exitCode: 0,
        signal: null,
        startPos: { ptySeq: seq, byteOffset: seq * 10 },
        outputStartPos: { ptySeq: seq, byteOffset: seq * 10 + 5 },
        endPos: { ptySeq: seq + 1, byteOffset: seq * 10 + 20 },
        outputBytes: 15,
        outputLines: 1,
        outputPreview: `${seq}\n`,
        errorExcerpt: null,
        inspectable: false,
        source: 'osc133',
        confidence: 'exact',
        persistence: 'persisted'
    };
}

describe('useShellHistoryStore', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        const store = useShellHistoryStore();
        store.resetForTests();
        invokeMock.mockReset();
    });

    it('ordena blocos por commandSeq, evita regressão de finalizado para running e respeita limite de sessão', () => {
        const store = useShellHistoryStore();
        store.maxBlocksPerSession = 3;

        store.handleCommandFinished(makeFinished(1, 'b-1'));
        // Evento started atrasado para o mesmo bloco finalizado não regride o status
        store.handleCommandStarted(makeStarted(1, 'b-1'));
        expect(store.blockById('b-1')?.status).toBe('succeeded');

        store.handleCommandStarted(makeStarted(3, 'b-3'));
        store.handleCommandStarted(makeStarted(2, 'b-2'));
        expect(store.blocksBySession(SESSION_ID).map((b) => b.id)).toEqual(['b-1', 'b-2', 'b-3']);

        // Ao inserir o 4º bloco, o mais antigo (b-1) é removido do cache da sessão
        store.handleCommandStarted(makeStarted(4, 'b-4'));
        expect(store.blocksBySession(SESSION_ID).map((b) => b.id)).toEqual(['b-2', 'b-3', 'b-4']);
        expect(store.blockById('b-1')).toBeNull();
    });

    it('busca ErrorContext com cache e expõe mensagem amigável em lastError quando IPC falha', async () => {
        const store = useShellHistoryStore();
        const blockId = '01960000-0000-7000-8000-000000000599';

        const mockCtx: ErrorContext_Serialize = {
            blockId,
            sessionId: SESSION_ID,
            tabId: null,
            paneId: null,
            command: 'false',
            commandTextQuality: 'exact',
            error: 'falha simulada',
            exitCode: 1,
            signal: null,
            cwd: '/tmp',
            durationMs: 12,
            startedAt: 1000,
            endedAt: 1012,
            truncated: false,
            planModePrompt: 'Ao executar o comando "false" tive o seguinte erro: "falha simulada"'
        };

        invokeMock.mockImplementation((cmd: string) => {
            if (cmd === 'command_block_error_context') {
                return Promise.resolve(mockCtx);
            }
            if (cmd === 'command_block_search') {
                return Promise.reject({
                    code: 'shell.search_invalid',
                    message: 'Consulta inválida'
                });
            }
            return Promise.resolve({ items: [] as CommandBlockDto_Serialize[], nextCursor: null });
        });

        const c1 = await store.fetchErrorContext(blockId);
        const c2 = await store.fetchErrorContext(blockId);
        expect(c1.planModePrompt).toContain('Ao executar o comando "false"');
        expect(c2).toEqual(c1);
        expect(
            invokeMock.mock.calls.filter((c) => c[0] === 'command_block_error_context')
        ).toHaveLength(1);

        await expect(store.search({ query: '***' })).rejects.toBeDefined();
        expect(store.lastError).toBe(
            'A consulta informada para busca no histórico de comandos é inválida.'
        );
    });

    it('hidrata sessões paginadas, busca blocos individuais e processa mudanças de cwd e modo de integração', async () => {
        const store = useShellHistoryStore();
        const blockDto: CommandBlockDto_Serialize = {
            id: '01960000-0000-7000-8000-000000000501',
            sessionId: SESSION_ID,
            tabId: '01960000-0000-7000-8000-000000000101',
            paneId: '01960000-0000-7000-8000-000000000201',
            ptyId: PTY_ID,
            commandSeq: 1,
            commandText: 'git status',
            commandTextQuality: 'exact',
            cwd: '/home/user/repo',
            startedAt: 1000,
            endedAt: 1100,
            durationMs: 100,
            exitCode: 0,
            signal: null,
            status: 'succeeded',
            endReason: 'exit_zero',
            isContinuous: false,
            source: 'osc133',
            confidence: 'exact',
            outputText: 'On branch main\n',
            outputKind: 'full',
            outputBytes: 15,
            outputLines: 1,
            errorExcerpt: null,
            inspectable: false,
            startPtySeq: 1,
            startByteOffset: 0,
            outputStartPtySeq: 1,
            outputStartByteOffset: 5,
            endPtySeq: 2,
            endByteOffset: 20,
            persistence: 'persisted'
        };

        invokeMock.mockImplementation((cmd: string) => {
            if (cmd === 'command_block_list') {
                return Promise.resolve({
                    items: [blockDto],
                    nextCursor: 'cursor-2'
                });
            }
            if (cmd === 'command_block_get') {
                return Promise.resolve(blockDto);
            }
            if (cmd === 'shell_integration_info') {
                return Promise.resolve({
                    ptyId: PTY_ID,
                    sessionId: SESSION_ID,
                    tabId: blockDto.tabId,
                    paneId: blockDto.paneId,
                    shellKind: 'bash',
                    mode: 'osc133',
                    continuousPolicy: 'auto',
                    cwd: '/home/user/repo',
                    oscEventsReceived: 4,
                    fallbackActivations: 0
                });
            }
            return Promise.resolve({ items: [], nextCursor: null });
        });

        await store.init();
        const page = await store.loadSessionPage(SESSION_ID);
        expect(page.items).toHaveLength(1);
        expect(store.nextCursorBySession[SESSION_ID]).toBe('cursor-2');
        expect(store.blocksBySession(SESSION_ID)).toHaveLength(1);

        const fetched = await store.fetchBlock(blockDto.id);
        expect(fetched.commandText).toBe('git status');

        const info = await store.fetchIntegrationInfo(PTY_ID);
        expect(info.mode).toBe('osc133');
        expect(store.cwdOf(PTY_ID)).toBe('/home/user/repo');

        store.handleCwdChanged({
            binding: {
                ptyId: PTY_ID,
                sessionId: SESSION_ID,
                tabId: blockDto.tabId,
                paneId: blockDto.paneId
            },
            cwd: '/home/user/repo/src',
            at: 2000
        });
        expect(store.cwdOf(PTY_ID)).toBe('/home/user/repo/src');

        store.handleIntegrationMode({
            binding: {
                ptyId: PTY_ID,
                sessionId: SESSION_ID,
                tabId: blockDto.tabId,
                paneId: blockDto.paneId
            },
            mode: 'fallback_proc',
            shellKind: 'bash',
            reason: 'activation_window_expired'
        });
        expect(store.integrationOf(PTY_ID)?.mode).toBe('fallback_proc');

        store.dispose();
    });
});
