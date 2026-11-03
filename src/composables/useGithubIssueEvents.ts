import { getCurrentScope, onScopeDispose } from 'vue';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { githubIpc } from '@/ipc/github';
import { useGithubIssuesStore } from '@/stores/githubIssues';

/**
 * Gerencia a inscrição nos 6 eventos Tauri `github:*` e sincroniza o `useGithubIssuesStore`.
 */
export function useGithubIssueEvents() {
    const store = useGithubIssuesStore();
    const unlisteners: UnlistenFn[] = [];
    let started = false;

    async function start(): Promise<void> {
        if (started) {
            return;
        }
        started = true;

        try {
            const u1 = await githubIpc.onIssuesChanged((payload) => {
                store.openCountByProject[payload.projectId] = payload.openIssuesCount;
                store.lastTransportByProject[payload.projectId] = payload.transportUsed;
                store.lastSuccessAtByProject[payload.projectId] = payload.syncedAtMs;
            });
            const u2 = await githubIpc.onIssueUpdated((payload) => {
                store.upsertIssueSummaryInList(payload.projectId, payload.issue);
            });
            const u3 = await githubIpc.onLabelsSynced((payload) => {
                store.missingCanonicalCountByProject[payload.projectId] =
                    payload.missingCanonicalCount;
            });
            const u4 = await githubIpc.onAuthChanged((payload) => {
                store.authStatus = payload.status;
                if (payload.status.rateLimit) {
                    store.rateLimit = payload.status.rateLimit;
                }
            });
            const u5 = await githubIpc.onBugCycleProgress((payload) => {
                store.activeCycleByProject[payload.cycle.projectId] = payload.cycle;
            });
            const u6 = await githubIpc.onRateLimit((payload) => {
                store.rateLimit = payload.rateLimit;
            });
            unlisteners.push(u1, u2, u3, u4, u5, u6);
        } catch {
            // Ambiente de teste sem mock de eventos Tauri.
        }
    }

    function stop(): void {
        while (unlisteners.length > 0) {
            const fn = unlisteners.pop();
            if (fn) {
                fn();
            }
        }
        started = false;
    }

    if (getCurrentScope()) {
        onScopeDispose(stop);
    }

    return {
        start,
        stop
    };
}
