<template>
    <MaxModal :visible="props.open" title="Autenticação GitHub (`gh` CLI & REST API)" :no-button="true" max-width="36rem" class="github-auth-panel" @update:visible="emit('close')">
        <div class="github-auth-panel__body">
            <div class="github-auth-panel__row">
                <span class="github-auth-panel__label">GitHub CLI (`gh`):</span>
                <span class="github-auth-panel__value">
                    <template v-if="props.status?.ghCli.installed">
                        Instalado ({{ props.status.ghCli.version || 'versão detectada' }}) —
                        <strong>{{ props.status.ghCli.authenticated ? `Autenticado (${props.status.ghCli.accountLogin || 'ativo'})` : 'Não autenticado (`gh auth login`)' }}</strong>
                    </template>
                    <template v-else>Não detectado no PATH</template>
                </span>
            </div>
            <div class="github-auth-panel__row">
                <span class="github-auth-panel__label">Transporte preferencial:</span>
                <span class="github-auth-panel__value">{{ preferredTransportLabel }}</span>
            </div>
            <div class="github-auth-panel__row">
                <span class="github-auth-panel__label">Token REST API:</span>
                <span class="github-auth-panel__value">
                    <template v-if="props.status?.restToken.configured">
                        Configurado ({{ props.status.restToken.maskedHint || 'mascarado' }})
                    </template>
                    <template v-else>Não configurado</template>
                </span>
            </div>
            <div v-if="props.status?.rateLimit" class="github-auth-panel__row">
                <span class="github-auth-panel__label">Rate Limit REST:</span>
                <span class="github-auth-panel__value">{{ props.status.rateLimit.remaining }} / {{ props.status.rateLimit.limit }} restantes</span>
            </div>
            <form class="github-auth-panel__form" @submit.prevent="onSaveToken">
                <label class="github-auth-panel__input-label" for="github-pat-input">Configurar Personal Access Token (fallback REST):</label>
                <div class="github-auth-panel__input-group">
                    <MaxInputText id="github-pat-input" v-model="tokenInput" type="password" class="github-auth-panel__input" placeholder="ghp_... ou github_pat_..." autocomplete="off" />
                    <MaxButton type="submit" label="Salvar Token" class="github-auth-panel__btn github-auth-panel__btn--primary" :disabled="!tokenInput.trim() || saving" />
                    <MaxButton v-if="props.status?.restToken.configured" type="button" label="Remover" severity="danger" class="github-auth-panel__btn github-auth-panel__btn--danger" :disabled="saving" @click="onClearToken" />
                </div>
            </form>
            <MaxAlert v-if="errorMessage" severity="danger" role="alert" :message="errorMessage" class="github-auth-panel__error" />
        </div>
    </MaxModal>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import { MaxAlert, MaxButton, MaxInputText, MaxModal } from '@maxvue/max-components-ui';
    import type { GithubAuthStatus } from '@/types/bindings';

    export interface GithubAuthPanelProps {
        open: boolean;
        status: GithubAuthStatus | null;
    }

    const props = defineProps<GithubAuthPanelProps>();

    const emit = defineEmits<{
        close: [];
        'save-token': [token: string];
        'clear-token': [];
    }>();

    const tokenInput = ref<string>('');
    const saving = ref<boolean>(false);
    const errorMessage = ref<string | null>(null);

    const preferredTransportLabel = computed<string>(() => {
        const transport = props.status?.preferredTransport;
        if (transport === 'gh_cli') {
            return 'gh CLI (primário)';
        }
        if (transport === 'rest') {
            return 'REST API (fallback)';
        }
        return 'Somente cache local SQLite (offline)';
    });

    function onSaveToken(): void {
        const trimmed = tokenInput.value.trim();
        if (!trimmed) {
            errorMessage.value = 'Informe um token GitHub válido.';
            return;
        }
        errorMessage.value = null;
        emit('save-token', trimmed);
        tokenInput.value = '';
    }

    function onClearToken(): void {
        errorMessage.value = null;
        emit('clear-token');
    }
</script>

<style lang="scss" scoped>
    .github-auth-panel {
        position: fixed;
        inset: 0;
        z-index: 60;
        display: flex;
        align-items: center;
        justify-content: center;
        background: var(--mt-ui-overlay, rgb(0 0 0 / 60%));
    }

    .github-auth-panel__card {
        width: 100%;
        max-width: 34rem;
        border-radius: 0.5rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-strong, var(--background-950));
        overflow: hidden;
    }

    .github-auth-panel__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.75rem 1rem;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .github-auth-panel__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
    }

    .github-auth-panel__close {
        border: none;
        background: transparent;
        color: var(--mt-ui-text-secondary, var(--background-600));
        cursor: pointer;
    }

    .github-auth-panel__body {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 1rem;
        font-size: 0.8125rem;
    }

    .github-auth-panel__row {
        display: flex;
        justify-content: space-between;
        gap: 0.75rem;
    }

    .github-auth-panel__label {
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .github-auth-panel__value {
        color: var(--mt-ui-text-strong, var(--background-950));
        text-align: right;
    }

    .github-auth-panel__form {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
        margin-top: 0.5rem;
        padding-top: 0.75rem;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .github-auth-panel__input-label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .github-auth-panel__input-group {
        display: flex;
        gap: 0.5rem;
    }

    .github-auth-panel__input {
        flex: 1;
        height: 2rem;
        padding: 0 0.625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.8125rem;
    }

    .github-auth-panel__btn {
        height: 2rem;
        padding: 0 0.75rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &--primary {
            border-color: var(--max-primary-500);
            background: color-mix(in srgb, var(--max-primary-500) 20%, transparent);
            color: var(--max-primary-400);
        }

        &--danger {
            border-color: var(--max-danger-500);
            background: color-mix(in srgb, var(--max-danger-500) 20%, transparent);
            color: var(--max-danger-300);
        }
    }

    .github-auth-panel__error {
        margin: 0;
        color: var(--max-danger-400);
        font-size: 0.75rem;
    }
</style>
