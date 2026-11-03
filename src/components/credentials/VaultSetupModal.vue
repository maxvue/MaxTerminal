<template>
    <MaxModal :visible="props.visible" title="Configurar Senha Mestra do Cofre" :no-button="true" max-width="30rem" class="vault-modal" :dismissable="!submitting" :close-on-escape="!submitting" @update:visible="emit('update:visible', $event)">
        <form class="vault-modal__form" @submit.prevent="handleSubmit">
            <p class="vault-modal__description">
                Para proteger suas senhas de conexões SSH salvas no SQLite com criptografia ponta a ponta (Argon2id + XChaCha20-Poly1305), defina uma senha mestra.
            </p>

            <div class="vault-modal__field">
                <label for="vault-setup-password" class="vault-modal__label">Senha Mestra (mínimo 6 caracteres)</label>
                <input id="vault-setup-password" v-model="password" type="password" class="vault-modal__input" placeholder="Nova senha mestra..." required autocomplete="new-password" minlength="6">
            </div>

            <div class="vault-modal__field">
                <label for="vault-setup-confirm" class="vault-modal__label">Confirmar Senha Mestra</label>
                <input id="vault-setup-confirm" v-model="confirmPassword" type="password" class="vault-modal__input" placeholder="Repita a senha mestra..." required autocomplete="new-password" minlength="6">
            </div>

            <MaxAlert v-if="localError || vaultStore.error" type="danger" role="alert" class="vault-modal__error">
                {{ localError || vaultStore.error }}
            </MaxAlert>

            <footer class="vault-modal__footer">
                <MaxButton type="button" variant="secondary" class="vault-modal__btn" :disabled="submitting" @click="emit('update:visible', false)">
                    Cancelar
                </MaxButton>
                <MaxButton type="submit" variant="primary" class="vault-modal__btn" :disabled="submitting || !password.trim() || password.length < 6" :loading="submitting">
                    {{ submitting ? 'Configurando...' : 'Configurar Cofre' }}
                </MaxButton>
            </footer>
        </form>
    </MaxModal>
</template>

<script setup lang="ts">
    import { ref, watch } from 'vue';
    import MaxAlert from '@maxvue/max-components-ui/components/MaxAlert';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import MaxModal from '@maxvue/max-components-ui/components/MaxModal';
    import { useVaultStore } from '@/stores/vault';

    export interface VaultSetupModalProps {
        visible: boolean;
    }

    const props = defineProps<VaultSetupModalProps>();

    const emit = defineEmits<{
        'update:visible': [value: boolean];
        configured: [];
    }>();

    const vaultStore = useVaultStore();
    const password = ref<string>('');
    const confirmPassword = ref<string>('');
    const submitting = ref<boolean>(false);
    const localError = ref<string | null>(null);

    watch(
        () => props.visible,
        (isVisible) => {
            if (isVisible) {
                password.value = '';
                confirmPassword.value = '';
                localError.value = null;
            }
        }
    );

    async function handleSubmit(): Promise<void> {
        if (!password.value.trim() || password.value.length < 6) {
            localError.value = 'A senha mestra deve ter pelo menos 6 caracteres.';
            return;
        }

        if (password.value !== confirmPassword.value) {
            localError.value = 'As senhas informadas não conferem.';
            return;
        }

        submitting.value = true;
        localError.value = null;

        try {
            await vaultStore.setup(password.value);
            emit('configured');
            emit('update:visible', false);
        } catch {
            localError.value = vaultStore.error ?? 'Falha ao configurar cofre.';
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
