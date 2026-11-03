import { getCurrentScope, onScopeDispose } from 'vue';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
    commandBlockList,
    onCommandFinished,
    onCommandStarted
} from '@/ipc/shell';
import { useCommandBlocksStore } from '@/stores/commandBlocks';
import { useShellHistoryStore } from '@/stores/shellHistory';
import { useShellMarkers } from '@/composables/useShellMarkers';
import type {
    BlockStatus,
    CommandBlockDto_Serialize,
    CommandFinished_Serialize,
    CommandStarted_Serialize,
    PaneId,
    PtyId,
    SessionId
} from '@/types/bindings';

/** Rótulo exibido quando o texto do comando não pôde ser capturado pelo shell. */
export const UNAVAILABLE_COMMAND_LABEL = 'Comando indisponível';

/** Opções de vinculação do adaptador `useShellCommandBlocks`. */
export interface UseShellCommandBlocksOptions {
    paneId?: PaneId | null | undefined;
    ptyId?: PtyId | null | undefined;
    sessionId?: SessionId | null | undefined;
}

/**
 * Converte o status e código de saída autoritativos do backend (`BlockStatus`) para o `exitCode`
 * consumido pelo `useCommandBlocksStore().feed.end(...)` da Fase 04:
 * - `succeeded` -> `0` (`status: 'success'`)
 * - `failed` (ou `terminated_by_signal` / `pty_exited` com código != 0) -> código não-zero (`status: 'error'`)
 * - `cancelled` / `interrupted_by_restart` / desconhecido -> `null` (`status: 'aborted'`)
 */
export function mapBackendResultToFeedExitCode(
    status: BlockStatus,
    exitCode: number | null
): number | null {
    if (status === 'succeeded') {
        return 0;
    }
    if (status === 'failed') {
        return exitCode !== null && exitCode !== 0 ? exitCode : 1;
    }
    if (
        (status === 'terminated_by_signal' || status === 'pty_exited') &&
        exitCode !== null &&
        exitCode !== 0
    ) {
        return exitCode;
    }
    return null;
}

/**
 * Composable que conecta os eventos `shell://command-started` e `shell://command-finished`
 * e a hidratação de blocos ao `useCommandBlocksStore().feed` existente da Fase 04,
 * sem criar store paralela e deduplicando por `CommandBlockId`.
 */
export function useShellCommandBlocks(options: UseShellCommandBlocksOptions = {}) {
    const commandBlocksStore = useCommandBlocksStore();
    const shellHistoryStore = useShellHistoryStore();
    const markers = useShellMarkers();

    const seenStartedKeys = new Set<string>();
    const seenFinishedKeys = new Set<string>();
    const cancelFlushCallbacks: Array<() => void> = [];
    let unlistenStarted: UnlistenFn | null = null;
    let unlistenFinished: UnlistenFn | null = null;
    let disposed = false;

    function resolveTargetPaneId(bindingPaneId: PaneId | null, ptyId: PtyId): PaneId {
        if (bindingPaneId) {
            return bindingPaneId;
        }
        if (options.paneId) {
            return options.paneId;
        }
        return ptyId;
    }

    function matchesFilter(bindingPaneId: PaneId | null, ptyId: PtyId, sessionId: SessionId): boolean {
        if (options.ptyId && ptyId !== options.ptyId) {
            return false;
        }
        if (options.sessionId && sessionId !== options.sessionId) {
            return false;
        }
        if (options.paneId && bindingPaneId && bindingPaneId !== options.paneId) {
            return false;
        }
        return true;
    }

    /** Alimenta o `feed` da Fase 04 a partir de um evento `shell://command-started`. */
    function applyStarted(ev: CommandStarted_Serialize): void {
        if (
            disposed ||
            !matchesFilter(ev.binding.paneId, ev.binding.ptyId, ev.binding.sessionId)
        ) {
            return;
        }

        const dedupKey = `${ev.binding.ptyId}:${ev.execution.blockId}:${ev.execution.commandSeq}`;
        if (seenStartedKeys.has(dedupKey)) {
            return;
        }
        seenStartedKeys.add(dedupKey);

        shellHistoryStore.handleCommandStarted(ev);

        const paneId = resolveTargetPaneId(ev.binding.paneId, ev.binding.ptyId);
        const rawCmd = ev.commandText ?? ev.execution.commandPreview;
        const commandLabel = rawCmd && rawCmd.trim().length > 0 ? rawCmd : UNAVAILABLE_COMMAND_LABEL;
        const anchor = markers.resolveAnchor(paneId);

        commandBlocksStore.feed.begin(paneId, {
            id: ev.execution.blockId,
            command: commandLabel,
            cwd: ev.cwd ?? null,
            startedAt: ev.execution.startedAt,
            line: anchor.line
        });

        commandBlocksStore.feed.markOutputStart(paneId, ev.execution.blockId, anchor.line);

        commandBlocksStore.feed.patchMeta(paneId, ev.execution.blockId, {
            ptyId: ev.binding.ptyId,
            sessionId: ev.binding.sessionId,
            tabId: ev.binding.tabId,
            commandSeq: ev.execution.commandSeq,
            commandTextQuality: ev.commandTextQuality,
            backendStatus: 'running',
            source: ev.execution.source,
            confidence: ev.confidence,
            persistence: ev.persistence,
            positionQuality: anchor.positionQuality,
            inspectable: false,
            isContinuous: false,
            outputTruncated: false
        });

        const cancelFlush = markers.runAfterFlush(paneId, (postFlushAnchor) => {
            commandBlocksStore.feed.patchMeta(paneId, ev.execution.blockId, {
                positionQuality: postFlushAnchor.positionQuality
            });
        });
        cancelFlushCallbacks.push(cancelFlush);
    }

    /** Alimenta o `feed` da Fase 04 a partir de um evento `shell://command-finished`. */
    function applyFinished(ev: CommandFinished_Serialize): void {
        if (
            disposed ||
            !matchesFilter(ev.binding.paneId, ev.binding.ptyId, ev.binding.sessionId)
        ) {
            return;
        }

        const dedupKey = `${ev.binding.ptyId}:${ev.blockId}:${ev.commandSeq}`;
        if (seenFinishedKeys.has(dedupKey)) {
            return;
        }
        seenFinishedKeys.add(dedupKey);

        shellHistoryStore.handleCommandFinished(ev);

        const paneId = resolveTargetPaneId(ev.binding.paneId, ev.binding.ptyId);
        const rawCmd = ev.commandText;
        const commandLabel = rawCmd && rawCmd.trim().length > 0 ? rawCmd : UNAVAILABLE_COMMAND_LABEL;
        const anchor = markers.resolveAnchor(paneId);

        // Garante que o bloco exista no feed caso o evento started não tenha sido visto antes
        const existing = commandBlocksStore.byPane[paneId]?.blocks[ev.blockId];
        if (!existing) {
            commandBlocksStore.feed.begin(paneId, {
                id: ev.blockId,
                command: commandLabel,
                cwd: ev.cwd ?? null,
                startedAt: ev.startedAt,
                line: anchor.line
            });
            commandBlocksStore.feed.markOutputStart(paneId, ev.blockId, anchor.line);
        } else if (
            existing.command === UNAVAILABLE_COMMAND_LABEL &&
            rawCmd &&
            rawCmd.trim().length > 0
        ) {
            existing.command = rawCmd;
        }

        const feedExitCode = mapBackendResultToFeedExitCode(ev.status, ev.exitCode);

        commandBlocksStore.feed.end(paneId, ev.blockId, {
            exitCode: feedExitCode,
            finishedAt: ev.endedAt,
            line: anchor.line
        });

        commandBlocksStore.feed.patchMeta(paneId, ev.blockId, {
            ptyId: ev.binding.ptyId,
            sessionId: ev.binding.sessionId,
            tabId: ev.binding.tabId,
            commandSeq: ev.commandSeq,
            commandTextQuality: ev.commandTextQuality,
            backendStatus: ev.status,
            endReason: ev.endReason,
            rawExitCode: ev.exitCode,
            signal: ev.signal,
            durationMs: ev.durationMs,
            outputBytes: ev.outputBytes,
            outputLines: ev.outputLines,
            outputPreview: ev.outputPreview,
            errorExcerpt: ev.errorExcerpt,
            inspectable: ev.inspectable,
            source: ev.source,
            confidence: ev.confidence,
            persistence: ev.persistence,
            positionQuality: anchor.positionQuality,
            outputTruncated: false
        });
    }

    /** Projeta um `CommandBlockDto` persistido no `feed` da Fase 04 (usado na hidratação inicial). */
    function applyDto(dto: CommandBlockDto_Serialize): void {
        if (disposed || !matchesFilter(dto.paneId, dto.ptyId, dto.sessionId)) {
            return;
        }

        shellHistoryStore.upsertBlock(dto);
        const paneId = resolveTargetPaneId(dto.paneId, dto.ptyId);
        const rawCmd = dto.commandText;
        const commandLabel = rawCmd && rawCmd.trim().length > 0 ? rawCmd : UNAVAILABLE_COMMAND_LABEL;
        const anchor = markers.resolveAnchor(paneId);

        commandBlocksStore.feed.begin(paneId, {
            id: dto.id,
            command: commandLabel,
            cwd: dto.cwd ?? null,
            startedAt: dto.startedAt,
            line: anchor.line
        });
        commandBlocksStore.feed.markOutputStart(paneId, dto.id, anchor.line);

        if (dto.status !== 'running') {
            const feedExitCode = mapBackendResultToFeedExitCode(dto.status, dto.exitCode);
            commandBlocksStore.feed.end(paneId, dto.id, {
                exitCode: feedExitCode,
                finishedAt: dto.endedAt ?? dto.startedAt,
                line: anchor.line
            });
        }

        commandBlocksStore.feed.patchMeta(paneId, dto.id, {
            ptyId: dto.ptyId,
            sessionId: dto.sessionId,
            tabId: dto.tabId,
            commandSeq: dto.commandSeq,
            commandTextQuality: dto.commandTextQuality,
            backendStatus: dto.status,
            endReason: dto.endReason,
            rawExitCode: dto.exitCode,
            signal: dto.signal,
            durationMs: dto.durationMs,
            outputBytes: dto.outputBytes,
            outputLines: dto.outputLines,
            errorExcerpt: dto.errorExcerpt,
            inspectable: dto.inspectable,
            isContinuous: dto.isContinuous,
            source: dto.source,
            confidence: dto.confidence,
            persistence: dto.persistence,
            positionQuality: anchor.positionQuality,
            outputTruncated:
                dto.outputKind === 'head_tail_truncated' || dto.outputKind === 'tail_only'
        });
    }

    /**
     * Assina os eventos de comandos antes de hidratar os blocos existentes do backend.
     */
    async function start(): Promise<void> {
        if (disposed || (unlistenStarted && unlistenFinished)) {
            return;
        }

        unlistenStarted = await onCommandStarted((ev) => {
            applyStarted(ev);
        });
        unlistenFinished = await onCommandFinished((ev) => {
            applyFinished(ev);
        });

        if (options.ptyId || options.sessionId) {
            const page = await commandBlockList({
                ptyId: options.ptyId ?? null,
                sessionId: options.sessionId ?? null,
                limit: 50
            });
            const ordered = [...page.items].sort((a, b) => a.commandSeq - b.commandSeq);
            for (const item of ordered) {
                applyDto(item);
            }
        }
    }

    function stop(): void {
        disposed = true;
        while (cancelFlushCallbacks.length > 0) {
            cancelFlushCallbacks.pop()?.();
        }
        if (unlistenStarted) {
            unlistenStarted();
            unlistenStarted = null;
        }
        if (unlistenFinished) {
            unlistenFinished();
            unlistenFinished = null;
        }
    }

    if (getCurrentScope()) {
        onScopeDispose(() => {
            stop();
        });
    }

    return {
        start,
        stop,
        applyStarted,
        applyFinished,
        applyDto
    };
}
