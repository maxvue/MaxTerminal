import {
    computed,
    getCurrentScope,
    onScopeDispose,
    ref,
    type ComputedRef,
    type Ref
} from 'vue';
import { formatShortcutFromKeyboardEvent } from '@/composables/useShortcutRecorder';
import { useAgentsStore } from '@/stores/agents';
import { useProjectsStore } from '@/stores/projects';
import { registerTerminalExtension } from '@/terminal/extensions/terminalExtensions';
import {
    isCopyShortcut,
    isPasteShortcut,
    isSearchShortcut,
    isToggleInputModeShortcut
} from '@/terminal/keybindings';
import type { AgentKind, ProjectId } from '@/types/bindings';
import type { Disposable, TerminalInstanceHandle } from '@/types/terminal';

/** Fallback estático dos atalhos padrão dos 4 agentes CLI (`AGENTS.md` §5.2). */
export const DEFAULT_AGENT_SHORTCUTS: Readonly<Record<AgentKind, string>> = Object.freeze({
    'claude-code': 'Ctrl+Alt+C',
    codex: 'Ctrl+Alt+X',
    agy: 'Ctrl+Alt+A',
    opencode: 'Ctrl+Alt+O'
});

/** Atalhos reservados pelas Fases 04, 07 e 09 que não podem ser sobrescritos por agentes. */
export const RESERVED_APP_SHORTCUTS: ReadonlySet<string> = new Set([
    'Ctrl+Shift+C',
    'Ctrl+Shift+V',
    'Ctrl+Shift+F',
    'Ctrl+Shift+I',
    'Ctrl+Alt+Backquote'
]);

export interface ShortcutToastNotice {
    title: string;
    severity: 'info' | 'warn' | 'error' | 'success';
}

type ToastListener = (notice: ShortcutToastNotice) => void;
const toastListeners = new Set<ToastListener>();

/** Permite observar ou interceptar avisos de toast emitidos pelos atalhos de agentes em testes. */
export function onAgentShortcutToast(listener: ToastListener): () => void {
    toastListeners.add(listener);
    return () => {
        toastListeners.delete(listener);
    };
}

function showAgentShortcutToast(
    title: string,
    severity: 'info' | 'warn' | 'error' | 'success'
): void {
    const notice: ShortcutToastNotice = { title, severity };
    for (const cb of toastListeners) {
        try {
            cb(notice);
        } catch {
            // Ignora falha de listener de teste.
        }
    }
    void import('@maxvue/max-components-ui')
        .then((mod) => {
            const toastObj = (
                mod as {
                    Toast?: { show?: (opts: { title: string; severity: string }) => void };
                }
            ).Toast;
            toastObj?.show?.(notice);
        })
        .catch(() => {
            // Ignora quando o container de Toast não está montado no ambiente de teste.
        });
}

/** Normaliza uma string de acelerador para o formato canônico `Ctrl+Alt+Shift+Super+<Tecla>`. */
export function normalizeAccelerator(raw: string): string {
    const tokens = raw
        .split('+')
        .map((t) => t.trim())
        .filter((t) => t.length > 0);
    if (tokens.length === 0) {
        return '';
    }

    let hasCtrl = false;
    let hasAlt = false;
    let hasShift = false;
    let hasSuper = false;
    let mainKey = '';

    for (const token of tokens) {
        const lower = token.toLowerCase();
        if (lower === 'ctrl' || lower === 'control') {
            hasCtrl = true;
        } else if (lower === 'alt' || lower === 'option') {
            hasAlt = true;
        } else if (lower === 'shift') {
            hasShift = true;
        } else if (lower === 'super' || lower === 'meta' || lower === 'cmd' || lower === 'win') {
            hasSuper = true;
        } else {
            mainKey = token.length === 1 ? token.toUpperCase() : token;
        }
    }

    if (!mainKey) {
        return '';
    }

    const parts: string[] = [];
    if (hasCtrl) {
        parts.push('Ctrl');
    }
    if (hasAlt) {
        parts.push('Alt');
    }
    if (hasShift) {
        parts.push('Shift');
    }
    if (hasSuper) {
        parts.push('Super');
    }
    parts.push(mainKey);
    return parts.join('+');
}

/** Adiciona o modificador `Shift` a um acelerador canônico. */
export function withShiftModifier(accelerator: string): string {
    const norm = normalizeAccelerator(accelerator);
    if (!norm) {
        return '';
    }
    const parts = norm.split('+');
    if (parts.includes('Shift')) {
        return norm;
    }
    const mainKey = parts.pop()!;
    return normalizeAccelerator([...parts, 'Shift', mainKey].join('+'));
}

function isBlockedTarget(event: KeyboardEvent): boolean {
    const candidates: Array<Element | null> = [];
    if (event.target instanceof Element) {
        candidates.push(event.target);
    }
    if (typeof document !== 'undefined' && document.activeElement instanceof Element) {
        candidates.push(document.activeElement);
    }

    for (const el of candidates) {
        if (!el) {
            continue;
        }
        if (
            el.closest('[data-shortcut-recorder]') ||
            el.closest('[data-modal-trap]') ||
            el.closest('[role="dialog"][aria-modal="true"]')
        ) {
            return true;
        }
    }
    return false;
}

export interface UseAgentShortcutsOptions {
    projectId?: ProjectId | Ref<ProjectId | null> | null;
}

export interface UseAgentShortcutsReturn {
    effectiveShortcuts: ComputedRef<Record<AgentKind, string>>;
    lastToast: Readonly<Ref<ShortcutToastNotice | null>>;
    handleKeyEvent: (event: KeyboardEvent) => boolean;
    dispose: () => void;
}

/**
 * Registra os atalhos dedicados dos 4 agentes CLI (`Ctrl+Alt+C/X/A/O` e `+Shift`)
 * tanto no `window` quanto na extensão de teclado do xterm (`registerTerminalExtension`),
 * prevenindo envio de bytes ao PTY quando o atalho é tratado (`CA-10`).
 */
export function useAgentShortcuts(options?: UseAgentShortcutsOptions): UseAgentShortcutsReturn {
    const agentsStore = useAgentsStore();
    const projectsStore = useProjectsStore();
    const lastToast = ref<ShortcutToastNotice | null>(null);
    const handledEvents = new WeakSet<KeyboardEvent>();

    const effectiveShortcuts = computed<Record<AgentKind, string>>(() => {
        const base: Record<AgentKind, string> = { ...DEFAULT_AGENT_SHORTCUTS };
        for (const info of agentsStore.agents) {
            if (info.descriptor.shortcut) {
                base[info.descriptor.kind] = normalizeAccelerator(info.descriptor.shortcut);
            }
        }
        const overrides = agentsStore.settings?.shortcuts;
        if (overrides) {
            for (const kind of ['claude-code', 'codex', 'agy', 'opencode'] as const) {
                const custom = overrides[kind];
                if (custom && custom.trim().length > 0) {
                    base[kind] = normalizeAccelerator(custom);
                }
            }
        }
        return base;
    });

    function resolveActiveProjectId(): ProjectId | null {
        if (options?.projectId !== undefined && options.projectId !== null) {
            const val =
                typeof options.projectId === 'object' && 'value' in options.projectId
                    ? options.projectId.value
                    : options.projectId;
            if (val) {
                return val;
            }
        }
        return projectsStore.selectedId ?? projectsStore.projects[0]?.id ?? 'default-project';
    }

    function matchAgentShortcut(
        event: KeyboardEvent
    ): { agent: AgentKind; forceIsolation: boolean } | null {
        if (
            event.type !== 'keydown' ||
            event.isComposing ||
            event.repeat ||
            isCopyShortcut(event) ||
            isPasteShortcut(event) ||
            isSearchShortcut(event) ||
            isToggleInputModeShortcut(event)
        ) {
            return null;
        }

        if (agentsStore.settings && agentsStore.settings.shortcutsEnabled === false) {
            return null;
        }

        if (isBlockedTarget(event)) {
            return null;
        }

        const captured = formatShortcutFromKeyboardEvent(event);
        if (!captured.accelerator) {
            return null;
        }
        const pressed = normalizeAccelerator(captured.accelerator);
        if (!pressed || RESERVED_APP_SHORTCUTS.has(pressed)) {
            return null;
        }

        const map = effectiveShortcuts.value;
        for (const kind of ['claude-code', 'codex', 'agy', 'opencode'] as const) {
            const baseAccel = map[kind];
            if (!baseAccel) {
                continue;
            }
            if (pressed === baseAccel) {
                return { agent: kind, forceIsolation: false };
            }
            const shifted = withShiftModifier(baseAccel);
            if (shifted && pressed === shifted) {
                return { agent: kind, forceIsolation: true };
            }
        }

        return null;
    }

    function handleKeyEvent(event: KeyboardEvent): boolean {
        if (handledEvents.has(event)) {
            event.preventDefault();
            event.stopPropagation();
            return true;
        }

        const matched = matchAgentShortcut(event);
        if (!matched) {
            return false;
        }

        handledEvents.add(event);
        event.preventDefault();
        event.stopPropagation();

        const info = agentsStore.agentByKind(matched.agent);
        if (info && !info.installation.installed) {
            const notice: ShortcutToastNotice = {
                title: 'Agente não encontrado no PATH',
                severity: 'warn'
            };
            lastToast.value = notice;
            showAgentShortcutToast(notice.title, notice.severity);
            return true;
        }

        const projectId = resolveActiveProjectId();
        if (!projectId) {
            return true;
        }

        const openIsolatedByDefault = Boolean(
            agentsStore.settings?.isolation.openIsolatedByDefault
        );

        if (matched.forceIsolation || openIsolatedByDefault) {
            agentsStore.openAgentDialog(projectId, matched.agent, true);
            return true;
        }

        void agentsStore
            .openAgent({
                projectId,
                agent: matched.agent,
                isolation: { mode: 'none' },
                focus: true
            })
            .catch(() => {
                // Erro já registrado na store.
            });

        return true;
    }

    const onWindowKeyDown = (event: KeyboardEvent): void => {
        handleKeyEvent(event);
    };

    if (typeof window !== 'undefined') {
        window.addEventListener('keydown', onWindowKeyDown, true);
    }

    const extDisposable: Disposable = registerTerminalExtension({
        id: 'agents.shortcuts',
        activate(instance: TerminalInstanceHandle): Disposable {
            if (instance.xterm.attachCustomKeyEventHandler) {
                instance.xterm.attachCustomKeyEventHandler((event: KeyboardEvent) => {
                    if (event.type !== 'keydown') {
                        return true;
                    }
                    if (handleKeyEvent(event)) {
                        return false;
                    }
                    if (isCopyShortcut(event)) {
                        const sel = instance.xterm.getSelection?.() ?? '';
                        if (
                            sel.length > 0 &&
                            typeof navigator !== 'undefined' &&
                            navigator.clipboard?.writeText
                        ) {
                            void navigator.clipboard.writeText(sel).catch(() => {});
                        }
                        return false;
                    }
                    if (isPasteShortcut(event)) {
                        if (
                            typeof navigator !== 'undefined' &&
                            navigator.clipboard?.readText
                        ) {
                            void navigator.clipboard
                                .readText()
                                .then((text) => {
                                    if (text) {
                                        instance.xterm.paste(text);
                                    }
                                })
                                .catch(() => {});
                        }
                        return false;
                    }
                    return true;
                });
            }

            return {
                dispose(): void {
                    if (instance.xterm.attachCustomKeyEventHandler) {
                        instance.xterm.attachCustomKeyEventHandler((event: KeyboardEvent) => {
                            if (event.type !== 'keydown') {
                                return true;
                            }
                            if (isCopyShortcut(event) || isPasteShortcut(event)) {
                                return false;
                            }
                            return true;
                        });
                    }
                }
            };
        }
    });

    let disposed = false;
    function dispose(): void {
        if (disposed) {
            return;
        }
        disposed = true;
        if (typeof window !== 'undefined') {
            window.removeEventListener('keydown', onWindowKeyDown, true);
        }
        extDisposable.dispose();
    }

    if (getCurrentScope()) {
        onScopeDispose(() => {
            dispose();
        });
    }

    return {
        effectiveShortcuts,
        lastToast,
        handleKeyEvent,
        dispose
    };
}
