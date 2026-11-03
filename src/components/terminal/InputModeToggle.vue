<template>
    <button type="button" class="input-mode-toggle" :class="{ 'is-passthrough': isPassthrough }" :disabled="isForcedByTui" :title="tooltipText" :aria-label="labelText" @click="onToggleClick">
        <span class="mode-dot" />
        <span class="mode-label">{{ labelText }}</span>
    </button>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import type { PaneId } from '@/types/bindings';
    import { useTerminalStore } from '@/stores/terminal';

    export interface InputModeToggleProps {
        paneId: PaneId;
    }

    const props = defineProps<InputModeToggleProps>();
    const store = useTerminalStore();

    const instanceState = computed(() => store.instance(props.paneId));
    const isPassthrough = computed<boolean>(() => instanceState.value?.inputMode === 'passthrough');
    const isForcedByTui = computed<boolean>(() => {
        const modes = instanceState.value?.modes;
        if (!modes) {
            return false;
        }
        return modes.altScreen || modes.mouseTracking !== 'none';
    });

    const labelText = computed<string>(() => {
        return isPassthrough.value ? 'Modo direto (raw)' : 'Input rico';
    });

    const tooltipText = computed<string>(() => {
        if (isForcedByTui.value) {
            return 'Modo direto exigido pela aplicação interativa (TUI) em execução';
        }
        return `${labelText.value} — alternar com Ctrl+Shift+I`;
    });

    function onToggleClick(): void {
        if (isForcedByTui.value) {
            return;
        }
        store.toggleInputMode(props.paneId);
    }
</script>

<style lang="scss" scoped>
    .mode-dot {
        width: 6px;
        height: 6px;
        border-radius: 50%;
        background: var(--max-success-500);
    }

    .input-mode-toggle {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 3px 8px;
        background: var(--background-800);
        border: 1px solid var(--background-700);
        border-radius: 4px;
        color: var(--background-200);
        font-size: 11px;
        cursor: pointer;

        &:disabled {
            opacity: 0.65;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            background: var(--background-700);
            color: var(--background-50);
        }

        &.is-passthrough .mode-dot {
            background: var(--max-info-500);
        }
    }
</style>
