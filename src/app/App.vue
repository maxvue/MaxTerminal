<template>
    <AppShell :loading="appStore.status === 'loading'">
        <template #workspace>
            <AppEmptyState v-if="appStore.status === 'error'" icon="mdi:alert-circle-outline" title="Falha ao inicializar o MaxTerminal" :description="errorDescription">
                <div flex flex-col items-center gap-2>
                    <span v-if="errorCode" class="app-error-code" text-xs px-2 py-0.5 rounded>{{ errorCode }}</span>
                    <button type="button" class="app-retry-btn" px-4 py-1.5 rounded text-sm font-medium cursor-pointer @click="handleRetry">
                        Tentar novamente
                    </button>
                </div>
            </AppEmptyState>
            <DbRecoveryPanel v-else-if="settingsStore.isRecoveryRequired" />
            <component :is="DevTerminalPlaygroundComponent" v-else-if="showDevPlayground && DevTerminalPlaygroundComponent" />
            <AppLayout v-else />
            <PreferencesDialog />
            <QuickCommandsHost />
            <GlobalSearchPalette />
            <PerfOverlay />
        </template>
    </AppShell>
</template>

<script setup lang="ts">
    import { computed, onBeforeUnmount, onMounted } from 'vue';
    import { DevTerminalPlaygroundComponent, isDevTerminalPlaygroundRoute } from '@/app/router';
    import PerfOverlay from '@/components/dev/PerfOverlay.vue';
    import AppLayout from '@/components/layout/AppLayout.vue';
    import PreferencesDialog from '@/components/preferences/PreferencesDialog.vue';
    import QuickCommandsHost from '@/components/quick-commands/QuickCommandsHost.vue';
    import GlobalSearchPalette from '@/components/search/GlobalSearchPalette.vue';
    import AppEmptyState from '@/components/shell/AppEmptyState.vue';
    import AppShell from '@/components/shell/AppShell.vue';
    import DbRecoveryPanel from '@/components/shell/DbRecoveryPanel.vue';
    import { useSearchShortcut } from '@/composables/search/useSearchShortcut';
    import { useActiveProjectSync } from '@/composables/useActiveProjectSync';
    import { useIpcError } from '@/composables/useIpcError';
    import { useThemeApplier } from '@/composables/useThemeApplier';
    import { useAgentsStore } from '@/stores/agents';
    import { useAiStore } from '@/stores/ai';
    import { useAppStore } from '@/stores/app';
    import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
    import { useSettingsStore } from '@/stores/settings';
    import { useShellHistoryStore } from '@/stores/shellHistory';
    import { useSkillsStore } from '@/stores/skills';
    import { useTabStatusStore } from '@/stores/tabStatus';
    import { useWindowStore } from '@/stores/window';
    import { installShellIntegrationExtension } from '@/terminal/extensions/shellIntegration';

    const appStore = useAppStore();
    const settingsStore = useSettingsStore();
    const windowStore = useWindowStore();
    const bridgeStore = useMaxCodeBridgeStore();
    const aiStore = useAiStore();
    const agentsStore = useAgentsStore();
    const skillsStore = useSkillsStore();
    const tabStatusStore = useTabStatusStore();
    const shellHistoryStore = useShellHistoryStore();
    const ipcError = useIpcError();
    useThemeApplier();
    useActiveProjectSync();
    useSearchShortcut();
    const shellExtDisposable = installShellIntegrationExtension();

    onBeforeUnmount(() => {
        shellExtDisposable.dispose();
    });

    const errorDescription = computed<string>(() => ipcError.message(appStore.error));
    const errorCode = computed<string | null>(() => ipcError.code(appStore.error));
    const showDevPlayground = computed<boolean>(() => isDevTerminalPlaygroundRoute());

    function initBackgroundSubsystems(): void {
        void windowStore.init().catch(() => {});
        void bridgeStore.init().catch(() => {});
        void aiStore.init().catch(() => {});
        void tabStatusStore.init().catch(() => {});
        void shellHistoryStore.init().catch(() => {});
        void agentsStore.init().catch(() => {});
        void skillsStore.init().catch(() => {});
    }

    async function handleRetry(): Promise<void> {
        await appStore.retry();
        if (appStore.status === 'ready') {
            await settingsStore.hydrate();
            initBackgroundSubsystems();
        }
    }

    onMounted(async () => {
        await appStore.bootstrap();
        if (appStore.status === 'ready') {
            await settingsStore.hydrate();
            initBackgroundSubsystems();
        }
    });
</script>

<style lang="scss" scoped>
    @use '@/styles/params' as *;

    .app-error-code {
        background: var(--background-800);
        color: var(--max-danger-400);
        border: 1px solid var(--max-danger-500);
        font-family: var(--mt-font-mono);
    }

    .app-retry-btn {
        background: var(--max-primary-600);
        color: var(--background-50);
        border: 1px solid var(--max-primary-500);

        &:hover {
            background: var(--max-primary-500);
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }
</style>