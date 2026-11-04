<template>
    <div class="agent-settings-panel" data-testid="agent-settings-panel">
        <header class="agent-settings-panel__header">
            <div>
                <h2 class="agent-settings-panel__title">Agentes CLI e Telemetria</h2>
                <p class="agent-settings-panel__subtitle">Configure atalhos dedicados, injeção de integração, StatusLine e consulte capacidades verificadas por versão.</p>
            </div>
            <div class="agent-settings-panel__header-actions">
                <MaxButton type="button" variant="secondary" :disabled="agentsStore.loading" data-testid="agent-refresh-btn" @click="onRefreshInstallations">
                    <MaxIcon icon="lucide:refresh-cw" />
                    <span>Redetectar CLIs</span>
                </MaxButton>
                <MaxButton type="button" variant="primary" :disabled="!draftSettings || saving" :loading="saving" data-testid="agent-save-settings-btn" @click="onSave">
                    <MaxIcon icon="lucide:check" />
                    <span>{{ saving ? 'Salvando...' : 'Salvar Alterações' }}</span>
                </MaxButton>
            </div>
        </header>

        <MaxAlert v-if="activeError" type="danger" role="alert" data-testid="agent-settings-error">
            {{ activeError }}
        </MaxAlert>

        <template v-if="draftSettings">
            <section class="agent-settings-panel__section">
                <h3 class="agent-settings-panel__section-title">Preferências Globais</h3>
                <div class="agent-settings-panel__toggles">
                    <label class="agent-settings-panel__checkbox">
                        <input v-model="draftSettings.shortcutsEnabled" type="checkbox" data-testid="toggle-shortcuts-enabled">
                        <span>Habilitar atalhos dedicados de abertura rápida (`Ctrl+Alt+C/X/A/O`)</span>
                    </label>
                    <label class="agent-settings-panel__checkbox">
                        <input v-model="draftSettings.telemetry.enabled" type="checkbox" data-testid="toggle-telemetry-enabled">
                        <span>Exibir telemetria `StatusLine` na barra da aba</span>
                    </label>
                    <label class="agent-settings-panel__checkbox">
                        <input v-model="draftSettings.telemetry.readSessionFiles" type="checkbox" data-testid="toggle-read-session-files">
                        <span>Permitir leitura local de arquivos de sessão/transcript (`.claude/projects`, `.codex/sessions`)</span>
                    </label>
                    <label class="agent-settings-panel__checkbox">
                        <input v-model="draftSettings.attention.enabled" type="checkbox" data-testid="toggle-attention-enabled">
                        <span>Detectar pausas de aprovação/input e destacar aba</span>
                    </label>
                    <label class="agent-settings-panel__checkbox">
                        <input v-model="draftSettings.attention.notify" type="checkbox" data-testid="toggle-attention-notify">
                        <span>Disparar notificação nativa do sistema ao aguardar intervenção</span>
                    </label>
                    <label class="agent-settings-panel__checkbox">
                        <input v-model="draftSettings.isolation.openIsolatedByDefault" type="checkbox" data-testid="toggle-isolation-default">
                        <span>Sugerir isolamento em Git Worktree por padrão ao abrir agentes</span>
                    </label>
                </div>
            </section>

            <section class="agent-settings-panel__section">
                <h3 class="agent-settings-panel__section-title">Agentes, Atalhos e Injeção</h3>
                <div class="agent-settings-panel__agents-grid">
                    <div v-for="info in agentsStore.agents" :key="info.descriptor.kind" class="agent-settings-panel__agent-card" :data-testid="`agent-card-${info.descriptor.kind}`">
                        <div class="agent-settings-panel__agent-head">
                            <div class="agent-settings-panel__agent-identity">
                                <MaxIcon :icon="info.descriptor.icon" />
                                <strong>{{ info.descriptor.displayName }}</strong>
                                <span class="agent-settings-panel__version">{{ info.installation.version ?? (info.installation.installed ? 'Instalado' : 'Não encontrado') }}</span>
                            </div>
                            <label class="agent-settings-panel__switch">
                                <input type="checkbox" :checked="isAgentEnabled(info.descriptor.kind)" :data-testid="`agent-enabled-${info.descriptor.kind}`" @change="onToggleAgentEnabled(info.descriptor.kind, $event)">
                                <span>Ativo</span>
                            </label>
                        </div>

                        <div class="agent-settings-panel__agent-row">
                            <label class="agent-settings-panel__field-label" :for="`shortcut-${info.descriptor.kind}`">Atalho:</label>
                            <input :id="`shortcut-${info.descriptor.kind}`" type="text" class="agent-settings-panel__input" data-shortcut-recorder="true" :value="shortcutFor(info.descriptor.kind, info.descriptor.shortcut)" :data-testid="`shortcut-input-${info.descriptor.kind}`" @input="onShortcutInput(info.descriptor.kind, $event)">
                        </div>

                        <label class="agent-settings-panel__checkbox">
                            <input type="checkbox" :checked="isInjectEnabled(info.descriptor.kind)" :data-testid="`agent-inject-${info.descriptor.kind}`" @change="onToggleInject(info.descriptor.kind, $event)">
                            <span>Injetar hooks/statusLine observacionais</span>
                        </label>
                    </div>
                </div>
            </section>

            <section class="agent-settings-panel__section">
                <h3 class="agent-settings-panel__section-title">Matriz de Capacidades Verificadas</h3>
                <div class="agent-settings-panel__table-wrap">
                    <table class="agent-settings-panel__table" data-testid="capabilities-table">
                        <thead>
                            <tr>
                                <th>Agente</th>
                                <th>Versão</th>
                                <th>Capacidade</th>
                                <th>Suporte</th>
                                <th>Fonte / Motivo</th>
                            </tr>
                        </thead>
                        <tbody>
                            <template v-for="info in agentsStore.agents" :key="`cap-${info.descriptor.kind}`">
                                <tr v-for="(evidence, featKey) in info.capabilities.features" :key="`${info.descriptor.kind}-${featKey}`">
                                    <td>{{ info.descriptor.displayName }}</td>
                                    <td>{{ info.capabilities.binaryVersion ?? '—' }}</td>
                                    <td><code>{{ featKey }}</code></td>
                                    <td>
                                        <span class="agent-settings-panel__support-badge" :class="`agent-settings-panel__support-badge--${evidence.support}`">{{ formatSupport(evidence.support) }}</span>
                                    </td>
                                    <td>{{ evidence.reason || evidence.source || '—' }}</td>
                                </tr>
                            </template>
                        </tbody>
                    </table>
                </div>
            </section>
        </template>
    </div>
</template>

<script setup lang="ts">
    import { computed, onMounted, ref, watch } from 'vue';
    import MaxAlert from '@maxvue/max-components-ui/components/MaxAlert';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import { normalizeAccelerator as normalizeShortcutCombo } from '@/composables/useAgentShortcuts';
    import { useAgentsStore } from '@/stores/agents';
    import type { AgentKind, AgentsSettings_Serialize, CapabilitySupport } from '@/types/bindings';

    const agentsStore = useAgentsStore();

    const draftSettings = ref<AgentsSettings_Serialize | null>(null);
    const saving = ref<boolean>(false);
    const localError = ref<string | null>(null);
    const activeError = computed<string | null>(() => localError.value || agentsStore.error?.message || null);

    function cloneSettings(src: AgentsSettings_Serialize): AgentsSettings_Serialize {
        return {
            shortcutsEnabled: src.shortcutsEnabled,
            shortcuts: { ...src.shortcuts },
            isolation: { ...src.isolation },
            attention: { ...src.attention },
            telemetry: { ...src.telemetry },
            inject: { ...src.inject },
            enabled: { ...src.enabled }
        };
    }

    watch(
        () => agentsStore.settings,
        (next) => {
            if (next) {
                draftSettings.value = cloneSettings(next);
            }
        },
        { immediate: true }
    );

    onMounted(() => {
        if (!agentsStore.initialized) {
            void agentsStore.init().catch(() => {
                // Erro já registrado na store.
            });
        }
    });

    function shortcutFor(kind: AgentKind, fallback: string): string {
        return draftSettings.value?.shortcuts[kind] ?? fallback;
    }

    function onShortcutInput(kind: AgentKind, ev: Event): void {
        if (!draftSettings.value) {
            return;
        }
        const target = ev.target as HTMLInputElement;
        draftSettings.value.shortcuts = {
            ...draftSettings.value.shortcuts,
            [kind]: target.value.trim()
        };
        localError.value = null;
    }

    function isAgentEnabled(kind: AgentKind): boolean {
        return draftSettings.value?.enabled[kind] !== false;
    }

    function onToggleAgentEnabled(kind: AgentKind, ev: Event): void {
        if (!draftSettings.value) {
            return;
        }
        const checked = (ev.target as HTMLInputElement).checked;
        draftSettings.value.enabled = {
            ...draftSettings.value.enabled,
            [kind]: checked
        };
    }

    function isInjectEnabled(kind: AgentKind): boolean {
        return draftSettings.value?.inject[kind] !== false;
    }

    function onToggleInject(kind: AgentKind, ev: Event): void {
        if (!draftSettings.value) {
            return;
        }
        const checked = (ev.target as HTMLInputElement).checked;
        draftSettings.value.inject = {
            ...draftSettings.value.inject,
            [kind]: checked
        };
    }

    function validateShortcutsCollision(): string | null {
        if (!draftSettings.value) {
            return null;
        }
        const seen = new Map<string, AgentKind>();
        for (const info of agentsStore.agents) {
            const raw = shortcutFor(info.descriptor.kind, info.descriptor.shortcut);
            const norm = normalizeShortcutCombo(raw);
            if (!norm) {
                continue;
            }
            const existing = seen.get(norm);
            if (existing && existing !== info.descriptor.kind) {
                return `Conflito de atalho detectado (${raw}) entre ${existing} e ${info.descriptor.kind} (agents.shortcut_conflict).`;
            }
            seen.set(norm, info.descriptor.kind);
        }
        return null;
    }

    async function onRefreshInstallations(): Promise<void> {
        localError.value = null;
        try {
            await agentsStore.refreshInstallations();
        } catch {
            // Erro já refletido na store.
        }
    }

    async function onSave(): Promise<void> {
        if (!draftSettings.value) {
            return;
        }
        const collision = validateShortcutsCollision();
        if (collision) {
            localError.value = collision;
            return;
        }
        localError.value = null;
        saving.value = true;
        try {
            await agentsStore.updateSettings(draftSettings.value);
        } catch {
            // Erro já refletido em agentsStore.error.
        } finally {
            saving.value = false;
        }
    }

    function formatSupport(support: CapabilitySupport): string {
        switch (support) {
            case 'available':
                return 'Disponível';
            case 'unsupported':
                return 'Não suportado';
            case 'unverified':
                return 'Não verificado';
            case 'disabled':
                return 'Desativado';
        }
    }
</script>

<style lang="scss" scoped>
    .agent-settings-panel {
        display: flex;
        flex-direction: column;
        gap: 1.25rem;
        padding: 1rem;
        color: var(--background-100);
    }

    .agent-settings-panel__header {
        display: flex;
        align-items: flex-start;
        justify-content: space-between;
        gap: 1rem;
        flex-wrap: wrap;
    }

    .agent-settings-panel__title {
        margin: 0;
        font-size: 1.0625rem;
        font-weight: 600;
    }

    .agent-settings-panel__subtitle {
        margin: 0.25rem 0 0;
        font-size: 0.8125rem;
        color: var(--background-400);
    }

    .agent-settings-panel__header-actions {
        display: flex;
        gap: 0.5rem;
    }

    .agent-settings-panel__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.375rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-100);
        font-size: 0.8125rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }
    }

    .agent-settings-panel__btn--primary {
        border-color: var(--max-primary-500);
        background: color-mix(in srgb, var(--max-primary-500) 18%, transparent);
        color: var(--max-primary-300);
    }

    .agent-settings-panel__error {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-danger-500);
        background: var(--mt-ui-danger-subtle, color-mix(in srgb, var(--max-danger-500) 15%, transparent));
        color: var(--max-danger-300);
        font-size: 0.8125rem;
    }

    .agent-settings-panel__section {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        padding: 0.875rem;
        border-radius: 0.5rem;
        border: 1px solid var(--background-700);
        background: var(--background-850);
    }

    .agent-settings-panel__section-title {
        margin: 0;
        font-size: 0.875rem;
        font-weight: 600;
        color: var(--background-200);
    }

    .agent-settings-panel__toggles {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .agent-settings-panel__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.8125rem;
        color: var(--background-200);
        cursor: pointer;
    }

    .agent-settings-panel__agents-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(17rem, 1fr));
        gap: 0.75rem;
    }

    .agent-settings-panel__agent-card {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-900);
    }

    .agent-settings-panel__agent-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
    }

    .agent-settings-panel__agent-identity {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        font-size: 0.8125rem;
    }

    .agent-settings-panel__version {
        font-size: 0.75rem;
        color: var(--background-400);
    }

    .agent-settings-panel__switch {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        font-size: 0.75rem;
        color: var(--background-300);
        cursor: pointer;
    }

    .agent-settings-panel__agent-row {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .agent-settings-panel__field-label {
        font-size: 0.75rem;
        color: var(--background-300);
    }

    .agent-settings-panel__input {
        flex: 1;
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-100);
        font-size: 0.7813rem;
    }

    .agent-settings-panel__table-wrap {
        overflow-x: auto;
    }

    .agent-settings-panel__table {
        width: 100%;
        border-collapse: collapse;
        font-size: 0.7813rem;

        th,
        td {
            padding: 0.4375rem 0.625rem;
            border-bottom: 1px solid var(--background-700);
            text-align: left;
        }

        th {
            color: var(--background-300);
            font-weight: 600;
        }
    }

    .agent-settings-panel__support-badge {
        display: inline-block;
        padding: 0.125rem 0.375rem;
        border-radius: 0.25rem;
        font-size: 0.6875rem;
        font-weight: 600;
    }

    .agent-settings-panel__support-badge--available {
        background: var(--mt-ui-success-subtle, color-mix(in srgb, var(--max-success-500) 18%, transparent));
        color: var(--max-success-400);
    }

    .agent-settings-panel__support-badge--unverified {
        background: var(--mt-ui-warning-subtle, color-mix(in srgb, var(--max-warning-500) 18%, transparent));
        color: var(--max-warning-400);
    }

    .agent-settings-panel__support-badge--unsupported,
    .agent-settings-panel__support-badge--disabled {
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-muted, var(--background-400));
    }
</style>
