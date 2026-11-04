<template>
    <div class="agent-launcher">
        <MaxPopover ref="popoverRef" :no-header="true" class="agent-launcher__popover-host">
            <template #trigger="slotProps">
                <button type="button" class="agent-launcher__trigger" title="Abrir aba de Agente CLI" aria-label="Abrir Agente CLI" v-bind="(slotProps as any)">
                    <MaxIcon icon="lucide:bot" />
                    <span>Agentes CLI</span>
                    <MaxIcon icon="lucide:chevron-down" class="agent-launcher__caret" />
                </button>
            </template>
            <div class="agent-launcher__popover" role="menu" aria-label="Agentes CLI disponíveis">
                <div v-for="item in agentItems" :key="item.kind" class="agent-launcher__row">
                    <button type="button" class="agent-launcher__item" role="menuitem" :disabled="!item.enabled" :title="item.tooltip" @click="onQuickLaunch(item.kind)">
                        <MaxIcon :icon="item.icon" class="agent-launcher__item-icon" />
                        <div class="agent-launcher__item-body">
                            <span class="agent-launcher__item-title">{{ item.displayName }}</span>
                            <span class="agent-launcher__item-meta" :class="{ 'is-installed': item.installed, 'is-missing': !item.installed }">{{ item.installed ? (item.version ? `v${item.version}` : 'Instalado') : 'Não instalado' }}</span>
                        </div>
                        <kbd v-if="item.shortcut" class="agent-launcher__kbd">{{ item.shortcut }}</kbd>
                    </button>
                    <MaxIconButton icon="lucide:git-branch-plus" size="1" class="agent-launcher__options-btn" :title="`Abrir ${item.displayName} com opções de Worktree e Modelo`" :aria-label="`Opções de abertura para ${item.displayName}`" @click="onOpenWithDialog(item.kind)" />
                </div>
            </div>
        </MaxPopover>
    </div>
</template>

<script setup lang="ts">
    import { computed, onMounted, ref } from 'vue';
    import MaxIconButton from '@maxvue/max-components-ui/components/MaxIconButton';
    import MaxPopover from '@maxvue/max-components-ui/components/MaxPopover';
    import { DEFAULT_AGENT_SHORTCUTS } from '@/composables/useAgentShortcuts';
    import { useAgentsStore } from '@/stores/agents';
    import type { AgentKind, ProjectId } from '@/types/bindings';

    export interface AgentLauncherMenuProps {
        projectId: ProjectId;
    }

    const props = defineProps<AgentLauncherMenuProps>();

    const agentsStore = useAgentsStore();
    const popoverRef = ref<InstanceType<typeof MaxPopover> | null>(null);

    interface LauncherAgentItem {
        kind: AgentKind;
        displayName: string;
        icon: string;
        shortcut: string;
        installed: boolean;
        enabled: boolean;
        version: string | null;
        tooltip: string;
    }

    const FALLBACK_META: Readonly<Record<AgentKind, { displayName: string; icon: string }>> = {
        'claude-code': { displayName: 'Claude Code', icon: 'simple-icons:anthropic' },
        codex: { displayName: 'Codex', icon: 'simple-icons:openai' },
        agy: { displayName: 'Antigravity (agy)', icon: 'lucide:sparkles' },
        opencode: { displayName: 'OpenCode', icon: 'lucide:terminal-square' }
    };

    const agentItems = computed<LauncherAgentItem[]>(() => {
        const order: AgentKind[] = ['claude-code', 'codex', 'agy', 'opencode'];
        return order.map((kind) => {
            const info = agentsStore.agentByKind(kind);
            const fallback = FALLBACK_META[kind];
            const shortcut =
                agentsStore.settings?.shortcuts?.[kind] ??
                info?.descriptor.shortcut ??
                DEFAULT_AGENT_SHORTCUTS[kind];
            const installed = info ? info.installation.installed : true;
            const enabled = info ? info.enabled : true;
            const version = info?.installation.version ?? null;
            const reason = info?.installation.reason ?? null;
            const tooltip = installed
                ? `Iniciar ${info?.descriptor.displayName ?? fallback.displayName} (${shortcut})`
                : reason || 'Agente não encontrado no PATH';

            return {
                kind,
                displayName: info?.descriptor.displayName ?? fallback.displayName,
                icon: info?.descriptor.icon ?? fallback.icon,
                shortcut,
                installed,
                enabled,
                version,
                tooltip
            };
        });
    });

    onMounted(() => {
        if (!agentsStore.initialized) {
            void agentsStore.init().catch(() => {
                // Ignora quando backend de teste não provê mocks de agentes.
            });
        }
    });

    async function onQuickLaunch(kind: AgentKind): Promise<void> {
        popoverRef.value?.hide();
        const info = agentsStore.agentByKind(kind);
        if (info && !info.installation.installed) {
            return;
        }
        if (agentsStore.settings?.isolation.openIsolatedByDefault) {
            agentsStore.openAgentDialog(props.projectId, kind, true);
            return;
        }
        try {
            await agentsStore.openAgent({
                projectId: props.projectId,
                agent: kind,
                isolation: { mode: 'none' },
                focus: true
            });
        } catch {
            // Erro registrado na store.
        }
    }

    function onOpenWithDialog(kind: AgentKind): void {
        popoverRef.value?.hide();
        agentsStore.openAgentDialog(props.projectId, kind, true);
    }
</script>

<style lang="scss" scoped>
    .agent-launcher {
        position: relative;
        display: inline-flex;
    }

    .agent-launcher__trigger {
        display: inline-flex;
        align-items: center;
        gap: 0.3125rem;
        height: 1.75rem;
        padding: 0 0.5625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--background-600);
        background: var(--background-850, var(--background-900));
        color: var(--background-200);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
        white-space: nowrap;

        &:hover {
            background: var(--background-700);
            color: var(--background-50);
        }
    }

    .agent-launcher__caret {
        font-size: 0.75rem;
        opacity: 0.7;
    }

    .agent-launcher__popover {
        position: absolute;
        top: calc(100% + 0.25rem);
        right: 0;
        z-index: 40;
        min-width: 17rem;
        padding: 0.375rem;
        border-radius: 0.5rem;
        background: var(--background-850, var(--background-900));
        border: 1px solid var(--background-600);
        box-shadow: 0 8px 24px rgb(0 0 0 / 40%);
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .agent-launcher__row {
        display: flex;
        align-items: center;
        gap: 0.25rem;
    }

    .agent-launcher__item {
        flex: 1;
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.375rem 0.5rem;
        border-radius: 0.375rem;
        border: none;
        background: transparent;
        color: var(--background-100);
        text-align: left;
        cursor: pointer;

        &:disabled {
            opacity: 0.45;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            background: var(--background-700);
        }
    }

    .agent-launcher__item-icon {
        font-size: 1rem;
        color: var(--max-primary-400);
        flex-shrink: 0;
    }

    .agent-launcher__item-body {
        display: flex;
        flex-direction: column;
        min-width: 0;
        flex: 1;
    }

    .agent-launcher__item-title {
        font-size: 0.7813rem;
        font-weight: 600;
    }

    .agent-launcher__item-meta {
        font-size: 0.65rem;

        &.is-installed {
            color: var(--max-success-400);
        }

        &.is-missing {
            color: var(--max-warning-400);
        }
    }

    .agent-launcher__kbd {
        padding: 0.125rem 0.3125rem;
        border-radius: 0.25rem;
        background: var(--background-800);
        border: 1px solid var(--background-600);
        color: var(--background-300);
        font-size: 0.65rem;
        font-family: monospace;
    }

    .agent-launcher__options-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.75rem;
        height: 1.75rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-200);
        cursor: pointer;

        &:hover {
            background: var(--background-700);
            color: var(--max-primary-400);
        }
    }
</style>
