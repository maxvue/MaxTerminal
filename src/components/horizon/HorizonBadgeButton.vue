<template>
    <MaxBadgeButton v-if="store.isAvailable" v-model="store.isModalOpen" label="HORIZON" :overlay="overlays" :title="buttonTitle" :aria-label="buttonAriaLabel" size="sm" class="horizon-badge-btn" @click="handleClick" />
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxBadgeButton, type OverlayData } from '@maxvue/max-components-ui';
    import { useHorizonStore } from '@/stores/horizon';

    const store = useHorizonStore();

    function handleClick(): void {
        store.openModal();
    }

    function formatCount(val: number | null | undefined): string {
        if (val === null || val === undefined) {
            return '—';
        }
        return String(val);
    }

    const overlays = computed<OverlayData[]>(() => [
        { value: formatCount(store.pendingCount), color: 'var(--blue-b-500)' },
        { value: formatCount(store.runningCount), color: 'var(--max-success-500)' },
        { value: formatCount(store.failedCount), color: 'var(--max-danger-500)' }
    ]);

    const buttonTitle = computed(() => {
        const p = formatCount(store.pendingCount);
        const r = formatCount(store.runningCount);
        const f = formatCount(store.failedCount);
        return `Laravel Horizon: ${p} pendentes, ${r} em execução, ${f} falhas (Status: ${store.status})`;
    });

    const buttonAriaLabel = computed(() => {
        const p = formatCount(store.pendingCount);
        const r = formatCount(store.runningCount);
        const f = formatCount(store.failedCount);
        return `Painel do Laravel Horizon: ${p} tarefas pendentes, ${r} tarefas em execução, ${f} tarefas com falha. Status: ${store.status}`;
    });
</script>

<style lang="scss" scoped>
    .horizon-badge-btn {
        display: inline-flex;
        align-items: center;
        user-select: none;
    }
</style>
