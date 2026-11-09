<template>
    <MaxAlert class="terminal-exit-banner" severity="info" role="status" aria-live="polite">
        <span class="exit-message">{{ exitMessage }}</span>
        <template #actions>
            <MaxButton label="Reiniciar" severity="secondary" size="small" class="restart-btn" @click="onRestartClick" />
        </template>
    </MaxAlert>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxAlert, MaxButton } from '@maxvue/max-components-ui';
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
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
        color: var(--mt-ui-text-strong, var(--background-950));
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
        color: var(--max-primary-content, #fff);
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;

        &:hover {
            background: var(--max-primary-500);
        }
    }
</style>
