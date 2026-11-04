<template>
    <MaxModal :visible="props.modelValue" title="Iniciar Agente CLI" :no-button="true" max-width="32rem" class="agent-open-dialog" :dismissable="!submitting" :close-on-escape="!submitting" @update:visible="emit('update:modelValue', $event)">
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
                <MaxInputRadio v-model="isolationMode" name="agent-isolation-mode" value="none" label="Sem isolamento (executar na pasta do projeto)" />
                <MaxInputRadio v-model="isolationMode" name="agent-isolation-mode" value="create" label="Criar nova Git Worktree isolada" />
                <MaxInputRadio v-model="isolationMode" name="agent-isolation-mode" value="existing" label="Vincular uma worktree existente do repositório" />
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
                    <MaxAlert v-if="!proposal.rootIgnored || proposal.warnings.length > 0" severity="warning" role="alert" class="agent-open-dialog__warning">
                        <p v-if="!proposal.rootIgnored">A pasta de worktrees não está ignorada pelo Git neste repositório.</p>
                        <p v-for="(warn, idx) in proposal.warnings" :key="idx">{{ warn }}</p>
                        <MaxInputCheckbox v-model="ensureGitignore" label="Registrar pasta de worktrees na exclusão local do Git" />
                    </MaxAlert>
                </div>
            </div>
            <div v-if="isolationMode === 'existing'" class="agent-open-dialog__worktree-box">
                <label class="agent-open-dialog__field">
                    <span class="agent-open-dialog__label">Caminho da Worktree Existente</span>
                    <input v-model="existingPath" type="text" class="agent-open-dialog__input" placeholder="/caminho/para/worktree">
                </label>
            </div>
            <MaxAlert v-if="errorMessage" severity="danger" role="alert" :message="errorMessage" class="agent-open-dialog__error" />
        </div>
        <footer class="agent-open-dialog__footer">
            <MaxButton type="button" severity="secondary" label="Cancelar" class="agent-open-dialog__btn" @click="closeDialog" />
            <MaxButton type="button" label="Abrir Aba de Agente" class="agent-open-dialog__btn agent-open-dialog__btn--primary" :disabled="submitting" :loading="submitting" @click="onSubmit" />
        </footer>
    </MaxModal>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import { MaxAlert, MaxButton, MaxInputCheckbox, MaxInputRadio, MaxModal } from '@maxvue/max-components-ui';
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
    :deep(.agent-open-dialog.max-modal) {
        background-color: var(--background-850, var(--background-900));
        border-color: var(--background-600);
        color: var(--background-100);

        .max-modal-content {
            padding: 1rem;
        }
    }

    .agent-open-dialog__body {
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
        font-size: 0.75rem;

        p {
            margin: 0 0 0.25rem;
        }
    }

    .agent-open-dialog__error {
        margin: 0;
        font-size: 0.75rem;
    }

    .agent-open-dialog__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        padding-top: 0.75rem;
    }
</style>
