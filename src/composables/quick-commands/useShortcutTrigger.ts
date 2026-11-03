import {
    getQuickCommandsAdapters,
    TRIGGER_PASSTHROUGH
} from '@/composables/quick-commands/adapters';
import { fuzzyMatch } from '@/composables/quick-commands/fuzzy';
import { quickCommandsIpc } from '@/ipc/quickCommands';
import { useQuickCommandsStore } from '@/stores/quickCommands';
import type {
    DetectedManager,
    ResolvedShortcut,
    ShortcutId
} from '@/types/bindings';
import type {
    CompletionItem,
    CompletionSection,
    TerminalContext,
    TriggerDefinition,
    TriggerSelectOutcome
} from '@/types/terminal';

const ALL_SHORTCUT_LABELS = [
    '/install',
    '/install-force',
    '/update',
    '/update-force',
    '/build',
    '/deploy',
    '/test'
] as const;

const SHORTCUT_ICONS: Record<ShortcutId, string> = {
    install: 'mdi:package-down',
    'install-force': 'mdi:package-down',
    update: 'mdi:update',
    'update-force': 'mdi:update',
    build: 'mdi:hammer',
    deploy: 'mdi:rocket-launch',
    test: 'mdi:test-tube'
};

export interface ShortcutItemPayload {
    kind: 'shortcut' | 'shortcut-warning';
    shortcut?: ResolvedShortcut | undefined;
    highlightIndices?: number[] | undefined;
}

function formatManagersDetail(
    shortcut: ResolvedShortcut,
    managers: readonly DetectedManager[]
): string | undefined {
    const partManagers = shortcut.parts
        .map((p) => p.manager)
        .filter((m): m is NonNullable<typeof m> => m !== null && m !== undefined);
    if (partManagers.length > 0) {
        return partManagers.join(' + ');
    }
    if (shortcut.parts.some((p) => p.source === 'max_code_config')) {
        return 'MaxCode config';
    }
    if (managers.length > 0) {
        return managers.map((m) => m.manager).join(' + ');
    }
    return undefined;
}

async function resolveAndConfirmShortcut(
    shortcut: ResolvedShortcut,
    ctx: TerminalContext,
    store: ReturnType<typeof useQuickCommandsStore>
): Promise<TriggerSelectOutcome> {
    if (!ctx.cwd) {
        return { kind: 'close' };
    }
    const adapters = getQuickCommandsAdapters();

    try {
        const fresh = await quickCommandsIpc.shortcutResolve(ctx.cwd, shortcut.id);
        if (fresh.evidenceRevision !== shortcut.evidenceRevision || !fresh.line) {
            await store.loadShortcuts(ctx.cwd, { force: true }).catch(() => {});
            adapters.notifyToast({
                title: 'Os manifestos do projeto mudaram',
                description: 'O plano do atalho foi atualizado; revise e selecione novamente.',
                severity: 'info'
            });
            return { kind: 'handled' };
        }

        if (fresh.requiresConfirmation) {
            const confirmed = await store.requestConfirm(
                `Confirmar ${fresh.label}?`,
                fresh.line,
                ctx.paneId
            );
            if (!confirmed) {
                return { kind: 'close' };
            }
        }

        return {
            kind: 'replace-input',
            text: fresh.line,
            cursor: fresh.line.length
        };
    } catch {
        await store.loadShortcuts(ctx.cwd, { force: true }).catch(() => {});
        return { kind: 'handled' };
    }
}

/**
 * Provedor de gatilho `/` para os 7 Atalhos Universais de Projeto (`AGENTS.md` §8.3).
 */
export function shortcutTrigger(
    store: ReturnType<typeof useQuickCommandsStore> = useQuickCommandsStore()
): TriggerDefinition {
    return {
        id: 'shortcuts',
        prefix: '/',
        label: 'Atalhos do projeto',
        isAvailable(ctx): boolean {
            return (ctx.inputMode ?? 'rich') === 'rich' && Boolean(ctx.cwd);
        },
        async provide(q, ctx, signal) {
            const rawQuery = q.query;
            // Caminho absoluto Unix (`/usr/bin/env`, `/etc/hosts`) ou argumento com espaço -> passthrough
            if (rawQuery.includes('/') || /\s/.test(rawQuery)) {
                return TRIGGER_PASSTHROUGH;
            }

            const candidatePrefix = `/${rawQuery.toLowerCase()}`;
            const matchingLabels = ALL_SHORTCUT_LABELS.filter((lbl) =>
                lbl.startsWith(candidatePrefix)
            );
            if (matchingLabels.length === 0 || !ctx.cwd) {
                return TRIGGER_PASSTHROUGH;
            }

            try {
                const list = await store.loadShortcuts(ctx.cwd);
                if (signal.aborted) {
                    return TRIGGER_PASSTHROUGH;
                }

                const filtered = list.items.filter((item) =>
                    item.label.toLowerCase().startsWith(candidatePrefix)
                );
                if (filtered.length === 0) {
                    return TRIGGER_PASSTHROUGH;
                }

                const items: CompletionItem[] = filtered.map((sc) => {
                    const match = fuzzyMatch(`/${rawQuery}`, sc.label);
                    const payload: ShortcutItemPayload = {
                        kind: 'shortcut',
                        shortcut: sc,
                        highlightIndices: match?.indices ?? []
                    };
                    return {
                        id: `shortcut:${sc.id}`,
                        label: sc.label,
                        description: sc.line ?? sc.disabledReason ?? sc.description,
                        detail: formatManagersDetail(sc, list.managers),
                        icon: SHORTCUT_ICONS[sc.id],
                        section: 'shortcuts',
                        kind: 'value',
                        disabled: sc.line === null,
                        payload
                    };
                });

                const sections: CompletionSection[] = [
                    { id: 'shortcuts', label: 'Atalhos do projeto', order: 0 }
                ];

                if (list.warnings.length > 0) {
                    sections.push({ id: 'warnings', label: 'Avisos', order: 99 });
                    list.warnings.forEach((warn, idx) => {
                        items.push({
                            id: `shortcut:warn:${idx}`,
                            label: warn,
                            icon: 'mdi:alert-outline',
                            section: 'warnings',
                            kind: 'value',
                            disabled: true,
                            payload: {
                                kind: 'shortcut-warning'
                            } satisfies ShortcutItemPayload
                        });
                    });
                }

                return {
                    items,
                    sections
                };
            } catch {
                return TRIGGER_PASSTHROUGH;
            }
        },
        async onSelect(item, _q, ctx) {
            const payload = item.payload as ShortcutItemPayload | undefined;
            if (!payload || payload.kind !== 'shortcut' || !payload.shortcut) {
                return { kind: 'handled' };
            }
            if (payload.shortcut.line === null) {
                return { kind: 'handled' };
            }
            return resolveAndConfirmShortcut(payload.shortcut, ctx, store);
        },
        async onSubmitWithoutSelection(q, ctx) {
            const trimmed = q.input.trim().toLowerCase();
            if (!ctx.cwd || !ALL_SHORTCUT_LABELS.includes(trimmed as (typeof ALL_SHORTCUT_LABELS)[number])) {
                return { kind: 'submit', text: q.input };
            }
            try {
                const list = await store.loadShortcuts(ctx.cwd);
                const found = list.items.find((i) => i.label.toLowerCase() === trimmed);
                if (!found || found.line === null) {
                    return { kind: 'handled' };
                }
                return resolveAndConfirmShortcut(found, ctx, store);
            } catch {
                return { kind: 'submit', text: q.input };
            }
        }
    };
}
