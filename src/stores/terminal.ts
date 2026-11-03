import { defineStore } from 'pinia';
import type { PaneId, PtyId } from '@/types/bindings';
import {
    DEFAULT_FLOW_CONFIG,
    DEFAULT_TERMINAL_PREFERENCES,
    type FlowConfig,
    type InputMode,
    type TerminalInstanceState,
    type TerminalModes,
    type TerminalPreferences
} from '@/types/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { resolveTerminalContext } from '@/terminal/extensions/terminalContext';
import {
    resolveInputMode,
    shouldClearOverrideOnModesChange
} from '@/terminal/input/inputModeResolver';
import { themeService } from '@/terminal/theme/themeService';

/** Estado raiz da store Pinia do Motor de Terminal Híbrido. */
export interface TerminalStoreState {
    instances: Record<PaneId, TerminalInstanceState>;
    preferences: TerminalPreferences;
    flowConfig: FlowConfig;
    activePaneId: PaneId | null;
}

const DEFAULT_MODES: Readonly<TerminalModes> = Object.freeze({
    altScreen: false,
    mouseTracking: 'none',
    bracketedPaste: false,
    applicationCursorKeys: false
});

function computeModeForState(
    paneId: PaneId,
    ptyId: PtyId,
    modes: TerminalModes,
    override: InputMode | null,
    richInputEnabled: boolean
): InputMode {
    const context = resolveTerminalContext({ paneId, ptyId });
    return resolveInputMode({
        context,
        modes,
        override,
        richInputEnabled
    }).mode;
}

export const useTerminalStore = defineStore('terminal', {
    state: (): TerminalStoreState => ({
        instances: {},
        preferences: { ...DEFAULT_TERMINAL_PREFERENCES },
        flowConfig: { ...DEFAULT_FLOW_CONFIG },
        activePaneId: null
    }),

    getters: {
        instance: (state) => {
            return (paneId: PaneId): TerminalInstanceState | undefined => state.instances[paneId];
        },
        isPassthrough: (state) => {
            return (paneId: PaneId): boolean => state.instances[paneId]?.inputMode === 'passthrough';
        },
        activeInstance: (state): TerminalInstanceState | null => {
            if (!state.activePaneId) {
                return null;
            }
            return state.instances[state.activePaneId] ?? null;
        },
        webglCount: (state): number => {
            let count = 0;
            for (const inst of Object.values(state.instances)) {
                if (inst.visible && inst.renderer === 'webgl') {
                    count += 1;
                }
            }
            return count;
        }
    },

    actions: {
        register(paneId: PaneId, ptyId: PtyId): TerminalInstanceState {
            const existing = this.instances[paneId];
            if (existing && existing.ptyId === ptyId) {
                return existing;
            }

            const modes: TerminalModes = { ...DEFAULT_MODES };
            const inputMode = computeModeForState(
                paneId,
                ptyId,
                modes,
                null,
                this.preferences.richInputEnabled
            );
            const activeThemeUi = { ...themeService.getActiveTheme().ui };

            const created: TerminalInstanceState = {
                paneId,
                ptyId,
                cols: 80,
                rows: 24,
                renderer: 'dom',
                modes,
                inputMode,
                inputModeOverride: null,
                title: '',
                visible: true,
                focused: this.activePaneId === paneId,
                flow: {
                    paused: false,
                    pendingBytes: 0,
                    droppedBytes: '0',
                    cursor: null,
                    partialView: false,
                    frameErrors: 0
                },
                search: {
                    open: false,
                    query: '',
                    resultIndex: 0,
                    resultCount: 0,
                    limitExceeded: false,
                    regexError: null
                },
                exit: null,
                themeUi: activeThemeUi
            };

            this.instances[paneId] = created;
            if (this.activePaneId === null) {
                this.activePaneId = paneId;
                created.focused = true;
            }
            return created;
        },

        unregister(paneId: PaneId): void {
            delete this.instances[paneId];
            if (this.activePaneId === paneId) {
                const remaining = Object.keys(this.instances);
                this.activePaneId = remaining[0] ?? null;
                if (this.activePaneId && this.instances[this.activePaneId]) {
                    this.instances[this.activePaneId]!.focused = true;
                }
            }
        },

        patch(paneId: PaneId, partial: Partial<TerminalInstanceState>): void {
            const current = this.instances[paneId];
            if (!current) {
                return;
            }

            const prevModes = current.modes;
            const nextModes = partial.modes ? { ...partial.modes } : prevModes;
            let nextOverride =
                partial.inputModeOverride !== undefined
                    ? partial.inputModeOverride
                    : current.inputModeOverride;

            if (partial.modes && shouldClearOverrideOnModesChange(prevModes, nextModes)) {
                nextOverride = null;
            }

            const shouldRecomputeMode =
                partial.modes !== undefined ||
                partial.inputModeOverride !== undefined;

            const nextInputMode =
                partial.inputMode !== undefined && !shouldRecomputeMode
                    ? partial.inputMode
                    : computeModeForState(
                        paneId,
                        current.ptyId,
                        nextModes,
                        nextOverride,
                        this.preferences.richInputEnabled
                    );

            this.instances[paneId] = {
                ...current,
                ...partial,
                modes: nextModes,
                inputModeOverride: nextOverride,
                inputMode: nextInputMode,
                flow: partial.flow ? { ...current.flow, ...partial.flow } : current.flow,
                search: partial.search ? { ...current.search, ...partial.search } : current.search,
                themeUi: partial.themeUi ? { ...partial.themeUi } : current.themeUi
            };
        },

        setInputModeOverride(paneId: PaneId, mode: InputMode | null): void {
            const current = this.instances[paneId];
            if (!current) {
                return;
            }
            if (
                mode === 'rich' &&
                (current.modes.altScreen || current.modes.mouseTracking !== 'none')
            ) {
                return;
            }
            current.inputModeOverride = mode;
            current.inputMode = computeModeForState(
                paneId,
                current.ptyId,
                current.modes,
                mode,
                this.preferences.richInputEnabled
            );
        },

        toggleInputMode(paneId: PaneId): void {
            const current = this.instances[paneId];
            if (!current) {
                return;
            }
            if (current.modes.altScreen || current.modes.mouseTracking !== 'none') {
                return;
            }
            const next: InputMode = current.inputMode === 'rich' ? 'passthrough' : 'rich';
            this.setInputModeOverride(paneId, next);
        },

        setPreferences(partial: Partial<TerminalPreferences>): void {
            this.preferences = {
                ...this.preferences,
                ...partial,
                macOptionIsMeta: false
            };

            for (const instState of Object.values(this.instances)) {
                instState.inputMode = computeModeForState(
                    instState.paneId,
                    instState.ptyId,
                    instState.modes,
                    instState.inputModeOverride,
                    this.preferences.richInputEnabled
                );
            }

            terminalInstanceRegistry.forEach((instance) => {
                instance.applyPreferences(this.preferences);
            });
        },

        openSearch(paneId: PaneId): void {
            const current = this.instances[paneId];
            if (!current) {
                return;
            }
            current.search = {
                ...current.search,
                open: true
            };
        },

        closeSearch(paneId: PaneId): void {
            const current = this.instances[paneId];
            if (!current) {
                return;
            }
            current.search = {
                ...current.search,
                open: false,
                regexError: null
            };
        },

        setActive(paneId: PaneId | null): void {
            this.activePaneId = paneId;
            for (const inst of Object.values(this.instances)) {
                inst.focused = inst.paneId === paneId;
            }
        }
    }
});
