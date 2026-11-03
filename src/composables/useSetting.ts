import { computed, onScopeDispose, ref, type ComputedRef, type Ref, type WritableComputedRef } from 'vue';
import type { MtIpcError } from '@/ipc';
import { useSettingsStore } from '@/stores/settings';
import type { SettingKey, SettingValue } from '@/types/settings';

export interface UseSettingOptions {
    /** Atraso de debounce em milissegundos antes de persistir via IPC (padrão: `150`). */
    debounceMs?: number;
}

export interface UseSettingReturn<K extends SettingKey> {
    value: WritableComputedRef<SettingValue<K>>;
    saving: Readonly<Ref<boolean>>;
    error: ComputedRef<MtIpcError | null>;
    flush: () => Promise<boolean>;
}

/**
 * Composable reativo para leitura e escrita com debounce de uma chave tipada do `SettingsSchema`.
 * Suporta tanto `useSetting(key, options)` quanto `useSetting(key, defaultValue, options)`.
 */
export function useSetting<K extends SettingKey>(
    key: K,
    options?: UseSettingOptions
): UseSettingReturn<K>;
export function useSetting<K extends SettingKey>(
    key: K,
    defaultValue: SettingValue<K>,
    options?: UseSettingOptions
): UseSettingReturn<K>;
export function useSetting<K extends SettingKey>(
    key: K,
    defaultOrOptions?: SettingValue<K> | UseSettingOptions,
    maybeOptions?: UseSettingOptions
): UseSettingReturn<K> {
    const isOptionsObject =
        typeof defaultOrOptions === 'object' &&
        defaultOrOptions !== null &&
        !Array.isArray(defaultOrOptions) &&
        ('debounceMs' in defaultOrOptions || Object.keys(defaultOrOptions).length === 0);

    const explicitDefault = isOptionsObject
        ? undefined
        : (defaultOrOptions as SettingValue<K> | undefined);
    const options: UseSettingOptions = isOptionsObject
        ? (defaultOrOptions as UseSettingOptions)
        : (maybeOptions ?? {});

    const store = useSettingsStore();
    const debounceMs = options.debounceMs ?? 150;
    const saving = ref<boolean>(false);
    let timer: ReturnType<typeof setTimeout> | null = null;
    let pendingValue: SettingValue<K> | undefined;
    let writeQueue: Promise<boolean> = Promise.resolve(true);

    async function persist(next: SettingValue<K>): Promise<boolean> {
        saving.value = true;
        const nextPromise = writeQueue.then(
            () => store.set(key, next),
            () => store.set(key, next)
        );
        writeQueue = nextPromise.catch(() => false);
        try {
            return await nextPromise;
        } finally {
            saving.value = false;
        }
    }

    function schedulePersist(next: SettingValue<K>): void {
        pendingValue = next;
        if (timer !== null) {
            clearTimeout(timer);
        }
        if (debounceMs <= 0) {
            const val = pendingValue;
            pendingValue = undefined;
            void persist(val);
            return;
        }
        timer = setTimeout(() => {
            timer = null;
            if (pendingValue !== undefined) {
                const val = pendingValue;
                pendingValue = undefined;
                void persist(val);
            }
        }, debounceMs);
    }

    async function flush(): Promise<boolean> {
        if (timer !== null) {
            clearTimeout(timer);
            timer = null;
        }
        if (pendingValue !== undefined) {
            const val = pendingValue;
            pendingValue = undefined;
            return persist(val);
        }
        return writeQueue;
    }

    onScopeDispose(() => {
        if (timer !== null) {
            clearTimeout(timer);
            timer = null;
        }
    });

    const value = computed<SettingValue<K>>({
        get: () => {
            const current = store.get(key);
            if (current === undefined && explicitDefault !== undefined) {
                return explicitDefault;
            }
            return current;
        },
        set: (next) => {
            (store.values as Record<string, unknown>)[key] = next;
            schedulePersist(next);
        }
    });

    const error = computed<MtIpcError | null>(() => store.lastError);

    return {
        value,
        saving,
        error,
        flush
    };
}
