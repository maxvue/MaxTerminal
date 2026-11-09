<template>
    <MaxButton type="button" class="input-mode-toggle" :class="isPassthrough ? 'is-passthrough' : ''" :disabled="isForcedByTui" :title="tooltipText" :aria-label="labelText" @click="onToggleClick">
        <span class="mode-dot" />
        <span class="mode-label">{{ labelText }}</span>
    </MaxButton>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxButton } from '@maxvue/max-components-ui';
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
        background: var(--mt-ui-surface-elevated, var(--background-50));
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 4px;
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 11px;
        cursor: pointer;

        &:disabled {
            opacity: 0.65;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            background: var(--mt-ui-surface-hover, var(--background-75));
            color: var(--mt-ui-text-strong, var(--background-950));
        }

        &.is-passthrough .mode-dot {
            background: var(--max-info-500);
        }
    }
</style>
