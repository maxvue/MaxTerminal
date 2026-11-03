import { getQuickCommandsAdapters } from '@/composables/quick-commands/adapters';
import {
    artisanArgsTriggers,
    artisanTrigger
} from '@/composables/quick-commands/useArtisanTrigger';
import { shortcutTrigger } from '@/composables/quick-commands/useShortcutTrigger';
import { useCommandGroupsStore } from '@/stores/commandGroups';
import { useQuickCommandsStore } from '@/stores/quickCommands';
import {
    commandGroupsTrigger,
    registerCommandGroupsTrigger
} from '@/terminal/triggers/providers/commandGroupsTrigger';
import { triggerRegistry, type TriggerRegistry } from '@/terminal/triggers/triggerRegistry';
import type { Disposable } from '@/types/terminal';

export { commandGroupsTrigger, registerCommandGroupsTrigger };

/**
 * Inicializa as stores `useQuickCommandsStore` e `useCommandGroupsStore` e registra os provedores
 * de gatilho das Fases 14 e 15 (`>` Artisan, `artisan.args:*`, `/` atalhos universais e `>>` Grupos de Comandos).
 */
export function registerTriggerProviders(
    registry?: TriggerRegistry
): Disposable {
    const store = useQuickCommandsStore();
    void store.init().catch(() => {});

    const groupsStore = useCommandGroupsStore();
    void groupsStore.initListeners().catch(() => {});

    const adapters = getQuickCommandsAdapters();
    const regFn = registry
        ? (def: Parameters<TriggerRegistry['register']>[0]) => registry.register(def)
        : adapters.registerTrigger;

    const targetRegistry = registry ?? triggerRegistry;
    const disposables: Disposable[] = [];

    const allDefs = [
        artisanTrigger(store),
        ...artisanArgsTriggers(store),
        shortcutTrigger(store),
        commandGroupsTrigger(groupsStore)
    ];

    for (const def of allDefs) {
        if (!targetRegistry.getById(def.id)) {
            disposables.push(regFn(def));
        }
    }

    return {
        dispose: () => {
            while (disposables.length > 0) {
                const d = disposables.pop();
                d?.dispose();
            }
        }
    };
}
