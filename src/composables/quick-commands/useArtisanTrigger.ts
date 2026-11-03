import {
    TRIGGER_PASSTHROUGH
} from '@/composables/quick-commands/adapters';
import {
    parseArtisanLine
} from '@/composables/quick-commands/artisanLineParser';
import {
    fuzzyMatch,
    rankCommands,
    rankOptions
} from '@/composables/quick-commands/fuzzy';
import { useQuickCommandsStore } from '@/stores/quickCommands';
import type {
    ArtisanCommand,
    ArtisanOption,
    ArtisanRoot
} from '@/types/bindings';
import type {
    CompletionItem,
    CompletionSection,
    TerminalContext,
    TriggerDefinition
} from '@/types/terminal';

const VALID_SIGNATURE_SEED_RE = /^[a-z][a-z0-9-]*(:[a-z0-9][a-z0-9-]*)*$/;

export const ARTISAN_INVOKER_HEADS = [
    './vendor/bin/sail',
    'vendor/bin/sail',
    './artisan',
    'sail',
    'php'
] as const;

export interface ArtisanItemPayload {
    kind: 'artisan-command' | 'artisan-create' | 'artisan-option' | 'artisan-arg-hint';
    root?: string | undefined;
    invoker?: string | undefined;
    name?: string | undefined;
    seed?: string | undefined;
    highlightIndices?: number[] | undefined;
    acceptValue?: boolean | null | undefined;
    isGlobal?: boolean | undefined;
    shortcut?: string | null | undefined;
    defaultValue?: unknown;
    tokenStart?: number | undefined;
    tokenEnd?: number | undefined;
}

function commandNamespace(name: string): string {
    const idx = name.indexOf(':');
    return idx === -1 ? '_global' : name.slice(0, idx);
}

function toCommandItem(
    cmd: ArtisanCommand,
    root: ArtisanRoot,
    query: string,
    includeSection: boolean
): CompletionItem {
    const match = query.trim() ? fuzzyMatch(query, cmd.name) : null;
    const ns = commandNamespace(cmd.name);
    const payload: ArtisanItemPayload = {
        kind: 'artisan-command',
        root: root.root,
        invoker: root.invoker,
        name: cmd.name,
        highlightIndices: match?.indices ?? []
    };

    return {
        id: `artisan:cmd:${cmd.name}`,
        label: cmd.name,
        description: cmd.description ?? undefined,
        detail: cmd.aliases && cmd.aliases.length > 0 ? cmd.aliases.join(', ') : undefined,
        icon: 'logos:laravel',
        section: includeSection ? ns : undefined,
        kind: 'value',
        payload
    };
}

function buildCommandSections(items: CompletionItem[]): CompletionSection[] {
    const seen = new Set<string>();
    for (const item of items) {
        if (item.section) {
            seen.add(item.section);
        }
    }
    const sorted = [...seen].sort((a, b) => {
        if (a === '_global') {
            return -1;
        }
        if (b === '_global') {
            return 1;
        }
        return a.localeCompare(b);
    });
    return sorted.map((id, idx) => ({
        id,
        label: id === '_global' ? 'Geral' : id,
        order: idx
    }));
}

function createActionItem(rootPath: string, query: string): CompletionItem {
    const payload: ArtisanItemPayload = {
        kind: 'artisan-create',
        root: rootPath,
        seed: query.trim()
    };
    return {
        id: 'artisan:create',
        label: 'Criar comando',
        description: 'Gerar novo comando Artisan (app/Console/Commands)',
        icon: 'mdi:plus-circle-outline',
        kind: 'action',
        pinned: 'bottom',
        payload
    };
}

async function resolveLaravelRootForCtx(
    ctx: TerminalContext,
    store: ReturnType<typeof useQuickCommandsStore>
): Promise<ArtisanRoot | null> {
    if ((ctx.inputMode ?? 'rich') !== 'rich' || !ctx.cwd) {
        return null;
    }
    return store.resolveRoot(ctx.cwd, ctx.projectId);
}

/**
 * Provedor de gatilho `>` para descoberta, filtro fuzzy e criação de comandos PHP Artisan.
 */
export function artisanTrigger(
    store: ReturnType<typeof useQuickCommandsStore> = useQuickCommandsStore()
): TriggerDefinition {
    return {
        id: 'artisan',
        prefix: '>',
        label: 'Comandos Artisan',
        async isAvailable(ctx): Promise<boolean> {
            const root = await resolveLaravelRootForCtx(ctx, store);
            return root !== null;
        },
        async provide(q, ctx, signal) {
            if (q.query.startsWith('>') || /^\s/.test(q.query)) {
                return TRIGGER_PASSTHROUGH;
            }
            const root = await resolveLaravelRootForCtx(ctx, store);
            if (!root || signal.aborted) {
                return TRIGGER_PASSTHROUGH;
            }

            const createItem = createActionItem(root.root, q.query);
            const catalog = store.catalogFor(root.root);

            if (!catalog) {
                const backoffError = store.catalogErrorFor(root.root);
                if (backoffError) {
                    return {
                        items: [createItem],
                        emptyMessage: backoffError,
                        stageLabel: backoffError
                    };
                }
                const pending = store.ensureCatalog(root.root).catch(() => undefined);
                return {
                    items: [createItem],
                    stageLabel: 'Carregando comandos Artisan…',
                    refreshAfter: pending
                };
            }

            const ranked = rankCommands(q.query, catalog.commands).slice(0, 200);
            const includeSections = q.query.trim().length === 0;
            const cmdItems = ranked.map((c) => toCommandItem(c, root, q.query, includeSections));
            const sections = includeSections ? buildCommandSections(cmdItems) : undefined;

            return {
                items: [...cmdItems, createItem],
                sections,
                emptyMessage: 'Nenhum comando corresponde',
                stageLabel: catalog.stale ? 'Atualizando…' : undefined
            };
        },
        onSelect(item, _q, ctx) {
            const payload = item.payload as ArtisanItemPayload | undefined;
            if (item.id === 'artisan:create' || item.kind === 'action') {
                const rootPath = payload?.root ?? '';
                const rawSeed = payload?.seed?.trim() ?? '';
                const validSeed = VALID_SIGNATURE_SEED_RE.test(rawSeed) ? rawSeed : undefined;
                if (rootPath) {
                    store.openCreateModal(rootPath, {
                        signatureName: validSeed,
                        paneId: ctx.paneId
                    });
                }
                return { kind: 'handled' };
            }

            const invoker = payload?.invoker ?? 'php artisan';
            const name = payload?.name ?? item.label;
            const text = `${invoker} ${name} `;
            return {
                kind: 'replace-input',
                text,
                cursor: text.length,
                keepOpen: true
            };
        }
    };
}

function formatOptionDetail(opt: ArtisanOption): string | undefined {
    const parts: string[] = [];
    if (opt.acceptValue === true) {
        parts.push(opt.isValueRequired ? 'aceita valor (obrigatório)' : 'aceita valor');
    }
    if (opt.default !== undefined && opt.default !== null && opt.default !== false) {
        const defStr =
            typeof opt.default === 'string' ? opt.default : JSON.stringify(opt.default);
        if (defStr && defStr !== '[]') {
            parts.push(`padrão: ${defStr}`);
        }
    }
    if (opt.shortcut) {
        const sc = opt.shortcut.startsWith('-') ? opt.shortcut : `-${opt.shortcut}`;
        parts.push(sc);
    }
    return parts.length > 0 ? parts.join(' · ') : undefined;
}

/**
 * Cria um provedor `artisan.args:<head>` para autocompletar `--flags` e exibir dicas de argumentos
 * posicionais quando a linha começa com o invocador Artisan.
 */
export function createArtisanArgsTriggerForHead(
    head: string,
    store: ReturnType<typeof useQuickCommandsStore> = useQuickCommandsStore()
): TriggerDefinition {
    return {
        id: `artisan.args:${head}`,
        prefix: head,
        label: 'Opções e Argumentos Artisan',
        async isAvailable(ctx): Promise<boolean> {
            const root = await resolveLaravelRootForCtx(ctx, store);
            return root !== null;
        },
        async provide(q, ctx, signal) {
            const root = await resolveLaravelRootForCtx(ctx, store);
            if (!root || signal.aborted) {
                return TRIGGER_PASSTHROUGH;
            }

            let catalog = store.catalogFor(root.root);
            if (!catalog && !store.isCatalogInBackoff(root.root)) {
                try {
                    catalog = await store.ensureCatalog(root.root);
                } catch {
                    return TRIGGER_PASSTHROUGH;
                }
            }
            if (!catalog || signal.aborted) {
                return TRIGGER_PASSTHROUGH;
            }

            const parsed = parseArtisanLine(q.input, q.cursor, root, (name) =>
                store.commandByName(root.root, name)
            );
            if (!parsed || !parsed.command) {
                return TRIGGER_PASSTHROUGH;
            }

            const cmd = store.commandByName(root.root, parsed.command);
            if (!cmd) {
                return TRIGGER_PASSTHROUGH;
            }

            if (parsed.mode === 'option') {
                const available = (cmd.options ?? []).filter(
                    (opt) => opt.isMultiple === true || !parsed.usedOptions.has(opt.name)
                );
                const ranked = rankOptions(parsed.current.text, available);
                if (ranked.length === 0) {
                    return TRIGGER_PASSTHROUGH;
                }

                const cleanQuery = parsed.current.text.replace(/^-+/, '');
                const items: CompletionItem[] = ranked.map((opt) => {
                    const match = cleanQuery ? fuzzyMatch(cleanQuery, opt.name) : null;
                    const payload: ArtisanItemPayload = {
                        kind: 'artisan-option',
                        root: root.root,
                        name: opt.name,
                        acceptValue: opt.acceptValue ?? null,
                        isGlobal: opt.isGlobal === true,
                        shortcut: opt.shortcut ?? null,
                        defaultValue: opt.default,
                        highlightIndices: match?.indices ?? [],
                        tokenStart: parsed.current.start,
                        tokenEnd: parsed.current.end
                    };
                    return {
                        id: `artisan:opt:${cmd.name}:${opt.name}`,
                        label: opt.name,
                        description: opt.description ?? undefined,
                        detail: formatOptionDetail(opt),
                        icon: opt.isGlobal ? 'mdi:earth' : 'mdi:flag-outline',
                        section: opt.isGlobal ? 'global' : 'command',
                        kind: 'value',
                        payload
                    };
                });

                const sections: CompletionSection[] = [
                    { id: 'command', label: 'Opções', order: 0 },
                    { id: 'global', label: 'Opções globais', order: 1 }
                ];

                return {
                    items,
                    sections,
                    stageLabel: `${cmd.name} — opções`
                };
            }

            if (parsed.mode === 'argument-hint') {
                const args = cmd.arguments ?? [];
                const targetArg =
                    parsed.positionalsBefore < args.length
                        ? args[parsed.positionalsBefore]
                        : args[args.length - 1];
                if (!targetArg) {
                    return TRIGGER_PASSTHROUGH;
                }

                const bracketLabel = targetArg.required
                    ? `<${targetArg.name}${targetArg.isArray ? '...' : ''}>`
                    : `[${targetArg.name}${targetArg.isArray ? '...' : ''}]`;
                const reqLabel = targetArg.required ? 'Obrigatório' : 'Opcional';
                const descParts = [reqLabel];
                if (targetArg.description) {
                    descParts.push(targetArg.description);
                }
                if (
                    targetArg.default !== undefined &&
                    targetArg.default !== null &&
                    targetArg.default !== ''
                ) {
                    const defText =
                        typeof targetArg.default === 'string'
                            ? targetArg.default
                            : JSON.stringify(targetArg.default);
                    if (defText && defText !== '[]') {
                        descParts.push(`padrão: ${defText}`);
                    }
                }

                const hintItem: CompletionItem = {
                    id: `artisan:arg-hint:${cmd.name}:${targetArg.name}`,
                    label: bracketLabel,
                    description: descParts.join(' · '),
                    icon: 'mdi:code-brackets',
                    kind: 'value',
                    disabled: true,
                    payload: {
                        kind: 'artisan-arg-hint',
                        name: targetArg.name
                    } satisfies ArtisanItemPayload
                };

                return {
                    items: [hintItem],
                    stageLabel: `${cmd.name} — argumento ${parsed.positionalsBefore + 1}`
                };
            }

            return TRIGGER_PASSTHROUGH;
        },
        onSelect(item, q, _ctx) {
            const payload = item.payload as ArtisanItemPayload | undefined;
            if (!payload || payload.kind !== 'artisan-option') {
                return { kind: 'handled' };
            }

            const start = payload.tokenStart ?? q.cursor;
            const end = payload.tokenEnd ?? q.cursor;
            const before = q.input.slice(0, start);
            const after = q.input.slice(end);

            // Se acceptValue === true, insere `--flag=` mantendo cursor após `=`;
            // se for flag booleana ou metadado legado desconhecido (null), insere `--flag `
            if (payload.acceptValue === true) {
                const replacement = `${ item.label }=`;
                const nextText = `${before}${replacement}${after}`;
                return {
                    kind: 'replace-input',
                    text: nextText,
                    cursor: before.length + replacement.length,
                    keepOpen: false
                };
            }

            const needsLeadingSpace = after.length > 0 && !/^\s/.test(after);
            const replacement = needsLeadingSpace ? `${item.label}` : `${item.label} `;
            const nextText = `${before}${replacement}${after}`;

            // Se o metadado veio de cache legado sem acceptValue definido, dispara revalidação em background
            if (payload.acceptValue === null && payload.root) {
                void store.ensureCatalog(payload.root, { force: true }).catch(() => {});
            }

            return {
                kind: 'replace-input',
                text: nextText,
                cursor: before.length + replacement.length,
                keepOpen: true
            };
        }
    };
}

/** Retorna todos os provedores de argumentos Artisan por primeiro token de invocador. */
export function artisanArgsTriggers(
    store: ReturnType<typeof useQuickCommandsStore> = useQuickCommandsStore()
): TriggerDefinition[] {
    return ARTISAN_INVOKER_HEADS.map((head) => createArtisanArgsTriggerForHead(head, store));
}
