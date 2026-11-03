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
        width: 100vw;
        height: 100vh;
        overflow: hidden;
        background: var(--background-900);
        color: var(--background-100);
    }

    .app-shell__body {
        display: grid;
        grid-template-columns: auto 1fr;
        min-height: 0;
        overflow: hidden;
    }

    .app-shell__sidebar {
        border-right: 1px solid var(--background-700);
        background: var(--background-800);
        min-height: 0;
        overflow: hidden;
    }

    .app-shell__workspace {
        display: flex;
        flex-direction: column;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background: var(--background-900);
    }

    .app-shell__statusbar {
        height: var(--mt-statusbar-height);
        border-top: 1px solid var(--background-700);
        background: var(--background-800);
    }
</style>
