import { computed, ref, watch, type Ref } from 'vue';
import { useSetting } from '@/composables/useSetting';
import { useSettingsStore } from '@/stores/settings';
import {
    LAYOUT_LEFT_DEFAULT_SIZE,
    LAYOUT_LEFT_MAX_SIZE,
    LAYOUT_LEFT_MIN_SIZE
} from '@/types/projectSettings';
import type { SettingKey } from '@/types/settings';

export interface UseLayoutPersistenceReturn {
    /** Largura percentual da coluna esquerda (`15..=45`, padrão `22`). */
    leftSize: Ref<number>;
    /** Adapter para o evento `@resized` da versão instalada de `splitpanes`. */
    onResized: (e: unknown) => void;
    /** Indica se o layout já pode renderizar o divisor com tamanho hidratado. */
    ready: Ref<boolean>;
    /** Força gravação imediata de qualquer alteração pendente no debounce. */
    flush: () => Promise<void>;
}

/** Normaliza e limita um valor candidato de largura da coluna esquerda para `15..=45`. */
export function normalizeLeftColumnSize(raw: unknown): number {
    if (typeof raw !== 'number' || !Number.isFinite(raw)) {
        return LAYOUT_LEFT_DEFAULT_SIZE;
    }
    return Math.min(Math.max(raw, LAYOUT_LEFT_MIN_SIZE), LAYOUT_LEFT_MAX_SIZE);
}

/**
 * Extrai o tamanho percentual do primeiro painel a partir do payload do `@resized` do `splitpanes`.
 * Aceita apenas payloads contendo exatamente 2 painéis com `size` numérico finito.
 */
export function extractFirstPaneSizeFromSplitpanes(payload: unknown): number | null {
    if (!payload || typeof payload !== 'object') {
        return null;
    }

    let panes: unknown[] | null = null;
    if (Array.isArray(payload)) {
        panes = payload;
    } else if ('panes' in payload && Array.isArray((payload as { panes?: unknown }).panes)) {
        panes = (payload as { panes: unknown[] }).panes;
    }

    if (!panes || panes.length !== 2) {
        return null;
    }

    const first = panes[0];
    const second = panes[1];
    if (
        !first ||
        typeof first !== 'object' ||
        !second ||
        typeof second !== 'object' ||
        !('size' in first) ||
        !('size' in second)
    ) {
        return null;
    }

    const firstSize = (first as { size: unknown }).size;
    const secondSize = (second as { size: unknown }).size;
    if (
        typeof firstSize !== 'number' ||
        !Number.isFinite(firstSize) ||
        typeof secondSize !== 'number' ||
        !Number.isFinite(secondSize)
    ) {
        return null;
    }

    return firstSize;
}

/**
 * Composable de persistência da proporção entre as 2 colunas principais (`layout.columns.left_size`).
 * Usa `useSetting` da Fase 03 com debounce de `300ms` sem criar segundo timer.
 */
export function useLayoutPersistence(
    key: SettingKey = 'layout.columns.left_size'
): UseLayoutPersistenceReturn {
    const settingsStore = useSettingsStore();
    const setting = useSetting(key as 'layout.columns.left_size', LAYOUT_LEFT_DEFAULT_SIZE, {
        debounceMs: 300
    });

    const leftSize = ref<number>(normalizeLeftColumnSize(setting.value.value));
    const ready = computed<boolean>(() => settingsStore.hydrated || !settingsStore.loading);

    watch(
        () => setting.value.value,
        (nextVal) => {
            const normalized = normalizeLeftColumnSize(nextVal);
            if (leftSize.value !== normalized) {
                leftSize.value = normalized;
            }
        },
        { immediate: true }
    );

    function onResized(e: unknown): void {
        const rawFirstSize = extractFirstPaneSizeFromSplitpanes(e);
        if (rawFirstSize === null) {
            return;
        }
        const clamped = normalizeLeftColumnSize(rawFirstSize);
        leftSize.value = clamped;
        setting.value.value = clamped;
    }

    async function flush(): Promise<void> {
        await setting.flush();
    }

    return {
        leftSize,
        onResized,
        ready,
        flush
    };
}
