<template>
    <button type="button" class="bridge-status-indicator" :class="[`is-${indicator.color}`, { 'is-compact': compact }]" :title="tooltipText" :aria-label="tooltipText" @click="onOpenSettings">
        <span class="bridge-status-indicator__dot" />
        <span v-if="!compact" class="bridge-status-indicator__label">{{ indicator.label }}</span>
    </button>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
    import { useWindowStore } from '@/stores/window';

    export interface BridgeStatusIndicatorProps {
        compact?: boolean;
    }

    withDefaults(defineProps<BridgeStatusIndicatorProps>(), {
        compact: false
    });

    const emit = defineEmits<{
        openSettings: [];
    }>();

    const bridgeStore = useMaxCodeBridgeStore();
    const windowStore = useWindowStore();

    const indicator = computed(() => bridgeStore.indicator);

    const tooltipText = computed<string>(() => {
        const port = bridgeStore.serverPort;
        const connected = bridgeStore.isConnected;
        const peerCount = connected ? 1 : 0;
        if (port !== null && bridgeStore.settings.enabled) {
            const peerLabel = peerCount === 1 ? '1 conexão' : `${peerCount} conexões`;
            return `${indicator.value.label} · Porta ${port} · ${peerLabel}`;
        }
        if (bridgeStore.status.lastError) {
            return `${indicator.value.label}: ${bridgeStore.status.lastError}`;
        }
        return indicator.value.label;
    });

    function onOpenSettings(): void {
        emit('openSettings');
        windowStore.openPreferences('bridge');
    }
</script>

<style lang="scss" scoped>
    .bridge-status-indicator__dot {
        width: 0.5rem;
        height: 0.5rem;
        border-radius: 50%;
        flex-shrink: 0;
    }

    .bridge-status-indicator__label {
        white-space: nowrap;
    }

    .bridge-status-indicator {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        height: 1.5rem;
        padding: 0 0.5rem;
        border-radius: 999px;
        border: 1px solid var(--background-700);
        background: var(--background-900);
        color: var(--background-200);
        font-size: 0.6875rem;
        font-weight: 500;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
            color: var(--background-50);
        }

        &.is-compact {
            padding: 0 0.375rem;
        }

        &.is-success .bridge-status-indicator__dot {
            background: var(--max-success-500);
        }

        &.is-info .bridge-status-indicator__dot {
            background: var(--max-info-500);
        }

        &.is-danger .bridge-status-indicator__dot {
            background: var(--max-danger-500);
        }

        &.is-neutral .bridge-status-indicator__dot {
            background: var(--background-500);
        }
    }
</style>
