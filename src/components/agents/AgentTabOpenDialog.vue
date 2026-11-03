<template>
    <div v-if="props.modelValue" class="agent-open-dialog__backdrop" role="dialog" aria-modal="true" aria-label="Abrir aba de Agente CLI" data-modal-trap="true">
        <div class="agent-open-dialog">
            <header class="agent-open-dialog__header">
                <h2 class="agent-open-dialog__title">Iniciar Agente CLI</h2>
                <button type="button" class="agent-open-dialog__close" aria-label="Fechar modal" @click="closeDialog">
                    <MaxIcon icon="lucide:x" />
                </button>
            </header>
            <div class="agent-open-dialog__body">
                <label class="agent-open-dialog__field">
                    <span class="agent-open-dialog__label">Agente CLI</span>
                    <select v-model="selectedAgent" class="agent-open-dialog__select">
                        <option value="claude-code">Claude Code (claude)</option>
                        <option value="codex">OpenAI Codex (codex)</option>
                        <option value="agy">Google Antigravity (agy)</option>
                        <option value="opencode">OpenCode (opencode)</option>
                    </select>
                </label>
                <label class="agent-open-dialog__field">
                    <span class="agent-open-dialog__label">Modelo CLI (opcional)</span>
                    <input v-model="modelInput" type="text" class="agent-open-dialog__input" placeholder="Padrão do agente (sem flag --model)" :disabled="!supportsModelSelection">
                </label>
                <fieldset class="agent-open-dialog__fieldset">
                    <legend class="agent-open-dialog__legend">Isolamento em Git Worktree</legend>
                    <label class="agent-open-dialog__radio">
                        <input v-model="isolationMode" type="radio" value="none">
                        <span>Sem isolamento (executar na pasta do projeto)</span>
                    </label>
                    <label class="agent-open-dialog__radio">
                        <input v-model="isolationMode" type="radio" value="create">
                        <span>Criar nova Git Worktree isolada</span>
                    </label>
                    <label class="agent-open-dialog__radio">
                        <input v-model="isolationMode" type="radio" value="existing">
                        <span>Vincular uma worktree existente do repositório</span>
                    </label>
                </fieldset>
                <div v-if="isolationMode === 'create'" class="agent-open-dialog__worktree-box">
                    <div class="agent-open-dialog__row">
                        <label class="agent-open-dialog__field">
                            <span class="agent-open-dialog__label">Localização da Worktree</span>
                            <select v-model="location" class="agent-open-dialog__select">
                                <option value="project">.worktrees/ (MaxTerminal)</option>
                                <option value="maxCode">.max-code-worktrees/ (MaxCode)</option>
                            </select>
                        </label>
                        <label class="agent-open-dialog__field">
                            <span class="agent-open-dialog__label">Sugestão de Slug (opcional)</span>
                            <input v-model="slugHint" type="text" class="agent-open-dialog__input" placeholder="Ex.: feat-login" @change="refreshProposal">
                        </label>
                    </div>
                    <div v-if="proposal" class="agent-open-dialog__proposal">
                        <div class="agent-open-dialog__proposal-line">
                            <strong>Pasta:</strong>
                            <code>{{ proposal.path }}</code>
                        </div>
                        <div class="agent-open-dialog__proposal-line">
                            <strong>Branch:</strong>
                            <code>{{ proposal.branch }}</code>
                        </div>
                        <div v-if="!proposal.rootIgnored || proposal.warnings.length > 0" class="agent-open-dialog__warning" role="alert">
                            <p v-if="!proposal.rootIgnored">A pasta de worktrees não está ignorada pelo Git neste repositório.</p>
                            <p v-for="(warn, idx) in proposal.warnings" :key="idx">{{ warn }}</p>
                            <label class="agent-open-dialog__checkbox">
                                <input v-model="ensureGitignore" type="checkbox">
                                <span>Registrar pasta de worktrees na exclusão local do Git</span>
                            </label>
                        </div>
                    </div>
                </div>
                <div v-if="isolationMode === 'existing'" class="agent-open-dialog__worktree-box">
                    <label class="agent-open-dialog__field">
                        <span class="agent-open-dialog__label">Caminho da Worktree Existente</span>
                        <input v-model="existingPath" type="text" class="agent-open-dialog__input" placeholder="/caminho/para/worktree">
                    </label>
                </div>
                <p v-if="errorMessage" class="agent-open-dialog__error" role="alert">{{ errorMessage }}</p>
            </div>
            <footer class="agent-open-dialog__footer">
                <button type="button" class="agent-open-dialog__btn" @click="closeDialog">Cancelar</button>
                <button type="button" class="agent-open-dialog__btn agent-open-dialog__btn--primary" :disabled="submitting" @click="onSubmit">Abrir Aba de Agente</button>
            </footer>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import { useAgentsStore } from '@/stores/agents';
    import type {
        AgentKind,
        AgentTabOpened_Serialize,
        ProjectId,
        WorktreeIsolation_Deserialize,
        WorktreeLocationDto_Deserialize,
        WorktreeProposal_Serialize
    } from '@/types/bindings';

    export interface AgentTabOpenDialogProps {
        modelValue: boolean;
        projectId: ProjectId;
        initialAgent?: AgentKind;
        initialIsolate?: boolean;
    }

    const props = withDefaults(defineProps<AgentTabOpenDialogProps>(), {
        initialAgent: 'claude-code',
        initialIsolate: false
    });

    const emit = defineEmits<{
        'update:modelValue': [value: boolean];
        opened: [result: AgentTabOpened_Serialize];
    }>();

    const agentsStore = useAgentsStore();

    const selectedAgent = ref<AgentKind>(props.initialAgent);
    const isolationMode = ref<'none' | 'create' | 'existing'>(
        props.initialIsolate ? 'create' : 'none'
    );
    const location = ref<WorktreeLocationDto_Deserialize>('project');
    const slugHint = ref<string>('');
    const existingPath = ref<string>('');
    const modelInput = ref<string>('');
    const ensureGitignore = ref<boolean>(false);
    const proposal = ref<WorktreeProposal_Serialize | null>(null);
    const submitting = ref<boolean>(false);
    const errorMessage = ref<string | null>(null);

    const supportsModelSelection = computed<boolean>(() => {
        const cap = agentsStore.capabilitiesByAgent[selectedAgent.value];
        const feature = cap?.features.modelSelection;
        if (!feature) {
            return selectedAgent.value !== 'agy';
        }
        return feature.support === 'available';
    });

    async function refreshProposal(): Promise<void> {
        if (!props.modelValue || isolationMode.value !== 'create' || !props.projectId) {
            return;
        }
        try {
            proposal.value = await agentsStore.proposeWorktree(
                props.projectId,
                selectedAgent.value,
                slugHint.value.trim() || null
            );
            location.value = proposal.value.location;
        } catch (err: unknown) {
            errorMessage.value =
                err instanceof Error ? err.message : 'Não foi possível propor a worktree.';
        }
    }

    watch(
        () => [props.modelValue, props.initialAgent, props.initialIsolate] as const,
        ([open, agent, isolate]) => {
            if (open) {
                selectedAgent.value = agent;
                isolationMode.value = isolate ? 'create' : 'none';
                errorMessage.value = null;
                if (isolationMode.value === 'create') {
                    void refreshProposal();
                }
            }
        },
        { immediate: true }
    );

    watch([selectedAgent, isolationMode], () => {
        if (props.modelValue && isolationMode.value === 'create') {
            void refreshProposal();
        }
    });

    function closeDialog(): void {
        emit('update:modelValue', false);
    }

    async function onSubmit(): Promise<void> {
        submitting.value = true;
        errorMessage.value = null;

        try {
            let isolation: WorktreeIsolation_Deserialize = { mode: 'none' };
            if (isolationMode.value === 'create') {
                if (!proposal.value) {
                    await refreshProposal();
                }
                const slug = proposal.value?.slug ?? (slugHint.value.trim() || 'agent-session');
                const branch =
                    proposal.value?.branch ?? `agent/${selectedAgent.value}/${slug}`;
                isolation = {
                    mode: 'create',
                    location: location.value,
                    slug,
                    branch,
                    base: proposal.value?.baseRef ?? null,
                    ensureGitignore: ensureGitignore.value
                };
            } else if (isolationMode.value === 'existing') {
                isolation = {
                    mode: 'existing',
                    path: existingPath.value.trim()
                };
            }

            const opened = await agentsStore.openAgent({
                projectId: props.projectId,
                agent: selectedAgent.value,
                isolation,
                model:
                    supportsModelSelection.value && modelInput.value.trim().length > 0
                        ? modelInput.value.trim()
                        : null,
                focus: true
            });

            emit('opened', opened);
            emit('update:modelValue', false);
        } catch (err: unknown) {
            errorMessage.value =
                agentsStore.error?.message ??
                (err instanceof Error ? err.message : 'Falha ao abrir aba de agente.');
        } finally {
            submitting.value = false;
        }
    }
</script>

<style lang="scss" scoped>
    .agent-open-dialog__backdrop {
        position: fixed;
        inset: 0;
        z-index: 60;
        background: rgb(0 0 0 / 60%);
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 1rem;
    }

    .agent-open-dialog {
        width: 100%;
        max-width: 32rem;
        border-radius: 0.5rem;
        background: var(--background-850, var(--background-900));
        border: 1px solid var(--background-600);
        color: var(--background-100);
        display: flex;
        flex-direction: column;
        overflow: hidden;
    }

    .agent-open-dialog__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.75rem 1rem;
        border-bottom: 1px solid var(--background-700);
    }

    .agent-open-dialog__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
    }

    .agent-open-dialog__close {
        border: none;
        background: transparent;
        color: var(--background-300);
        cursor: pointer;
    }

    .agent-open-dialog__body {
        padding: 1rem;
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
    }

    .agent-open-dialog__field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        flex: 1;
    }

    .agent-open-dialog__label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--background-300);
    }

    .agent-open-dialog__select,
    .agent-open-dialog__input {
        height: 2rem;
        padding: 0 0.5rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--background-600);
        background: var(--background-900);
        color: var(--background-100);
        font-size: 0.8125rem;
    }

    .agent-open-dialog__fieldset {
        margin: 0;
        padding: 0.625rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .agent-open-dialog__legend {
        padding: 0 0.25rem;
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--background-300);
    }

    .agent-open-dialog__radio,
    .agent-open-dialog__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.8125rem;
        cursor: pointer;
    }

    .agent-open-dialog__worktree-box {
        padding: 0.75rem;
        border-radius: 0.375rem;
        background: var(--background-800);
        border: 1px solid var(--background-700);
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
    }

    .agent-open-dialog__row {
        display: flex;
        gap: 0.5rem;
    }

    .agent-open-dialog__proposal {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        font-size: 0.75rem;
    }

    .agent-open-dialog__proposal-line {
        display: flex;
        gap: 0.375rem;
        word-break: break-all;
    }

    .agent-open-dialog__warning {
        margin-top: 0.375rem;
        padding: 0.5rem;
        border-radius: 0.3125rem;
        background: rgb(245 158 11 / 16%);
        border: 1px solid var(--max-warning-500);
        color: var(--max-warning-300);
        font-size: 0.75rem;

        p {
            margin: 0 0 0.25rem;
        }
    }

    .agent-open-dialog__error {
        margin: 0;
        color: var(--max-danger-400);
        font-size: 0.75rem;
    }

    .agent-open-dialog__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        padding: 0.75rem 1rem;
        border-top: 1px solid var(--background-700);
    }

    .agent-open-dialog__btn {
        height: 2rem;
        padding: 0 0.875rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-200);
        font-size: 0.8125rem;
        font-weight: 600;
        cursor: pointer;

        &--primary {
            border-color: var(--max-primary-500);
            background: rgb(20 184 166 / 18%);
            color: var(--max-primary-300);
        }
    }
</style>
