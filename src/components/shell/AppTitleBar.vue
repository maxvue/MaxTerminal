<template>
    <header class="app-title-bar" flex items-center gap-2 px-3 h-10>
        <MaxIcon icon="mdi:console" class="app-title-bar__icon" />
        <span class="app-title-bar__title" font-semibold>{{ props.title }}</span>
        <span v-if="props.version" class="app-title-bar__version" text-xs op-60>{{ props.version }}</span>
        <div ml-auto flex items-center gap-2>
            <MaxButton type="button" class="app-title-bar__search-btn" title="Buscar em tudo… (Ctrl+K)" aria-label="Buscar em tudo… (Ctrl+K)" variant="outlined" size="small" @click="emit('open-global-search')">
                <MaxIcon icon="lucide:search" />
                <span>Buscar em tudo…</span>
                <kbd class="app-title-bar__kbd">Ctrl+K</kbd>
            </MaxButton>
            <slot name="actions" />
        </div>
    </header>
</template>

<script setup lang="ts">
    import { MaxButton } from '@maxvue/max-components-ui';

    interface AppTitleBarProps {
        title: string;
        version?: string;
    }

    const props = defineProps<AppTitleBarProps>();

    const emit = defineEmits<{
        'open-global-search': [];
    }>();
</script>

<style lang="scss" scoped>
    .app-title-bar {
        height: var(--mt-titlebar-height);
        background: var(--background-800);
        border-bottom: 1px solid var(--background-700);
        user-select: none;
    }

    .app-title-bar__icon {
        color: var(--max-primary-400);
    }

    .app-title-bar__title {
        color: var(--background-50);
    }

    .app-title-bar__version {
        color: var(--max-primary-300);
        font-family: var(--mt-font-mono);
    }

    .app-title-bar__search-btn {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        padding: 3px 10px;
        border: 1px solid var(--background-700);
        border-radius: 5px;
        background: var(--background-900);
        color: var(--background-200);
        font-size: 12px;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
            color: var(--background-50);
        }
    }

    .app-title-bar__kbd {
        padding: 1px 4px;
        border-radius: 3px;
        background: var(--background-800);
        color: var(--background-300);
        font-family: var(--mt-font-mono);
        font-size: 10px;
    }
</style>
