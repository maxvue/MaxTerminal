<template>
    <div class="app-shell" :data-loading="props.loading ? 'true' : 'false'">
        <div v-if="slots.titlebar" class="app-shell__titlebar">
            <slot name="titlebar" />
        </div>
        <div class="app-shell__body">
            <aside v-if="slots.sidebar" class="app-shell__sidebar">
                <slot name="sidebar" />
            </aside>
            <main class="app-shell__workspace">
                <slot name="workspace" />
            </main>
        </div>
        <footer v-if="slots.statusbar" class="app-shell__statusbar">
            <slot name="statusbar" />
        </footer>
    </div>
</template>

<script setup lang="ts">
    import { useSlots } from 'vue';

    interface AppShellProps {
        loading?: boolean;
    }

    const props = defineProps<AppShellProps>();
    const slots = useSlots();
</script>

<style lang="scss" scoped>
    .app-shell {
        display: grid;
        grid-template-rows: auto 1fr auto;
        width: 100vw !important;
        height: 100vh;
        overflow: hidden;
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
    }

    .app-shell__body {
        display: grid;
        grid-template-columns: 1fr;
        min-height: 0;
        overflow: hidden;
        width: 100vw;
        height: 100vh;
    }

    .app-shell__sidebar {
        border-right: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
        min-height: 0;
        overflow: hidden;
    }

    .app-shell__workspace {
        display: flex;
        flex-direction: column;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background: var(--mt-ui-surface, var(--background-0));
    }

    .app-shell__statusbar {
        height: var(--mt-statusbar-height);
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }
</style>
