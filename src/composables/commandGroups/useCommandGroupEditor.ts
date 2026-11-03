import { computed, ref, type ComputedRef, type Ref } from 'vue';
import { isMtIpcError } from '@/ipc/core';
import { useCommandGroupsStore } from '@/stores/commandGroups';
import type {
    CommandGroup,
    CommandGroupInput,
    CommandGroupItemInput,
    EditableCommandGroupDraft,
    EditableGroupItem,
    ExecutionMode,
    GroupEditorFieldErrors,
    GroupScope,
    RecurringCommandEntry,
    SequentialSeparator
} from '@/types/commandGroups';

const ENV_KEY_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;
const SECRET_KEY_HINT_RE = /(TOKEN|SECRET|PASSWORD|PASSWD|KEY|AUTH|CREDENTIAL|PRIVATE)/i;

let localItemCounter = 0;

function nextLocalKey(): string {
    localItemCounter += 1;
    return `group-item-local-${Date.now()}-${localItemCounter}`;
}

export function isSensitiveEnvKey(key: string): boolean {
    return SECRET_KEY_HINT_RE.test(key);
}

function recordToEnvPairs(env: Record<string, string> | undefined): Array<{ key: string; value: string }> {
    if (!env) {
        return [];
    }
    return Object.entries(env).map(([key, value]) => ({ key, value }));
}

function envPairsToRecord(pairs: ReadonlyArray<{ key: string; value: string }>): Record<string, string> {
    const out: Record<string, string> = {};
    for (const pair of pairs) {
        const trimmedKey = pair.key.trim();
        if (trimmedKey.length > 0) {
            out[trimmedKey] = pair.value;
        }
    }
    return out;
}

export function createEmptyEditableItem(
    partial?: Partial<CommandGroupItemInput>
): EditableGroupItem {
    const envRecord = partial?.env ? { ...partial.env } : {};
    return {
        localKey: nextLocalKey(),
        id: partial?.id ?? null,
        title: partial?.title ?? '',
        command: partial?.command ?? '',
        cwd: partial?.cwd ?? '',
        env: envRecord,
        envPairs: recordToEnvPairs(envRecord),
        continuous: partial?.continuous ?? false
    };
}

function cloneDraft(draft: EditableCommandGroupDraft): EditableCommandGroupDraft {
    return {
        scope: draft.scope,
        projectId: draft.projectId,
        name: draft.name,
        description: draft.description,
        labels: [...draft.labels],
        mode: draft.mode,
        separator: draft.separator,
        cwd: draft.cwd,
        envPairs: draft.envPairs.map((p) => ({ key: p.key, value: p.value })),
        items: draft.items.map((item) => ({
            localKey: item.localKey,
            id: item.id ?? null,
            title: item.title,
            command: item.command,
            cwd: item.cwd ?? '',
            env: { ...item.env },
            envPairs: item.envPairs.map((p) => ({ key: p.key, value: p.value })),
            continuous: item.continuous
        }))
    };
}

function buildDraftFromGroup(
    group: CommandGroup | null,
    scope: GroupScope,
    projectId: string | null,
    initialName = ''
): EditableCommandGroupDraft {
    if (!group) {
        return {
            scope,
            projectId: scope === 'local' ? projectId : null,
            name: initialName,
            description: '',
            labels: [],
            mode: 'sequential',
            separator: 'and',
            cwd: '',
            envPairs: [],
            items: [createEmptyEditableItem()]
        };
    }

    return {
        scope: group.scope,
        projectId: group.projectId ?? null,
        name: group.name,
        description: group.description,
        labels: [...group.labels],
        mode: group.mode,
        separator: group.separator,
        cwd: group.cwd ?? '',
        envPairs: recordToEnvPairs(group.env),
        items:
            group.items.length > 0
                ? group.items.map((it) => ({
                    localKey: nextLocalKey(),
                    id: it.id,
                    title: it.title,
                    command: it.command,
                    cwd: it.cwd ?? '',
                    env: { ...it.env },
                    envPairs: recordToEnvPairs(it.env),
                    continuous: it.continuous
                }))
                : [createEmptyEditableItem()]
    };
}

function isRelativePathValid(cwd: string): boolean {
    const trimmed = cwd.trim();
    if (!trimmed) {
        return true;
    }
    if (trimmed.startsWith('/') || trimmed.startsWith('~') || trimmed.includes('\0')) {
        return false;
    }
    const segments = trimmed.split(/[\\/]+/);
    return !segments.includes('..');
}

export interface UseCommandGroupEditorOptions {
    initialGroup?: CommandGroup | null;
    initialScope?: GroupScope;
    initialProjectId?: string | null;
    initialName?: string;
}

export interface UseCommandGroupEditorReturn {
    draft: Ref<EditableCommandGroupDraft>;
    baseGroup: Ref<CommandGroup | null>;
    expectedRevision: Ref<number>;
    draftRevision: Ref<number>;
    isEditMode: ComputedRef<boolean>;
    dirty: ComputedRef<boolean>;
    fieldErrors: Ref<GroupEditorFieldErrors>;
    generalError: Ref<string | null>;
    staleSuggestionWarning: Ref<string | null>;
    saving: Ref<boolean>;
    deleting: Ref<boolean>;
    revisionConflictGroup: Ref<CommandGroup | null>;
    undoSnapshot: Ref<EditableCommandGroupDraft | null>;
    hasUndoSnapshot: ComputedRef<boolean>;
    hasMidSequenceContinuousWarning: ComputedRef<boolean>;
    reorderAnnouncement: Ref<string>;
    resetFromGroup: (
        group: CommandGroup | null,
        scope: GroupScope,
        projectId: string | null,
        initialName?: string
    ) => void;
    markMutated: () => void;
    setScope: (scope: GroupScope, projectId: string | null) => void;
    setMode: (mode: ExecutionMode) => void;
    setSeparator: (separator: SequentialSeparator) => void;
    addLabel: (rawLabel: string) => void;
    removeLabel: (index: number) => void;
    addGroupEnvPair: () => void;
    removeGroupEnvPair: (index: number) => void;
    addItem: (partial?: Partial<CommandGroupItemInput>) => EditableGroupItem;
    updateItem: (index: number, item: EditableGroupItem) => void;
    removeItem: (index: number) => void;
    duplicateItem: (index: number) => EditableGroupItem | null;
    moveItem: (fromIndex: number, toIndex: number) => boolean;
    moveItemUp: (index: number) => boolean;
    moveItemDown: (index: number) => boolean;
    toInput: () => CommandGroupInput;
    validateLocal: () => boolean;
    applySuggestion: (suggestion: CommandGroupInput, responseDraftRevision?: number) => boolean;
    undoSuggestion: () => boolean;
    appendRecurringCommand: (entry: RecurringCommandEntry) => void;
    copyToScope: (targetScope: GroupScope, targetProjectId: string | null) => void;
    save: (forceOverwriteRevision?: number) => Promise<CommandGroup | null>;
    loadLatestOnConflict: () => Promise<CommandGroup | null>;
    forceSaveOverConflict: () => Promise<CommandGroup | null>;
    deleteCurrentGroup: () => Promise<boolean>;
}

/**
 * Composable de estado do editor `CommandGroupModal.vue`:
 * gerencia rascunho clonado, validação por campo, reordenação acessível,
 * aplicação/desfazer de sugestão IA e resolução de conflito otimista de revisão.
 */
export function useCommandGroupEditor(
    options: UseCommandGroupEditorOptions = {}
): UseCommandGroupEditorReturn {
    const store = useCommandGroupsStore();

    const baseGroup = ref<CommandGroup | null>(options.initialGroup ?? null);
    const expectedRevision = ref<number>(options.initialGroup?.revision ?? 0);
    const draftRevision = ref<number>(1);

    const initialDraft = buildDraftFromGroup(
        options.initialGroup ?? null,
        options.initialScope ?? options.initialGroup?.scope ?? 'local',
        options.initialProjectId ?? options.initialGroup?.projectId ?? null,
        options.initialName ?? ''
    );

    const draft = ref<EditableCommandGroupDraft>(cloneDraft(initialDraft));
    const baselineJson = ref<string>(JSON.stringify(toInputFromDraft(initialDraft)));

    const fieldErrors = ref<GroupEditorFieldErrors>({});
    const generalError = ref<string | null>(null);
    const staleSuggestionWarning = ref<string | null>(null);
    const saving = ref<boolean>(false);
    const deleting = ref<boolean>(false);
    const revisionConflictGroup = ref<CommandGroup | null>(null);
    const undoSnapshot = ref<EditableCommandGroupDraft | null>(null);
    const reorderAnnouncement = ref<string>('');

    const isEditMode = computed<boolean>(() => baseGroup.value !== null);
    const hasUndoSnapshot = computed<boolean>(() => undoSnapshot.value !== null);

    const dirty = computed<boolean>(() => {
        const currentJson = JSON.stringify(toInputFromDraft(draft.value));
        return currentJson !== baselineJson.value;
    });

    const hasMidSequenceContinuousWarning = computed<boolean>(() => {
        if (draft.value.mode !== 'sequential') {
            return false;
        }
        const items = draft.value.items;
        for (let i = 0; i < items.length - 1; i++) {
            if (items[i]?.continuous) {
                return true;
            }
        }
        return false;
    });

    function markMutated(): void {
        draftRevision.value += 1;
        staleSuggestionWarning.value = null;
    }

    function resetFromGroup(
        group: CommandGroup | null,
        scope: GroupScope,
        projectId: string | null,
        initialName = ''
    ): void {
        baseGroup.value = group;
        expectedRevision.value = group?.revision ?? 0;
        const next = buildDraftFromGroup(group, scope, projectId, initialName);
        draft.value = cloneDraft(next);
        baselineJson.value = JSON.stringify(toInputFromDraft(next));
        fieldErrors.value = {};
        generalError.value = null;
        staleSuggestionWarning.value = null;
        revisionConflictGroup.value = null;
        undoSnapshot.value = null;
        reorderAnnouncement.value = '';
        draftRevision.value += 1;
    }

    function setScope(scope: GroupScope, projectId: string | null): void {
        if (isEditMode.value) {
            return;
        }
        draft.value.scope = scope;
        draft.value.projectId = scope === 'local' ? projectId : null;
        markMutated();
    }

    function setMode(mode: ExecutionMode): void {
        draft.value.mode = mode;
        markMutated();
    }

    function setSeparator(separator: SequentialSeparator): void {
        draft.value.separator = separator;
        markMutated();
    }

    function addLabel(rawLabel: string): void {
        const clean = rawLabel.trim().normalize('NFC');
        if (!clean) {
            return;
        }
        const lowerKey = clean.toLowerCase();
        const exists = draft.value.labels.some(
            (existing) => existing.normalize('NFC').toLowerCase() === lowerKey
        );
        if (!exists && draft.value.labels.length < 20) {
            draft.value.labels.push(clean);
            markMutated();
        }
    }

    function removeLabel(index: number): void {
        if (index >= 0 && index < draft.value.labels.length) {
            draft.value.labels.splice(index, 1);
            markMutated();
        }
    }

    function addGroupEnvPair(): void {
        if (draft.value.envPairs.length < 64) {
            draft.value.envPairs.push({ key: '', value: '' });
            markMutated();
        }
    }

    function removeGroupEnvPair(index: number): void {
        if (index >= 0 && index < draft.value.envPairs.length) {
            draft.value.envPairs.splice(index, 1);
            markMutated();
        }
    }

    function addItem(partial?: Partial<CommandGroupItemInput>): EditableGroupItem {
        const item = createEmptyEditableItem(partial);
        draft.value.items.push(item);
        markMutated();
        return item;
    }

    function updateItem(index: number, item: EditableGroupItem): void {
        if (index >= 0 && index < draft.value.items.length) {
            draft.value.items[index] = {
                ...item,
                env: envPairsToRecord(item.envPairs)
            };
            markMutated();
        }
    }

    function removeItem(index: number): void {
        if (index < 0 || index >= draft.value.items.length) {
            return;
        }
        if (draft.value.items.length <= 1) {
            draft.value.items[0] = createEmptyEditableItem();
        } else {
            draft.value.items.splice(index, 1);
        }
        markMutated();
    }

    function duplicateItem(index: number): EditableGroupItem | null {
        const source = draft.value.items[index];
        if (!source || draft.value.items.length >= 50) {
            return null;
        }
        const copy: EditableGroupItem = {
            localKey: nextLocalKey(),
            id: null,
            title: `${source.title} (cópia)`.slice(0, 80),
            command: source.command,
            cwd: source.cwd ?? '',
            env: { ...source.env },
            envPairs: source.envPairs.map((p) => ({ key: p.key, value: p.value })),
            continuous: source.continuous
        };
        draft.value.items.splice(index + 1, 0, copy);
        markMutated();
        return copy;
    }

    function moveItem(fromIndex: number, toIndex: number): boolean {
        const list = draft.value.items;
        if (
            fromIndex < 0 ||
            fromIndex >= list.length ||
            toIndex < 0 ||
            toIndex >= list.length ||
            fromIndex === toIndex
        ) {
            return false;
        }
        const [moved] = list.splice(fromIndex, 1);
        if (!moved) {
            return false;
        }
        list.splice(toIndex, 0, moved);
        reorderAnnouncement.value = `Item movido para a posição ${toIndex + 1} de ${list.length}`;
        markMutated();
        return true;
    }

    function moveItemUp(index: number): boolean {
        return moveItem(index, index - 1);
    }

    function moveItemDown(index: number): boolean {
        return moveItem(index, index + 1);
    }

    function toInput(): CommandGroupInput {
        return toInputFromDraft(draft.value);
    }

    function validateEnvPairs(
        pairs: ReadonlyArray<{ key: string; value: string }>,
        fieldPrefix: string,
        errors: GroupEditorFieldErrors
    ): void {
        if (pairs.length > 64) {
            errors[fieldPrefix] = 'Máximo de 64 variáveis de ambiente por nível.';
            return;
        }
        for (let i = 0; i < pairs.length; i++) {
            const pair = pairs[i]!;
            const key = pair.key.trim();
            if (key.length === 0 && pair.value.length === 0) {
                continue;
            }
            if (!ENV_KEY_RE.test(key)) {
                errors[fieldPrefix] = `Chave de variável inválida: "${key || '(vazia)'}".`;
                return;
            }
            if (key.toUpperCase().startsWith('MAXTERMINAL_')) {
                errors[fieldPrefix] = 'O prefixo MAXTERMINAL_ é reservado pelo sistema.';
                return;
            }
            if (pair.value.includes('\0')) {
                errors[fieldPrefix] = `O valor de "${key}" contém caracteres nulos inválidos.`;
                return;
            }
        }
    }

    function validateLocal(): boolean {
        const errors: GroupEditorFieldErrors = {};
        const d = draft.value;

        const trimmedName = d.name.trim();
        if (trimmedName.length === 0) {
            errors['name'] = 'Informe o nome do grupo de comandos.';
        } else if (trimmedName.length > 80) {
            errors['name'] = 'O nome do grupo deve ter no máximo 80 caracteres.';
        }

        if (d.description.length > 2048) {
            errors['description'] = 'A descrição deve ter no máximo 2048 caracteres.';
        }

        if (d.scope === 'local' && !d.projectId) {
            errors['projectId'] = 'Selecione um projeto para criar um grupo local.';
        }

        if (d.labels.length > 20) {
            errors['labels'] = 'O grupo pode ter no máximo 20 etiquetas.';
        } else if (d.labels.some((l) => l.trim().length === 0 || l.trim().length > 32)) {
            errors['labels'] = 'Cada etiqueta deve ter entre 1 e 32 caracteres.';
        }

        if (!isRelativePathValid(d.cwd)) {
            errors['cwd'] =
                'O diretório de trabalho deve ser relativo à raiz do projeto (sem "/" inicial ou "..").';
        }

        validateEnvPairs(d.envPairs, 'env', errors);

        if (d.items.length === 0) {
            errors['items'] = 'Adicione pelo menos 1 item ao grupo de comandos.';
        } else if (d.items.length > 50) {
            errors['items'] = 'O grupo pode conter no máximo 50 itens.';
        }

        for (let i = 0; i < d.items.length; i++) {
            const item = d.items[i]!;
            const title = item.title.trim();
            if (title.length === 0) {
                errors[`items.${i}.title`] = 'Informe o título do item.';
            } else if (title.length > 80) {
                errors[`items.${i}.title`] = 'O título do item deve ter no máximo 80 caracteres.';
            }

            const cmd = item.command.trim();
            if (cmd.length === 0) {
                errors[`items.${i}.command`] = 'Informe o comando shell do item.';
            } else if (new TextEncoder().encode(item.command).length > 8192) {
                errors[`items.${i}.command`] = 'O comando excede o limite de 8192 bytes.';
            } else if (/[\r\n\0]/.test(item.command)) {
                errors[`items.${i}.command`] = 'O comando não pode conter quebras de linha ou NUL.';
            }

            if (item.cwd && !isRelativePathValid(item.cwd)) {
                errors[`items.${i}.cwd`] =
                    'O diretório do item deve ser relativo à raiz do projeto (sem "/" inicial ou "..").';
            }

            validateEnvPairs(item.envPairs, `items.${i}.env`, errors);
        }

        fieldErrors.value = errors;
        return Object.keys(errors).length === 0;
    }

    function applySuggestion(
        suggestion: CommandGroupInput,
        responseDraftRevision?: number
    ): boolean {
        if (
            responseDraftRevision !== undefined &&
            responseDraftRevision !== draftRevision.value
        ) {
            staleSuggestionWarning.value =
                'O rascunho foi alterado enquanto a IA gerava a sugestão. Revise antes de aplicar.';
            return false;
        }

        undoSnapshot.value = cloneDraft(draft.value);
        draft.value = {
            scope: draft.value.scope,
            projectId: draft.value.projectId,
            name: suggestion.name || draft.value.name,
            description: suggestion.description ?? '',
            labels: [...(suggestion.labels ?? [])],
            mode: suggestion.mode,
            separator: suggestion.separator,
            cwd: suggestion.cwd ?? '',
            envPairs: recordToEnvPairs(suggestion.env),
            items:
                suggestion.items.length > 0
                    ? suggestion.items.map((it) => ({
                        localKey: nextLocalKey(),
                        id: null,
                        title: it.title,
                        command: it.command,
                        cwd: it.cwd ?? '',
                        env: { ...it.env },
                        envPairs: recordToEnvPairs(it.env),
                        continuous: it.continuous
                    }))
                    : [createEmptyEditableItem()]
        };
        fieldErrors.value = {};
        generalError.value = null;
        staleSuggestionWarning.value = null;
        draftRevision.value += 1;
        return true;
    }

    function undoSuggestion(): boolean {
        if (!undoSnapshot.value) {
            return false;
        }
        draft.value = cloneDraft(undoSnapshot.value);
        undoSnapshot.value = null;
        fieldErrors.value = {};
        draftRevision.value += 1;
        return true;
    }

    function appendRecurringCommand(entry: RecurringCommandEntry): void {
        const cleanTitle = entry.command.trim().slice(0, 80) || 'Comando recorrente';
        const first = draft.value.items[0];
        if (
            draft.value.items.length === 1 &&
            first &&
            first.title.trim().length === 0 &&
            first.command.trim().length === 0
        ) {
            first.title = cleanTitle;
            first.command = entry.command;
            if (entry.cwd && !first.cwd) {
                first.cwd = entry.cwd;
            }
            markMutated();
            return;
        }

        if (draft.value.items.length < 50) {
            draft.value.items.push(
                createEmptyEditableItem({
                    title: cleanTitle,
                    command: entry.command,
                    cwd: entry.cwd ?? null,
                    env: {},
                    continuous: false
                })
            );
            markMutated();
        }
    }

    function copyToScope(targetScope: GroupScope, targetProjectId: string | null): void {
        baseGroup.value = null;
        expectedRevision.value = 0;
        draft.value.scope = targetScope;
        draft.value.projectId = targetScope === 'local' ? targetProjectId : null;
        draft.value.name = `${draft.value.name.trim()} (${targetScope === 'global' ? 'Global' : 'Local'})`.slice(
            0,
            80
        );
        draft.value.items = draft.value.items.map((item) => ({
            ...item,
            id: null,
            localKey: nextLocalKey()
        }));
        revisionConflictGroup.value = null;
        fieldErrors.value = {};
        generalError.value = null;
        markMutated();
    }

    function extractBackendFieldErrors(details: unknown, code: string, message: string): void {
        const nextErrors: GroupEditorFieldErrors = { ...fieldErrors.value };
        if (typeof details === 'object' && details !== null) {
            const det = details as Record<string, unknown>;
            const fields = det.fields;
            if (Array.isArray(fields)) {
                for (const f of fields) {
                    if (typeof f === 'string' && f.length > 0) {
                        nextErrors[f] =
                            code === 'commands.group.secret_detected'
                                ? 'Segredo literal detectado neste campo. Use referência a variável ($TOKEN).'
                                : message;
                    }
                }
            } else if (typeof det.field === 'string' && det.field.length > 0) {
                nextErrors[det.field] = message;
            }
        }
        if (code === 'commands.group.name_conflict') {
            nextErrors['name'] = message;
        }
        fieldErrors.value = nextErrors;
    }

    async function save(forceOverwriteRevision?: number): Promise<CommandGroup | null> {
        generalError.value = null;
        if (!validateLocal()) {
            return null;
        }

        saving.value = true;
        try {
            const payload = toInput();
            let saved: CommandGroup;
            if (!baseGroup.value) {
                saved = await store.create(payload);
            } else {
                const rev = forceOverwriteRevision ?? expectedRevision.value;
                saved = await store.update(baseGroup.value.id, rev, payload);
            }
            baseGroup.value = saved;
            expectedRevision.value = saved.revision;
            revisionConflictGroup.value = null;
            baselineJson.value = JSON.stringify(toInputFromDraft(draft.value));
            return saved;
        } catch (err: unknown) {
            if (isMtIpcError(err)) {
                generalError.value = err.message;
                extractBackendFieldErrors(err.details, err.code, err.message);
                if (err.code === 'commands.group.revision_conflict' && baseGroup.value) {
                    try {
                        const latest = await store.get(baseGroup.value.id);
                        revisionConflictGroup.value = latest;
                    } catch {
                        // Mantém banner de conflito mesmo se a releitura falhar.
                    }
                }
            } else if (err instanceof Error) {
                generalError.value = err.message;
            } else {
                generalError.value = 'Falha ao salvar o grupo de comandos.';
            }
            return null;
        } finally {
            saving.value = false;
        }
    }

    async function loadLatestOnConflict(): Promise<CommandGroup | null> {
        if (!baseGroup.value) {
            return null;
        }
        const latest =
            revisionConflictGroup.value ?? (await store.get(baseGroup.value.id));
        resetFromGroup(latest, latest.scope, latest.projectId ?? null, latest.name);
        return latest;
    }

    async function forceSaveOverConflict(): Promise<CommandGroup | null> {
        if (!baseGroup.value) {
            return null;
        }
        const latest =
            revisionConflictGroup.value ?? (await store.get(baseGroup.value.id));
        expectedRevision.value = latest.revision;
        return save(latest.revision);
    }

    async function deleteCurrentGroup(): Promise<boolean> {
        if (!baseGroup.value) {
            return false;
        }
        deleting.value = true;
        generalError.value = null;
        try {
            await store.remove(baseGroup.value.id, expectedRevision.value);
            return true;
        } catch (err: unknown) {
            if (isMtIpcError(err)) {
                generalError.value = err.message;
            } else if (err instanceof Error) {
                generalError.value = err.message;
            } else {
                generalError.value = 'Não foi possível excluir o grupo de comandos.';
            }
            return false;
        } finally {
            deleting.value = false;
        }
    }

    return {
        draft,
        baseGroup,
        expectedRevision,
        draftRevision,
        isEditMode,
        dirty,
        fieldErrors,
        generalError,
        staleSuggestionWarning,
        saving,
        deleting,
        revisionConflictGroup,
        undoSnapshot,
        hasUndoSnapshot,
        hasMidSequenceContinuousWarning,
        reorderAnnouncement,
        resetFromGroup,
        markMutated,
        setScope,
        setMode,
        setSeparator,
        addLabel,
        removeLabel,
        addGroupEnvPair,
        removeGroupEnvPair,
        addItem,
        updateItem,
        removeItem,
        duplicateItem,
        moveItem,
        moveItemUp,
        moveItemDown,
        toInput,
        validateLocal,
        applySuggestion,
        undoSuggestion,
        appendRecurringCommand,
        copyToScope,
        save,
        loadLatestOnConflict,
        forceSaveOverConflict,
        deleteCurrentGroup
    };
}

function toInputFromDraft(d: EditableCommandGroupDraft): CommandGroupInput {
    const trimmedCwd = d.cwd.trim();
    return {
        scope: d.scope,
        projectId: d.scope === 'local' ? d.projectId : null,
        name: d.name.trim(),
        description: d.description,
        labels: d.labels.map((l) => l.trim()).filter((l) => l.length > 0),
        mode: d.mode,
        separator: d.separator,
        cwd: trimmedCwd.length > 0 ? trimmedCwd : null,
        env: envPairsToRecord(d.envPairs),
        items: d.items.map((it) => {
            const itemCwd = (it.cwd ?? '').trim();
            return {
                id: it.id ?? null,
                title: it.title.trim(),
                command: it.command,
                cwd: itemCwd.length > 0 ? itemCwd : null,
                env: envPairsToRecord(it.envPairs),
                continuous: Boolean(it.continuous)
            };
        })
    };
}
