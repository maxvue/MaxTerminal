import { createGroupRequestId } from '@/composables/commandGroups/useCommandGroupRun';
import { useCommandGroupsStore } from '@/stores/commandGroups';
import { triggerRegistry, type TriggerRegistry } from '@/terminal/triggers/triggerRegistry';
import type {
    CommandGroup,
    ExecutionMode,
    GroupTriggerSecondaryAction
} from '@/types/commandGroups';
import type {
    CompletionItem,
    CompletionResult,
    CompletionSection,
    Disposable,
    InputMode,
    TerminalContext,
    TriggerDefinition,
    TriggerQuery,
    TriggerSelectOutcome
} from '@/types/terminal';

export const COMMAND_GROUPS_TRIGGER_ID = 'command-groups' as const;
export const COMMAND_GROUPS_PREFIX = '>>' as const;

export interface CommandGroupStageState {
    group: CommandGroup;
}

export interface CommandGroupItemPayload {
    kind: 'group' | 'create-local' | 'create-global' | 'secondary-action';
    group?: CommandGroup | undefined;
    secondaryAction?: GroupTriggerSecondaryAction | undefined;
    seedName?: string | undefined;
}

export interface CommandGroupsTriggerMatch {
    prefix: typeof COMMAND_GROUPS_PREFIX;
    query: string;
}

const MODE_LABELS_PT_BR: Record<ExecutionMode, string> = {
    sequential: 'Sequencial',
    parallel: 'Paralelo',
    multi_tabs: 'Multi-Abas'
};

const GROUP_SECTIONS: CompletionSection[] = [
    { id: 'local', label: 'Deste projeto', order: 0 },
    { id: 'global', label: 'Globais', order: 1 }
];

function normalizeNfcLower(value: string): string {
    return value.normalize('NFC').toLowerCase();
}

/**
 * Avalia se um buffer de texto e modo de entrada correspondem ao gatilho `>>` (`CA-12`).
 * - Somente ativo em `inputMode === 'rich'`.
 * - Rejeita espaço inicial (`" >>"`) e escape com barra invertida (`"\\>>"`).
 * - Exige que os dois primeiros caracteres sejam `">>"`.
 * - Tudo após os dois primeiros caracteres `">>"` constitui a `query` bruta (`">>>dev"` → `">dev"`).
 */
export function matchCommandGroupsTrigger(
    input: string,
    inputMode: InputMode = 'rich'
): CommandGroupsTriggerMatch | null {
    if (inputMode !== 'rich') {
        return null;
    }
    if (input.startsWith('\\') || input.startsWith(' ') || input.startsWith('\t')) {
        return null;
    }
    if (!input.startsWith(COMMAND_GROUPS_PREFIX)) {
        return null;
    }
    return {
        prefix: COMMAND_GROUPS_PREFIX,
        query: input.slice(COMMAND_GROUPS_PREFIX.length)
    };
}

function scoreSubsequence(normText: string, normQuery: string): number | null {
    if (normQuery.length === 0) {
        return 0;
    }
    if (normQuery.length > normText.length) {
        return null;
    }

    let score = 0;
    let textIdx = 0;
    let prevMatchIdx = -2;

    if (normText === normQuery) {
        score += 180;
    } else if (normText.startsWith(normQuery)) {
        score += 120;
    } else if (normText.includes(normQuery)) {
        score += 60;
    }

    for (let i = 0; i < normQuery.length; i++) {
        const ch = normQuery[i]!;
        const found = normText.indexOf(ch, textIdx);
        if (found === -1) {
            return null;
        }
        score += 10;
        if (found === 0) {
            score += 30;
        } else {
            const prevChar = normText[found - 1]!;
            if (prevChar === ' ' || prevChar === '-' || prevChar === '_' || prevChar === ':') {
                score += 20;
            }
        }
        if (found === prevMatchIdx + 1) {
            score += 15;
        } else if (prevMatchIdx >= 0) {
            score -= Math.min(10, found - prevMatchIdx);
        }
        prevMatchIdx = found;
        textIdx = found + 1;
    }

    score -= Math.min(15, normText.length - normQuery.length);
    return score;
}

/**
 * Calcula a pontuação fuzzy de um `CommandGroup` sobre `name`, `description` e `labels`
 * (normalizados Unicode NFC + lowercase), concedendo bônus para match de prefixo no `name` (`CA-13`, `CA-25`).
 */
export function scoreCommandGroup(group: CommandGroup, rawQuery: string): number | null {
    const trimmed = rawQuery.trim();
    if (trimmed.length === 0) {
        return 0;
    }

    const normQuery = normalizeNfcLower(trimmed);
    const normName = normalizeNfcLower(group.name);
    const normDesc = normalizeNfcLower(group.description ?? '');

    let bestScore: number | null = null;

    const nameScore = scoreSubsequence(normName, normQuery);
    if (nameScore !== null) {
        const prefixBonus = normName.startsWith(normQuery) ? 150 : 0;
        bestScore = nameScore + 80 + prefixBonus;
    }

    for (const label of group.labels ?? []) {
        const normLabel = normalizeNfcLower(label);
        const labelScore = scoreSubsequence(normLabel, normQuery);
        if (labelScore !== null) {
            const candidate = labelScore + (normLabel.startsWith(normQuery) ? 90 : 50);
            if (bestScore === null || candidate > bestScore) {
                bestScore = candidate;
            }
        }
    }

    if (normDesc.length > 0) {
        const descScore = scoreSubsequence(normDesc, normQuery);
        if (descScore !== null) {
            const candidate = descScore + 20;
            if (bestScore === null || candidate > bestScore) {
                bestScore = candidate;
            }
        }
    }

    return bestScore;
}

function compareGroupTieBreak(a: CommandGroup, b: CommandGroup): number {
    if (a.sortOrder !== b.sortOrder) {
        return a.sortOrder - b.sortOrder;
    }
    const nameCmp = a.name.localeCompare(b.name);
    if (nameCmp !== 0) {
        return nameCmp;
    }
    return a.id.localeCompare(b.id);
}

function filterAndOrderSingleScope(
    groups: readonly CommandGroup[],
    rawQuery: string
): CommandGroup[] {
    const trimmed = rawQuery.trim();
    if (trimmed.length === 0) {
        return [...groups].sort(compareGroupTieBreak);
    }

    const scored: Array<{ group: CommandGroup; score: number }> = [];
    for (const group of groups) {
        const score = scoreCommandGroup(group, trimmed);
        if (score !== null) {
            scored.push({ group, score });
        }
    }

    scored.sort((a, b) => {
        if (b.score !== a.score) {
            return b.score - a.score;
        }
        return compareGroupTieBreak(a.group, b.group);
    });

    return scored.map((entry) => entry.group);
}

/**
 * Filtra e ordena separadamente os grupos locais e globais, sem nunca misturar escopos por score (`CA-13`).
 */
export function rankAndFilterGroupsByScope(
    localGroups: readonly CommandGroup[],
    globalGroups: readonly CommandGroup[],
    rawQuery: string
): { local: CommandGroup[]; global: CommandGroup[] } {
    return {
        local: filterAndOrderSingleScope(localGroups, rawQuery),
        global: filterAndOrderSingleScope(globalGroups, rawQuery)
    };
}

function toGroupCompletionItem(group: CommandGroup): CompletionItem {
    const labelChips =
        group.labels && group.labels.length > 0
            ? group.labels.map((l) => `#${l}`).join(' ')
            : '';
    const descParts: string[] = [];
    if (group.description && group.description.trim().length > 0) {
        descParts.push(group.description.trim());
    }
    if (labelChips) {
        descParts.push(labelChips);
    }

    return {
        id: `group:${group.scope}:${group.id}`,
        label: group.name,
        description: descParts.length > 0 ? descParts.join(' · ') : undefined,
        detail: `${MODE_LABELS_PT_BR[group.mode]} · ${group.items.length} item(ns)`,
        icon: group.scope === 'local' ? 'lucide:layers' : 'lucide:globe',
        section: group.scope === 'local' ? 'local' : 'global',
        kind: 'value',
        payload: {
            kind: 'group',
            group
        } satisfies CommandGroupItemPayload
    };
}

function buildPinnedCreateActions(
    projectId: string | null | undefined,
    rawQuery: string
): [CompletionItem, CompletionItem] {
    const seedName = rawQuery.trim();
    const hasProject = Boolean(projectId);

    const createLocal: CompletionItem = {
        id: 'action:create-local',
        label: 'Criar grupo de comandos local',
        description: hasProject
            ? 'Cria um novo grupo de comandos exclusivo deste projeto'
            : 'Selecione um projeto para criar um grupo local',
        icon: 'lucide:folder-plus',
        kind: 'action',
        pinned: 'bottom',
        disabled: !hasProject,
        disabledReason: !hasProject
            ? 'Selecione um projeto para criar um grupo local'
            : undefined,
        payload: {
            kind: 'create-local',
            seedName
        } satisfies CommandGroupItemPayload
    };

    const createGlobal: CompletionItem = {
        id: 'action:create-global',
        label: 'Criar grupo de comandos global',
        description: 'Cria um novo grupo de comandos disponível em todos os projetos',
        icon: 'lucide:globe',
        kind: 'action',
        pinned: 'bottom',
        disabled: false,
        payload: {
            kind: 'create-global',
            seedName
        } satisfies CommandGroupItemPayload
    };

    return [createLocal, createGlobal];
}

function buildSecondaryStageItems(group: CommandGroup): CompletionItem[] {
    const entries: Array<{
        id: string;
        action: GroupTriggerSecondaryAction;
        label: string;
        description: string;
        icon: string;
    }> = [
        {
            id: 'action:run_sequential',
            action: 'run_sequential',
            label: 'Executar em modo Sequencial',
            description: 'Executa os itens em fila no painel atual',
            icon: 'lucide:list-ordered'
        },
        {
            id: 'action:run_parallel',
            action: 'run_parallel',
            label: 'Executar em modo Paralelo',
            description: 'Dispara os itens concorrentemente em background',
            icon: 'lucide:cpu'
        },
        {
            id: 'action:run_multi_tabs',
            action: 'run_multi_tabs',
            label: 'Executar em modo Multi-Abas',
            description: 'Abre uma aba dedicada para cada item do grupo',
            icon: 'lucide:app-window'
        },
        {
            id: 'action:edit',
            action: 'edit',
            label: 'Editar grupo',
            description: 'Abre o modal de edição do grupo de comandos',
            icon: 'lucide:pencil'
        },
        {
            id: 'action:delete',
            action: 'delete',
            label: 'Excluir grupo',
            description: 'Remove a definição deste grupo de comandos',
            icon: 'lucide:trash-2'
        }
    ];

    return entries.map((entry) => ({
        id: entry.id,
        label: entry.label,
        description: entry.description,
        icon: entry.icon,
        kind: 'action',
        payload: {
            kind: 'secondary-action',
            group,
            secondaryAction: entry.action
        } satisfies CommandGroupItemPayload
    }));
}

/**
 * Cria o provedor do gatilho `>>` (Grupos de Comandos — Fase 15).
 */
export function commandGroupsTrigger(
    store: ReturnType<typeof useCommandGroupsStore> = useCommandGroupsStore()
): TriggerDefinition {
    const pendingSubmissions = new Map<
        string,
        { requestId: string; inFlight: boolean }
    >();

    async function executeGroupSubmission(
        group: CommandGroup,
        ctx: TerminalContext,
        modeOverride: ExecutionMode | null
    ): Promise<TriggerSelectOutcome> {
        if (!ctx.projectId) {
            throw new Error('Selecione um projeto antes de executar este grupo de comandos.');
        }

        const effectiveMode = modeOverride ?? group.mode;
        const gestureKey = `${ctx.paneId}:${group.id}:${effectiveMode}`;
        const existing = pendingSubmissions.get(gestureKey);
        if (existing?.inFlight) {
            return { kind: 'handled' };
        }

        const requestId = existing?.requestId ?? createGroupRequestId();
        pendingSubmissions.set(gestureKey, { requestId, inFlight: true });

        try {
            await store.startRun({
                requestId,
                groupId: group.id,
                expectedRevision: group.revision,
                projectId: ctx.projectId,
                originPaneId: ctx.paneId,
                modeOverride,
                templateByItem: {}
            });
            pendingSubmissions.delete(gestureKey);
            return { kind: 'handled' };
        } catch (err: unknown) {
            // Mantém o mesmo requestId caso a próxima tentativa seja um retry imediato, mas libera inFlight
            pendingSubmissions.set(gestureKey, { requestId, inFlight: false });
            throw err;
        }
    }

    return {
        id: COMMAND_GROUPS_TRIGGER_ID,
        prefix: COMMAND_GROUPS_PREFIX,
        label: 'Grupos de Comandos (>>)',
        debounceMs: 20,

        isAvailable(ctx: TerminalContext): boolean {
            return (ctx.inputMode ?? 'rich') === 'rich';
        },

        async provide(
            q: TriggerQuery,
            ctx: TerminalContext,
            signal: AbortSignal
        ): Promise<CompletionResult> {
            const matched = matchCommandGroupsTrigger(q.input, ctx.inputMode ?? 'rich');
            if (!matched || signal.aborted) {
                return { items: [], passthrough: true };
            }

            if (q.stage === 1) {
                const state = q.stageState as CommandGroupStageState | undefined;
                if (!state?.group) {
                    return {
                        items: buildPinnedCreateActions(ctx.projectId, matched.query),
                        emptyMessage: 'Grupo não encontrado'
                    };
                }
                return {
                    stageLabel: `${state.group.name} — Ações`,
                    items: buildSecondaryStageItems(state.group)
                };
            }

            const projectAtStart = ctx.projectId ?? null;
            const pinnedActions = buildPinnedCreateActions(projectAtStart, matched.query);

            try {
                await store.load(projectAtStart);
            } catch {
                if (signal.aborted || (ctx.projectId ?? null) !== projectAtStart) {
                    return { items: [], passthrough: true };
                }
                return {
                    items: [...pinnedActions],
                    sections: GROUP_SECTIONS,
                    emptyMessage: 'Falha ao carregar grupos de comandos',
                    stageLabel: 'Erro ao carregar catálogo'
                };
            }

            if (signal.aborted || (ctx.projectId ?? null) !== projectAtStart) {
                return { items: [], passthrough: true };
            }

            const catalog = store.groupsForProject(projectAtStart);
            const ranked = rankAndFilterGroupsByScope(
                catalog.localGroups,
                catalog.globalGroups,
                matched.query
            );

            const localItems = ranked.local.map(toGroupCompletionItem);
            const globalItems = ranked.global.map(toGroupCompletionItem);

            return {
                items: [...localItems, ...globalItems, ...pinnedActions],
                sections: GROUP_SECTIONS,
                emptyMessage: 'Nenhum grupo de comandos corresponde à busca'
            };
        },

        async onSelect(
            item: CompletionItem,
            q: TriggerQuery,
            ctx: TerminalContext
        ): Promise<TriggerSelectOutcome> {
            const payload = item.payload as CommandGroupItemPayload | undefined;
            const rawSeed = (payload?.seedName ?? q.query).trim();

            if (item.id === 'action:create-local' || payload?.kind === 'create-local') {
                if (!ctx.projectId) {
                    return { kind: 'close' };
                }
                store.openCreateModal('local', ctx.projectId, rawSeed);
                return { kind: 'handled' };
            }

            if (item.id === 'action:create-global' || payload?.kind === 'create-global') {
                store.openCreateModal('global', ctx.projectId ?? null, rawSeed);
                return { kind: 'handled' };
            }

            if (q.stage === 1 || payload?.kind === 'secondary-action') {
                const state = q.stageState as CommandGroupStageState | undefined;
                const targetGroup = payload?.group ?? state?.group;
                if (!targetGroup) {
                    return { kind: 'close' };
                }

                const action = payload?.secondaryAction;
                if (action === 'run_sequential') {
                    return executeGroupSubmission(targetGroup, ctx, 'sequential');
                }
                if (action === 'run_parallel') {
                    return executeGroupSubmission(targetGroup, ctx, 'parallel');
                }
                if (action === 'run_multi_tabs') {
                    return executeGroupSubmission(targetGroup, ctx, 'multi_tabs');
                }
                if (action === 'edit') {
                    store.openEditModal(targetGroup);
                    return { kind: 'handled' };
                }
                if (action === 'delete') {
                    await store.remove(targetGroup.id, targetGroup.revision);
                    return { kind: 'handled' };
                }
                return { kind: 'close' };
            }

            if (payload?.kind === 'group' && payload.group) {
                return executeGroupSubmission(payload.group, ctx, null);
            }

            return { kind: 'close' };
        },

        onSecondarySelect(
            item: CompletionItem,
            q: TriggerQuery,
            ctx: TerminalContext
        ): TriggerSelectOutcome | Promise<TriggerSelectOutcome> {
            const payload = item.payload as CommandGroupItemPayload | undefined;
            if (q.stage === 0 && payload?.kind === 'group' && payload.group) {
                return {
                    kind: 'push-stage',
                    stageState: { group: payload.group } satisfies CommandGroupStageState,
                    initialQuery: ''
                };
            }
            return this.onSelect(item, q, ctx);
        }
    };
}

/**
 * Registra o provedor do gatilho `>>` (`command-groups`) no `TriggerRegistry` e inicializa listeners da store.
 */
export function registerCommandGroupsTrigger(
    registry: TriggerRegistry = triggerRegistry,
    store: ReturnType<typeof useCommandGroupsStore> = useCommandGroupsStore()
): Disposable {
    void store.initListeners().catch(() => {});
    const def = commandGroupsTrigger(store);
    if (registry.getById(def.id)) {
        return { dispose: () => {} };
    }
    return registry.register(def);
}
