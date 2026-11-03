import {
    computed,
    getCurrentScope,
    onScopeDispose,
    ref,
    toValue,
    type ComputedRef,
    type MaybeRefOrGetter,
    type Ref
} from 'vue';
import {
    projectContextField,
    projectCostField,
    projectModelField,
    projectQuotaField,
    type TelemetryFieldProjection
} from '@/composables/agents/adapters';
import { useAgentsStore } from '@/stores/agents';
import type { AgentTelemetry_Serialize, TabId } from '@/types/bindings';

const sharedNowMs = ref<number>(Date.now());
let activeSubscribers = 0;
let sharedInterval: ReturnType<typeof setInterval> | null = null;

function retainClock(): void {
    activeSubscribers += 1;
    if (activeSubscribers === 1 && sharedInterval === null) {
        sharedNowMs.value = Date.now();
        sharedInterval = setInterval(() => {
            sharedNowMs.value = Date.now();
        }, 1000);
    }
}

function releaseClock(): void {
    activeSubscribers = Math.max(0, activeSubscribers - 1);
    if (activeSubscribers === 0 && sharedInterval !== null) {
        clearInterval(sharedInterval);
        sharedInterval = null;
    }
}

/** Atualiza manualmente o relógio compartilhado de telemetria (útil para testes determinísticos). */
export function setTelemetryClockNowForTests(nowMs: number): void {
    sharedNowMs.value = nowMs;
}

export interface UseAgentTelemetryReturn {
    telemetry: ComputedRef<AgentTelemetry_Serialize | null>;
    nowMs: Readonly<Ref<number>>;
    staleAfterS: ComputedRef<number>;
    agentActive: ComputedRef<boolean>;
    model: ComputedRef<TelemetryFieldProjection>;
    context: ComputedRef<TelemetryFieldProjection>;
    quota5h: ComputedRef<TelemetryFieldProjection>;
    quotaWeekly: ComputedRef<TelemetryFieldProjection>;
    cost: ComputedRef<TelemetryFieldProjection>;
    refresh: () => Promise<AgentTelemetry_Serialize | null>;
    stop: () => void;
}

/**
 * Composable de projeção reativa da `StatusLine` de uma aba de agente CLI (`spec.md` §6.3, `CA-19`, `CA-34`).
 * Mantém um relógio compartilhado de 1 s para calcular a idade individual por campo sem renovar `observedAtMs`.
 */
export function useAgentTelemetry(
    tabId: MaybeRefOrGetter<TabId>
): UseAgentTelemetryReturn {
    const agentsStore = useAgentsStore();
    retainClock();

    let stopped = false;
    function stop(): void {
        if (stopped) {
            return;
        }
        stopped = true;
        releaseClock();
    }

    if (getCurrentScope()) {
        onScopeDispose(() => {
            stop();
        });
    }

    const telemetry = computed<AgentTelemetry_Serialize | null>(() => {
        const id = toValue(tabId);
        return agentsStore.telemetryForTab(id);
    });

    const staleAfterS = computed<number>(() => {
        return agentsStore.settings?.telemetry.staleAfterS ?? 60;
    });

    const agentActive = computed<boolean>(() => {
        return Boolean(telemetry.value?.agentActive);
    });

    const model = computed<TelemetryFieldProjection>(() => {
        const snap = telemetry.value;
        return projectModelField(
            snap?.model,
            snap?.availability.model,
            sharedNowMs.value,
            staleAfterS.value
        );
    });

    const context = computed<TelemetryFieldProjection>(() => {
        const snap = telemetry.value;
        return projectContextField(
            snap?.context,
            snap?.availability.context,
            sharedNowMs.value,
            staleAfterS.value
        );
    });

    const quota5h = computed<TelemetryFieldProjection>(() => {
        const snap = telemetry.value;
        return projectQuotaField(
            'Cota 5h',
            snap?.quota5h,
            snap?.availability.quota5h,
            sharedNowMs.value,
            staleAfterS.value
        );
    });

    const quotaWeekly = computed<TelemetryFieldProjection>(() => {
        const snap = telemetry.value;
        return projectQuotaField(
            'Cota semanal',
            snap?.quotaWeekly,
            snap?.availability.quotaWeekly,
            sharedNowMs.value,
            staleAfterS.value
        );
    });

    const cost = computed<TelemetryFieldProjection>(() => {
        const snap = telemetry.value;
        return projectCostField(
            snap?.costUsd,
            snap?.availability.costUsd,
            sharedNowMs.value,
            staleAfterS.value
        );
    });

    async function refresh(): Promise<AgentTelemetry_Serialize | null> {
        const id = toValue(tabId);
        return agentsStore.fetchTabTelemetry(id);
    }

    return {
        telemetry,
        nowMs: sharedNowMs,
        staleAfterS,
        agentActive,
        model,
        context,
        quota5h,
        quotaWeekly,
        cost,
        refresh,
        stop
    };
}
