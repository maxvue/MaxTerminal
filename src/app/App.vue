<template>
    <!-- O host global de notificações <MaxToast /> é montado internamente por <MaxApp> -->
    <MaxApp blank-mode no-login controlled-theme :colors="maxAppColors">
        <template #blank>
            <div class="app-workspace" :data-loading="appStore.status === 'loading' ? 'true' : 'false'">
                <AppEmptyState v-if="appStore.status === 'error'" icon="mdi:alert-circle-outline" title="Falha ao inicializar o MaxTerminal" :description="errorDescription">
                    <div flex flex-col items-center gap-2>
                        <span v-if="errorCode" class="app-error-code" text-xs px-2 py-0.5 rounded>{{ errorCode }}</span>
                        <MaxButton type="button" class="app-retry-btn" label="Tentar novamente" @click="handleRetry" />
                    </div>
                </AppEmptyState>
                <DbRecoveryPanel v-else-if="settingsStore.isRecoveryRequired" />
                <component :is="DevTerminalPlaygroundComponent" v-else-if="showDevPlayground && DevTerminalPlaygroundComponent" />
                <AppLayout v-else />
                <PreferencesDialog />
                <ProjectSettingsDialog />
                <QuickCommandsHost />
                <GlobalSearchPalette />
                <PerfOverlay />
                <VaultUnlockModal :visible="vaultStore.unlockModalOpen" @update:visible="vaultStore.unlockModalOpen = $event" @unlocked="vaultStore.handleUnlocked" />
                <VaultSetupModal :visible="vaultStore.setupModalOpen" @update:visible="vaultStore.setupModalOpen = $event" @configured="vaultStore.handleConfigured" />
                <HorizonModal />
            </div>
        </template>
    </MaxApp>
</template>

<script setup lang="ts">
    import { computed, getCurrentInstance, onBeforeUnmount, onMounted } from 'vue';
    import { MaxApp, MaxButton } from '@maxvue/max-components-ui';
    import { applyMaxAppOfflineConfig } from '@/app/plugins';
    import {
        DevTerminalPlaygroundComponent,
        ensureAppRouterInstalled,
        isDevTerminalPlaygroundRoute
    } from '@/app/router';
    import PerfOverlay from '@/components/dev/PerfOverlay.vue';
    import VaultSetupModal from '@/components/credentials/VaultSetupModal.vue';
    import VaultUnlockModal from '@/components/credentials/VaultUnlockModal.vue';
    import AppLayout from '@/components/layout/AppLayout.vue';
    import HorizonModal from '@/components/horizon/HorizonModal.vue';
    import PreferencesDialog from '@/components/preferences/PreferencesDialog.vue';
    import ProjectSettingsDialog from '@/components/projects/ProjectSettingsDialog.vue';
    import QuickCommandsHost from '@/components/quick-commands/QuickCommandsHost.vue';
    import GlobalSearchPalette from '@/components/search/GlobalSearchPalette.vue';
    import AppEmptyState from '@/components/shell/AppEmptyState.vue';
    import DbRecoveryPanel from '@/components/shell/DbRecoveryPanel.vue';
    import { useSearchShortcut } from '@/composables/search/useSearchShortcut';
    import { useActiveProjectSync } from '@/composables/useActiveProjectSync';
    import { useIpcError } from '@/composables/useIpcError';
    import { usePageReloadShortcut } from '@/composables/usePageReloadShortcut';
    import { useThemeApplier } from '@/composables/useThemeApplier';
    import { useAgentsStore } from '@/stores/agents';
    import { useAiStore } from '@/stores/ai';
    import { useAppStore } from '@/stores/app';
    import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
    import { useSettingsStore } from '@/stores/settings';
    import { useShellHistoryStore } from '@/stores/shellHistory';
    import { useSkillsStore } from '@/stores/skills';
    import { useTabStatusStore } from '@/stores/tabStatus';
    import { useThemeStore } from '@/stores/theme';
    import { useVaultStore } from '@/stores/vault';
    import { useWindowStore } from '@/stores/window';
    import { installShellIntegrationExtension } from '@/terminal/extensions/shellIntegration';
    import { loadThemeBootstrapSnapshot } from '@/theme/bootstrap';
    import { resolveMaxAppColors } from '@/theme/maxAppPalette';

    applyMaxAppOfflineConfig();
    const vmInstance = getCurrentInstance();
    if (vmInstance?.appContext?.app) {
        ensureAppRouterInstalled(vmInstance.appContext.app);
    }

    const appStore = useAppStore();
    const settingsStore = useSettingsStore();
    const windowStore = useWindowStore();
    const bridgeStore = useMaxCodeBridgeStore();
    const aiStore = useAiStore();
    const agentsStore = useAgentsStore();
    const skillsStore = useSkillsStore();
    const tabStatusStore = useTabStatusStore();
    const themeStore = useThemeStore();
    const vaultStore = useVaultStore();
    const shellHistoryStore = useShellHistoryStore();
    const ipcError = useIpcError();
    useThemeApplier();
    usePageReloadShortcut();
    useActiveProjectSync();
    useSearchShortcut();
    const shellExtDisposable = installShellIntegrationExtension();

    onBeforeUnmount(() => {
        shellExtDisposable.dispose();
    });

    const bootstrapSnapshot = loadThemeBootstrapSnapshot();
    const maxAppColors = computed(() =>
        resolveMaxAppColors(themeStore.effectiveResolved, bootstrapSnapshot)
    );
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
        void vaultStore.fetchState().catch(() => {});
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

    .app-workspace {
        display: flex;
        flex-direction: column;
        width: 100vw;
        height: 100vh;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
    }

    .app-error-code {
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--max-danger-400);
        border: 1px solid var(--max-danger-500);
        font-family: var(--mt-font-mono);
    }

    .app-retry-btn {
        background: var(--max-primary-600);
        color: var(--max-primary-content);
        border: 1px solid var(--max-primary-500);

        &:hover {
            background: var(--max-primary-500);
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }
</style>
