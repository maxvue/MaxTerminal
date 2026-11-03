<template>
    <aside v-if="visible" class="perf-overlay" aria-label="Telemetria de performance de desenvolvimento" data-testid="perf-overlay">
        <header class="perf-overlay__header">
            <span class="perf-overlay__title">Telemetria Dev</span>
            <div class="perf-overlay__actions">
                <MaxButton label="Atualizar" variant="secondary" size="small" class="perf-overlay__btn" data-testid="perf-refresh-btn" @click="refreshBackendSnapshot" />
                <MaxButton label="Exportar JSON" variant="secondary" size="small" class="perf-overlay__btn" data-testid="perf-export-btn" @click="handleExport" />
                <MaxButton label="Fechar" variant="ghost" size="small" class="perf-overlay__btn" data-testid="perf-close-btn" @click="visible = false" />
            </div>
        </header>

        <section class="perf-overlay__section">
            <h3 class="perf-overlay__heading">Busca Global (Última Tecla → Pintura)</h3>
            <div class="perf-overlay__grid">
                <span>p50: {{ searchKeyToPaintSummary.p50Ms }} ms</span>
                <span>p95: {{ searchKeyToPaintSummary.p95Ms }} ms</span>
                <span>p99: {{ searchKeyToPaintSummary.p99Ms }} ms</span>
                <span>Amostras: {{ searchKeyToPaintSummary.count }}</span>
            </div>
            <div v-if="lastSearchSample" class="perf-overlay__subgrid">
                <span>Debounce: {{ lastSearchSample.debounceMs }} ms</span>
                <span>Fila: {{ lastSearchSample.queueMs }} ms</span>
                <span>SQL: {{ lastSearchSample.sqlMs }} ms</span>
                <span>IPC: {{ lastSearchSample.ipcTotalMs }} ms</span>
            </div>
        </section>

        <section class="perf-overlay__section">
            <h3 class="perf-overlay__heading">Terminal (Input → Eco / Pintura)</h3>
            <div class="perf-overlay__grid">
                <span>Consumo p95: {{ terminalConsumeSummary.p95Ms }} ms</span>
                <span>Pintura p95: {{ terminalPaintSummary.p95Ms }} ms</span>
                <span>Quadro p95: {{ frameIntervalSummary.p95Ms }} ms</span>
                <span>Amostras: {{ terminalPaintSummary.count }}</span>
            </div>
        </section>

        <section v-if="backendSnapshot" class="perf-overlay__section">
            <h3 class="perf-overlay__heading">Fluxo PTY e Memória Rust</h3>
            <div class="perf-overlay__grid">
                <span>RSS: {{ formatMegabytes(backendSnapshot.rustRssBytes) }} MiB</span>
                <span>PSS: {{ formatMegabytes(backendSnapshot.rustPssBytes) }} MiB</span>
                <span>Lidos: {{ backendSnapshot.ptyFlow.bytesRead }} B</span>
                <span>ACK: {{ backendSnapshot.ptyFlow.bytesAcked }} B</span>
            </div>
        </section>

        <pre v-if="exportedJson" class="perf-overlay__export" data-testid="perf-exported-json">{{ exportedJson }}</pre>
    </aside>
</template>

<script setup lang="ts">
    import { onBeforeUnmount, onMounted, ref } from 'vue';
    import { MaxButton } from '@maxvue/max-components-ui';
    import { usePerfTelemetry } from '@/composables/dev/usePerfTelemetry';

    const {
        visible,
        backendSnapshot,
        lastSearchSample,
        searchKeyToPaintSummary,
        terminalConsumeSummary,
        terminalPaintSummary,
        frameIntervalSummary,
        recordFrameInterval,
        refreshBackendSnapshot,
        exportJsonSample
    } = usePerfTelemetry();

    const exportedJson = ref('');
    let rafHandle: number | null = null;
    let lastFrameTime = 0;

    function tickFrame(now: number): void {
        if (lastFrameTime > 0) {
            recordFrameInterval(now - lastFrameTime);
        }
        lastFrameTime = now;
        if (visible.value && typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
            rafHandle = window.requestAnimationFrame(tickFrame);
        }
    }

    function formatMegabytes(bytes: number): string {
        return (bytes / (1024 * 1024)).toFixed(1);
    }

    function handleExport(): void {
        exportedJson.value = exportJsonSample();
    }

    onMounted(() => {
        void refreshBackendSnapshot();
        if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
            rafHandle = window.requestAnimationFrame(tickFrame);
        }
    });

    onBeforeUnmount(() => {
        if (rafHandle !== null && typeof window !== 'undefined' && typeof window.cancelAnimationFrame === 'function') {
            window.cancelAnimationFrame(rafHandle);
        }
    });
</script>

<style lang="scss" scoped>
.perf-overlay {
    position: fixed;
    right: 12px;
    bottom: 12px;
    z-index: 9999;
    width: 360px;
    padding: 10px 12px;
    border: 1px solid var(--max-primary-500);
    border-radius: 8px;
    background: var(--mt-ui-surface, var(--background-0));
    color: var(--mt-ui-text-strong, var(--background-950));
    font-size: 11px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 45%);

    &__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        margin-bottom: 8px;
    }

    &__title {
        color: var(--max-primary-400);
        font-weight: 700;
        font-size: 12px;
    }

    &__actions {
        display: flex;
        gap: 4px;
    }

    &__btn {
        padding: 2px 6px;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 4px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 10px;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-400);
        }
    }

    &__section {
        margin-top: 6px;
        padding-top: 6px;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
    }

    &__heading {
        margin: 0 0 4px;
        color: var(--max-info-400);
        font-weight: 600;
        font-size: 11px;
    }

    &__grid,
    &__subgrid {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 2px 8px;
    }

    &__subgrid {
        margin-top: 4px;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    &__export {
        max-height: 120px;
        margin-top: 6px;
        padding: 6px;
        overflow: auto;
        border-radius: 4px;
        background: var(--mt-ui-surface-panel, var(--background-25));
        font-size: 10px;
    }
}
</style>
