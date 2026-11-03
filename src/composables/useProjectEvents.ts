import type { UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentScope, onScopeDispose } from 'vue';
import * as projectsIpc from '@/ipc/projects';
import { useProjectsStore } from '@/stores/projects';

const EVENT_COALESCE_MS = 50;
const RECOVERY_POLL_INTERVAL_MS = 30_000;
const FOCUS_THROTTLE_MS = 2_000;

export interface UseProjectEventsOptions {
    /** Intervalo de coalescência de eventos antes de disparar `invalidate` (padrão: `50ms`). */
    coalesceMs?: number;
    /** Intervalo do polling periódico de recuperação quando visível (padrão: `30_000ms`). */
    pollIntervalMs?: number;
    /** Throttle mínimo para `focus` / `visibilitychange` (padrão: `2_000ms`). */
    focusThrottleMs?: number;
}

export interface UseProjectEventsReturn {
    start: () => Promise<void>;
    stop: () => void;
    triggerFocusCheck: () => Promise<void>;
}

/**
 * Gerencia o ciclo de vida dos 4 listeners Tauri `project://*`, inicialização ordenada
 * antes do primeiro `projectSnapshot()`, coalescência de `50ms`, polling de recuperação de `30s`
 * e reconciliação de pastas ausentes/raízes ao ganhar foco.
 */
export function useProjectEvents(options: UseProjectEventsOptions = {}): UseProjectEventsReturn {
    const projectsStore = useProjectsStore();
    const coalesceMs = options.coalesceMs ?? EVENT_COALESCE_MS;
    const pollIntervalMs = options.pollIntervalMs ?? RECOVERY_POLL_INTERVAL_MS;
    const focusThrottleMs = options.focusThrottleMs ?? FOCUS_THROTTLE_MS;

    let unlisteners: UnlistenFn[] = [];
    let startPromise: Promise<void> | null = null;
    let started = false;
    let disposed = false;
    let generation = 0;

    let coalesceTimer: ReturnType<typeof setTimeout> | null = null;
    let pendingEvent: { epoch: string; revision: number } | null = null;
    let pollTimer: ReturnType<typeof setInterval> | null = null;
    let lastFocusCheckAt = 0;

    function isDocumentVisible(): boolean {
        if (typeof document === 'undefined') {
            return true;
        }
        return document.visibilityState !== 'hidden';
    }

    function flushPendingInvalidation(): void {
        if (coalesceTimer !== null) {
            clearTimeout(coalesceTimer);
            coalesceTimer = null;
        }
        if (disposed || !pendingEvent) {
            pendingEvent = null;
            return;
        }
        const ev = pendingEvent;
        pendingEvent = null;
        projectsStore.invalidate(ev);
    }

    function scheduleInvalidation(ev: { epoch: string; revision: number }): void {
        if (disposed) {
            return;
        }
        if (
            !pendingEvent ||
            pendingEvent.epoch !== ev.epoch ||
            ev.revision > pendingEvent.revision
        ) {
            pendingEvent = { epoch: ev.epoch, revision: ev.revision };
        }
        if (coalesceMs <= 0) {
            flushPendingInvalidation();
            return;
        }
        if (coalesceTimer === null) {
            coalesceTimer = setTimeout(() => {
                coalesceTimer = null;
                flushPendingInvalidation();
            }, coalesceMs);
        }
    }

    async function triggerFocusCheck(): Promise<void> {
        if (disposed || !isDocumentVisible()) {
            return;
        }
        const now = Date.now();
        if (now - lastFocusCheckAt < focusThrottleMs) {
            return;
        }
        lastFocusCheckAt = now;

        try {
            await projectsStore.refreshMissing();
        } catch {
            // Ignora falha transitória em checagem de foco.
        }
        if (disposed) {
            return;
        }

        const enabledRoots = projectsStore.roots.filter((r) => r.watchEnabled);
        for (const root of enabledRoots) {
            if (disposed) {
                return;
            }
            try {
                await projectsStore.rescanRoot(root.id);
            } catch {
                // Ignora falha individual de rescan.
            }
        }

        if (!disposed) {
            try {
                await projectsStore.resync();
            } catch {
                //Erro já tratado pela store.
            }
        }
    }

    function handleWindowFocus(): void {
        void triggerFocusCheck();
    }

    function handleVisibilityChange(): void {
        if (isDocumentVisible()) {
            void triggerFocusCheck();
        }
    }

    function safeUnlistenAll(list: UnlistenFn[]): void {
        for (const fn of list) {
            try {
                fn();
            } catch {
                // Ignora erro de unlisten.
            }
        }
    }

    async function start(): Promise<void> {
        if (disposed) {
            return;
        }
        if (started) {
            return;
        }
        if (startPromise !== null) {
            return startPromise;
        }

        const myGen = ++generation;
        const localUnlisteners: UnlistenFn[] = [];

        const registerOne = async (
            registrar: (cb: (ev: { epoch: string; revision: number }) => void) => Promise<UnlistenFn>
        ): Promise<void> => {
            const unlisten = await registrar((ev) => {
                if (disposed || myGen !== generation) {
                    return;
                }
                scheduleInvalidation({ epoch: ev.epoch, revision: ev.revision });
            });
            if (disposed || myGen !== generation) {
                try {
                    unlisten();
                } catch {
                    // Ignora erro ao liberar listener tardio.
                }
                return;
            }
            localUnlisteners.push(unlisten);
        };

        startPromise = (async () => {
            try {
                await Promise.all([
                    registerOne(projectsIpc.onProjectDiscovered),
                    registerOne(projectsIpc.onProjectRemoved),
                    registerOne(projectsIpc.onProjectChanged),
                    registerOne(projectsIpc.onProjectRootChanged)
                ]);

                if (disposed || myGen !== generation) {
                    safeUnlistenAll(localUnlisteners);
                    return;
                }

                unlisteners = localUnlisteners;

                if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
                    window.addEventListener('focus', handleWindowFocus);
                }
                if (typeof document !== 'undefined' && typeof document.addEventListener === 'function') {
                    document.addEventListener('visibilitychange', handleVisibilityChange);
                }

                if (pollIntervalMs > 0) {
                    pollTimer = setInterval(() => {
                        if (!disposed && isDocumentVisible()) {
                            void projectsStore.resync().catch(() => {
                                // Ignora falha periódica.
                            });
                        }
                    }, pollIntervalMs);
                }

                await projectsStore.init();
                if (disposed || myGen !== generation) {
                    return;
                }
                started = true;
            } catch (err: unknown) {
                safeUnlistenAll(localUnlisteners);
                unlisteners = [];
                throw err;
            } finally {
                if (myGen === generation) {
                    startPromise = null;
                }
            }
        })();

        return startPromise;
    }

    function stop(): void {
        if (disposed) {
            return;
        }
        disposed = true;
        started = false;
        generation += 1;
        startPromise = null;

        if (coalesceTimer !== null) {
            clearTimeout(coalesceTimer);
            coalesceTimer = null;
        }
        pendingEvent = null;

        if (pollTimer !== null) {
            clearInterval(pollTimer);
            pollTimer = null;
        }

        if (typeof window !== 'undefined' && typeof window.removeEventListener === 'function') {
            window.removeEventListener('focus', handleWindowFocus);
        }
        if (typeof document !== 'undefined' && typeof document.removeEventListener === 'function') {
            document.removeEventListener('visibilitychange', handleVisibilityChange);
        }

        const toClean = unlisteners.splice(0, unlisteners.length);
        safeUnlistenAll(toClean);
    }

    if (getCurrentScope()) {
        onScopeDispose(stop);
    }

    return {
        start,
        stop,
        triggerFocusCheck
    };
}
