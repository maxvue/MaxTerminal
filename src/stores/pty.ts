import { defineStore } from 'pinia';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
    onPtyCleanupFailed,
    onPtyExit,
    onPtyForegroundChanged,
    onPtySpawned,
    ptyKill,
    ptyList,
    ptyProcessInfo,
    ptySpawn
} from '@/ipc/pty';
import type {
    ExitInfo,
    KillMode,
    PtyCleanupFailedEvent,
    PtyExitEvent,
    PtyForegroundChangedEvent,
    PtyId,
    PtyProcessReport,
    PtySpawnedEvent,
    PtySummary,
    SpawnSpec,
    TabId
} from '@/types/bindings';

/** Limite máximo de eventos retidos no buffer temporário durante o bootstrap `init()`. */
const MAX_BUFFERED_INIT_EVENTS = 1024;

type BufferedPtyEvent =
    | { type: 'spawned'; payload: PtySpawnedEvent }
    | { type: 'exit'; payload: PtyExitEvent }
    | { type: 'foreground'; payload: PtyForegroundChangedEvent }
    | { type: 'cleanupFailed'; payload: PtyCleanupFailedEvent };

/** Estado reativo em memória da store de pseudo-terminais (`usePtyStore`). */
export interface PtyStoreState {
    ptys: Record<PtyId, PtySummary>;
    processReports: Record<PtyId, PtyProcessReport>;
    initialized: boolean;
}

let initInFlight: Promise<void> | null = null;
let activeUnlisteners: UnlistenFn[] = [];

function parseRevision(revision: string): bigint {
    try {
        return BigInt(revision);
    } catch {
        return 0n;
    }
}

function isTerminalExited(summary: PtySummary | undefined): boolean {
    return summary !== undefined && summary.status.kind === 'exited';
}

export const usePtyStore = defineStore('pty', {
    state: (): PtyStoreState => ({
        ptys: {},
        processReports: {},
        initialized: false
    }),

    getters: {
        /** Retorna todos os PTYs ainda vivos (`running`, `exiting` ou `cleanupFailed`). */
        alive(state): PtySummary[] {
            return Object.values(state.ptys).filter(
                (pty) =>
                    pty.status.kind === 'running' ||
                    pty.status.kind === 'exiting' ||
                    pty.status.kind === 'cleanupFailed'
            );
        },

        /** Filtra os PTYs pertencentes a uma aba específica (`owner.tabId === tabId`). */
        byTab(state): (tabId: TabId) => PtySummary[] {
            return (tabId: TabId): PtySummary[] =>
                Object.values(state.ptys).filter((pty) => pty.owner.tabId === tabId);
        },

        /** Retorna o `ExitInfo` de um PTY caso já tenha encerrado (`exited`), ou `null`. */
        exitOf(state): (ptyId: PtyId) => ExitInfo | null {
            return (ptyId: PtyId): ExitInfo | null => {
                const pty = state.ptys[ptyId];
                if (pty !== undefined && pty.status.kind === 'exited') {
                    return pty.status.exit;
                }
                return null;
            };
        }
    },

    actions: {
        /**
         * Aplica um `PtySummary` respeitando a monotonicidade de `revision` e a invariante
         * de que um PTY já em `exited` nunca regride para `running`.
         */
        applySummaryWithRevision(incoming: PtySummary): void {
            const existing = this.ptys[incoming.id];
            if (existing === undefined) {
                this.ptys[incoming.id] = incoming;
                return;
            }

            if (isTerminalExited(existing) && incoming.status.kind !== 'exited') {
                return;
            }

            if (parseRevision(incoming.revision) >= parseRevision(existing.revision)) {
                this.ptys[incoming.id] = incoming;
            }
        },

        /** Trata o evento `pty://spawned`. */
        handleSpawned(e: PtySpawnedEvent): void {
            const incoming: PtySummary = {
                ...e.summary,
                revision: e.revision
            };
            this.applySummaryWithRevision(incoming);
        },

        /** Trata o evento `pty://foreground-changed`. */
        handleForegroundChanged(e: PtyForegroundChangedEvent): void {
            const existing = this.ptys[e.ptyId];
            if (existing === undefined || isTerminalExited(existing)) {
                return;
            }
            if (parseRevision(e.revision) <= parseRevision(existing.revision)) {
                return;
            }
            this.ptys[e.ptyId] = {
                ...existing,
                pids: {
                    ...existing.pids,
                    foregroundPgid: e.pgid ?? null
                },
                revision: e.revision
            };

            const existingReport = this.processReports[e.ptyId];
            if (existingReport !== undefined) {
                this.processReports[e.ptyId] = {
                    ...existingReport,
                    pids: {
                        ...existingReport.pids,
                        foregroundPgid: e.pgid ?? null
                    },
                    foreground: e.process ?? null
                };
            }
        },

        /**
         * Trata o evento `pty://exit`, preservando o registro do PTY como tombstone
         * com `status = { kind: 'exited', exit: e.exit }`.
         */
        handleExit(e: PtyExitEvent): void {
            const existing = this.ptys[e.ptyId];
            if (existing !== undefined) {
                if (
                    isTerminalExited(existing) &&
                    parseRevision(e.revision) <= parseRevision(existing.revision)
                ) {
                    return;
                }
                this.ptys[e.ptyId] = {
                    ...existing,
                    owner: e.owner,
                    status: {
                        kind: 'exited',
                        exit: e.exit
                    },
                    seq: e.finalSeq,
                    revision: e.revision
                };
                return;
            }

            this.ptys[e.ptyId] = {
                id: e.ptyId,
                owner: e.owner,
                shell: '',
                args: [],
                cwdInitial: '',
                size: { rows: 24, cols: 80, pixelWidth: 0, pixelHeight: 0 },
                status: {
                    kind: 'exited',
                    exit: e.exit
                },
                pids: {
                    shellPid: 0,
                    pgid: 0,
                    sid: 0,
                    foregroundPgid: null
                },
                attached: false,
                createdAtMs: e.exit.atMs,
                bytesOut: '0',
                seq: e.finalSeq,
                revision: e.revision
            };
        },

        /** Trata o evento `pty://cleanup-failed`. */
        handleCleanupFailed(e: PtyCleanupFailedEvent): void {
            const existing = this.ptys[e.ptyId];
            if (existing === undefined) {
                return;
            }
            if (parseRevision(e.revision) <= parseRevision(existing.revision)) {
                return;
            }
            this.ptys[e.ptyId] = {
                ...existing,
                status: {
                    kind: 'cleanupFailed',
                    error: `Processos sobreviventes no cgroup (${e.survivors.length})`
                },
                revision: e.revision
            };
        },

        /**
         * Inicializa a store de forma idempotente: registra os 4 listeners antes de buscar o snapshot
         * inicial com `ptyList()` e reconcilia eventos concorrentes por `revision`.
         */
        async init(): Promise<void> {
            if (this.initialized) {
                return;
            }
            if (initInFlight !== null) {
                return initInFlight;
            }

            const runInit = async (): Promise<void> => {
                const localUnlisteners: UnlistenFn[] = [];
                const bufferedEvents: BufferedPtyEvent[] = [];
                let buffering = true;

                const enqueueOrApply = (evt: BufferedPtyEvent): void => {
                    if (buffering) {
                        if (bufferedEvents.length >= MAX_BUFFERED_INIT_EVENTS) {
                            bufferedEvents.shift();
                        }
                        bufferedEvents.push(evt);
                        return;
                    }
                    this.applyBufferedEvent(evt);
                };

                try {
                    localUnlisteners.push(
                        await onPtySpawned((payload) => {
                            enqueueOrApply({ type: 'spawned', payload });
                        })
                    );
                    localUnlisteners.push(
                        await onPtyExit((payload) => {
                            enqueueOrApply({ type: 'exit', payload });
                        })
                    );
                    localUnlisteners.push(
                        await onPtyForegroundChanged((payload) => {
                            enqueueOrApply({ type: 'foreground', payload });
                        })
                    );
                    localUnlisteners.push(
                        await onPtyCleanupFailed((payload) => {
                            enqueueOrApply({ type: 'cleanupFailed', payload });
                        })
                    );

                    const snapshot = await ptyList();
                    for (const item of snapshot) {
                        this.applySummaryWithRevision(item);
                    }

                    buffering = false;
                    for (const evt of bufferedEvents) {
                        this.applyBufferedEvent(evt);
                    }

                    activeUnlisteners = localUnlisteners;
                    this.initialized = true;
                } catch (err: unknown) {
                    for (const unlisten of localUnlisteners) {
                        try {
                            unlisten();
                        } catch {
                            // Ignora erro de limpeza parcial
                        }
                    }
                    throw err;
                }
            };

            initInFlight = runInit().finally(() => {
                initInFlight = null;
            });

            return initInFlight;
        },

        applyBufferedEvent(evt: BufferedPtyEvent): void {
            switch (evt.type) {
                case 'spawned':
                    this.handleSpawned(evt.payload);
                    break;
                case 'exit':
                    this.handleExit(evt.payload);
                    break;
                case 'foreground':
                    this.handleForegroundChanged(evt.payload);
                    break;
                case 'cleanupFailed':
                    this.handleCleanupFailed(evt.payload);
                    break;
            }
        },

        /** Cria um novo PTY e registra imediatamente seu `PtySummary` reconciliado por revisão. */
        async spawn(spec: SpawnSpec): Promise<PtySummary> {
            const summary = await ptySpawn(spec);
            this.applySummaryWithRevision(summary);
            return this.ptys[summary.id] ?? summary;
        },

        /** Solicita o encerramento de um PTY (`graceful` por padrão ou `force`). */
        async kill(ptyId: PtyId, mode?: KillMode): Promise<void> {
            await ptyKill(ptyId, mode);
        },

        /** Atualiza e retorna o relatório detalhado de processos (`PtyProcessReport`) do PTY. */
        async refreshProcessInfo(ptyId: PtyId): Promise<PtyProcessReport> {
            const report = await ptyProcessInfo(ptyId);
            this.processReports[ptyId] = report;
            return report;
        },

        /** Remove todos os listeners globais de eventos e marca a store como não inicializada. */
        dispose(): void {
            const toRemove = activeUnlisteners;
            activeUnlisteners = [];
            for (const unlisten of toRemove) {
                try {
                    unlisten();
                } catch {
                    // Ignora erro ao remover listener
                }
            }
            this.initialized = false;
        }
    }
});
