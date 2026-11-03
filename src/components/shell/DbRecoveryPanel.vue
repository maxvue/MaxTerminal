<template>
    <section v-if="recoveryState" class="db-recovery-panel" role="alertdialog" aria-labelledby="db-recovery-title" aria-describedby="db-recovery-desc">
        <header class="db-recovery-panel__header">
            <h2 id="db-recovery-title" class="db-recovery-panel__title">Recuperação do banco de dados necessária</h2>
            <span class="db-recovery-panel__code">{{ recoveryState.code }}</span>
        </header>

        <p id="db-recovery-desc" class="db-recovery-panel__reason">{{ recoveryState.reason }}</p>
        <p class="db-recovery-panel__path">Arquivo: <code>{{ recoveryState.db_path }}</code></p>

        <div v-if="recoveryState.available_backups.length > 0" class="db-recovery-panel__backups">
            <label for="db-backup-select" class="db-recovery-panel__label">Backups disponíveis para restauração:</label>
            <select id="db-backup-select" v-model="selectedBackup" class="db-recovery-panel__select" :disabled="settingsStore.loading">
                <option v-for="b in recoveryState.available_backups" :key="b.fileName" :value="b.fileName">
                    {{ b.fileName }} ({{ formatBytes(b.sizeBytes) }})
                </option>
            </select>
        </div>
        <p v-else class="db-recovery-panel__empty-backups">Nenhum backup anterior encontrado no diretório local.</p>

        <p v-if="errorMessage" class="db-recovery-panel__error" role="alert">{{ errorMessage }}</p>

        <footer class="db-recovery-panel__actions">
            <button type="button" class="db-recovery-panel__btn db-recovery-panel__btn--secondary" :disabled="settingsStore.loading" @click="onRetry">
                Tentar reabrir
            </button>
            <button v-if="selectedBackup" type="button" class="db-recovery-panel__btn db-recovery-panel__btn--primary" :disabled="settingsStore.loading" @click="onRestore">
                Restaurar backup selecionado
            </button>
            <button type="button" class="db-recovery-panel__btn db-recovery-panel__btn--danger" :disabled="settingsStore.loading" @click="onReset">
                Recriar banco limpo (preservar cópia em quarentena)
            </button>
        </footer>
    </section>
</template>

<script setup lang="ts">
    import { computed, ref, watchEffect } from 'vue';
    import { useIpcError } from '@/composables/useIpcError';
    import { useSettingsStore } from '@/stores/settings';

    const settingsStore = useSettingsStore();
    const { message: formatError } = useIpcError();

    const recoveryState = computed(() => {
        const health = settingsStore.dbStatus?.health;
        return health?.state === 'recovery_required' ? health : null;
    });

    const selectedBackup = ref<string>('');

    watchEffect(() => {
        const firstBackup = recoveryState.value?.available_backups[0];
        if (firstBackup && !selectedBackup.value) {
            selectedBackup.value = firstBackup.fileName;
        }
    });

    const errorMessage = computed(() =>
        settingsStore.lastError ? formatError(settingsStore.lastError) : null
    );

    function formatBytes(bytes: number): string {
        if (bytes < 1024) {
            return `${bytes} B`;
        }
        if (bytes < 1024 * 1024) {
            return `${(bytes / 1024).toFixed(1)} KiB`;
        }
        return `${(bytes / (1024 * 1024)).toFixed(2)} MiB`;
    }

    async function onRetry(): Promise<void> {
        await settingsStore.recover({ action: 'retry_open' });
    }

    async function onRestore(): Promise<void> {
        if (!selectedBackup.value) {
            return;
        }
        await settingsStore.recover({
            action: 'restore_from_backup',
            backup_file_name: selectedBackup.value
        });
    }

    async function onReset(): Promise<void> {
        await settingsStore.recover({ action: 'reset_database' });
    }
</script>

<style lang="scss" scoped>
    .db-recovery-panel {
        display: flex;
        flex-direction: column;
        gap: 0.875rem;
        max-width: 42rem;
        margin: 2rem auto;
        padding: 1.5rem;
        border-radius: 0.5rem;
        background-color: var(--mt-ui-surface-panel, var(--background-25));
        border: 1px solid var(--max-warning-500);
        color: var(--mt-ui-text-strong, var(--background-950));

        &__header {
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 0.75rem;
        }

        &__title {
            margin: 0;
            font-size: 1.125rem;
            font-weight: 600;
            color: var(--max-warning-400);
        }

        &__code {
            font-family: monospace;
            font-size: 0.75rem;
            padding: 0.125rem 0.5rem;
            border-radius: 0.25rem;
            background-color: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--mt-ui-text-secondary, var(--background-600));
        }

        &__reason,
        &__path,
        &__empty-backups {
            margin: 0;
            font-size: 0.875rem;
            line-height: 1.4;
        }

        &__backups {
            display: flex;
            flex-direction: column;
            gap: 0.375rem;
        }

        &__label {
            font-size: 0.8125rem;
            color: var(--mt-ui-text-secondary, var(--background-600));
        }

        &__select {
            padding: 0.5rem 0.625rem;
            border-radius: 0.375rem;
            background-color: var(--mt-ui-surface, var(--background-0));
            border: 1px solid var(--mt-ui-border, var(--background-100));
            color: var(--mt-ui-text, var(--background-800));
        }

        &__error {
            margin: 0;
            padding: 0.5rem 0.75rem;
            border-radius: 0.375rem;
            background-color: var(--max-danger-900);
            border: 1px solid var(--max-danger-500);
            color: var(--max-danger-200);
            font-size: 0.8125rem;
        }

        &__actions {
            display: flex;
            flex-wrap: wrap;
            gap: 0.5rem;
            margin-top: 0.25rem;
        }

        &__btn {
            padding: 0.5rem 0.875rem;
            border-radius: 0.375rem;
            font-size: 0.8125rem;
            font-weight: 500;
            cursor: pointer;
            border: 1px solid transparent;

            &:disabled {
                opacity: 0.5;
                cursor: not-allowed;
            }

            &--primary {
                background-color: var(--max-primary-600);
                color: var(--max-primary-content, #fff);
            }

            &--secondary {
                background-color: var(--mt-ui-surface-elevated, var(--background-50));
                border-color: var(--mt-ui-border, var(--background-100));
                color: var(--mt-ui-text, var(--background-800));
            }

            &--danger {
                background-color: var(--max-danger-600);
                color: #fff;
            }
        }
    }
</style>
