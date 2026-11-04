<template>
    <div class="ai-context-preview" data-testid="ai-context-preview">
        <div class="ai-context-preview__grid">
            <div class="ai-context-preview__item">
                <span class="ai-context-preview__label">Shell</span>
                <code class="ai-context-preview__value">{{ context.shell }}</code>
            </div>
            <div class="ai-context-preview__item">
                <span class="ai-context-preview__label">Diretório (CWD)</span>
                <code class="ai-context-preview__value">{{ context.cwd }}</code>
            </div>
            <div v-if="context.projectName" class="ai-context-preview__item">
                <span class="ai-context-preview__label">Projeto</span>
                <span class="ai-context-preview__value">
                    {{ context.projectName }}
                    <MaxTag v-if="context.projectStack" size="sm" severity="secondary" :value="context.projectStack" />
                </span>
            </div>
            <div class="ai-context-preview__item">
                <span class="ai-context-preview__label">Sistema</span>
                <code class="ai-context-preview__value">{{ context.os.distro || context.os.family }} ({{ context.os.kernel }})</code>
            </div>
        </div>

        <div v-if="context.recentCommands.length > 0" class="ai-context-preview__section">
            <span class="ai-context-preview__section-title">Últimos comandos sanitizados ({{ context.recentCommands.length }})</span>
            <ul class="ai-context-preview__commands">
                <li v-for="(cmd, idx) in context.recentCommands" :key="idx" class="ai-context-preview__cmd">
                    <code class="ai-context-preview__cmd-text">{{ cmd.command }}</code>
                    <MaxTag v-if="cmd.exitCode !== null && cmd.exitCode !== undefined" size="sm" :severity="cmd.exitCode === 0 ? 'success' : 'danger'" :value="`exit ${cmd.exitCode}`" class="ai-context-preview__cmd-exit" />
                </li>
            </ul>
        </div>

        <div v-if="context.outputTail" class="ai-context-preview__section">
            <span class="ai-context-preview__section-title">Cauda de saída sanitizada</span>
            <pre class="ai-context-preview__tail">{{ context.outputTail }}</pre>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { MaxTag } from '@maxvue/max-components-ui';
    import type { AiContext_Serialize } from '@/types/bindings';

    export interface AiContextPreviewProps {
        context: AiContext_Serialize;
    }

    defineProps<AiContextPreviewProps>();
</script>

<style lang="scss" scoped>
    .ai-context-preview {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-800);
        background: var(--background-950);
        font-size: 0.75rem;
    }

    .ai-context-preview__grid {
        display: grid;
        grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
        gap: 0.375rem;
    }

    .ai-context-preview__item {
        display: flex;
        flex-direction: column;
        gap: 0.0625rem;
    }

    .ai-context-preview__label {
        font-size: 0.6875rem;
        color: var(--background-400);
    }

    .ai-context-preview__value {
        font-family: var(--mt-font-mono);
        color: var(--background-100);
    }

    .ai-context-preview__section {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .ai-context-preview__section-title {
        font-size: 0.6875rem;
        font-weight: 600;
        color: var(--background-300);
    }

    .ai-context-preview__commands {
        margin: 0;
        padding: 0;
        list-style: none;
        display: flex;
        flex-direction: column;
        gap: 0.1875rem;
    }

    .ai-context-preview__cmd {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.1875rem 0.375rem;
        border-radius: 0.25rem;
        background: var(--background-900);
    }

    .ai-context-preview__cmd-text {
        font-family: var(--mt-font-mono);
        font-size: 0.6875rem;
        color: var(--background-100);
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .ai-context-preview__cmd-exit {
        font-family: var(--mt-font-mono);
        font-size: 0.625rem;
        color: var(--max-success-400);

        &.is-error {
            color: var(--max-danger-400);
        }
    }

    .ai-context-preview__tail {
        margin: 0;
        padding: 0.375rem 0.5rem;
        border-radius: 0.25rem;
        background: var(--background-900);
        color: var(--background-200);
        font-family: var(--mt-font-mono);
        font-size: 0.6875rem;
        max-height: 6rem;
        overflow-y: auto;
        white-space: pre-wrap;
    }
</style>
