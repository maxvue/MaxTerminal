<template>
    <span v-if="props.worktree && props.worktree.isLinked" class="worktree-badge" :class="kindClass" :title="badgeTitle">
        <MaxIcon icon="lucide:git-fork" size="0.6875" class="worktree-badge__icon" />
        <span class="worktree-badge__label">WT: {{ props.worktree.name }}</span>
        <MaxIcon v-if="props.worktree.locked" icon="lucide:lock" size="0.6875" class="worktree-badge__lock" aria-label="Worktree bloqueada" />
    </span>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import type { GitWorktree } from '@/types/bindings';

    export interface WorktreeBadgeProps {
        worktree: GitWorktree | null;
    }

    const props = defineProps<WorktreeBadgeProps>();

    const kindClass = computed<string>(() => {
        const wt = props.worktree;
        if (!wt) {
            return '';
        }
        if (wt.kind === 'max_terminal') {
            return 'worktree-badge--max-terminal';
        }
        if (wt.kind === 'max_code') {
            return 'worktree-badge--max-code';
        }
        return 'worktree-badge--external';
    });

    const badgeTitle = computed<string>(() => {
        const wt = props.worktree;
        if (!wt) {
            return '';
        }
        let originLabel = 'Worktree externa';
        if (wt.kind === 'max_terminal') {
            originLabel = 'Worktree (.worktrees)';
        } else if (wt.kind === 'max_code') {
            originLabel = 'Worktree MaxCode (.max-code-worktrees)';
        }
        return `${originLabel}: ${wt.path}`;
    });
</script>

<style lang="scss" scoped>
    .worktree-badge {
        display: inline-flex;
        align-items: center;
        gap: 3px;
        max-width: 110px;
        padding: 1px 5px;
        font-size: 0.6875rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 4px;

        &--max-terminal {
            color: var(--max-info-500);
            background: color-mix(in srgb, var(--max-info-500) 12%, transparent);
            border-color: color-mix(in srgb, var(--max-info-500) 40%, transparent);
        }

        &--max-code {
            color: var(--max-primary-500);
            background: color-mix(in srgb, var(--max-primary-500) 12%, transparent);
            border-color: color-mix(in srgb, var(--max-primary-500) 40%, transparent);
        }

        &--external {
            color: var(--mt-ui-text-secondary, var(--background-600));
            background: var(--mt-ui-surface-elevated, var(--background-50));
        }

        &__icon,
        &__lock {
            flex-shrink: 0;
        }

        &__label {
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }
    }
</style>
