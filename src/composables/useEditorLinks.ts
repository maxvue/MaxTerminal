import type { IBufferRange, ILink, ILinkProvider, Terminal } from '@xterm/xterm';
import { getCurrentScope, onScopeDispose } from 'vue';
import { LinkResolutionCache } from '@/composables/editorLinks/linkCache';
import { parseLinkCandidates, type ParsedLink } from '@/composables/editorLinks/parseLinkCandidates';
import { osIpc } from '@/ipc';
import { hasActivationModifier } from '@/terminal/links/linkHandlerRegistry';
import type {
    LinkCandidate_Deserialize,
    LinkContext,
    ResolvedLink_Serialize
} from '@/types/bindings';
import type { Disposable } from '@/types/terminal';

export interface UseEditorLinksOptions {
    /** Provedor do `LinkContext` atual do painel (`tabId`, `paneId`, `ptyId`, `cwdEpoch`). */
    getContext: () => LinkContext | null;
    /** Cache compartilhado opcional (cria instância padrão de 512 itens / 5s se omitido). */
    cache?: LinkResolutionCache;
    /** Callback opcional chamado quando `os_link_open` falha. */
    onOpenError?: (error: unknown, candidate: LinkCandidate_Deserialize) => void;
    /** Se `true`, permite clique simples sem modificador `Ctrl`/`Meta` (default `false`). */
    allowUnmodifiedClick?: boolean;
}

export interface UseEditorLinksReturn extends Disposable {
    /** Resolve manualmente os links presentes em uma linha textual. */
    resolveLine: (lineText: string) => Promise<Array<{ parsed: ParsedLink; resolved: ResolvedLink_Serialize }>>;
    /** Aciona a abertura de um candidato de link no editor configurado. */
    openCandidate: (candidate: LinkCandidate_Deserialize, event?: MouseEvent) => Promise<boolean>;
    /** Cache LRU em uso. */
    cache: LinkResolutionCache;
}

function toCandidateDto(parsed: ParsedLink): LinkCandidate_Deserialize {
    return {
        key: `${parsed.startCol}:${parsed.endCol}:${parsed.path}`,
        path: parsed.path,
        line: parsed.line ?? null,
        column: parsed.column ?? null
    };
}

function buildBufferRange(bufferLineNumber: number, parsed: ParsedLink): IBufferRange {
    return {
        start: {
            x: parsed.startCol,
            y: bufferLineNumber
        },
        end: {
            x: parsed.endCol,
            y: bufferLineNumber
        }
    };
}

/**
 * Registra o `ILinkProvider` de arquivos locais (`caminho:linha:coluna`) em uma instância `@xterm/xterm`,
 * validando a existência dos arquivos via `os_link_resolve` com cache LRU e abrindo-os no editor via `os_link_open`.
 */
export function useEditorLinks(
    terminal: Pick<Terminal, 'buffer' | 'registerLinkProvider'> | null,
    options: UseEditorLinksOptions
): UseEditorLinksReturn {
    const cache = options.cache ?? new LinkResolutionCache(512, 5_000);
    let providerDisposable: { dispose(): void } | null = null;
    let unlistenContext: (() => void) | null = null;
    let disposed = false;

    void osIpc
        .onLinkContextChanged((ev) => {
            if (disposed) {
                return;
            }
            cache.invalidatePane(ev.payload.paneId);
        })
        .then((unlisten) => {
            if (disposed) {
                unlisten();
            } else {
                unlistenContext = unlisten;
            }
        })
        .catch(() => {
            // Ambiente de teste sem mock de eventos Tauri.
        });

    async function resolveLine(
        lineText: string
    ): Promise<Array<{ parsed: ParsedLink; resolved: ResolvedLink_Serialize }>> {
        const ctx = options.getContext();
        if (!ctx || disposed) {
            return [];
        }
        const parsedList = parseLinkCandidates(lineText);
        if (parsedList.length === 0) {
            return [];
        }

        const results: Array<{ parsed: ParsedLink; resolved: ResolvedLink_Serialize } | null> =
            new Array(parsedList.length).fill(null);
        const missingEntries: Array<{ idx: number; parsed: ParsedLink; dto: LinkCandidate_Deserialize }> = [];

        for (let i = 0; i < parsedList.length; i += 1) {
            const parsed = parsedList[i];
            if (!parsed) {
                continue;
            }
            const dto = toCandidateDto(parsed);
            const cached = cache.lookup(ctx, dto);
            if (cached.hit) {
                if (cached.value) {
                    results[i] = { parsed, resolved: cached.value };
                }
            } else {
                missingEntries.push({ idx: i, parsed, dto });
            }
        }

        if (missingEntries.length > 0) {
            try {
                const resolvedBatch = await osIpc.resolveLinks(
                    ctx,
                    missingEntries.map((m) => m.dto)
                );
                const byKey = new Map<string, ResolvedLink_Serialize>();
                for (const item of resolvedBatch) {
                    byKey.set(item.key, item);
                }
                for (const entry of missingEntries) {
                    const resolvedItem = byKey.get(entry.dto.key) ?? null;
                    cache.set(ctx, entry.dto, resolvedItem);
                    if (resolvedItem) {
                        results[entry.idx] = { parsed: entry.parsed, resolved: resolvedItem };
                    }
                }
            } catch {
                // Em caso de contexto stale ou falha transitória, não sublinha os candidatos pendentes.
            }
        }

        return results.filter(
            (item): item is { parsed: ParsedLink; resolved: ResolvedLink_Serialize } =>
                item !== null
        );
    }

    async function openCandidate(
        candidate: LinkCandidate_Deserialize,
        event?: MouseEvent
    ): Promise<boolean> {
        if (disposed) {
            return false;
        }
        if (event && !options.allowUnmodifiedClick && !hasActivationModifier(event)) {
            return false;
        }
        const ctx = options.getContext();
        if (!ctx) {
            return false;
        }
        try {
            await osIpc.openLink(ctx, candidate);
            return true;
        } catch (err) {
            options.onOpenError?.(err, candidate);
            return false;
        }
    }

    if (terminal && typeof terminal.registerLinkProvider === 'function') {
        const provider: ILinkProvider = {
            provideLinks(bufferLineNumber: number, callback: (links: ILink[] | undefined) => void): void {
                if (disposed) {
                    callback(undefined);
                    return;
                }
                const line = terminal.buffer.active.getLine(bufferLineNumber - 1);
                if (!line) {
                    callback(undefined);
                    return;
                }
                const lineText = line.translateToString(true);
                if (!lineText || lineText.trim().length === 0) {
                    callback(undefined);
                    return;
                }

                void resolveLine(lineText).then((validItems) => {
                    if (disposed || validItems.length === 0) {
                        callback(undefined);
                        return;
                    }
                    const links: ILink[] = validItems.map(({ parsed }) => {
                        const dto = toCandidateDto(parsed);
                        return {
                            range: buildBufferRange(bufferLineNumber, parsed),
                            text: parsed.raw,
                            decorations: {
                                pointerCursor: true,
                                underline: true
                            },
                            activate(event: MouseEvent): void {
                                void openCandidate(dto, event);
                            }
                        };
                    });
                    callback(links);
                });
            }
        };

        providerDisposable = terminal.registerLinkProvider(provider);
    }

    function dispose(): void {
        if (disposed) {
            return;
        }
        disposed = true;
        providerDisposable?.dispose();
        providerDisposable = null;
        unlistenContext?.();
        unlistenContext = null;
    }

    if (getCurrentScope()) {
        onScopeDispose(() => {
            dispose();
        });
    }

    return {
        resolveLine,
        openCandidate,
        cache,
        dispose
    };
}
