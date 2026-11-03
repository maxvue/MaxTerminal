<template>
    <div class="issue-actions-menu" role="toolbar" aria-label="Ações da Issue">
        <MaxButton label="Abrir no MaxCode" icon="lucide:external-link" class="issue-actions-menu__btn" :disabled="props.busy" title="Enviar para o MaxCode (Abrir Issue)" @click="emit('send-maxcode', 'open')" />
        <MaxButton label="Analisar (BUG PLAN)" icon="lucide:compass" class="issue-actions-menu__btn issue-actions-menu__btn--plan" :disabled="props.busy" title="Enviar para o MaxCode em modo Analisar (BUG PLAN)" @click="emit('send-maxcode', 'bug_plan')" />
        <MaxButton label="Corrigir (BUG FIX)" icon="lucide:wrench" class="issue-actions-menu__btn issue-actions-menu__btn--fix" :disabled="props.busy" title="Enviar para o MaxCode em modo Corrigir (BUG FIX)" @click="emit('send-maxcode', 'bug_fix')" />
        <MaxButton label="Abrir Aba no MaxTerminal" icon="lucide:terminal" class="issue-actions-menu__btn issue-actions-menu__btn--terminal" :disabled="props.busy" title="Abrir Aba no MaxTerminal posicionada na worktree/branch da issue" @click="emit('open-tab-modal')" />
    </div>
</template>

<script setup lang="ts">
    import { MaxButton } from '@maxvue/max-components-ui';
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
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-100);
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
