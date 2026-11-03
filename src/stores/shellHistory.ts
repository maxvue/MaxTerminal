import { defineStore } from 'pinia';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
    commandBlockErrorContext,
    commandBlockGet,
    commandBlockList,
    commandBlockSearch,
    onCommandFinished,
    onCommandStarted,
    onShellCwdChanged,
    onShellIntegrationMode,
    shellIntegrationInfo
} from '@/ipc/shell';
import { errorMessages } from '@/types/errors';
import type {
    CommandBlockDto_Serialize,
    CommandBlockHit_Serialize,
    CommandBlockId,
    CommandBlockSearchQuery_Deserialize,
    CommandFinished_Serialize,
    CommandStarted_Serialize,
    ErrorContext_Serialize,
    IntegrationInfo_Serialize,
    Page,
    PtyId,
    SessionId,
    ShellCwdChangedEvent_Serialize,
    ShellIntegrationModeEvent_Serialize
} from '@/types/bindings';

/** Limite máximo de blocos mantidos em memória por sessão na `useShellHistoryStore`. */
export const MAX_HISTORY_BLOCKS_PER_SESSION = 500;

/** Limite máximo de contextos de erro mantidos em cache na `useShellHistoryStore`. */
const MAX_CACHED_ERROR_CONTEXTS = 32;

/** Estado reativo da store `useShellHistoryStore`. */
export interface ShellHistoryStoreState {
    byId: Record<CommandBlockId, CommandBlockDto_Serialize>;
    idsBySession: Record<SessionId, CommandBlockId[]>;
    nextCursorBySession: Record<SessionId, string | null>;
    errorContexts: Record<CommandBlockId, ErrorContext_Serialize>;
    errorContextOrder: CommandBlockId[];
    integrationByPty: Record<PtyId, IntegrationInfo_Serialize>;
    cwdByPty: Record<PtyId, string>;
    searchHits: CommandBlockHit_Serialize[];
    searchNextCursor: string | null;
    lastError: string | null;
    maxBlocksPerSession: number;
    initialized: boolean;
}

let initInFlight: Promise<void> | null = null;
let initGeneration = 0;
let activeUnlisteners: UnlistenFn[] = [];

function clearActiveUnlisteners(): void {
    const list = activeUnlisteners.splice(0, activeUnlisteners.length);
    for (const unlisten of list) {
        try {
            unlisten();
        } catch {
            // Ignora falha de remoção de listener.
        }
    }
}

function formatStoreError(err: unknown): string {
    if (err && typeof err === 'object' && 'code' in err && typeof (err as { code: unknown }).code === 'string') {
        const code = (err as { code: string }).code;
        if (errorMessages[code]) {
            return errorMessages[code]!;
        }
    }
    if (err instanceof Error && err.message) {
        return err.message;
    }
    return 'Não foi possível consultar o histórico de comandos.';
}

export const useShellHistoryStore = defineStore('shellHistory', {
    state: (): ShellHistoryStoreState => ({
        byId: {},
        idsBySession: {},
        nextCursorBySession: {},
        errorContexts: {},
        errorContextOrder: [],
        integrationByPty: {},
        cwdByPty: {},
        searchHits: [],
        searchNextCursor: null,
        lastError: null,
        maxBlocksPerSession: MAX_HISTORY_BLOCKS_PER_SESSION,
        initialized: false
    }),

    getters: {
        /** Obtém um bloco de comando pelo ID no cache da store. */
        blockById(state): (id: CommandBlockId) => CommandBlockDto_Serialize | null {
            return (id: CommandBlockId): CommandBlockDto_Serialize | null => state.byId[id] ?? null;
        },

        /** Lista os blocos de comando de uma sessão ordenados por `commandSeq` crescente. */
        blocksBySession(state): (sessionId: SessionId) => CommandBlockDto_Serialize[] {
            return (sessionId: SessionId): CommandBlockDto_Serialize[] => {
                const ids = state.idsBySession[sessionId];
                if (!ids) {
                    return [];
                }
                const list: CommandBlockDto_Serialize[] = [];
                for (const id of ids) {
                    const item = state.byId[id];
                    if (item) {
                        list.push(item);
                    }
                }
                return list;
            };
        },

        /** Retorna as informações de integração de shell de um PTY. */
        integrationOf(state): (ptyId: PtyId) => IntegrationInfo_Serialize | null {
            return (ptyId: PtyId): IntegrationInfo_Serialize | null =>
                state.integrationByPty[ptyId] ?? null;
        },

        /** Retorna o último diretório de trabalho (`cwd`) conhecido para um PTY. */
        cwdOf(state): (ptyId: PtyId) => string | null {
            return (ptyId: PtyId): string | null =>
                state.cwdByPty[ptyId] ?? state.integrationByPty[ptyId]?.cwd ?? null;
        }
    },

    actions: {
        /**
         * Insere ou atualiza um `CommandBlockDto` mantendo o limite de `maxBlocksPerSession` (500)
         * e ordenação determinística por `commandSeq`.
         */
        upsertBlock(block: CommandBlockDto_Serialize): void {
            const existing = this.byId[block.id];
            if (existing) {
                // Não regride um bloco já finalizado para 'running' caso eventos cheguem fora de ordem
                if (existing.status !== 'running' && block.status === 'running') {
                    return;
                }
                this.byId[block.id] = {
                    ...existing,
                    ...block,
                    outputText: block.outputText ?? existing.outputText
                };
            } else {
                this.byId[block.id] = block;
            }

            let sessionList = this.idsBySession[block.sessionId];
            if (!sessionList) {
                sessionList = [];
                this.idsBySession[block.sessionId] = sessionList;
            }
            if (!sessionList.includes(block.id)) {
                sessionList.push(block.id);
                sessionList.sort((a, b) => {
                    const ba = this.byId[a];
                    const bb = this.byId[b];
                    if (!ba || !bb) {
                        return 0;
                    }
                    if (ba.commandSeq !== bb.commandSeq) {
                        return ba.commandSeq - bb.commandSeq;
                    }
                    return ba.startedAt - bb.startedAt;
                });
            }

            while (sessionList.length > this.maxBlocksPerSession) {
                const evictedId = sessionList.shift();
                if (evictedId !== undefined) {
                    delete this.byId[evictedId];
                    delete this.errorContexts[evictedId];
                    const errIdx = this.errorContextOrder.indexOf(evictedId);
                    if (errIdx !== -1) {
                        this.errorContextOrder.splice(errIdx, 1);
                    }
                }
            }
        },

        /** Trata o evento `shell://command-started`. */
        handleCommandStarted(ev: CommandStarted_Serialize): void {
            if (ev.cwd) {
                this.cwdByPty[ev.binding.ptyId] = ev.cwd;
            }
            const dto: CommandBlockDto_Serialize = {
                id: ev.execution.blockId,
                sessionId: ev.binding.sessionId,
                tabId: ev.binding.tabId,
                paneId: ev.binding.paneId,
                ptyId: ev.binding.ptyId,
                commandSeq: ev.execution.commandSeq,
                commandText: ev.commandText ?? ev.execution.commandPreview,
                commandTextQuality: ev.commandTextQuality,
                cwd: ev.cwd,
                startedAt: ev.execution.startedAt,
                endedAt: null,
                durationMs: null,
                exitCode: null,
                signal: null,
                status: 'running',
                endReason: null,
                isContinuous: false,
                source: ev.execution.source,
                confidence: ev.confidence,
                outputText: null,
                outputKind: 'none',
                outputBytes: 0,
                outputLines: 0,
                errorExcerpt: null,
                inspectable: false,
                startPtySeq: ev.startPos.ptySeq,
                startByteOffset: ev.startPos.byteOffset,
                outputStartPtySeq: ev.outputStartPos?.ptySeq ?? null,
                outputStartByteOffset: ev.outputStartPos?.byteOffset ?? null,
                endPtySeq: null,
                endByteOffset: null,
                persistence: ev.persistence
            };
            this.upsertBlock(dto);
        },

        /** Trata o evento `shell://command-finished`. */
        handleCommandFinished(ev: CommandFinished_Serialize): void {
            if (ev.cwd) {
                this.cwdByPty[ev.binding.ptyId] = ev.cwd;
            }
            const existing = this.byId[ev.blockId];
            const dto: CommandBlockDto_Serialize = {
                id: ev.blockId,
                sessionId: ev.binding.sessionId,
                tabId: ev.binding.tabId,
                paneId: ev.binding.paneId,
                ptyId: ev.binding.ptyId,
                commandSeq: ev.commandSeq,
                commandText: ev.commandText ?? existing?.commandText ?? null,
                commandTextQuality: ev.commandTextQuality,
                cwd: ev.cwd ?? existing?.cwd ?? null,
                startedAt: ev.startedAt,
                endedAt: ev.endedAt,
                durationMs: ev.durationMs,
                exitCode: ev.exitCode,
                signal: ev.signal,
                status: ev.status,
                endReason: ev.endReason,
                isContinuous: existing?.isContinuous ?? false,
                source: ev.source,
                confidence: ev.confidence,
                outputText: existing?.outputText ?? ev.outputPreview ?? null,
                outputKind: existing?.outputKind ?? (ev.outputBytes > 0 ? 'full' : 'none'),
                outputBytes: ev.outputBytes,
                outputLines: ev.outputLines,
                errorExcerpt: ev.errorExcerpt,
                inspectable: ev.inspectable,
                startPtySeq: ev.startPos.ptySeq,
                startByteOffset: ev.startPos.byteOffset,
                outputStartPtySeq: ev.outputStartPos?.ptySeq ?? null,
                outputStartByteOffset: ev.outputStartPos?.byteOffset ?? null,
                endPtySeq: ev.endPos.ptySeq,
                endByteOffset: ev.endPos.byteOffset,
                persistence: ev.persistence
            };
            this.upsertBlock(dto);
        },

        /** Trata o evento `shell://cwd-changed`. */
        handleCwdChanged(ev: ShellCwdChangedEvent_Serialize): void {
            this.cwdByPty[ev.binding.ptyId] = ev.cwd;
            const info = this.integrationByPty[ev.binding.ptyId];
            if (info) {
                this.integrationByPty[ev.binding.ptyId] = {
                    ...info,
                    cwd: ev.cwd
                };
            }
        },

        /** Trata o evento `shell://integration-mode`. */
        handleIntegrationMode(ev: ShellIntegrationModeEvent_Serialize): void {
            const existing = this.integrationByPty[ev.binding.ptyId];
            if (existing) {
                this.integrationByPty[ev.binding.ptyId] = {
                    ...existing,
                    mode: ev.mode,
                    shellKind: ev.shellKind
                };
                return;
            }
            this.integrationByPty[ev.binding.ptyId] = {
                ptyId: ev.binding.ptyId,
                sessionId: ev.binding.sessionId,
                tabId: ev.binding.tabId,
                paneId: ev.binding.paneId,
                shellKind: ev.shellKind,
                mode: ev.mode,
                continuousPolicy: 'auto',
                cwd: this.cwdByPty[ev.binding.ptyId] ?? null,
                oscEventsReceived: 0,
                fallbackActivations: 0
            };
        },

        /** Inicializa listeners globais de eventos `shell://*` de forma idempotente. */
        async init(): Promise<void> {
            if (this.initialized) {
                return;
            }
            if (initInFlight !== null) {
                return initInFlight;
            }

            const myGen = ++initGeneration;

            const runInit = async (): Promise<void> => {
                const localUnlisteners: UnlistenFn[] = [];
                try {
                    localUnlisteners.push(
                        await onCommandStarted((ev) => {
                            if (myGen === initGeneration) {
                                this.handleCommandStarted(ev);
                            }
                        }),
                        await onCommandFinished((ev) => {
                            if (myGen === initGeneration) {
                                this.handleCommandFinished(ev);
                            }
                        }),
                        await onShellCwdChanged((ev) => {
                            if (myGen === initGeneration) {
                                this.handleCwdChanged(ev);
                            }
                        }),
                        await onShellIntegrationMode((ev) => {
                            if (myGen === initGeneration) {
                                this.handleIntegrationMode(ev);
                            }
                        })
                    );

                    if (myGen !== initGeneration) {
                        for (const u of localUnlisteners) {
                            u();
                        }
                        return;
                    }

                    clearActiveUnlisteners();
                    activeUnlisteners = localUnlisteners;
                    this.initialized = true;
                } catch (err: unknown) {
                    for (const u of localUnlisteners) {
                        try {
                            u();
                        } catch {
                            // Ignora erro de cleanup.
                        }
                    }
                    throw err;
                } finally {
                    if (myGen === initGeneration) {
                        initInFlight = null;
                    }
                }
            };

            initInFlight = runInit();
            return initInFlight;
        },

        /** Remove listeners ativos e invalida inicializações pendentes. */
        dispose(): void {
            initGeneration += 1;
            initInFlight = null;
            clearActiveUnlisteners();
            this.initialized = false;
        },

        /** Carrega uma página de blocos de comando de uma sessão via `command_block_list`. */
        async loadSessionPage(
            sessionId: SessionId,
            opts?: { cursor?: string | null; limit?: number }
        ): Promise<Page<CommandBlockDto_Serialize>> {
            this.lastError = null;
            try {
                const page = await commandBlockList({
                    sessionId,
                    cursor: opts?.cursor ?? null,
                    limit: opts?.limit ?? 50
                });
                for (const item of page.items) {
                    this.upsertBlock(item);
                }
                this.nextCursorBySession[sessionId] = page.nextCursor;
                return page;
            } catch (err: unknown) {
                this.lastError = formatStoreError(err);
                throw err;
            }
        },

        /** Busca os detalhes completos de um bloco (incluindo `outputText`) via `command_block_get`. */
        async fetchBlock(blockId: CommandBlockId): Promise<CommandBlockDto_Serialize> {
            this.lastError = null;
            try {
                const block = await commandBlockGet(blockId);
                this.upsertBlock(block);
                return block;
            } catch (err: unknown) {
                this.lastError = formatStoreError(err);
                throw err;
            }
        },

        /** Obtém e armazena em cache limitado o `ErrorContext` sanitizado de um bloco com falha. */
        async fetchErrorContext(blockId: CommandBlockId): Promise<ErrorContext_Serialize> {
            const cached = this.errorContexts[blockId];
            if (cached) {
                return cached;
            }
            this.lastError = null;
            try {
                const ctx = await commandBlockErrorContext(blockId);
                this.errorContexts[blockId] = ctx;
                this.errorContextOrder.push(blockId);
                while (this.errorContextOrder.length > MAX_CACHED_ERROR_CONTEXTS) {
                    const oldest = this.errorContextOrder.shift();
                    if (oldest !== undefined) {
                        delete this.errorContexts[oldest];
                    }
                }
                return ctx;
            } catch (err: unknown) {
                this.lastError = formatStoreError(err);
                throw err;
            }
        },

        /** Executa busca FTS5 via `command_block_search` e atualiza `searchHits`. */
        async search(
            query: CommandBlockSearchQuery_Deserialize
        ): Promise<Page<CommandBlockHit_Serialize>> {
            this.lastError = null;
            try {
                const page = await commandBlockSearch(query);
                for (const hit of page.items) {
                    this.upsertBlock(hit.block);
                }
                if (query.cursor) {
                    this.searchHits = [...this.searchHits, ...page.items];
                } else {
                    this.searchHits = page.items;
                }
                this.searchNextCursor = page.nextCursor;
                return page;
            } catch (err: unknown) {
                this.lastError = formatStoreError(err);
                throw err;
            }
        },

        /** Consulta e atualiza o diagnóstico de integração de shell de um PTY. */
        async fetchIntegrationInfo(ptyId: PtyId): Promise<IntegrationInfo_Serialize> {
            this.lastError = null;
            try {
                const info = await shellIntegrationInfo(ptyId);
                this.integrationByPty[ptyId] = info;
                if (info.cwd) {
                    this.cwdByPty[ptyId] = info.cwd;
                }
                return info;
            } catch (err: unknown) {
                this.lastError = formatStoreError(err);
                throw err;
            }
        },

        /** Reseta o estado da store (exclusivo para testes). */
        resetForTests(): void {
            this.dispose();
            this.byId = {};
            this.idsBySession = {};
            this.nextCursorBySession = {};
            this.errorContexts = {};
            this.errorContextOrder = [];
            this.integrationByPty = {};
            this.cwdByPty = {};
            this.searchHits = [];
            this.searchNextCursor = null;
            this.lastError = null;
        }
    }
});
