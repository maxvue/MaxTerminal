import { getCurrentInstance, onBeforeUnmount } from 'vue';
import {
    createAtTriggerDefinition,
    createQuestionTriggerDefinition
} from '@/terminal/triggers/providers/ai';
import { triggerRegistry, type TriggerRegistry } from '@/terminal/triggers/triggerRegistry';
import type { Disposable } from '@/types/terminal';

let refCount = 0;
let registeredDisposables: Disposable[] = [];

/**
 * Registra de forma idempotente os gatilhos `@` (`ai-at`) e `?` (`ai-question`)
 * no `TriggerRegistry` informado (ou no singleton global).
 */
export function registerAiTriggers(registry: TriggerRegistry = triggerRegistry): Disposable {
    if (registry !== triggerRegistry) {
        const d1 = registry.register(createAtTriggerDefinition());
        const d2 = registry.register(createQuestionTriggerDefinition());
        return {
            dispose() {
                d1.dispose();
                d2.dispose();
            }
        };
    }

    if (refCount === 0 && !registry.getById('ai-at') && !registry.getById('ai-question')) {
        registeredDisposables = [
            registry.register(createAtTriggerDefinition()),
            registry.register(createQuestionTriggerDefinition())
        ];
    }
    refCount += 1;

    let disposed = false;
    return {
        dispose() {
            if (disposed) {
                return;
            }
            disposed = true;
            refCount = Math.max(0, refCount - 1);
            if (refCount === 0) {
                while (registeredDisposables.length > 0) {
                    registeredDisposables.pop()?.dispose();
                }
            }
        }
    };
}

/**
 * Composable Vue que mantém os gatilhos `@` e `?` registrados enquanto o componente estiver montado.
 */
export function useAiTriggers(registry: TriggerRegistry = triggerRegistry): {
    unregister: () => void;
} {
    const sub = registerAiTriggers(registry);

    if (getCurrentInstance()) {
        onBeforeUnmount(() => {
            sub.dispose();
        });
    }

    return {
        unregister: () => sub.dispose()
    };
}
