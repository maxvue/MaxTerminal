<template>
    <MaxModal v-if="props.modelValue && currentInspection" :visible="props.modelValue" title="Resolver Git Worktree pós-fechamento" :no-button="true" max-width="36rem" class="agent-close-dialog" :dismissable="!submitting" :close-on-escape="!submitting" @update:visible="emit('update:modelValue', $event)">
        <div class="agent-close-dialog__body">
            <div class="agent-close-dialog__meta">
                <div class="agent-close-dialog__meta-row">
                    <span class="agent-close-dialog__meta-label">Worktree:</span>
                    <code class="agent-close-dialog__code">{{ currentInspection.link.worktreePath ?? '—' }}</code>
                </div>
                <div class="agent-close-dialog__meta-row">
                    <span class="agent-close-dialog__meta-label">Branch:</span>
                    <code class="agent-close-dialog__code">{{ currentInspection.link.branch ?? '—' }}</code>
                </div>
                <div class="agent-close-dialog__meta-row">
                    <span class="agent-close-dialog__meta-label">Commits à frente da base:</span>
                    <strong>{{ currentInspection.aheadOfBase }}</strong>
                </div>
            </div>
            <MaxAlert v-if="blockerReasons.length > 0" type="warning" role="alert" class="agent-close-dialog__blockers">
                <strong class="agent-close-dialog__blockers-title">Restrições de segurança detectadas:</strong>
                <ul class="agent-close-dialog__blockers-list">
                    <li v-for="(reason, idx) in blockerReasons" :key="idx" class="agent-close-dialog__blocker-item">{{ reason }}</li>
                </ul>
            </MaxAlert>
            <div v-if="currentInspection.changedFiles.length > 0" class="agent-close-dialog__files">
                <span class="agent-close-dialog__files-title">Arquivos modificados ou não rastreados ({{ currentInspection.changedFiles.length }}{{ currentInspection.changedFilesTruncated ? '+' : '' }}):</span>
                <ul class="agent-close-dialog__files-list">
                    <li v-for="file in currentInspection.changedFiles" :key="file"><code>{{ file }}</code></li>
                </ul>
            </div>
            <MaxAlert v-if="errorMessage" type="danger" role="alert" class="agent-close-dialog__error">{{ errorMessage }}</MaxAlert>
        </div>
        <footer class="agent-close-dialog__footer">
            <MaxButton type="button" variant="primary" class="agent-close-dialog__btn agent-close-dialog__btn--keep" :disabled="submitting" @click="onResolve('keep')">Manter worktree</MaxButton>
            <MaxButton type="button" variant="secondary" class="agent-close-dialog__btn agent-close-dialog__btn--remove" :disabled="submitting || !canRemoveWorktree" @click="onResolve('remove')">Remover worktree</MaxButton>
            <MaxButton type="button" variant="danger" class="agent-close-dialog__btn agent-close-dialog__btn--delete-branch" :disabled="submitting || !canRemoveAndDeleteBranch" @click="onResolve('removeAndDeleteBranch')">Remover worktree e apagar branch</MaxButton>
        </footer>
    </MaxModal>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import MaxAlert from '@maxvue/max-components-ui/components/MaxAlert';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import MaxModal from '@maxvue/max-components-ui/components/MaxModal';
    import { isMtIpcError } from '@/ipc/core';
    import { useAgentsStore } from '@/stores/agents';
    import type {
        AgentCloseInspection_Serialize,
        AgentTabLink_Serialize,
        WorktreeResolution
    } from '@/types/bindings';

    export interface AgentWorktreeCloseDialogProps {
        modelValue: boolean;
        inspection: AgentCloseInspection_Serialize | null;
    }

    const props = defineProps<AgentWorktreeCloseDialogProps>();

    const emit = defineEmits<{
        'update:modelValue': [value: boolean];
        resolved: [link: AgentTabLink_Serialize];
    }>();

    const agentsStore = useAgentsStore();
    const localInspection = ref<AgentCloseInspection_Serialize | null>(props.inspection);
    const submitting = ref<boolean>(false);
    const errorMessage = ref<string | null>(null);

    watch(
        () => props.inspection,
        (next) => {
            localInspection.value = next;
            errorMessage.value = null;
        },
        { immediate: true }
    );

    const currentInspection = computed<AgentCloseInspection_Serialize | null>(
        () => localInspection.value ?? props.inspection
    );

    const canRemoveWorktree = computed<boolean>(() => {
        return Boolean(currentInspection.value?.allowed.includes('remove'));
    });

    const canRemoveAndDeleteBranch = computed<boolean>(() => {
        return Boolean(currentInspection.value?.allowed.includes('removeAndDeleteBranch'));
    });

    const blockerReasons = computed<string[]>(() => {
        const insp = currentInspection.value;
        if (!insp) {
            return [];
        }
        const reasons: string[] = [];
        if (!insp.link.createdByApp) {
            reasons.push('Worktree existente vinculada: apenas "Manter worktree" é permitido.');
        }
        if (insp.dirty) {
            reasons.push(
                'Existem alterações rastreadas ou arquivos não rastreados na worktree.'
            );
        }
        if (insp.hasIgnoredFiles) {
            reasons.push('Existem arquivos ignorados dentro da worktree.');
        }
        if (insp.inUse) {
            reasons.push('A worktree ainda está em uso por processos ativos ou outra aba.');
        }
        if (!insp.inspectionComplete) {
            reasons.push('A inspeção do estado Git não pôde ser concluída com segurança.');
        }
        if (insp.aheadOfBase > 0 || !insp.branchMerged) {
            reasons.push(
                'A branch possui commits exclusivos ou ainda não foi mesclada na branch base.'
            );
        }
        return reasons;
    });

    async function onResolve(action: WorktreeResolution): Promise<void> {
        const insp = currentInspection.value;
        if (!insp) {
            return;
        }
        submitting.value = true;
        errorMessage.value = null;
        try {
            const link = await agentsStore.resolveWorktree(
                insp.link.id,
                action,
                insp.inspectionToken
            );
            emit('resolved', link);
            emit('update:modelValue', false);
        } catch (err: unknown) {
            if (isMtIpcError(err) && err.code === 'agents.inspection_stale') {
                errorMessage.value =
                    'O estado da worktree mudou desde a inspeção; atualizando dados...';
                try {
                    const refreshed = await agentsStore.inspectClose(insp.link.tabId);
                    if (refreshed) {
                        localInspection.value = refreshed;
                    }
                } catch {
                    // Mantém mensagem de erro se revalidação falhar.
                }
            } else {
                errorMessage.value =
                    agentsStore.error?.message ??
                    (err instanceof Error
                        ? err.message
                        : 'Não foi possível resolver a worktree.');
            }
        } finally {
            submitting.value = false;
        }
    }
</script>

<style lang="scss" scoped>
    :deep(.agent-close-dialog.max-modal) {
        background-color: var(--background-850, var(--background-900));
        border-color: var(--background-600);
        color: var(--background-100);

        .max-modal-content {
            padding: 1rem;
        }
    }

    .agent-close-dialog__body {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
    }

    .agent-close-dialog__meta {
        display: flex;
        flex-direction: column;
        gap: 0.3125rem;
        font-size: 0.7813rem;
    }

    .agent-close-dialog__meta-row {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        flex-wrap: wrap;
    }

    .agent-close-dialog__meta-label {
        color: var(--background-400);
        font-weight: 600;
    }

    .agent-close-dialog__code {
        padding: 0.0625rem 0.3125rem;
        border-radius: 0.25rem;
        background: var(--background-800);
        font-size: 0.75rem;
    }

    .agent-close-dialog__blockers {
        font-size: 0.75rem;
    }

    .agent-close-dialog__blockers-title {
        display: block;
        margin-bottom: 0.25rem;
    }

    .agent-close-dialog__blockers-list {
        margin: 0;
        padding-left: 1.125rem;
    }

    .agent-close-dialog__files {
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        background: var(--background-800);
        border: 1px solid var(--background-700);
        font-size: 0.75rem;
        max-height: 9rem;
        overflow-y: auto;
    }

    .agent-close-dialog__files-title {
        display: block;
        margin-bottom: 0.25rem;
        font-weight: 600;
        color: var(--background-300);
    }

    .agent-close-dialog__files-list {
        margin: 0;
        padding-left: 1.125rem;
    }

    .agent-close-dialog__error {
        margin: 0;
        font-size: 0.75rem;
    }

    .agent-close-dialog__footer {
        display: flex;
        justify-content: flex-end;
        flex-wrap: wrap;
        gap: 0.5rem;
        padding-top: 0.75rem;
    }
</style>
