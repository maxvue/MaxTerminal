<template>
    <section class="bridge-settings" aria-label="Configurações da Integração MaxCode">
        <header v-if="!embedded" class="bridge-settings__header">
            <div>
                <h3 class="bridge-settings__title">Integração Bidirecional com MaxCode</h3>
                <p class="bridge-settings__subtitle">Sincronização de projetos em tempo real, inspeção de erros em Plan Mode e protocolo de Issues.</p>
            </div>
            <MaxTag :severity="bridgeBadgeSeverity" :value="bridgeStore.indicator.label" size="sm" class="bridge-settings__badge" />
        </header>

        <div class="bridge-settings__grid">
            <label class="bridge-settings__row">
                <div class="bridge-settings__row-info">
                    <span class="bridge-settings__row-title">Ativar ponte com MaxCode</span>
                    <span class="bridge-settings__row-desc">Mantém o servidor loopback autenticado em 127.0.0.1 ativo.</span>
                </div>
                <MaxInputCheckbox :model-value="bridgeStore.settings.enabled" data-testid="toggle-enabled" @update:model-value="onToggleEnabledValue" />
            </label>

            <label class="bridge-settings__row">
                <div class="bridge-settings__row-info">
                    <span class="bridge-settings__row-title">Sincronizar projeto ativo</span>
                    <span class="bridge-settings__row-desc">Seleciona automaticamente o mesmo projeto ao alternar no MaxCode ou no MaxTerminal.</span>
                </div>
                <MaxInputCheckbox :model-value="bridgeStore.settings.autoSyncActiveProject" data-testid="toggle-sync-project" @update:model-value="onToggleAutoSyncValue" />
            </label>

            <label class="bridge-settings__row">
                <div class="bridge-settings__row-info">
                    <span class="bridge-settings__row-title">Cadastrar automaticamente projetos desconhecidos</span>
                    <span class="bridge-settings__row-desc">Importa projetos abertos no MaxCode que ainda não estejam na lista.</span>
                </div>
                <MaxInputCheckbox :model-value="bridgeStore.settings.autoRegisterUnknownProject" data-testid="toggle-auto-register" @update:model-value="onToggleAutoRegisterValue" />
            </label>

            <label class="bridge-settings__row">
                <div class="bridge-settings__row-info">
                    <span class="bridge-settings__row-title">Focar janela ao receber comando do MaxCode</span>
                    <span class="bridge-settings__row-desc">Traz o MaxTerminal para frente ao abrir uma aba de Issue a partir do MaxCode.</span>
                </div>
                <MaxInputCheckbox :model-value="bridgeStore.settings.focusWindowOnIncomingCommand" data-testid="toggle-focus-window" @update:model-value="onToggleFocusWindowValue" />
            </label>

            <div class="bridge-settings__row">
                <div class="bridge-settings__row-info">
                    <span class="bridge-settings__row-title">Modo de fallback do botão Inspecionar</span>
                    <span class="bridge-settings__row-desc">Comportamento quando o MaxCode estiver fechado no momento da inspeção.</span>
                </div>
                <MaxInputSelect :model-value="bridgeStore.settings.inspectFallbackMode" :options="fallbackOptions" option-label="label" option-value="value" aria-label="Modo de fallback do botão Inspecionar" class="bridge-settings__select" data-testid="select-fallback-mode" @update:model-value="onChangeFallbackModeComp" />
            </div>

            <div class="bridge-settings__row">
                <div class="bridge-settings__row-info">
                    <span class="bridge-settings__row-title">Porta preferida</span>
                    <span class="bridge-settings__row-desc">Porta TCP opcional (1024–65535). Vazio aloca porta efêmera automática.</span>
                </div>
                <MaxInputNumber :model-value="bridgeStore.settings.preferredPort ?? undefined" :min="1024" :max="65535" :step="1" placeholder="Automática" aria-label="Porta preferida" class="bridge-settings__input" data-testid="input-preferred-port" @update:model-value="onChangePreferredPortComp" />
            </div>
        </div>

        <div class="bridge-settings__telemetry">
            <div class="bridge-settings__stat">
                <span class="bridge-settings__stat-label">Porta ativa</span>
                <span class="bridge-settings__stat-value">{{ bridgeStore.serverPort ?? '—' }}</span>
            </div>
            <div class="bridge-settings__stat">
                <span class="bridge-settings__stat-label">Versão do MaxCode</span>
                <span class="bridge-settings__stat-value">{{ bridgeStore.status.peerVersion ?? 'Desconectado' }}</span>
            </div>
            <div class="bridge-settings__stat">
                <span class="bridge-settings__stat-label">Capabilities</span>
                <span class="bridge-settings__stat-value">{{ capabilitiesSummary }}</span>
            </div>
        </div>

        <footer class="bridge-settings__actions">
            <MaxButton type="button" icon="lucide:refresh-cw" label="Reconectar agora" data-testid="btn-reconnect" @click="onReconnect" />
        </footer>
    </section>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxButton, MaxInputCheckbox, MaxInputNumber, MaxInputSelect, MaxTag } from '@maxvue/max-components-ui';
    import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
    import type { BridgeSettings_Serialize, InspectFallbackMode } from '@/types/bindings';

    export interface BridgeSettingsSectionProps {
        embedded?: boolean;
    }

    withDefaults(defineProps<BridgeSettingsSectionProps>(), {
        embedded: false
    });

    const emit = defineEmits<{
        saved: [settings: BridgeSettings_Serialize];
    }>();

    const bridgeStore = useMaxCodeBridgeStore();

    const bridgeBadgeSeverity = computed<'success' | 'info' | 'danger' | 'secondary'>(() => {
        switch (bridgeStore.indicator.color) {
            case 'success':
                return 'success';
            case 'info':
                return 'info';
            case 'danger':
                return 'danger';
            case 'neutral':
            default:
                return 'secondary';
        }
    });

    const capabilitiesSummary = computed<string>(() => {
        const caps = bridgeStore.peerCapabilities;
        return caps.length > 0 ? caps.join(', ') : 'Nenhuma sessão ativa';
    });

    const fallbackOptions = [
        { label: 'Acordar via URI (vscode://)', value: 'uri_scheme' },
        { label: 'Apenas copiar prompt', value: 'clipboard_only' },
        { label: 'Desativado', value: 'disabled' }
    ];

    async function onChangeFallbackModeComp(val: unknown): Promise<void> {
        const mode = String(val ?? 'uri_scheme') as InspectFallbackMode;
        const updated = await bridgeStore.updateSettings({ inspectFallbackMode: mode });
        emit('saved', updated);
    }

    async function onChangePreferredPortComp(val: unknown): Promise<void> {
        if (val === null || val === undefined || val === '') {
            const updated = await bridgeStore.updateSettings({ preferredPort: null });
            emit('saved', updated);
            return;
        }
        const num = typeof val === 'number' ? val : Number(val);
        if (Number.isNaN(num) || !Number.isInteger(num) || num < 1024 || num > 65535) {
            return;
        }
        const updated = await bridgeStore.updateSettings({ preferredPort: num });
        emit('saved', updated);
    }

    async function onToggleEnabledValue(checked: boolean): Promise<void> {
        const updated = await bridgeStore.updateSettings({ enabled: Boolean(checked) });
        emit('saved', updated);
    }

    async function onToggleAutoSyncValue(checked: boolean): Promise<void> {
        const updated = await bridgeStore.updateSettings({ autoSyncActiveProject: Boolean(checked) });
        emit('saved', updated);
    }

    async function onToggleAutoRegisterValue(checked: boolean): Promise<void> {
        const updated = await bridgeStore.updateSettings({ autoRegisterUnknownProject: Boolean(checked) });
        emit('saved', updated);
    }

    async function onToggleFocusWindowValue(checked: boolean): Promise<void> {
        const updated = await bridgeStore.updateSettings({
            focusWindowOnIncomingCommand: Boolean(checked)
        });
        emit('saved', updated);
    }

    async function onReconnect(): Promise<void> {
        await bridgeStore.reconnectNow();
    }
</script>

<style lang="scss" scoped>
    .bridge-settings {
        display: flex;
        flex-direction: column;
        gap: 1rem;
    }

    .bridge-settings__header {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 1rem;
    }

    .bridge-settings__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .bridge-settings__subtitle {
        margin: 0.25rem 0 0;
        font-size: 0.75rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .bridge-settings__badge {
        white-space: nowrap;
        padding: 0.1875rem 0.5rem;
        border-radius: 999px;
        font-size: 0.6875rem;
        font-weight: 600;

        &.is-success {
            background: color-mix(in srgb, var(--max-success-500) 18%, transparent);
            color: var(--max-success-400);
        }

        &.is-info {
            background: color-mix(in srgb, var(--max-info-500) 18%, transparent);
            color: var(--max-info-400);
        }

        &.is-danger {
            background: color-mix(in srgb, var(--max-danger-500) 18%, transparent);
            color: var(--max-danger-400);
        }

        &.is-neutral {
            background: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--mt-ui-text-muted, var(--background-400));
        }
    }

    .bridge-settings__grid {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
    }

    .bridge-settings__row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 1rem;
        padding: 0.625rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .bridge-settings__row-info {
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
    }

    .bridge-settings__row-title {
        font-size: 0.8125rem;
        font-weight: 500;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .bridge-settings__row-desc {
        font-size: 0.75rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .bridge-settings__select,
    .bridge-settings__input {
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
    }

    .bridge-settings__telemetry {
        display: grid;
        grid-template-columns: repeat(3, 1fr);
        gap: 0.5rem;
    }

    .bridge-settings__stat {
        display: flex;
        flex-direction: column;
        gap: 0.125rem;
        padding: 0.5rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .bridge-settings__stat-label {
        font-size: 0.6875rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .bridge-settings__stat-value {
        font-size: 0.75rem;
        font-family: var(--mt-font-mono);
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .bridge-settings__actions {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
    }

    .bridge-settings__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.375rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-primary-500);
        background: var(--max-primary-600);
        color: var(--max-primary-content, #fff);
        font-size: 0.75rem;
        font-weight: 500;
        cursor: pointer;

        &:hover {
            background: var(--max-primary-500);
        }
    }
</style>
