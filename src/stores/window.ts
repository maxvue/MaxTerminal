import type { UnlistenFn } from '@tauri-apps/api/event';
import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { osIpc } from '@/ipc';
import { useProjectsStore } from '@/stores/projects';
import type {
    FocusTabPayload_Serialize,
    LinkContext,
    MtError_Serialize,
    OsEnvironment_Serialize,
    OsRuntimeSnapshot_Serialize,
    PaneId,
    ProjectId,
    QuitOutcome,
    ShortcutStatus_Serialize,
    ShutdownPhase,
    StatusCounters,
    ToggleDecisionDto,
    ToggleSource
} from '@/types/bindings';

const DEFAULT_COUNTERS: StatusCounters = {
    running: 0,
    continuous: 0,
    finishedUnseen: 0,
    errorUnseen: 0,
    idle: 0,
    awaitingInput: 0
};

/**
 * Store reativa do ciclo de vida da janela principal, bandeja do sistema,
 * ambiente gráfico (X11/Wayland), atalho global e coordenador de desligamento (Fase 09).
 */
export const useWindowStore = defineStore('window', () => {
    const visible = ref<boolean>(true);
    const focused = ref<boolean>(true);
    const minimized = ref<boolean>(false);
    const environment = ref<OsEnvironment_Serialize | null>(null);
    const shortcutStatus = ref<ShortcutStatus_Serialize | null>(null);
    const counters = ref<StatusCounters>({ ...DEFAULT_COUNTERS });
    const shuttingDown = ref<boolean>(false);
    const shutdownPhase = ref<ShutdownPhase>('idle');
    const shutdownError = ref<MtError_Serialize | null>(null);
    const preferencesOpen = ref<boolean>(false);
    const preferencesSection = ref<string>('system');
    const projectSettingsProjectId = ref<ProjectId | null>(null);
    const linkContexts = ref<Record<PaneId, LinkContext>>({});
    const pendingFocus = ref<FocusTabPayload_Serialize | null>(null);
    const lastRevision = ref<number>(0);
    const initialized = ref<boolean>(false);

    let initPromise: Promise<void> | null = null;
    const unlisteners: UnlistenFn[] = [];

    const isWayland = computed<boolean>(() => environment.value?.displayServer === 'wayland');
    const trayAvailable = computed<boolean>(() => environment.value?.trayAvailable ?? false);
    const shortcutHealthy = computed<boolean>(() => {
        const st = shortcutStatus.value;
        if (!st) {
            return false;
        }
        return (st.registered || st.backend === 'cli_only') && st.error === null;
    });
    const needsAttention = computed<boolean>(
        () => counters.value.errorUnseen > 0 || counters.value.awaitingInput > 0
    );

    function acceptRevision(rev: number): boolean {
        if (rev <= lastRevision.value) {
            return false;
        }
        lastRevision.value = rev;
        return true;
    }

    function applySnapshot(snap: OsRuntimeSnapshot_Serialize): void {
        if (snap.revision < lastRevision.value) {
            return;
        }
        lastRevision.value = snap.revision;
        visible.value = snap.window.visible;
        focused.value = snap.window.focused;
        minimized.value = snap.window.minimized;
        environment.value = snap.environment;
        shortcutStatus.value = snap.shortcutStatus;
        counters.value = { ...snap.counters };
        shutdownPhase.value = snap.shutdownPhase;
        shuttingDown.value =
            snap.shutdownPhase === 'stopping' ||
            snap.shutdownPhase === 'draining' ||
            snap.shutdownPhase === 'ready_to_exit';
        const nextContexts: Record<PaneId, LinkContext> = {};
        for (const ctx of snap.linkContexts) {
            nextContexts[ctx.paneId] = ctx;
        }
        linkContexts.value = nextContexts;
        if (snap.pendingFocus) {
            pendingFocus.value = snap.pendingFocus;
        }
    }

    async function registerListeners(): Promise<void> {
        const subs = await Promise.all([
            osIpc.onWindowVisibility((ev) => {
                if (!acceptRevision(ev.revision)) {
                    return;
                }
                visible.value = ev.payload.visible;
                focused.value = ev.payload.focused;
                minimized.value = ev.payload.minimized;
            }),
            osIpc.onShortcutStatus((ev) => {
                if (!acceptRevision(ev.revision)) {
                    return;
                }
                shortcutStatus.value = ev.payload;
            }),
            osIpc.onCounters((ev) => {
                if (!acceptRevision(ev.revision)) {
                    return;
                }
                counters.value = { ...ev.payload };
            }),
            osIpc.onOpenPreferences((ev) => {
                if (!acceptRevision(ev.revision)) {
                    return;
                }
                openPreferences(ev.payload.section || 'system');
            }),
            osIpc.onShutdownStarted((ev) => {
                if (!acceptRevision(ev.revision)) {
                    return;
                }
                shuttingDown.value = true;
                shutdownPhase.value = 'stopping';
                shutdownError.value = null;
            }),
            osIpc.onShutdownFailed((ev) => {
                if (!acceptRevision(ev.revision)) {
                    return;
                }
                shuttingDown.value = false;
                shutdownPhase.value = ev.payload.phase;
                shutdownError.value = ev.payload.error;
            }),
            osIpc.onEnvironmentChanged((ev) => {
                if (!acceptRevision(ev.revision)) {
                    return;
                }
                environment.value = ev.payload;
            }),
            osIpc.onLinkContextChanged((ev) => {
                if (!acceptRevision(ev.revision)) {
                    return;
                }
                linkContexts.value = {
                    ...linkContexts.value,
                    [ev.payload.paneId]: ev.payload
                };
            })
        ]);
        unlisteners.push(...subs);
    }

    async function init(): Promise<void> {
        if (initialized.value) {
            return;
        }
        if (initPromise) {
            return initPromise;
        }
        initPromise = (async () => {
            try {
                await registerListeners();
                const snap = await osIpc.uiReady();
                applySnapshot(snap);
                initialized.value = true;
            } finally {
                initPromise = null;
            }
        })();
        return initPromise;
    }

    async function refreshEnvironment(): Promise<OsEnvironment_Serialize> {
        const env = await osIpc.environment(true);
        environment.value = env;
        return env;
    }

    async function toggle(source: ToggleSource = 'ipc'): Promise<ToggleDecisionDto> {
        return osIpc.toggleWindow(source);
    }

    async function show(): Promise<void> {
        await osIpc.showWindow();
    }

    async function hide(): Promise<void> {
        await osIpc.hideWindow();
    }

    async function quit(): Promise<QuitOutcome> {
        return osIpc.requestQuit();
    }

    function openPreferences(section = 'system'): void {
        projectSettingsProjectId.value = null;
        preferencesSection.value = section;
        preferencesOpen.value = true;
    }

    function closePreferences(): void {
        preferencesOpen.value = false;
    }

    function openProjectSettings(id: ProjectId): void {
        const projectsStore = useProjectsStore();
        if (!projectsStore.projectById(id)) {
            return;
        }
        projectsStore.select(id);
        preferencesOpen.value = false;
        projectSettingsProjectId.value = id;
    }

    function closeProjectSettings(): void {
        projectSettingsProjectId.value = null;
    }

    function linkContextForPane(paneId: PaneId): LinkContext | null {
        return linkContexts.value[paneId] ?? null;
    }

    function consumePendingFocus(): FocusTabPayload_Serialize | null {
        const current = pendingFocus.value;
        pendingFocus.value = null;
        return current;
    }

    function dispose(): void {
        while (unlisteners.length > 0) {
            const fn = unlisteners.pop();
            fn?.();
        }
        initialized.value = false;
        initPromise = null;
    }

    function $reset(): void {
        dispose();
        visible.value = true;
        focused.value = true;
        minimized.value = false;
        environment.value = null;
        shortcutStatus.value = null;
        counters.value = { ...DEFAULT_COUNTERS };
        shuttingDown.value = false;
        shutdownPhase.value = 'idle';
        shutdownError.value = null;
        preferencesOpen.value = false;
        preferencesSection.value = 'system';
        projectSettingsProjectId.value = null;
        linkContexts.value = {};
        pendingFocus.value = null;
        lastRevision.value = 0;
    }

    return {
        visible,
        focused,
        minimized,
        environment,
        shortcutStatus,
        counters,
        shuttingDown,
        shutdownPhase,
        shutdownError,
        preferencesOpen,
        preferencesSection,
        projectSettingsProjectId,
        linkContexts,
        pendingFocus,
        lastRevision,
        initialized,
        isWayland,
        trayAvailable,
        shortcutHealthy,
        needsAttention,
        init,
        applySnapshot,
        refreshEnvironment,
        toggle,
        show,
        hide,
        quit,
        openPreferences,
        closePreferences,
        openProjectSettings,
        closeProjectSettings,
        linkContextForPane,
        consumePendingFocus,
        dispose,
        $reset
    };
});
