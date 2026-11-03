<template>
    <MaxButton type="button" class="shortcut-badge" :class="`shortcut-badge--${variant}`" :title="tooltipText" aria-label="Status do atalho global e preferências do sistema" variant="outlined" size="small" @click="onOpenPreferences">
        <span class="shortcut-badge__dot" />
        <span class="shortcut-badge__key">{{ acceleratorLabel }}</span>
        <span class="shortcut-badge__mode">{{ modeLabel }}</span>
    </MaxButton>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxButton } from '@maxvue/max-components-ui';
    import { useWindowStore } from '@/stores/window';

    const windowStore = useWindowStore();

    const status = computed(() => windowStore.shortcutStatus);

    const variant = computed<'ok' | 'cli' | 'warn'>(() => {
        const st = status.value;
        if (!st) {
            return 'cli';
        }
        if (st.error) {
            return 'warn';
        }
        if (st.registered) {
            return 'ok';
        }
        if (st.backend === 'cli_only') {
            return 'cli';
        }
        return 'warn';
    });

    const acceleratorLabel = computed<string>(() => {
        const st = status.value;
        if (!st) {
            return 'Ctrl+Alt+`';
        }
        return st.effectiveTrigger || st.accelerator || 'Ctrl+Alt+`';
    });

    const modeLabel = computed<string>(() => {
        const st = status.value;
        if (!st) {
            return 'Sistema';
        }
        if (st.error) {
            return 'Conflito';
        }
        if (st.backend === 'portal' && st.registered) {
            return 'Portal';
        }
        if (st.backend === 'plugin' && st.registered) {
            return 'Ativo';
        }
        return 'CLI';
    });

    const tooltipText = computed<string>(() => {
        const st = status.value;
        if (!st) {
            return 'Abrir preferências do sistema e atalho global';
        }
        if (st.error) {
            return `${st.error.message} — Clique para configurar`;
        }
        if (st.backend === 'cli_only') {
            return `Modo CLI (${st.cliCommand}) — Clique para configurar`;
        }
        return `Atalho global ativo (${acceleratorLabel.value}) — Clique para configurar`;
    });

    function onOpenPreferences(): void {
        windowStore.openPreferences('system');
    }
</script>

<style lang="scss" scoped>
    .shortcut-badge {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        height: 1.5rem;
        padding: 0 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--background-700);
        background: var(--background-900);
        color: var(--background-200);
        font-size: 0.6875rem;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
            color: var(--background-50);
        }
    }

    .shortcut-badge__dot {
        width: 0.375rem;
        height: 0.375rem;
        border-radius: 9999px;
        flex-shrink: 0;
    }

    .shortcut-badge__key {
        font-family: var(--mt-font-mono);
        font-weight: 600;
    }

    .shortcut-badge__mode {
        color: var(--background-400);
    }

    .shortcut-badge--ok .shortcut-badge__dot {
        background: var(--max-success-500);
    }

    .shortcut-badge--cli .shortcut-badge__dot {
        background: var(--max-info-500);
    }

    .shortcut-badge--warn {
        border-color: var(--max-warning-500);

        .shortcut-badge__dot {
            background: var(--max-warning-500);
        }
    }
</style>
