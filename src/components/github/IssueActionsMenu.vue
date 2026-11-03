<template>
    <div class="issue-actions-menu" role="toolbar" aria-label="Ações da Issue">
        <button type="button" class="issue-actions-menu__btn" :disabled="props.busy" title="Enviar para o MaxCode (Abrir Issue)" @click="emit('send-maxcode', 'open')">
            <MaxIcon icon="lucide:external-link" />
            <span>Abrir no MaxCode</span>
        </button>
        <button type="button" class="issue-actions-menu__btn issue-actions-menu__btn--plan" :disabled="props.busy" title="Enviar para o MaxCode em modo Analisar (BUG PLAN)" @click="emit('send-maxcode', 'bug_plan')">
            <MaxIcon icon="lucide:compass" />
            <span>Analisar (BUG PLAN)</span>
        </button>
        <button type="button" class="issue-actions-menu__btn issue-actions-menu__btn--fix" :disabled="props.busy" title="Enviar para o MaxCode em modo Corrigir (BUG FIX)" @click="emit('send-maxcode', 'bug_fix')">
            <MaxIcon icon="lucide:wrench" />
            <span>Corrigir (BUG FIX)</span>
        </button>
        <button type="button" class="issue-actions-menu__btn issue-actions-menu__btn--terminal" :disabled="props.busy" title="Abrir Aba no MaxTerminal posicionada na worktree/branch da issue" @click="emit('open-tab-modal')">
            <MaxIcon icon="lucide:terminal" />
            <span>Abrir Aba no MaxTerminal</span>
        </button>
    </div>
</template>

<script setup lang="ts">
    import type { MaxCodeIssueAction } from '@/types/bindings';

    export interface IssueActionsMenuProps {
        busy?: boolean;
    }

    const props = withDefaults(defineProps<IssueActionsMenuProps>(), {
        busy: false
    });

    const emit = defineEmits<{
        'send-maxcode': [action: MaxCodeIssueAction];
        'open-tab-modal': [];
    }>();
</script>

<style lang="scss" scoped>
    .issue-actions-menu {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.5rem;
    }

    .issue-actions-menu__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        height: 1.875rem;
        padding: 0 0.6875rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }

        &--plan {
            border-color: var(--max-info-500);
            background: rgb(14 165 233 / 18%);
            color: var(--max-info-300);
        }

        &--fix {
            border-color: var(--max-warning-500);
            background: rgb(245 158 11 / 18%);
            color: var(--max-warning-300);
        }

        &--terminal {
            border-color: var(--max-primary-500);
            background: rgb(20 184 166 / 18%);
            color: var(--max-primary-400);
        }
    }
</style>
