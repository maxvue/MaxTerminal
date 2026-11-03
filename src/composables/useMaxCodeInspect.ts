import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
import type { InspectDispatchResult_Serialize } from '@/types/bindings';

export interface InspectableBlockLike {
    id: string;
    exitCode?: number | null;
    status?: string;
}

/**
 * Composable reativo para o botão "Inspecionar" de blocos de comando com falha (`exit != 0`).
 */
export function useMaxCodeInspect(block: MaybeRefOrGetter<InspectableBlockLike>): {
    visible: ComputedRef<boolean>;
    loading: ComputedRef<boolean>;
    inspect: () => Promise<InspectDispatchResult_Serialize>;
} {
    const bridgeStore = useMaxCodeBridgeStore();

    const visible = computed<boolean>(() => {
        const current = toValue(block);
        return (
            current.exitCode !== null &&
            current.exitCode !== undefined &&
            current.exitCode !== 0
        );
    });

    const loading = computed<boolean>(() => {
        const current = toValue(block);
        return bridgeStore.isInspecting(current.id);
    });

    const inspect = async (): Promise<InspectDispatchResult_Serialize> => {
        const current = toValue(block);
        return await bridgeStore.inspectCommandError(current.id);
    };

    return {
        visible,
        loading,
        inspect
    };
}
