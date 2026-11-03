import { onBeforeUnmount, onMounted, watch } from 'vue';
import { useGitStore } from '@/stores/git';
import { usePortsStore } from '@/stores/ports';
import { useTabsStore } from '@/stores/tabs';

export interface UseTabIndicatorsSyncReturn {
    stop: () => void;
}

/**
 * Sincroniza as stores de indicadores contextuais (`useGitStore` e `usePortsStore`)
 * com as abas/painéis do `useTabsStore` e com a visibilidade da janela.
 */
export function useTabIndicatorsSync(): UseTabIndicatorsSyncReturn {
    const tabsStore = useTabsStore();
    const gitStore = useGitStore();
    const portsStore = usePortsStore();

    function onVisibilityChange(): void {
        const mode = document.visibilityState === 'hidden' ? 'background' : 'active';
        void portsStore.setPollMode(mode);
    }

    const stopWatchTabs = watch(
        () => Object.values(tabsStore.byId),
        (tabs) => {
            for (const tab of tabs) {
                const activePane =
                    tab.panes.find((p) => p.id === tab.activePaneId) ?? tab.panes[0];
                if (activePane && !gitStore.contextForPane(activePane.id)) {
                    void gitStore.ensurePaneStatus(activePane.id, activePane.cwd);
                }
                if (!(tab.id in portsStore.portsByTab)) {
                    void portsStore.loadForTab(tab.id);
                }
            }
        },
        { immediate: true, deep: false }
    );

    onMounted(() => {
        void gitStore.startListening().catch(() => {
            // Ignora em ambiente sem runtime Tauri.
        });
        void portsStore.startListening().catch(() => {
            // Ignora em ambiente sem runtime Tauri.
        });
        document.addEventListener('visibilitychange', onVisibilityChange);
    });

    function stop(): void {
        stopWatchTabs();
        gitStore.stopListening();
        portsStore.stopListening();
        if (typeof document !== 'undefined') {
            document.removeEventListener('visibilitychange', onVisibilityChange);
        }
    }

    onBeforeUnmount(() => {
        stop();
    });

    return {
        stop
    };
}
