import { computed, reactive, ref } from 'vue';
import { commands, type PerfSnapshotDto } from '@/types/bindings';

/** Amostra individual de latência de busca global (`CA-10`). */
export interface SearchLatencySample {
    capturedAtMs: number;
    debounceMs: number;
    ipcTotalMs: number;
    queueMs: number;
    sqlMs: number;
    serializeMs: number;
    keyToPaintMs: number;
}

/** Amostra individual de latência de eco de terminal (`CA-18`). */
export interface TerminalEchoSample {
    correlationId: string;
    capturedAtMs: number;
    inputToConsumeMs: number;
    inputToPaintMs: number;
}

/** Estatísticas de percentis calculadas no frontend. */
export interface LatencySummary {
    count: number;
    p50Ms: number;
    p95Ms: number;
    p99Ms: number;
    maxMs: number;
}

interface PendingSearchCycle {
    tKey: number;
    tDebounceEnd: number;
    tIpcStart: number;
    tIpcEnd: number;
    queueUs: number;
    sqlUs: number;
    serializeUs: number;
}

interface PendingInputProbe {
    tInput: number;
    tConsume: number | null;
}

const MAX_FRONTEND_SAMPLES = 256;

const searchSamples = ref<SearchLatencySample[]>([]);
const terminalEchoSamples = ref<TerminalEchoSample[]>([]);
const backendSnapshot = ref<PerfSnapshotDto | null>(null);
const frameIntervalsMs = ref<number[]>([]);
const visible = ref(false);

const pendingSearch = reactive<PendingSearchCycle>({
    tKey: 0,
    tDebounceEnd: 0,
    tIpcStart: 0,
    tIpcEnd: 0,
    queueUs: 0,
    sqlUs: 0,
    serializeUs: 0
});

const pendingProbes = new Map<string, PendingInputProbe>();

function computePercentile(sorted: number[], pct: number): number {
    if (sorted.length === 0) {
        return 0;
    }
    const idx = Math.min(
        sorted.length - 1,
        Math.max(0, Math.round((pct / 100) * (sorted.length - 1)))
    );
    return Number((sorted[idx] ?? 0).toFixed(2));
}

function summarizeValues(values: number[]): LatencySummary {
    if (values.length === 0) {
        return {
            count: 0,
            p50Ms: 0,
            p95Ms: 0,
            p99Ms: 0,
            maxMs: 0
        };
    }
    const sorted = [...values].sort((a, b) => a - b);
    return {
        count: sorted.length,
        p50Ms: computePercentile(sorted, 50),
        p95Ms: computePercentile(sorted, 95),
        p99Ms: computePercentile(sorted, 99),
        maxMs: Number((sorted[sorted.length - 1] ?? 0).toFixed(2))
    };
}

/**
 * Composable de telemetria de desenvolvimento para medição de busca (`CA-10`),
 * eco de terminal (`CA-18`), intervalo de quadros (`rAF`) e snapshot IPC (`perf_snapshot`).
 */
export function usePerfTelemetry() {
    function recordSearchKey(now = performance.now()): void {
        pendingSearch.tKey = now;
    }

    function recordSearchDebounceEnd(now = performance.now()): void {
        pendingSearch.tDebounceEnd = now;
        if (pendingSearch.tKey <= 0) {
            pendingSearch.tKey = now;
        }
    }

    function recordSearchIpcSpan(
        tIpcStart: number,
        tIpcEnd: number,
        meta?: { queueUs?: number; sqlUs?: number; serializeUs?: number }
    ): void {
        pendingSearch.tIpcStart = tIpcStart;
        pendingSearch.tIpcEnd = tIpcEnd;
        pendingSearch.queueUs = meta?.queueUs ?? 0;
        pendingSearch.sqlUs = meta?.sqlUs ?? 0;
        pendingSearch.serializeUs = meta?.serializeUs ?? 0;
    }

    function recordSearchPainted(now = performance.now()): SearchLatencySample | null {
        if (pendingSearch.tKey <= 0) {
            return null;
        }
        const tDebounce = pendingSearch.tDebounceEnd > 0 ? pendingSearch.tDebounceEnd : pendingSearch.tKey;
        const sample: SearchLatencySample = {
            capturedAtMs: Date.now(),
            debounceMs: Number(Math.max(0, tDebounce - pendingSearch.tKey).toFixed(2)),
            ipcTotalMs: Number(Math.max(0, pendingSearch.tIpcEnd - pendingSearch.tIpcStart).toFixed(2)),
            queueMs: Number((pendingSearch.queueUs / 1000).toFixed(2)),
            sqlMs: Number((pendingSearch.sqlUs / 1000).toFixed(2)),
            serializeMs: Number((pendingSearch.serializeUs / 1000).toFixed(2)),
            keyToPaintMs: Number(Math.max(0, now - pendingSearch.tKey).toFixed(2))
        };
        searchSamples.value = [
            ...searchSamples.value.slice(-(MAX_FRONTEND_SAMPLES - 1)),
            sample
        ];
        pendingSearch.tKey = 0;
        return sample;
    }

    function recordTerminalInput(correlationId: string, now = performance.now()): void {
        pendingProbes.set(correlationId, {
            tInput: now,
            tConsume: null
        });
    }

    function recordTerminalConsume(correlationId: string, now = performance.now()): void {
        const probe = pendingProbes.get(correlationId);
        if (!probe) {
            return;
        }
        probe.tConsume = now;
    }

    function recordTerminalPaint(
        correlationId: string,
        now = performance.now()
    ): TerminalEchoSample | null {
        const probe = pendingProbes.get(correlationId);
        if (!probe) {
            return null;
        }
        pendingProbes.delete(correlationId);
        const consumeTime = probe.tConsume ?? now;
        const sample: TerminalEchoSample = {
            correlationId,
            capturedAtMs: Date.now(),
            inputToConsumeMs: Number(Math.max(0, consumeTime - probe.tInput).toFixed(2)),
            inputToPaintMs: Number(Math.max(0, now - probe.tInput).toFixed(2))
        };
        terminalEchoSamples.value = [
            ...terminalEchoSamples.value.slice(-(MAX_FRONTEND_SAMPLES - 1)),
            sample
        ];
        return sample;
    }

    function recordFrameInterval(deltaMs: number): void {
        if (deltaMs <= 0 || !Number.isFinite(deltaMs)) {
            return;
        }
        frameIntervalsMs.value = [
            ...frameIntervalsMs.value.slice(-(MAX_FRONTEND_SAMPLES - 1)),
            Number(deltaMs.toFixed(2))
        ];
    }

    async function refreshBackendSnapshot(): Promise<PerfSnapshotDto | null> {
        if (typeof commands.perfSnapshot !== 'function') {
            return null;
        }
        const result = await commands.perfSnapshot();
        if (result.status === 'ok') {
            backendSnapshot.value = result.data;
            return result.data;
        }
        return null;
    }

    const searchKeyToPaintSummary = computed<LatencySummary>(() =>
        summarizeValues(searchSamples.value.map((s) => s.keyToPaintMs))
    );

    const terminalConsumeSummary = computed<LatencySummary>(() =>
        summarizeValues(terminalEchoSamples.value.map((s) => s.inputToConsumeMs))
    );

    const terminalPaintSummary = computed<LatencySummary>(() =>
        summarizeValues(terminalEchoSamples.value.map((s) => s.inputToPaintMs))
    );

    const frameIntervalSummary = computed<LatencySummary>(() =>
        summarizeValues(frameIntervalsMs.value)
    );

    const lastSearchSample = computed<SearchLatencySample | null>(() => {
        const list = searchSamples.value;
        return list.length > 0 ? (list[list.length - 1] ?? null) : null;
    });

    function resetSamples(): void {
        searchSamples.value = [];
        terminalEchoSamples.value = [];
        frameIntervalsMs.value = [];
        pendingProbes.clear();
    }

    function exportJsonSample(): string {
        return JSON.stringify(
            {
                exportedAtIso: new Date().toISOString(),
                searchKeyToPaint: searchKeyToPaintSummary.value,
                lastSearchSample: lastSearchSample.value,
                terminalInputToConsume: terminalConsumeSummary.value,
                terminalInputToPaint: terminalPaintSummary.value,
                frameIntervalsMs: frameIntervalSummary.value,
                backendSnapshot: backendSnapshot.value
            },
            null,
            2
        );
    }

    return {
        visible,
        searchSamples,
        terminalEchoSamples,
        frameIntervalsMs,
        backendSnapshot,
        lastSearchSample,
        searchKeyToPaintSummary,
        terminalConsumeSummary,
        terminalPaintSummary,
        frameIntervalSummary,
        recordSearchKey,
        recordSearchDebounceEnd,
        recordSearchIpcSpan,
        recordSearchPainted,
        recordTerminalInput,
        recordTerminalConsume,
        recordTerminalPaint,
        recordFrameInterval,
        refreshBackendSnapshot,
        resetSamples,
        exportJsonSample
    };
}
