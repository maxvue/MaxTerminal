import type { Disposable, TerminalExtension, TerminalInstanceHandle } from '@/types/terminal';

const extensions = new Map<string, TerminalExtension>();
const activeInstances = new Set<TerminalInstanceHandle>();
const instanceExtensionDisposables = new Map<TerminalInstanceHandle, Map<string, Disposable>>();

function activateExtensionOnInstance(
    ext: TerminalExtension,
    instance: TerminalInstanceHandle
): void {
    let perInstanceMap = instanceExtensionDisposables.get(instance);
    if (!perInstanceMap) {
        perInstanceMap = new Map<string, Disposable>();
        instanceExtensionDisposables.set(instance, perInstanceMap);
    }
    if (perInstanceMap.has(ext.id)) {
        return;
    }

    try {
        const maybeDisposable = ext.activate(instance);
        if (maybeDisposable && typeof maybeDisposable.dispose === 'function') {
            perInstanceMap.set(ext.id, maybeDisposable);
        } else {
            perInstanceMap.set(ext.id, { dispose: () => {} });
        }
    } catch (err: unknown) {
        console.error('[extension.activate_failed]', ext.id, err);
    }
}

/**
 * Registra uma extensão global do terminal, ativando-a em todas as instâncias vivas e futuras.
 */
export function registerTerminalExtension(ext: TerminalExtension): Disposable {
    extensions.set(ext.id, ext);
    for (const instance of activeInstances) {
        activateExtensionOnInstance(ext, instance);
    }

    return {
        dispose(): void {
            if (extensions.get(ext.id) !== ext) {
                return;
            }
            extensions.delete(ext.id);
            for (const [, map] of instanceExtensionDisposables) {
                const sub = map.get(ext.id);
                if (sub) {
                    map.delete(ext.id);
                    try {
                        sub.dispose();
                    } catch {
                        // Ignora falhas de cleanup da extensão.
                    }
                }
            }
        }
    };
}

/**
 * Conecta todas as extensões registradas a uma instância de terminal recém-criada.
 */
export function attachExtensionsToInstance(instance: TerminalInstanceHandle): Disposable {
    activeInstances.add(instance);
    for (const ext of extensions.values()) {
        activateExtensionOnInstance(ext, instance);
    }

    return {
        dispose(): void {
            activeInstances.delete(instance);
            const map = instanceExtensionDisposables.get(instance);
            if (map) {
                instanceExtensionDisposables.delete(instance);
                for (const sub of map.values()) {
                    try {
                        sub.dispose();
                    } catch {
                        // Ignora falhas de cleanup.
                    }
                }
                map.clear();
            }
        }
    };
}

/** Remove todas as extensões registradas (exclusivo para testes). */
export function clearTerminalExtensionsForTests(): void {
    for (const [, map] of instanceExtensionDisposables) {
        for (const sub of map.values()) {
            try {
                sub.dispose();
            } catch {
                // Ignora falhas de cleanup.
            }
        }
        map.clear();
    }
    instanceExtensionDisposables.clear();
    activeInstances.clear();
    extensions.clear();
}
