<template>
    <div v-if="repositoryContext" class="git-indicator">
        <MaxButton type="button" class="git-indicator__chip" :class="chipStateClass" :title="tooltipText" @click.stop="toggleDetails">
            <MaxIcon icon="lucide:git-branch" :size="0.75" class="git-indicator__icon" />
            <span class="git-indicator__branch">{{ branchLabel }}</span>
            <span v-if="repositoryContext.changes.dirty" class="git-indicator__dirty-dot" aria-label="Repositório com alterações locais" />
            <span v-if="upstreamSummary" class="git-indicator__upstream">{{ upstreamSummary }}</span>
        </MaxButton>
        <div v-if="detailsOpen" class="git-indicator__popover" role="dialog" aria-label="Detalhes do Git" @click.stop>
            <div class="git-indicator__popover-head">
                <span class="git-indicator__popover-title">{{ branchLabel }}</span>
                <MaxButton type="button" size="sm" severity="secondary" icon="lucide:refresh-cw" aria-label="Atualizar status" class="git-indicator__refresh-btn" @click="onRefresh">
                    <span>Atualizar status</span>
                </MaxButton>
            </div>
            <div class="git-indicator__stats">
                <span class="git-indicator__stat">Staged: <strong>{{ repositoryContext.changes.staged }}</strong></span>
                <span class="git-indicator__stat">Modificados: <strong>{{ repositoryContext.changes.unstaged }}</strong></span>
                <span class="git-indicator__stat">Untracked: <strong>{{ repositoryContext.changes.untracked }}</strong></span>
                <span v-if="repositoryContext.changes.conflicted > 0" class="git-indicator__stat git-indicator__stat--conflict">Conflitos: <strong>{{ repositoryContext.changes.conflicted }}</strong></span>
            </div>
            <p class="git-indicator__root">{{ repositoryContext.worktreeRoot }}</p>
        </div>
    </div>
    <div v-else-if="degradedContext" class="git-indicator git-indicator--degraded">
        <span class="git-indicator__chip git-indicator__chip--degraded" :title="degradedContext.message">
            <MaxIcon icon="lucide:alert-triangle" :size="0.75" class="git-indicator__icon" />
            <span class="git-indicator__branch">Git indisponível</span>
        </span>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import { MaxButton, MaxIcon } from '@maxvue/max-components-ui';
    import type { GitContext_Serialize } from '@/types/bindings';

    export interface GitIndicatorProps {
        context: GitContext_Serialize | null;
    }

    const props = defineProps<GitIndicatorProps>();

    const emit = defineEmits<{
        refresh: [path: string];
    }>();

    const detailsOpen = ref<boolean>(false);

    const repositoryContext = computed(() =>
        props.context?.kind === 'repository' ? props.context : null
    );

    const degradedContext = computed(() =>
        props.context?.kind === 'degraded' ? props.context : null
    );

    const branchLabel = computed<string>(() => {
        const repo = repositoryContext.value;
        if (!repo) {
            return '';
        }
        const b = repo.branch;
        if (b.state === 'named') {
            return b.name;
        }
        if (b.state === 'detached') {
            return `HEAD @ ${b.oid}`;
        }
        return `${b.name} (sem commits)`;
    });

    const upstreamSummary = computed<string>(() => {
        const up = repositoryContext.value?.upstream;
        if (!up) {
            return '';
        }
        const parts: string[] = [];
        if (up.ahead > 0) {
            parts.push(`↑${up.ahead}`);
        }
        if (up.behind > 0) {
            parts.push(`↓${up.behind}`);
        }
        return parts.join(' ');
    });

    const chipStateClass = computed<string>(() => {
        const repo = repositoryContext.value;
        if (!repo) {
            return '';
        }
        if (repo.changes.conflicted > 0) {
            return 'git-indicator__chip--conflict';
        }
        if (repo.changes.dirty) {
            return 'git-indicator__chip--dirty';
        }
        return 'git-indicator__chip--clean';
    });

    const tooltipText = computed<string>(() => {
        const repo = repositoryContext.value;
        if (!repo) {
            return '';
        }
        const c = repo.changes;
        const statusLabel = c.dirty
            ? `+${c.staged} ~${c.unstaged} ?${c.untracked}${c.conflicted > 0 ? ` !${c.conflicted}` : ''}`
            : 'Árvore limpa';
        return `Branch: ${branchLabel.value} (${statusLabel})`;
    });

    function toggleDetails(): void {
        detailsOpen.value = !detailsOpen.value;
    }

    function onRefresh(): void {
        const repo = repositoryContext.value;
        if (repo) {
            emit('refresh', repo.worktreeRoot);
        }
    }
</script>

<style lang="scss" scoped>
    .git-indicator {
        position: relative;
        display: inline-flex;
        align-items: center;

        &__chip {
            display: inline-flex;
            align-items: center;
            gap: 4px;
            max-width: 140px;
            padding: 1px 6px;
            font-size: 0.6875rem;
            color: var(--mt-ui-text-secondary, var(--background-600));
            background: var(--mt-ui-surface-elevated, var(--background-50));
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 4px;
            cursor: pointer;

            &--clean {
                border-color: color-mix(in srgb, var(--max-success-500) 35%, transparent);
            }

            &--dirty {
                color: var(--max-warning-500);
                border-color: color-mix(in srgb, var(--max-warning-500) 45%, transparent);
            }

            &--conflict {
                color: var(--max-danger-500);
                border-color: color-mix(in srgb, var(--max-danger-500) 55%, transparent);
            }

            &--degraded {
                color: var(--max-warning-500);
                cursor: default;
            }
        }

        &__icon {
            flex-shrink: 0;
            font-size: 0.75rem;
        }

        &__branch {
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }

        &__dirty-dot {
            flex-shrink: 0;
            width: 6px;
            height: 6px;
            background: var(--max-warning-500);
            border-radius: 999px;
        }

        &__upstream {
            flex-shrink: 0;
            font-family: monospace;
            font-size: 0.625rem;
        }

        &__popover {
            position: absolute;
            top: calc(100% + 4px);
            left: 0;
            z-index: 40;
            min-width: 220px;
            padding: 8px 10px;
            background: var(--mt-ui-surface-panel, var(--background-25));
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 6px;
        }

        &__popover-head {
            display: flex;
            align-items: center;
            justify-content: space-between;
            gap: 8px;
            margin-bottom: 6px;
        }

        &__popover-title {
            font-size: 0.75rem;
            font-weight: 600;
            color: var(--mt-ui-text-strong, var(--background-950));
        }

        &__refresh-btn {
            display: inline-flex;
            align-items: center;
            gap: 4px;
            padding: 2px 6px;
            font-size: 0.6875rem;
            color: var(--mt-ui-text-secondary, var(--background-600));
            background: var(--mt-ui-surface-elevated, var(--background-50));
            border: 1px solid var(--mt-ui-border, var(--background-100));
            border-radius: 4px;
            cursor: pointer;
        }

        &__stats {
            display: flex;
            flex-wrap: wrap;
            gap: 6px;
            font-size: 0.6875rem;
            color: var(--mt-ui-text-muted, var(--background-400));
        }

        &__stat--conflict {
            color: var(--max-danger-500);
        }

        &__root {
            margin: 6px 0 0;
            overflow: hidden;
            font-family: monospace;
            font-size: 0.625rem;
            color: var(--background-400);
            text-overflow: ellipsis;
            white-space: nowrap;
        }
    }
</style>
