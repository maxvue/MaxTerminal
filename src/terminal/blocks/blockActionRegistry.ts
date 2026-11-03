import type { IMarker, Terminal } from '@xterm/xterm';
import {
    MtTerminalError,
    type BlockAction,
    type CommandBlockModel,
    type Disposable,
    type TerminalContext
} from '@/types/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';

/** Registro de markers reais do Xterm associados a um bloco de comando (mantido fora do Pinia). */
export interface BlockMarkersRecord {
    promptMarker: IMarker | null;
    outputStartMarker: IMarker | null;
    outputEndMarker: IMarker | null;
    partialTrimmed: boolean;
}

/** Resultado da extração de texto renderizado do buffer normal de um bloco. */
export interface ExtractedBlockOutput {
    text: string;
    partial: boolean;
    available: boolean;
}

const customActions = new Map<string, BlockAction>();
let getMarkersCallback: ((paneId: string, blockId: string) => BlockMarkersRecord | undefined) | null = null;

/** Permite que a store de blocos injete o resolvedor de markers sem dependência circular. */
export function bindBlockMarkersLookup(
    fn: (paneId: string, blockId: string) => BlockMarkersRecord | undefined
): void {
    getMarkersCallback = fn;
}

function showSafeToast(title: string, severity: 'info' | 'warning' | 'error' | 'success'): void {
    void import('@maxvue/max-components-ui')
        .then((mod) => {
            mod.Toast?.show?.({ title, severity });
        })
        .catch(() => {
            // Ignora em ambientes de teste sem container de Toast montado.
        });
}

async function writeClipboardText(text: string): Promise<void> {
    if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
    }
}

/**
 * Extrai o texto renderizado ainda retido no `buffer.normal` do xterm para o bloco informado,
 * reconstruindo linhas com `isWrapped === true` sem quebras artificiais.
 */
export function extractRenderedBlockOutput(
    xterm: Terminal,
    block: CommandBlockModel,
    markers?: BlockMarkersRecord
): ExtractedBlockOutput {
    const normalBuf = xterm.buffer?.normal;
    if (!normalBuf) {
        return { text: '', partial: false, available: false };
    }

    let startLine: number | null = null;
    let endLine: number | null = null;
    let partial = Boolean(markers?.partialTrimmed);

    if (markers) {
        const startMarkerValid =
            markers.outputStartMarker !== null &&
            !markers.outputStartMarker.isDisposed &&
            markers.outputStartMarker.line >= 0;
        const endMarkerValid =
            markers.outputEndMarker !== null &&
            !markers.outputEndMarker.isDisposed &&
            markers.outputEndMarker.line >= 0;

        if (startMarkerValid) {
            startLine = markers.outputStartMarker!.line;
            if (endMarkerValid) {
                endLine = markers.outputEndMarker!.line;
            } else if (block.outputEndLine !== null) {
                endLine = block.outputEndLine;
            } else {
                endLine = normalBuf.baseY + normalBuf.cursorY;
            }
        } else if (endMarkerValid) {
            partial = true;
            startLine = 0;
            endLine = markers.outputEndMarker!.line;
        } else {
            return { text: '', partial: true, available: false };
        }
    } else {
        if (block.outputStartLine === null || block.outputStartLine < 0) {
            return { text: '', partial: false, available: false };
        }
        startLine = block.outputStartLine;
        endLine = block.outputEndLine ?? normalBuf.baseY + normalBuf.cursorY;
    }

    if (startLine === null || endLine === null || endLine < startLine) {
        return { text: '', partial, available: false };
    }

    const maxBufLine = Math.max(0, normalBuf.length - 1);
    const clampedStart = Math.max(0, Math.min(startLine, maxBufLine));
    const clampedEnd = Math.max(clampedStart, Math.min(endLine, maxBufLine));

    let result = '';
    let hasFirstLine = false;

    for (let i = clampedStart; i <= clampedEnd; i++) {
        const line = normalBuf.getLine(i);
        if (!line) {
            continue;
        }
        const content = line.translateToString(true);
        if (!hasFirstLine) {
            result = content;
            hasFirstLine = true;
        } else if (line.isWrapped) {
            result += content;
        } else {
            result += `\n${content}`;
        }
    }

    return {
        text: result,
        partial,
        available: hasFirstLine
    };
}

const builtinCopyCommandAction: BlockAction = {
    id: 'copy-command',
    label: 'Copiar comando',
    icon: 'mdi:content-copy',
    order: 10,
    when(block: CommandBlockModel): boolean {
        return block.command.trim().length > 0;
    },
    async run(block: CommandBlockModel): Promise<void> {
        await writeClipboardText(block.command);
    }
};

const builtinCopyOutputAction: BlockAction = {
    id: 'copy-output',
    label: 'Copiar saída',
    icon: 'mdi:text-box-outline',
    order: 20,
    when(block: CommandBlockModel): boolean {
        return block.status !== 'running' && block.outputStartLine !== null;
    },
    async run(block: CommandBlockModel, ctx: TerminalContext): Promise<void> {
        const instance = terminalInstanceRegistry.get(ctx.paneId);
        if (!instance) {
            showSafeToast(
                'A saída deste comando já foi descartada do histórico do terminal.',
                'warning'
            );
            return;
        }
        const markers = getMarkersCallback?.(ctx.paneId, block.id);
        const extracted = extractRenderedBlockOutput(instance.xterm, block, markers);
        if (!extracted.available) {
            showSafeToast(
                'A saída deste comando já foi descartada do histórico do terminal.',
                'warning'
            );
            return;
        }
        await writeClipboardText(extracted.text);
        if (extracted.partial) {
            showSafeToast(
                'Saída copiada parcialmente (linhas antigas atingiram o limite do buffer).',
                'info'
            );
        }
    }
};

function getAllActionsSorted(): BlockAction[] {
    const all: BlockAction[] = [
        builtinCopyCommandAction,
        builtinCopyOutputAction,
        ...customActions.values()
    ];
    return all.sort((a, b) => a.order - b.order);
}

/** Registra uma ação contextual de bloco de comando (ex.: Fases 11 e 12). */
export function registerBlockAction(action: BlockAction): Disposable {
    customActions.set(action.id, action);
    return {
        dispose(): void {
            if (customActions.get(action.id) === action) {
                customActions.delete(action.id);
            }
        }
    };
}

/** Retorna as ações aplicáveis ao bloco informado ordenadas por `order asc`. */
export function getBlockActionsFor(block: CommandBlockModel): BlockAction[] {
    return getAllActionsSorted().filter((action) => {
        try {
            return action.when(block);
        } catch {
            return false;
        }
    });
}

/** Executa uma ação de bloco capturando falhas com `MtTerminalError('block_action.failed')` e Toast. */
export async function executeBlockAction(
    actionId: string,
    block: CommandBlockModel,
    ctx: TerminalContext
): Promise<void> {
    const target = getAllActionsSorted().find((a) => a.id === actionId);
    if (!target) {
        return;
    }
    try {
        await target.run(block, ctx);
    } catch (err: unknown) {
        const message =
            err instanceof Error ? err.message : 'Não foi possível executar a ação do bloco.';
        const wrapped = new MtTerminalError('block_action.failed', message, err);
        showSafeToast(wrapped.message, 'error');
        throw wrapped;
    }
}

/** Restaura apenas as ações nativas (exclusivo para testes). */
export function resetBlockActionsForTests(): void {
    customActions.clear();
}
