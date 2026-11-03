<template>
    <div class="terminal-exit-banner" role="status" aria-live="polite">
        <span class="exit-message">{{ exitMessage }}</span>
        <button type="button" class="restart-btn" @click="onRestartClick">
            Reiniciar
        </button>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import type { ExitInfo } from '@/types/bindings';

    export interface TerminalExitBannerProps {
        exit: ExitInfo;
    }

    const props = defineProps<TerminalExitBannerProps>();

    const emit = defineEmits<{
        'restart-requested': [];
    }>();

    const exitMessage = computed<string>(() => {
        if (props.exit.signalName) {
            return `Processo encerrado pelo sinal ${props.exit.signalName}`;
        }
        if (props.exit.signal !== undefined && props.exit.signal !== null) {
            return `Processo encerrado pelo sinal ${props.exit.signal}`;
        }
        const code = props.exit.code ?? 0;
        return `Processo encerrado (código ${code})`;
    });

    function onRestartClick(): void {
        emit('restart-requested');
    }
</script>

<style lang="scss" scoped>
    .terminal-exit-banner {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 8px 12px;
        background: var(--background-900);
        border-top: 1px solid var(--background-700);
        color: var(--background-100);
        font-size: 12px;
    }

    .exit-message {
        font-family: var(--mt-font-mono);
    }

    .restart-btn {
        padding: 4px 10px;
        background: var(--max-primary-600);
        border: 1px solid var(--max-primary-500);
        border-radius: 4px;
        color: var(--background-50);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;

        &:hover {
            background: var(--max-primary-500);
        }
    }
</style>
