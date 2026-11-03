<template>
    <aside class="group-run-output-view" data-testid="group-run-output-view" aria-label="Saída ao vivo do comando em background">
        <header class="group-run-output-view__header">
            <div class="group-run-output-view__title-block">
                <span class="group-run-output-view__badge">Somente leitura</span>
                <strong class="group-run-output-view__title">{{ itemTitle }}</strong>
                <code class="group-run-output-view__command">{{ props.item.snapshot.command }}</code>
            </div>

            <div class="group-run-output-view__actions">
                <span v-if="output.ended.value" class="group-run-output-view__ended-tag" data-testid="group-run-output-ended">
                    Stream encerrado
                </span>
                <button type="button" class="group-run-output-view__close-btn" data-testid="group-run-output-close" aria-label="Fechar visualização de saída" @click="onClose">
                    <MaxIcon icon="lucide:x" />
                    <span>Fechar</span>
                </button>
            </div>
        </header>

        <div v-if="output.isTruncated.value || props.item.outputTruncated || output.droppedBytesTotal.value > 0" class="group-run-output-view__truncation-banner" role="status" data-testid="group-run-output-truncated-banner">
            <MaxIcon icon="lucide:alert-triangle" />
            <span>
                Exibindo os últimos 256 KiB sanitizados do ring buffer em memória.
                <template v-if="output.droppedBytesTotal.value > 0">
                    ({{ formatBytes(output.droppedBytesTotal.value) }} anteriores descartados por limite de retenção).
                </template>
            </span>
        </div>

        <div v-if="output.error.value" class="group-run-output-view__error" role="alert">
            {{ output.error.value }}
        </div>

        <div ref="terminalViewportRef" class="group-run-output-view__terminal" data-testid="group-run-output-terminal" tabindex="0" role="log" aria-readonly="true" aria-live="polite" @keydown="onReadonlyKeydown" @paste.prevent="onBlockedInput" @drop.prevent="onBlockedInput">
            <pre class="group-run-output-view__pre">{{ output.outputText.value || emptyPlaceholder }}</pre>
        </div>

        <footer class="group-run-output-view__footer">
            <span class="group-run-output-view__hint">
                Terminal somente leitura (execução em background sem stdin). Fechar este painel não interrompe o processo.
            </span>
            <span v-if="blockedInputNotice" class="group-run-output-view__blocked-notice" data-testid="group-run-output-blocked-notice">
                Entrada bloqueada: comandos paralelos em background não aceitam digitação.
            </span>
        </footer>
    </aside>
</template>

<script setup lang="ts">
    import { computed, onBeforeUnmount, ref, watch } from 'vue';
    import { useGroupRunOutput } from '@/composables/commandGroups/useGroupRunOutput';
    import type { GroupRunId, GroupRunItem } from '@/types/commandGroups';

    export interface GroupRunOutputViewProps {
        runId: GroupRunId;
        item: GroupRunItem;
    }

    const props = defineProps<GroupRunOutputViewProps>();

    export interface GroupRunOutputViewEmits {
        (e: 'close'): void;
    }

    const emit = defineEmits<GroupRunOutputViewEmits>();

    const terminalViewportRef = ref<HTMLElement | null>(null);
    const blockedInputNotice = ref<boolean>(false);

    const output = useGroupRunOutput({
        onTextChunk: () => {
            scrollToBottom();
        }
    });

    const itemTitle = computed<string>(() => {
        const lbl = props.item.snapshot.title?.trim();
        return lbl && lbl.length > 0 ? lbl : `#${props.item.position + 1}`;
    });

    const emptyPlaceholder = computed<string>(() => {
        if (output.attaching.value) {
            return 'Conectando ao stream de saída...';
        }
        return 'Nenhuma saída registrada até o momento.';
    });

    watch(
        [() => props.runId, () => props.item.id],
        ([runId, itemId]) => {
            blockedInputNotice.value = false;
            void output.attach(runId, itemId, props.item.outputTruncated);
        },
        { immediate: true }
    );

    onBeforeUnmount(() => {
        void output.detach();
    });

    function scrollToBottom(): void {
        const el = terminalViewportRef.value;
        if (!el) {
            return;
        }
        el.scrollTop = el.scrollHeight;
    }

    function formatBytes(bytes: number): string {
        if (bytes < 1024) {
            return `${bytes} B`;
        }
        const kib = (bytes / 1024).toFixed(1);
        return `${kib} KiB`;
    }

    function onReadonlyKeydown(event: KeyboardEvent): void {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'c') {
            return;
        }
        if (event.key === 'Escape') {
            event.preventDefault();
            void onClose();
            return;
        }
        event.preventDefault();
        event.stopPropagation();
        blockedInputNotice.value = true;
    }

    function onBlockedInput(): void {
        blockedInputNotice.value = true;
    }

    async function onClose(): Promise<void> {
        await output.detach();
        emit('close');
    }
</script>

<style lang="scss" scoped>
    .group-run-output-view {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 16rem;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
    }

    .group-run-output-view__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 0.5rem 0.875rem;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .group-run-output-view__title-block {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        min-width: 0;
        flex-wrap: wrap;
    }

    .group-run-output-view__badge {
        padding: 0.125rem 0.4375rem;
        border-radius: 0.25rem;
        background: rgb(14 165 233 / 15%);
        border: 1px solid var(--max-info-500);
        color: var(--max-info-400);
        font-size: 0.6875rem;
        font-weight: 600;
    }

    .group-run-output-view__title {
        font-size: 0.8125rem;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .group-run-output-view__command {
        font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
        font-size: 0.7188rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        padding: 0.125rem 0.375rem;
        border-radius: 0.25rem;
    }

    .group-run-output-view__actions {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .group-run-output-view__ended-tag {
        font-size: 0.6875rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .group-run-output-view__close-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.7188rem;
        font-weight: 600;
        cursor: pointer;
    }

    .group-run-output-view__truncation-banner {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.3125rem 0.875rem;
        background: rgb(245 158 11 / 15%);
        border-bottom: 1px solid var(--max-warning-500);
        color: var(--max-warning-300);
        font-size: 0.7188rem;
    }

    .group-run-output-view__error {
        padding: 0.375rem 0.875rem;
        background: rgb(239 68 68 / 15%);
        color: var(--max-danger-300);
        font-size: 0.75rem;
    }

    .group-run-output-view__terminal {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: 0.625rem 0.875rem;
        outline: none;
        background: var(--mt-ui-surface, var(--background-0));
    }

    .group-run-output-view__pre {
        margin: 0;
        font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
        font-size: 0.75rem;
        line-height: 1.45;
        color: var(--mt-ui-text, var(--background-800));
        white-space: pre-wrap;
        word-break: break-word;
    }

    .group-run-output-view__footer {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.3125rem 0.875rem;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
        font-size: 0.6875rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .group-run-output-view__blocked-notice {
        color: var(--max-warning-400);
        font-weight: 600;
    }
</style>
