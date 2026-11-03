<template>
    <div v-if="props.missingCount > 0 || reportMessage" class="label-sync-banner" role="region" aria-label="Sincronização de labels canônicas MaxCode">
        <div class="label-sync-banner__info">
            <MaxIcon icon="lucide:tags" />
            <span v-if="reportMessage">{{ reportMessage }}</span>
            <span v-else>Faltam {{ props.missingCount }} labels canônicas do workflow MaxCode neste repositório.</span>
        </div>
        <div class="label-sync-banner__actions">
            <button v-if="props.missingCount > 0" type="button" class="label-sync-banner__btn" :disabled="syncing" @click="onSync">
                <MaxIcon icon="lucide:refresh-cw" />
                <span>{{ syncing ? 'Sincronizando...' : 'Sincronizar 24 labels MaxCode' }}</span>
            </button>
            <button v-if="reportMessage" type="button" class="label-sync-banner__dismiss" aria-label="Fechar aviso" @click="reportMessage = null">OK</button>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { ref } from 'vue';

    export interface LabelSyncBannerProps {
        missingCount: number;
    }

    const props = defineProps<LabelSyncBannerProps>();

    const emit = defineEmits<{
        sync: [];
    }>();

    const syncing = ref<boolean>(false);
    const reportMessage = ref<string | null>(null);

    async function onSync(): Promise<void> {
        syncing.value = true;
        try {
            emit('sync');
            reportMessage.value = 'Catálogo canônico de 24 labels MaxCode sincronizado.';
        } finally {
            syncing.value = false;
        }
    }
</script>

<style lang="scss" scoped>
    .label-sync-banner {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 0.5rem 0.875rem;
        background: rgb(168 85 247 / 12%);
        border-bottom: 1px solid rgb(168 85 247 / 35%);
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.7813rem;
    }

    .label-sync-banner__info {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .label-sync-banner__actions {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .label-sync-banner__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.25rem 0.625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--max-purple-500, #a855f7);
        background: rgb(168 85 247 / 20%);
        color: var(--max-purple-100, #f3e8ff);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }
    }

    .label-sync-banner__dismiss {
        padding: 0.1875rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: transparent;
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.7188rem;
        cursor: pointer;
    }
</style>
