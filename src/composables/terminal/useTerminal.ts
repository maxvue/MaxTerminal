import {
    computed,
    markRaw,
    onBeforeUnmount,
    onMounted,
    shallowRef,
    unref,
    watch,
    type ComputedRef,
    type Ref,
    type ShallowRef
} from 'vue';
import type { ExitInfo, PaneId, PtyId } from '@/types/bindings';
import type {
    TerminalInstanceHandle,
    TerminalInstanceState,
    TerminalModes
} from '@/types/terminal';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { themeService } from '@/terminal/theme/themeService';

/** Callbacks opcionais do composable `useTerminal`. */
export interface UseTerminalCallbacks {
    onReady?: ((handle: TerminalInstanceHandle) => void) | undefined;
    onExit?: ((exit: ExitInfo) => void) | undefined;
    onTitleChange?: ((title: string) => void) | undefined;
    onModesChange?: ((modes: TerminalModes) => void) | undefined;
}

/** Retorno do composable `useTerminal`. */
export interface UseTerminalReturn {
    handle: ShallowRef<TerminalInstanceHandle | null>;
    state: ComputedRef<TerminalInstanceState | undefined>;
    focus: () => void;
    paste: (text: string) => void;
}

/**
 * Gerencia o vínculo de ciclo de vida entre um componente Vue e a `TerminalInstance` do painel.
 */
export function useTerminal(
    paneId: Ref<PaneId> | PaneId,
    ptyId: Ref<PtyId> | PtyId,
    hostRef: Ref<HTMLElement | null>,
    visibleRef?: Ref<boolean>,
    callbacks?: UseTerminalCallbacks
): UseTerminalReturn {
    const store = useTerminalStore();
    const handle = shallowRef<TerminalInstanceHandle | null>(null);

    const state = computed<TerminalInstanceState | undefined>(() => {
        return store.instance(unref(paneId));
    });

    async function mountCurrent(): Promise<void> {
        const currentPaneId = unref(paneId);
        const currentPtyId = unref(ptyId);
        const host = hostRef.value;
        if (!host) {
            return;
        }

        store.register(currentPaneId, currentPtyId);

        const instance = markRaw(
            terminalInstanceRegistry.getOrCreate({
                paneId: currentPaneId,
                ptyId: currentPtyId,
                prefs: store.preferences,
                theme: themeService.getActiveTheme(),
                flow: store.flowConfig,
                callbacks: {
                    onReady: (h) => {
                        handle.value = markRaw(h);
                        callbacks?.onReady?.(h);
                    },
                    onDimensionsChange: (cols, rows) => {
                        store.patch(currentPaneId, { cols, rows });
                    },
                    onRendererChange: (renderer) => {
                        store.patch(currentPaneId, { renderer });
                    },
                    onModesChange: (modes) => {
                        store.patch(currentPaneId, { modes });
                        callbacks?.onModesChange?.(modes);
                    },
                    onTitleChange: (title) => {
                        store.patch(currentPaneId, { title });
                        callbacks?.onTitleChange?.(title);
                    },
                    onThemeChange: (theme) => {
                        store.patch(currentPaneId, { themeUi: { ...theme.ui } });
                    },
                    onFlowStateChange: (flow) => {
                        store.patch(currentPaneId, { flow });
                    },
                    onExit: (exit) => {
                        store.patch(currentPaneId, { exit });
                        callbacks?.onExit?.(exit);
                    }
                }
            })
        );

        await instance.attach(host);
        if (visibleRef !== undefined) {
            instance.setVisible(visibleRef.value);
            store.patch(currentPaneId, { visible: visibleRef.value });
        }
    }

    onMounted(() => {
        void mountCurrent();
    });

    if (visibleRef) {
        watch(visibleRef, (vis) => {
            const currentPaneId = unref(paneId);
            const instance = terminalInstanceRegistry.get(currentPaneId);
            if (instance) {
                instance.setVisible(vis);
            }
            store.patch(currentPaneId, { visible: vis });
        });
    }

    onBeforeUnmount(() => {
        const currentPaneId = unref(paneId);
        const instance = terminalInstanceRegistry.get(currentPaneId);
        if (instance) {
            instance.detach();
            store.patch(currentPaneId, { visible: false });
        }
    });

    function focus(): void {
        const currentPaneId = unref(paneId);
        const instance = terminalInstanceRegistry.get(currentPaneId);
        instance?.focus();
    }

    function paste(text: string): void {
        const currentPaneId = unref(paneId);
        const instance = terminalInstanceRegistry.get(currentPaneId);
        instance?.paste(text);
    }

    return {
        handle,
        state,
        focus,
        paste
    };
}
