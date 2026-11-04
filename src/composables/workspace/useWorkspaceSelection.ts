import { ref, watch } from 'vue';
import { useGithubIssuesStore } from '@/stores/githubIssues';
import { useProjectsStore } from '@/stores/projects';
import { useSettingsStore } from '@/stores/settings';
import { useTabsStore } from '@/stores/tabs';
import { useWorkspaceSelectionStore } from '@/stores/workspaceSelection';

export function useWorkspaceSelection() {
    const projectsStore = useProjectsStore();
    const tabsStore = useTabsStore();
    const githubIssuesStore = useGithubIssuesStore();
    const selectionStore = useWorkspaceSelectionStore();
    const settingsStore = useSettingsStore();

    const isRestored = ref<boolean>(false);
    const isRestoring = ref<boolean>(false);
    const suppressCapture = ref<boolean>(true);
    const userInteracted = ref<boolean>(false);

    /**
     * Aplica a seleção persistida de forma atômica e coordenada uma única vez (§3 da spec).
     * Não invoca `tabsStore.focusTab` para não disparar marcação de visto no backend Rust.
     */
    async function restoreSelection(): Promise<void> {
        if (isRestored.value) {
            return;
        }

        // Se o usuário já interagiu ativamente antes do término dos carregamentos,
        // a intenção mais nova do usuário prevalece sobre o histórico persistido.
        if (userInteracted.value) {
            isRestored.value = true;
            suppressCapture.value = false;
            captureCurrentSelection();
            return;
        }

        isRestoring.value = true;
        try {
            // Garante que a settingsStore está hidratada antes de consultar a seleção
            if (!settingsStore.hydrated && !settingsStore.loading) {
                try {
                    await settingsStore.hydrate();
                } catch {
                    // Silencioso em caso de erro no boot
                }
            }

            const selection = selectionStore.data;
            const targetProjectId = selection.selectedProjectId;

            if (targetProjectId && projectsStore.projectById(targetProjectId)) {
                // 1. Aplica o projeto selecionado
                projectsStore.select(targetProjectId);

                // 2. Consulta visão salva (terminal ou issues)
                const savedView = selectionStore.viewForProject(targetProjectId);

                // 3. Aplica a aba terminal correspondente se existir e pertencer ao projeto
                const projectTabs = tabsStore.orderedIdsByProject[targetProjectId] ?? [];
                const savedTerminalTabId = savedView?.terminalTabId ?? null;

                if (savedTerminalTabId && tabsStore.byId[savedTerminalTabId]?.projectId === targetProjectId) {
                    try {
                        projectsStore.setActiveTab(targetProjectId, savedTerminalTabId);
                    } catch {
                        // Ignora caso aba pertença a outro projeto
                    }
                } else if (projectTabs.length > 0) {
                    const firstTabId = projectTabs[0];
                    if (firstTabId) {
                        try {
                            projectsStore.setActiveTab(targetProjectId, firstTabId);
                        } catch {
                            // Ignora
                        }
                    }
                }

                // 4. Se a visão visível era Issues, valida se o repositório é GitHub
                if (savedView?.kind === 'issues') {
                    try {
                        const resolution = await githubIssuesStore.resolveProjectRepo(targetProjectId);
                        if (resolution.kind === 'git_hub' && !userInteracted.value) {
                            githubIssuesStore.setPinnedTabActive(targetProjectId, true);
                        } else if (resolution.kind !== 'git_hub') {
                            // Não é repositório GitHub: mantém visão de terminal
                            githubIssuesStore.setPinnedTabActive(targetProjectId, false);
                        }
                    } catch {
                        // Erro transitório de rede não descarta a intenção
                    }
                }

                // 5. Restaura abas ativas lembradas para os demais projetos em segundo plano
                for (const [projId, view] of Object.entries(selection.activeViewByProject)) {
                    if (projId !== targetProjectId && view.terminalTabId) {
                        if (tabsStore.byId[view.terminalTabId]?.projectId === projId) {
                            try {
                                projectsStore.setActiveTab(projId, view.terminalTabId);
                            } catch {
                                // Ignora
                            }
                        }
                    }
                }
            }
        } finally {
            isRestoring.value = false;
            isRestored.value = true;
            suppressCapture.value = false;
        }
    }

    /**
     * Captura o estado atual da UI e envia para o storage síncrono e MaxPinia.
     */
    function captureCurrentSelection(): void {
        const currentPid = projectsStore.selectedId;
        if (!currentPid) {
            selectionStore.setSelection({ selectedProjectId: null });
            return;
        }

        const isIssuesActive = githubIssuesStore.isPinnedTabActive(currentPid);
        const currentActiveTab = projectsStore.activeTabByProject[currentPid] ?? null;

        selectionStore.setSelection({
            selectedProjectId: currentPid,
            kind: isIssuesActive ? 'issues' : 'terminal',
            terminalTabId: currentActiveTab
        });
    }

    // Observa mudanças explícitas no projeto selecionado
    watch(
        () => projectsStore.selectedId,
        (nextProjectId, prevProjectId) => {
            if (isRestoring.value || nextProjectId === prevProjectId) {
                return;
            }
            if (suppressCapture.value) {
                userInteracted.value = true;
                return;
            }
            captureCurrentSelection();
        },
        { flush: 'sync' }
    );

    // Observa mudanças na aba terminal ativa por projeto
    watch(
        () => ({ ...projectsStore.activeTabByProject }),
        (nextActiveTabs, prevActiveTabs) => {
            if (isRestoring.value || suppressCapture.value) {
                return;
            }
            const currentPid = projectsStore.selectedId;
            if (!currentPid) {
                return;
            }
            if (nextActiveTabs[currentPid] !== prevActiveTabs[currentPid]) {
                captureCurrentSelection();
            }
        },
        { deep: true, flush: 'sync' }
    );

    // Observa ativação da aba de Issues
    watch(
        () => ({ ...githubIssuesStore.pinnedIssuesTabActiveByProject }),
        (nextPinned, prevPinned) => {
            if (isRestoring.value || suppressCapture.value) {
                return;
            }
            const currentPid = projectsStore.selectedId;
            if (!currentPid) {
                return;
            }
            if (nextPinned[currentPid] !== prevPinned[currentPid]) {
                captureCurrentSelection();
            }
        },
        { deep: true, flush: 'sync' }
    );

    return {
        isRestored,
        suppressCapture,
        userInteracted,
        restoreSelection,
        captureCurrentSelection
    };
}
