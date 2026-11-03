<template>
    <div class="quick-commands-host" data-testid="quick-commands-host">
        <ArtisanCommandModal :open="store.modal.open" :root="store.modal.root ?? ''" @close="store.closeModal({ preserveDraft: true })" />

        <div v-if="store.confirm?.open" class="qc-confirm-backdrop" role="alertdialog" aria-modal="true" :aria-label="store.confirm.title" data-testid="shortcut-confirm-dialog" @keydown.esc.stop.prevent="store.resolveConfirm(false)">
            <div class="qc-confirm">
                <header class="qc-confirm__header">
                    <MaxIcon icon="mdi:alert-circle-outline" />
                    <h3 class="qc-confirm__title" data-testid="shortcut-confirm-title">{{ store.confirm.title }}</h3>
                </header>
                <p class="qc-confirm__desc">
                    Revise a linha de comando que será inserida na barra de entrada do terminal:
                </p>
                <pre class="qc-confirm__code" data-testid="shortcut-confirm-line"><code>{{ store.confirm.line }}</code></pre>
                <footer class="qc-confirm__actions">
                    <button type="button" class="qc-confirm__btn qc-confirm__btn--cancel" data-testid="shortcut-confirm-cancel" @click="store.resolveConfirm(false)">
                        Cancelar
                    </button>
                    <button type="button" class="qc-confirm__btn qc-confirm__btn--accept" data-testid="shortcut-confirm-accept" @click="store.resolveConfirm(true)">
                        Inserir no terminal
                    </button>
                </footer>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { onMounted, onUnmounted } from 'vue';
    import ArtisanCommandModal from '@/components/quick-commands/ArtisanCommandModal.vue';
    import { useQuickCommandsStore } from '@/stores/quickCommands';
    import { registerTriggerProviders } from '@/terminal/triggers/providers';
    import type { Disposable } from '@/types/terminal';

    const store = useQuickCommandsStore();
    let providerSub: Disposable | null = null;

    onMounted(() => {
        providerSub = registerTriggerProviders();
    });

    onUnmounted(() => {
        providerSub?.dispose();
        providerSub = null;
    });
</script>

<style lang="scss" scoped>
    .qc-confirm-backdrop {
        position: fixed;
        inset: 0;
        z-index: 1250;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 1rem;
        background: rgb(0 0 0 / 55%);
    }

    .qc-confirm {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        width: min(32rem, 94vw);
        padding: 1rem;
        border: 1px solid var(--background-700);
        border-radius: 0.5rem;
        background: var(--background-900);
        color: var(--background-50);

        &__header {
            display: flex;
            align-items: center;
            gap: 0.5rem;
            color: var(--max-warning-400);
        }

        &__title {
            margin: 0;
            font-size: 0.9375rem;
            font-weight: 600;
            color: var(--background-50);
        }

        &__desc {
            margin: 0;
            font-size: 0.8125rem;
            color: var(--background-300);
        }

        &__code {
            margin: 0;
            padding: 0.5rem 0.625rem;
            border: 1px solid var(--background-800);
            border-radius: 0.375rem;
            background: var(--background-950);
            color: var(--max-primary-400);
            font-family: var(--mt-font-mono);
            font-size: 0.75rem;
            overflow-x: auto;
            white-space: pre-wrap;
        }

        &__actions {
            display: flex;
            align-items: center;
            justify-content: flex-end;
            gap: 0.5rem;
        }

        &__btn {
            padding: 0.375rem 0.75rem;
            border-radius: 0.375rem;
            font-size: 0.8125rem;
            font-weight: 500;
            cursor: pointer;

            &--cancel {
                border: 1px solid var(--background-700);
                background: var(--background-800);
                color: var(--background-200);
            }

            &--accept {
                border: 1px solid var(--max-primary-500);
                background: var(--max-primary-600);
                color: var(--background-50);
            }
        }
    }
</style>
