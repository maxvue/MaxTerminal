<template>
    <MaxModal :visible="props.visible" title="Desbloquear Cofre de Senhas" :no-button="true" max-width="28rem" class="vault-modal" :dismissable="!submitting" :close-on-escape="!submitting" @update:visible="emit('update:visible', $event)">
        <form class="vault-modal__form" @submit.prevent="handleSubmit">
            <p class="vault-modal__description">
                Digite a senha mestra do MaxTerminal para desbloquear as credenciais SSH salvas.
            </p>

            <div class="vault-modal__field">
                <label for="vault-master-password" class="vault-modal__label">Senha Mestra</label>
                <MaxInputText id="vault-master-password" ref="passwordInputRef" v-model="password" type="password" class="vault-modal__input" placeholder="Digite sua senha mestra..." required autocomplete="current-password" autofocus />
            </div>

            <MaxAlert v-if="localError || vaultStore.error" severity="danger" role="alert" class="vault-modal__error">
                {{ localError || vaultStore.error }}
            </MaxAlert>

            <footer class="vault-modal__footer">
                <MaxButton type="button" severity="secondary" class="vault-modal__btn" :disabled="submitting" @click="emit('update:visible', false)">
                    Cancelar
                </MaxButton>
                <MaxButton type="submit" class="vault-modal__btn" :disabled="submitting || !password.trim()" :loading="submitting">
                    {{ submitting ? 'Desbloqueando...' : 'Desbloquear' }}
                </MaxButton>
            </footer>
        </form>
    </MaxModal>
</template>

<script setup lang="ts">
    import { ref, watch } from 'vue';
    import {
        MaxAlert,
        MaxButton,
        MaxInputText,
        MaxModal
    } from '@maxvue/max-components-ui';
    import { useVaultStore } from '@/stores/vault';

    export interface VaultUnlockModalProps {
        visible: boolean;
    }

    const props = defineProps<VaultUnlockModalProps>();

    const emit = defineEmits<{
        'update:visible': [value: boolean];
        unlocked: [];
    }>();

    const vaultStore = useVaultStore();
    const password = ref<string>('');
    const submitting = ref<boolean>(false);
    const localError = ref<string | null>(null);
    const passwordInputRef = ref<{ focus: () => void } | HTMLInputElement | null>(null);

    watch(
        () => props.visible,
        (isVisible) => {
            if (isVisible) {
                password.value = '';
                localError.value = null;
                setTimeout(() => {
                    passwordInputRef.value?.focus();
                }, 50);
            }
        }
    );

    async function handleSubmit(): Promise<void> {
        if (!password.value.trim()) {
            localError.value = 'Informe a senha mestra.';
            return;
        }

        submitting.value = true;
        localError.value = null;

        try {
            await vaultStore.unlock(password.value);
            emit('unlocked');
            emit('update:visible', false);
        } catch {
            localError.value = vaultStore.error ?? 'Senha mestra incorreta.';
        } finally {
            submitting.value = false;
        }
    }
</script>

<style lang="scss" scoped>
    .vault-modal__form {
        display: flex;
        flex-direction: column;
        gap: 1rem;
        padding: 0.5rem 0;
    }

    .vault-modal__description {
        margin: 0;
        font-size: 0.875rem;
        color: var(--mt-ui-text-muted, var(--background-400));
        line-height: 1.4;
    }

    .vault-modal__field {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .vault-modal__label {
        font-size: 0.8125rem;
        font-weight: 500;
        color: var(--mt-ui-text, var(--background-800));
    }

    .vault-modal__input {
        width: 100%;
        padding: 0.5rem 0.75rem;
        background: var(--mt-ui-surface, var(--background-0));
        border: 1px solid var(--mt-ui-border, var(--background-200));
        border-radius: 0.375rem;
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.875rem;
        outline: none;
        transition: border-color 0.15s ease;

        &:focus {
            border-color: var(--max-primary-500);
        }
    }

    .vault-modal__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        margin-top: 0.5rem;
        padding-top: 0.75rem;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
    }
</style>
