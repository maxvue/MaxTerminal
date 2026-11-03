import { computed, markRaw, ref } from 'vue';
import { defineStore } from 'pinia';
import { useDebounceFn } from '@maxvue/max-use';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
    ARTISAN_COMMAND_SCHEMA_ID,
    decodeArtisanCommandForm,
    getQuickCommandsAdapters,
    type AiStructuredGenerator
} from '@/composables/quick-commands/adapters';
import { useIpcError } from '@/composables/useIpcError';
import {
    onCatalogUpdated,
    onCommandCreated,
    onDiscoveryFailed,
    quickCommandsIpc
} from '@/ipc/quickCommands';
import type {
    ArtisanCatalog,
    ArtisanCommand,
    ArtisanCommandForm,
    ArtisanCreateResult,
    ArtisanPreview,
    ArtisanRoot,
    PaneId,
    ProjectId,
    ShortcutList,
    ValidationReport
} from '@/types/bindings';

const ROOT_TTL_MS = 30_000;
const SHORTCUTS_TTL_MS = 10_000;
const FAILURE_BACKOFF_MS = 5_000;

export interface CatalogSlot {
    catalog: ArtisanCatalog | null;
    loading: boolean;
    error: string | null;
    failedAtMs: number | null;
    revision: number;
}

export interface QuickCommandsModalState {
    open: boolean;
    root: string | null;
    form: ArtisanCommandForm;
    preview: ArtisanPreview | null;
    previewLoading: boolean;
    report: ValidationReport | null;
    creating: boolean;
    aiLoading: boolean;
    aiSnapshot: ArtisanCommandForm | null;
    lastCreateResult: ArtisanCreateResult | null;
    paneId: PaneId | null;
    contextRevision: number;
    formRevision: number;
    previewRevision: number;
}

export interface QuickCommandConfirmState {
    open: boolean;
    title: string;
    line: string;
    paneId: PaneId | null;
}

export function suggestClassNameFromSignature(signatureName: string): string {
    const cleaned = signatureName.trim().replace(/^>+/, '');
    if (!cleaned) {
        return '';
    }
    const tokens = cleaned
        .split(/[:\-_\s/]+/)
        .map((s) => s.replace(/[^a-zA-Z0-9]/g, ''))
        .filter((s) => s.length > 0);
    if (tokens.length === 0) {
        return '';
    }
    const pascal = tokens
        .map((tok) => tok.charAt(0).toUpperCase() + tok.slice(1).toLowerCase())
        .join('');
    if (!/^[A-Z]/.test(pascal)) {
        return `Cmd${pascal}`.slice(0, 64);
    }
    return pascal.slice(0, 64);
}

export function createEmptyArtisanForm(seed?: { signatureName?: string }): ArtisanCommandForm {
    const sig = seed?.signatureName?.trim() ?? '';
    return {
        className: sig ? suggestClassNameFromSignature(sig) : '',
        signatureName: sig,
        description: '',
        params: [],
        isolatable: false,
        promptsForMissingInput: false,
        schedule: null,
        objective: ''
    };
}

function cloneForm(form: ArtisanCommandForm): ArtisanCommandForm {
    return {
        className: form.className,
        signatureName: form.signatureName,
        description: form.description,
        params: (form.params ?? []).map((p) => ({
            kind: p.kind,
            name: p.name,
            description: p.description ?? '',
            optional: p.optional ?? false,
            isArray: p.isArray ?? false,
            default: p.default ?? null,
            shortcut: p.shortcut ?? null,
            acceptsValue: p.acceptsValue ?? false
        })),
        isolatable: form.isolatable ?? false,
        promptsForMissingInput: form.promptsForMissingInput ?? false,
        schedule: form.schedule
            ? {
                frequency: { ...form.schedule.frequency },
                arguments: [...(form.schedule.arguments ?? [])],
                options: (form.schedule.options ?? []).map((o) => ({
                    name: o.name,
                    values: [...(o.values ?? [])],
                    enabled: o.enabled ?? true
                })),
                withoutOverlapping: form.schedule.withoutOverlapping ?? false,
                onOneServer: form.schedule.onOneServer ?? false,
                runInBackground: form.schedule.runInBackground ?? false
            }
            : null,
        objective: form.objective ?? ''
    };
}

export const useQuickCommandsStore = defineStore('quickCommands', () => {
    const ipcError = useIpcError();

    const rootsByCwd = ref<Record<string, { root: ArtisanRoot | null; at: number }>>({});
    const catalogs = ref<Record<string, CatalogSlot>>({});
    const shortcutsByCwd = ref<Record<string, { list: ShortcutList; at: number }>>({});

    const modal = ref<QuickCommandsModalState>({
        open: false,
        root: null,
        form: createEmptyArtisanForm(),
        preview: null,
        previewLoading: false,
        report: null,
        creating: false,
        aiLoading: false,
        aiSnapshot: null,
        lastCreateResult: null,
        paneId: null,
        contextRevision: 0,
        formRevision: 0,
        previewRevision: -1
    });

    const confirm = ref<QuickCommandConfirmState | null>(null);
    const initialized = ref(false);

    // Estruturas não-reativas para deduplicação de promessas em voo e rascunhos por raiz
    const inFlightRoots = new Map<string, Promise<ArtisanRoot | null>>();
    const inFlightCatalogs = new Map<string, Promise<ArtisanCatalog>>();
    const inFlightShortcuts = new Map<string, Promise<ShortcutList>>();
    const draftsByRoot = new Map<string, ArtisanCommandForm>();
    const unlisteners: UnlistenFn[] = [];
    let initPromise: Promise<void> | null = null;
    let initGen = 0;
    let cachedSchema: Record<string, unknown> | null = null;
    let activeAiGenerator: AiStructuredGenerator<ArtisanCommandForm> | null = null;
    let activeAiAbort: AbortController | null = null;
    let pendingConfirmResolve: ((confirmed: boolean) => void) | null = null;

    function rootCacheKey(cwd: string, projectId: ProjectId | null): string {
        return `${projectId ?? '_'}::${cwd}`;
    }

    function ensureSlot(root: string): CatalogSlot {
        const existing = catalogs.value[root];
        if (existing) {
            return existing;
        }
        const created: CatalogSlot = {
            catalog: null,
            loading: false,
            error: null,
            failedAtMs: null,
            revision: 0
        };
        catalogs.value = {
            ...catalogs.value,
            [root]: created
        };
        return created;
    }

    function setSlot(root: string, patch: Partial<CatalogSlot>): void {
        const prev = ensureSlot(root);
        catalogs.value = {
            ...catalogs.value,
            [root]: {
                ...prev,
                ...patch
            }
        };
    }

    function catalogFor(root: string): ArtisanCatalog | null {
        return catalogs.value[root]?.catalog ?? null;
    }

    function catalogErrorFor(root: string): string | null {
        const slot = catalogs.value[root];
        if (!slot || slot.catalog) {
            return null;
        }
        if (
            slot.error &&
            slot.failedAtMs !== null &&
            Date.now() - slot.failedAtMs < FAILURE_BACKOFF_MS
        ) {
            return slot.error;
        }
        return null;
    }

    function isCatalogInBackoff(root: string): boolean {
        return catalogErrorFor(root) !== null;
    }

    function commandByName(root: string, name: string): ArtisanCommand | undefined {
        const cat = catalogFor(root);
        if (!cat) {
            return undefined;
        }
        const trimmed = name.trim();
        return cat.commands.find(
            (c) => c.name === trimmed || (c.aliases?.includes(trimmed) ?? false)
        );
    }

    const canCreate = computed<boolean>(() =>
        Boolean(
            modal.value.open &&
                modal.value.report?.valid &&
                modal.value.preview?.php &&
                !modal.value.creating &&
                modal.value.previewRevision === modal.value.formRevision
        )
    );

    async function runPreviewForCurrentModal(): Promise<void> {
        if (!modal.value.open || !modal.value.root) {
            return;
        }
        const reqRoot = modal.value.root;
        const reqCtxRev = modal.value.contextRevision;
        const reqFormRev = modal.value.formRevision;
        const snapshotForm = cloneForm(modal.value.form);

        modal.value.previewLoading = true;
        try {
            const preview = await quickCommandsIpc.preview(reqRoot, snapshotForm);
            if (
                !modal.value.open ||
                modal.value.root !== reqRoot ||
                modal.value.contextRevision !== reqCtxRev ||
                modal.value.formRevision !== reqFormRev
            ) {
                return;
            }
            modal.value.preview = preview;
            modal.value.report = preview.report;
            modal.value.previewRevision = reqFormRev;
        } catch (err: unknown) {
            if (
                modal.value.open &&
                modal.value.root === reqRoot &&
                modal.value.contextRevision === reqCtxRev &&
                modal.value.formRevision === reqFormRev
            ) {
                const msg = ipcError.message(err);
                modal.value.report = {
                    valid: false,
                    issues: [
                        {
                            path: 'general',
                            severity: 'error',
                            code: 'preview_failed',
                            message: msg
                        }
                    ]
                };
                modal.value.preview = null;
            }
        } finally {
            if (
                modal.value.open &&
                modal.value.root === reqRoot &&
                modal.value.contextRevision === reqCtxRev &&
                modal.value.formRevision === reqFormRev
            ) {
                modal.value.previewLoading = false;
            }
        }
    }

    const triggerPreviewDebounced = useDebounceFn(() => {
        void runPreviewForCurrentModal();
    }, 250);

    async function init(): Promise<void> {
        if (initialized.value) {
            return;
        }
        if (initPromise) {
            return initPromise;
        }

        const myGen = ++initGen;
        initPromise = (async () => {
            try {
                const listeners = await Promise.all([
                    onCatalogUpdated((payload) => {
                        if (myGen !== initGen) {
                            return;
                        }
                        const slot = ensureSlot(payload.root);
                        if (payload.revision <= slot.revision) {
                            return;
                        }
                        setSlot(payload.root, { revision: payload.revision });
                        void ensureCatalog(payload.root, { force: false }).catch(() => {});
                    }),
                    onDiscoveryFailed((payload) => {
                        if (myGen !== initGen) {
                            return;
                        }
                        const slot = ensureSlot(payload.root);
                        if (payload.revision < slot.revision) {
                            return;
                        }
                        const msg = ipcError.message(payload.error);
                        setSlot(payload.root, {
                            loading: false,
                            error: msg,
                            failedAtMs: Date.now(),
                            revision: payload.revision
                        });
                    }),
                    onCommandCreated((payload) => {
                        if (myGen !== initGen) {
                            return;
                        }
                        const slot = ensureSlot(payload.root);
                        if (payload.revision > slot.revision) {
                            setSlot(payload.root, { revision: payload.revision });
                        }
                        void ensureCatalog(payload.root, { force: false }).catch(() => {});
                    })
                ]);

                if (myGen !== initGen) {
                    for (const u of listeners) {
                        u();
                    }
                    return;
                }

                unlisteners.push(...listeners);
                initialized.value = true;
            } finally {
                if (myGen === initGen) {
                    initPromise = null;
                }
            }
        })();

        return initPromise;
    }

    function dispose(): void {
        initGen += 1;
        initPromise = null;
        while (unlisteners.length > 0) {
            const u = unlisteners.pop();
            u?.();
        }
        if (activeAiAbort) {
            activeAiAbort.abort();
            activeAiAbort = null;
        }
        if (activeAiGenerator) {
            void activeAiGenerator.cancel();
            activeAiGenerator = null;
        }
        if (pendingConfirmResolve) {
            const resolve = pendingConfirmResolve;
            pendingConfirmResolve = null;
            confirm.value = null;
            resolve(false);
        }
        inFlightRoots.clear();
        inFlightCatalogs.clear();
        inFlightShortcuts.clear();
        initialized.value = false;
    }

    async function resolveRoot(
        cwd: string,
        projectId: ProjectId | null = null
    ): Promise<ArtisanRoot | null> {
        const cleanCwd = cwd.trim();
        if (!cleanCwd) {
            return null;
        }
        const key = rootCacheKey(cleanCwd, projectId);
        const cached = rootsByCwd.value[key];
        const now = Date.now();
        if (cached && now - cached.at <= ROOT_TTL_MS) {
            return cached.root;
        }

        const inFlight = inFlightRoots.get(key);
        if (inFlight) {
            return inFlight;
        }

        const promise = (async (): Promise<ArtisanRoot | null> => {
            try {
                const resolved = await quickCommandsIpc.resolveRoot(cleanCwd, projectId);
                rootsByCwd.value = {
                    ...rootsByCwd.value,
                    [key]: { root: resolved, at: Date.now() }
                };
                return resolved;
            } catch {
                rootsByCwd.value = {
                    ...rootsByCwd.value,
                    [key]: { root: null, at: Date.now() }
                };
                return null;
            } finally {
                inFlightRoots.delete(key);
            }
        })();

        inFlightRoots.set(key, promise);
        return promise;
    }

    async function ensureCatalog(
        root: string,
        opts?: { force?: boolean }
    ): Promise<ArtisanCatalog> {
        const force = opts?.force === true;
        const slot = ensureSlot(root);

        if (!force && !slot.catalog && isCatalogInBackoff(root)) {
            throw new Error(slot.error ?? 'Falha recente ao carregar comandos Artisan.');
        }

        const existingPromise = inFlightCatalogs.get(root);
        if (existingPromise && !force) {
            return existingPromise;
        }

        setSlot(root, {
            loading: true,
            ...(force ? { error: null, failedAtMs: null } : {})
        });

        const promise = (async (): Promise<ArtisanCatalog> => {
            try {
                const fetched = force
                    ? await quickCommandsIpc.catalogRefresh(root)
                    : await quickCommandsIpc.catalogGet(root);
                const rawCatalog = markRaw(fetched);
                setSlot(root, {
                    catalog: rawCatalog,
                    loading: false,
                    error: fetched.lastError?.message ?? null,
                    failedAtMs: null
                });
                return rawCatalog;
            } catch (err: unknown) {
                const msg = ipcError.message(err);
                setSlot(root, {
                    loading: false,
                    error: msg,
                    failedAtMs: Date.now()
                });
                throw err;
            } finally {
                inFlightCatalogs.delete(root);
            }
        })();

        inFlightCatalogs.set(root, promise);
        return promise;
    }

    async function loadShortcuts(
        cwd: string,
        opts?: { force?: boolean }
    ): Promise<ShortcutList> {
        const cleanCwd = cwd.trim();
        const force = opts?.force === true;
        const cached = shortcutsByCwd.value[cleanCwd];
        const now = Date.now();
        if (!force && cached && now - cached.at <= SHORTCUTS_TTL_MS) {
            return cached.list;
        }

        const existing = inFlightShortcuts.get(cleanCwd);
        if (existing && !force) {
            return existing;
        }

        const promise = (async (): Promise<ShortcutList> => {
            try {
                const list = await quickCommandsIpc.shortcutList(cleanCwd);
                shortcutsByCwd.value = {
                    ...shortcutsByCwd.value,
                    [cleanCwd]: { list, at: Date.now() }
                };
                return list;
            } finally {
                inFlightShortcuts.delete(cleanCwd);
            }
        })();

        inFlightShortcuts.set(cleanCwd, promise);
        return promise;
    }

    function openCreateModal(
        root: string,
        seed?: { signatureName?: string | undefined; paneId?: PaneId | null | undefined }
    ): void {
        const savedDraft = draftsByRoot.get(root);
        let initialForm: ArtisanCommandForm;

        if (savedDraft) {
            initialForm = cloneForm(savedDraft);
            if (seed?.signatureName && !initialForm.signatureName) {
                initialForm.signatureName = seed.signatureName;
                if (!initialForm.className) {
                    initialForm.className = suggestClassNameFromSignature(seed.signatureName);
                }
            }
        } else {
            initialForm = createEmptyArtisanForm(
                seed?.signatureName ? { signatureName: seed.signatureName } : undefined
            );
        }

        const nextCtxRev = modal.value.contextRevision + 1;
        const nextFormRev = modal.value.formRevision + 1;

        modal.value = {
            open: true,
            root,
            form: initialForm,
            preview: null,
            previewLoading: false,
            report: null,
            creating: false,
            aiLoading: false,
            aiSnapshot: null,
            lastCreateResult: null,
            paneId: seed?.paneId ?? null,
            contextRevision: nextCtxRev,
            formRevision: nextFormRev,
            previewRevision: -1
        };

        void runPreviewForCurrentModal();
    }

    function closeModal(opts?: { preserveDraft?: boolean }): void {
        const preserve = opts?.preserveDraft ?? true;
        const currentRoot = modal.value.root;
        const originPaneId = modal.value.paneId;

        if (preserve && currentRoot) {
            draftsByRoot.set(currentRoot, cloneForm(modal.value.form));
        } else if (currentRoot) {
            draftsByRoot.delete(currentRoot);
        }

        if (activeAiAbort) {
            activeAiAbort.abort();
            activeAiAbort = null;
        }
        if (activeAiGenerator) {
            void activeAiGenerator.cancel();
            activeAiGenerator = null;
        }

        modal.value = {
            ...modal.value,
            open: false,
            creating: false,
            aiLoading: false,
            previewLoading: false,
            contextRevision: modal.value.contextRevision + 1
        };

        getQuickCommandsAdapters().focusPaneInput(originPaneId);
    }

    function updateForm(
        patch: Partial<ArtisanCommandForm>,
        opts?: { immediatePreview?: boolean }
    ): void {
        const merged: ArtisanCommandForm = {
            ...modal.value.form,
            ...patch
        };
        modal.value.form = merged;
        modal.value.formRevision += 1;
        if (modal.value.root) {
            draftsByRoot.set(modal.value.root, cloneForm(merged));
        }
        if (opts?.immediatePreview) {
            void runPreviewForCurrentModal();
        } else {
            triggerPreviewDebounced();
        }
    }

    async function refreshPreviewNow(): Promise<void> {
        await runPreviewForCurrentModal();
    }

    async function create(): Promise<ArtisanCreateResult | null> {
        if (!modal.value.open || !modal.value.root || !canCreate.value || !modal.value.preview) {
            return null;
        }
        const adapters = getQuickCommandsAdapters();
        const reqRoot = modal.value.root;
        const reqCtxRev = modal.value.contextRevision;
        const snapshotForm = cloneForm(modal.value.form);
        const expectedRevision = modal.value.preview.revision;

        modal.value.creating = true;
        try {
            const result = await quickCommandsIpc.create(
                reqRoot,
                snapshotForm,
                expectedRevision
            );
            if (!modal.value.open || modal.value.contextRevision !== reqCtxRev) {
                return result;
            }

            draftsByRoot.delete(reqRoot);
            if (result.manualSteps.length > 0) {
                modal.value.lastCreateResult = result;
                adapters.notifyToast({
                    title: `Comando criado em ${result.path}`,
                    description: 'Revise os passos manuais pendentes antes de fechar.',
                    severity: 'info'
                });
            } else {
                adapters.notifyToast({
                    title: `Comando criado em ${result.path}`,
                    severity: 'success'
                });
                closeModal({ preserveDraft: false });
            }
            return result;
        } catch (err: unknown) {
            const code = ipcError.code(err);
            const msg = ipcError.message(err);
            if (code === 'artisan.preview_stale') {
                adapters.notifyToast({
                    title: 'O preview ficou desatualizado',
                    description: msg,
                    severity: 'warn'
                });
                await runPreviewForCurrentModal();
                return null;
            }
            adapters.notifyToast({
                title: 'Não foi possível criar o comando Artisan',
                description: msg,
                severity: 'error'
            });
            return null;
        } finally {
            if (modal.value.contextRevision === reqCtxRev) {
                modal.value.creating = false;
            }
        }
    }

    async function createWithMaxCode(): Promise<void> {
        if (!modal.value.open || !modal.value.root) {
            return;
        }
        const adapters = getQuickCommandsAdapters();
        const reqRoot = modal.value.root;
        const snapshotForm = cloneForm(modal.value.form);

        try {
            const prompt = await quickCommandsIpc.maxcodePrompt(reqRoot, snapshotForm);
            await adapters.openMaxCodePlan(prompt, reqRoot);
            adapters.notifyToast({
                title: 'Instrução enviada ao MaxCode (Plan Mode)',
                severity: 'info'
            });
        } catch (err: unknown) {
            adapters.notifyToast({
                title: 'Não foi possível enviar para o MaxCode',
                description: ipcError.message(err),
                severity: 'error'
            });
        }
    }

    async function fillWithAi(): Promise<void> {
        if (!modal.value.open || !modal.value.root) {
            return;
        }
        const adapters = getQuickCommandsAdapters();
        const reqRoot = modal.value.root;
        const reqCtxRev = modal.value.contextRevision;
        const reqFormRev = modal.value.formRevision;
        const beforeForm = cloneForm(modal.value.form);

        const promptText =
            (beforeForm.objective ?? '').trim() ||
            beforeForm.description.trim() ||
            beforeForm.signatureName.trim();
        if (!promptText) {
            adapters.notifyToast({
                title: 'Informe o objetivo ou descrição do comando para preencher com IA.',
                severity: 'warn'
            });
            return;
        }

        if (activeAiAbort) {
            activeAiAbort.abort();
        }
        const abortController = new AbortController();
        activeAiAbort = abortController;
        modal.value.aiLoading = true;

        try {
            if (!cachedSchema) {
                cachedSchema = await quickCommandsIpc.formSchema();
            }
            const generator = adapters.createAiGenerator<ArtisanCommandForm>(
                ARTISAN_COMMAND_SCHEMA_ID,
                cachedSchema,
                decodeArtisanCommandForm
            );
            activeAiGenerator = generator;

            const candidate = await generator.run(promptText, {
                signal: abortController.signal,
                paneId: modal.value.paneId
            });

            if (abortController.signal.aborted) {
                return;
            }

            if (
                !modal.value.open ||
                modal.value.root !== reqRoot ||
                modal.value.contextRevision !== reqCtxRev
            ) {
                return;
            }

            if (!candidate) {
                adapters.notifyToast({
                    title: 'Não foi possível gerar uma sugestão válida com a IA.',
                    severity: 'warn'
                });
                return;
            }

            const report = await quickCommandsIpc.validateForm(reqRoot, candidate);
            if (!report.valid) {
                adapters.notifyToast({
                    title: 'A sugestão retornada pela IA contém campos inválidos e foi descartada.',
                    severity: 'warn'
                });
                return;
            }

            if (modal.value.formRevision !== reqFormRev) {
                adapters.notifyToast({
                    title: 'O formulário foi editado durante a geração da IA; suas alterações foram preservadas.',
                    severity: 'info'
                });
                return;
            }

            modal.value.aiSnapshot = beforeForm;
            modal.value.form = cloneForm(candidate);
            modal.value.formRevision += 1;
            draftsByRoot.set(reqRoot, cloneForm(candidate));
            await runPreviewForCurrentModal();
        } catch {
            if (!abortController.signal.aborted) {
                adapters.notifyToast({
                    title: 'Não foi possível gerar uma sugestão válida com a IA.',
                    severity: 'error'
                });
            }
        } finally {
            if (activeAiAbort === abortController) {
                activeAiAbort = null;
                activeAiGenerator = null;
            }
            if (modal.value.contextRevision === reqCtxRev) {
                modal.value.aiLoading = false;
            }
        }
    }

    function undoAiSuggestion(): void {
        if (!modal.value.aiSnapshot) {
            return;
        }
        const restored = cloneForm(modal.value.aiSnapshot);
        modal.value.aiSnapshot = null;
        modal.value.form = restored;
        modal.value.formRevision += 1;
        if (modal.value.root) {
            draftsByRoot.set(modal.value.root, cloneForm(restored));
        }
        void runPreviewForCurrentModal();
    }

    function requestConfirm(
        title: string,
        line: string,
        paneId: PaneId | null = null
    ): Promise<boolean> {
        if (pendingConfirmResolve) {
            const prev = pendingConfirmResolve;
            pendingConfirmResolve = null;
            prev(false);
        }

        confirm.value = {
            open: true,
            title,
            line,
            paneId
        };

        return new Promise<boolean>((resolve) => {
            pendingConfirmResolve = resolve;
        });
    }

    function resolveConfirm(accepted: boolean, currentPaneId?: PaneId | null): void {
        const pending = pendingConfirmResolve;
        const activeConfirm = confirm.value;
        pendingConfirmResolve = null;
        confirm.value = null;

        if (!pending) {
            return;
        }
        if (
            accepted &&
            activeConfirm?.paneId &&
            currentPaneId !== undefined &&
            currentPaneId !== null &&
            activeConfirm.paneId !== currentPaneId
        ) {
            pending(false);
            return;
        }
        pending(accepted);
    }

    function notifyActivePaneChanged(newPaneId: PaneId | null): void {
        if (confirm.value && confirm.value.paneId && confirm.value.paneId !== newPaneId) {
            resolveConfirm(false);
        }
    }

    return {
        rootsByCwd,
        catalogs,
        shortcutsByCwd,
        modal,
        confirm,
        initialized,
        canCreate,
        catalogFor,
        catalogErrorFor,
        isCatalogInBackoff,
        commandByName,
        init,
        dispose,
        resolveRoot,
        ensureCatalog,
        loadShortcuts,
        openCreateModal,
        closeModal,
        updateForm,
        refreshPreviewNow,
        create,
        createWithMaxCode,
        fillWithAi,
        undoAiSuggestion,
        requestConfirm,
        resolveConfirm,
        notifyActivePaneChanged
    };
});
