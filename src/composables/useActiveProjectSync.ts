import { onScopeDispose, watch } from 'vue';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { onBridgeProjectSynced } from '@/ipc/bridge';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
import { useProjectsStore } from '@/stores/projects';

/**
 * Sincroniza bidirecionalmente o projeto ativo entre `useProjectsStore` (Fase 06)
 * e a extensão `MaxCode` via `useMaxCodeBridgeStore` (Fase 11), prevenindo loops de eco.
 */
export function useActiveProjectSync(): void {
    const bridgeStore = useMaxCodeBridgeStore();
    const projectsStore = useProjectsStore();

    const remoteSyncedProjectIds = new Set<string>();
    let disposed = false;
    let unlistenFn: UnlistenFn | null = null;

    void onBridgeProjectSynced(async (ev) => {
        if (disposed || !bridgeStore.syncEnabled) {
            return;
        }
        remoteSyncedProjectIds.add(ev.projectId);
        if (ev.created || !projectsStore.projectById(ev.projectId)) {
            await projectsStore.resync().catch(() => {});
        }
        if (disposed) {
            return;
        }
        projectsStore.select(ev.projectId);
    }).then((unlisten) => {
        if (disposed) {
            unlisten();
        } else {
            unlistenFn = unlisten;
        }
    });

    const stopWatch = watch(
        () => projectsStore.selectedProject,
        (project, prevProject) => {
            if (!project) {
                return;
            }
            if (prevProject && prevProject.id === project.id) {
                return;
            }
            if (remoteSyncedProjectIds.has(project.id)) {
                remoteSyncedProjectIds.delete(project.id);
                return;
            }
            if (!bridgeStore.syncEnabled) {
                return;
            }
            void bridgeStore.notifyProjectSelected(project.path, project.title, null);
        }
    );

    onScopeDispose(() => {
        disposed = true;
        stopWatch();
        if (unlistenFn) {
            unlistenFn();
            unlistenFn = null;
        }
    });
}
