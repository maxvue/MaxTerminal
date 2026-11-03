<template>
    <span v-if="props.connection && props.connection.kind === 'ssh'" class="ssh-badge" :title="badgeTitle">
        <MaxIcon icon="lucide:server" size="0.6875" class="ssh-badge__icon" />
        <span class="ssh-badge__label">SSH: {{ props.connection.user }}@{{ props.connection.host }}</span>
    </span>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxIcon } from '@maxvue/max-components-ui';
    import type { TemplateConnection_Serialize } from '@/types/bindings';

    export interface SshBadgeProps {
        connection: TemplateConnection_Serialize | null | undefined;
    }

    const props = defineProps<SshBadgeProps>();

    const badgeTitle = computed<string>(() => {
        if (!props.connection || props.connection.kind !== 'ssh') {
            return '';
        }
        return `Conexão SSH remota supervisionada: ${props.connection.user}@${props.connection.host}`;
    });
</script>

<style lang="scss" scoped>
    .ssh-badge {
        display: inline-flex;
        align-items: center;
        gap: 3px;
        max-width: 130px;
        padding: 1px 5px;
        font-size: 0.6875rem;
        border-radius: 4px;
        color: var(--max-warning-400);
        background: color-mix(in srgb, var(--max-warning-500) 12%, transparent);
        border: 1px solid color-mix(in srgb, var(--max-warning-500) 40%, transparent);

        &__icon {
            flex-shrink: 0;
        }

        &__label {
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }
    }
</style>
