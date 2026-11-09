<template>
    <MaxEmptyDiv class="app-empty-state" :icon="props.icon ?? 'mdi:console-line'" :icon-size="1.5">
        <template #default>
            <div class="app-empty-state__badge" flex items-center justify-center w-12 h-12 rounded-full>
                <MaxIcon :icon="props.icon ?? 'mdi:console-line'" size="1.5" class="app-empty-state__icon" />
            </div>
            <h2 class="app-empty-state__title" text-base font-semibold m-0>{{ props.title }}</h2>
            <p v-if="props.description" class="app-empty-state__description" text-sm op-75 m-0>{{ props.description }}</p>
            <div v-if="slots.default" mt-2>
                <slot />
            </div>
        </template>
    </MaxEmptyDiv>
</template>

<script setup lang="ts">
    import { useSlots } from 'vue';
    import { MaxEmptyDiv, MaxIcon } from '@maxvue/max-components-ui';

    interface AppEmptyStateProps {
        icon?: string;
        title: string;
        description?: string;
    }

    const props = defineProps<AppEmptyStateProps>();
    const slots = useSlots();
</script>

<style lang="scss" scoped>
    .app-empty-state {
        width: 100%;
        height: 100%;
        color: var(--background-100);
    }

    .app-empty-state__badge {
        background: var(--background-800);
        border: 1px solid var(--background-700);
    }

    .app-empty-state__icon {
        color: var(--max-primary-400);
    }

    .app-empty-state__title {
        color: var(--background-50);
    }

    .app-empty-state__description {
        max-width: 28rem;
        color: var(--background-300);
    }
</style>
