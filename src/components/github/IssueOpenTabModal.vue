<template>
    <MaxModal v-if="props.issue" :visible="props.open" :title="`Abrir Aba no MaxTerminal — Issue #${props.issue.number}`" :no-button="true" max-width="36rem" class="issue-open-tab-modal" :dismissable="!props.submitting" :close-on-escape="!props.submitting" :before-close="onBeforeClose" @update:visible="onUpdateVisible">
        <div class="issue-open-tab-modal__body">
            <div class="issue-open-tab-modal__section">
                <span class="issue-open-tab-modal__label">Tipo de Aba:</span>
                <div class="issue-open-tab-modal__targets" role="radiogroup" aria-label="Tipo de aba a abrir">
                    <MaxInputRadio v-model="selectedTarget" name="issue-open-tab-target" value="terminal" label="Terminal Padrão" />
                    <MaxInputRadio v-model="selectedTarget" name="issue-open-tab-target" value="claude-code" label="Claude Code" />
                    <MaxInputRadio v-model="selectedTarget" name="issue-open-tab-target" value="codex" label="Codex" />
                    <MaxInputRadio v-model="selectedTarget" name="issue-open-tab-target" value="agy" label="Antigravity (agy)" />
                    <MaxInputRadio v-model="selectedTarget" name="issue-open-tab-target" value="opencode" label="OpenCode" />
                </div>
            </div>
            <div v-if="props.resolution" class="issue-open-tab-modal__resolution">
                <p v-if="props.resolution.status === 'existing_worktree'" class="issue-open-tab-modal__res-info">
                    Worktree existente detectada: <code>{{ props.resolution.path }}</code> (branch <code>{{ props.resolution.branch }}</code>)
                </p>
                <p v-else-if="props.resolution.status === 'existing_branch'" class="issue-open-tab-modal__res-info">
                    Branch local encontrada (<code>{{ props.resolution.branch }}</code>). Worktree sugerida: <code>{{ props.resolution.suggested_worktree_path }}</code>
                </p>
                <p v-else class="issue-open-tab-modal__res-info">
                    Nova worktree sugerida: <code>{{ props.resolution.suggested_worktree_path }}</code> (branch <code>{{ props.resolution.suggested_branch }}</code>)
                </p>
            </div>
            <MaxInputCheckbox v-model="createWorktreeIfMissing" label="Criar worktree isolada automaticamente caso ainda não exista" />
        </div>
        <footer class="issue-open-tab-modal__footer">
            <MaxButton label="Cancelar" severity="secondary" class="issue-open-tab-modal__btn" @click="handleCancel" />
            <MaxButton :label="props.submitting ? 'Abrindo...' : 'Abrir Aba'" class="issue-open-tab-modal__btn issue-open-tab-modal__btn--primary" :disabled="props.submitting" :loading="props.submitting" @click="onConfirm" />
        </footer>
    </MaxModal>
</template>

<script setup lang="ts">
    import { ref } from 'vue';
    import { MaxButton, MaxInputCheckbox, MaxInputRadio, MaxModal } from '@maxvue/max-components-ui';
    import type {
        AgentKind,
        IssueSummary,
        IssueTabDirectoryStrategy,
        IssueTabTargetKind,
        IssueWorktreeResolution,
        OpenIssueTabRequest,
        ProjectId
    } from '@/types/bindings';

    export interface IssueOpenTabModalProps {
        open: boolean;
        projectId: ProjectId;
        issue: IssueSummary | null;
        resolution: IssueWorktreeResolution | null;
        submitting?: boolean;
    }

    const props = withDefaults(defineProps<IssueOpenTabModalProps>(), {
        submitting: false
    });

    const emit = defineEmits<{
        close: [];
        confirm: [request: OpenIssueTabRequest];
    }>();

    type TargetOption = 'terminal' | AgentKind;

    const selectedTarget = ref<TargetOption>('terminal');
    const createWorktreeIfMissing = ref<boolean>(true);

    function onConfirm(): void {
        if (!props.issue) {
            return;
        }
        const target: IssueTabTargetKind =
            selectedTarget.value === 'terminal'
                ? { kind: 'terminal' }
                : {
                    kind: 'agent',
                    agent: selectedTarget.value,
                    inject_issue_context: true
                };

        const directory: IssueTabDirectoryStrategy = createWorktreeIfMissing.value
            ? {
                strategy: 'use_or_create_issue_worktree',
                branch_name: null,
                base_ref: null
            }
            : { strategy: 'use_project_root' };

        emit('confirm', {
            projectId: props.projectId,
            issueNumber: props.issue.number,
            target,
            directory
        });
    }

    async function onBeforeClose(event: { preventDefault: () => void }): Promise<boolean> {
        if (props.submitting) {
            event.preventDefault();
            return false;
        }
        return true;
    }

    function handleCancel(): void {
        if (props.submitting) {
            return;
        }
        emit('close');
    }

    function onUpdateVisible(visible: boolean): void {
        if (!visible) {
            if (props.submitting) {
                return;
            }
            emit('close');
        }
    }
</script>

<style lang="scss" scoped>
    .issue-open-tab-modal {
        position: fixed;
        inset: 0;
        z-index: 60;
        display: flex;
        align-items: center;
        justify-content: center;
        background: var(--mt-ui-overlay, rgb(0 0 0 / 60%));
    }

    .issue-open-tab-modal__card {
        width: 100%;
        max-width: 34rem;
        border-radius: 0.5rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-strong, var(--background-950));
        overflow: hidden;
    }

    .issue-open-tab-modal__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.75rem 1rem;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .issue-open-tab-modal__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
    }

    .issue-open-tab-modal__close {
        border: none;
        background: transparent;
        color: var(--mt-ui-text-secondary, var(--background-600));
        cursor: pointer;
    }

    .issue-open-tab-modal__body {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        padding: 1rem;
        font-size: 0.8125rem;
    }

    .issue-open-tab-modal__section {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .issue-open-tab-modal__label {
        font-weight: 600;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .issue-open-tab-modal__targets {
        display: flex;
        flex-wrap: wrap;
        gap: 0.625rem;
    }

    .issue-open-tab-modal__radio {
        display: inline-flex;
        align-items: center;
        gap: 0.3125rem;
        cursor: pointer;
    }

    .issue-open-tab-modal__resolution {
        padding: 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .issue-open-tab-modal__res-info {
        margin: 0;
        font-size: 0.75rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .issue-open-tab-modal__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        cursor: pointer;
    }

    .issue-open-tab-modal__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        padding: 0.75rem 1rem;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .issue-open-tab-modal__btn {
        height: 2rem;
        padding: 0 0.875rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &--primary {
            border-color: var(--max-primary-500);
            background: color-mix(in srgb, var(--max-primary-500) 20%, transparent);
            color: var(--max-primary-400);
        }
    }
</style>
