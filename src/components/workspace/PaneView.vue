<template>
    <div class="pane-view" :class="{ 'is-focused': props.focused, 'is-exited': props.pane.state === 'exited' }" @mousedown="onMouseDown">
        <header class="pane-view__toolbar">
            <span class="pane-view__cwd" :title="props.pane.cwd">{{ shortCwd }}</span>
            <div class="pane-view__actions">
                <MaxIconButton icon="lucide:columns-2" label="Dividir painel verticalmente" variant="ghost" class="pane-view__action-btn" title="Dividir verticalmente" :disabled="!props.canSplit" @click.stop="emit('split', props.pane.id, 'vertical')" />
                <MaxIconButton icon="lucide:rows-2" label="Dividir painel horizontalmente" variant="ghost" class="pane-view__action-btn" title="Dividir horizontalmente" :disabled="!props.canSplit" @click.stop="emit('split', props.pane.id, 'horizontal')" />
                <MaxIconButton icon="lucide:x" label="Fechar painel" variant="danger" class="pane-view__action-btn pane-view__action-btn--danger" title="Fechar painel" @click.stop="emit('close', props.pane.id)" />
            </div>
        </header>
        <div class="pane-view__body">
            <HybridTerminal v-if="props.pane.ptyId" :key="`${props.pane.id}:${props.pane.generation}:${props.pane.ptyId}`" :tab-id="props.tabId" :pane-id="props.pane.id" :pty-id="props.pane.ptyId" :visible="props.visible" @focus="emit('focus', props.pane.id)" @restart-requested="emit('restart', props.pane.id)" />
            <div v-else class="pane-view__exited" role="status">
                <p class="pane-view__exited-title">{{ exitDescription }}</p>
                <div class="pane-view__exited-actions">
                    <MaxButton label="Reiniciar Painel" variant="primary" size="small" class="pane-view__restart-btn" @click="emit('restart', props.pane.id)" />
                    <MaxButton label="Fechar Painel" variant="secondary" size="small" class="pane-view__close-btn" @click="emit('close', props.pane.id)" />
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, onMounted, watch } from 'vue';
    import { MaxButton, MaxIconButton } from '@maxvue/max-components-ui';
    import HybridTerminal from '@/components/terminal/HybridTerminal.vue';
    import type { PaneId, PaneView_Serialize, SplitDirection, TabId } from '@/types/bindings';

    export interface PaneViewProps {
        tabId: TabId;
        pane: PaneView_Serialize;
        focused: boolean;
        visible?: boolean;
        canSplit?: boolean;
    }

    const props = withDefaults(defineProps<PaneViewProps>(), {
        visible: true,
        canSplit: true
    });

    const emit = defineEmits<{
        focus: [paneId: PaneId];
        split: [paneId: PaneId, direction: SplitDirection];
        close: [paneId: PaneId];
        restart: [paneId: PaneId];
        'ready-for-initial-command': [tabId: TabId, paneId: PaneId, generation: number];
    }>();

    const shortCwd = computed<string>(() => {
        const raw = props.pane.cwd || '';
        const parts = raw.split('/').filter(Boolean);
        if (parts.length <= 2) {
            return raw || '~';
        }
        return `…/${parts.slice(-2).join('/')}`;
    });

    const exitDescription = computed<string>(() => {
        if (props.pane.error) {
            return props.pane.error.message;
        }
        if (props.pane.exit) {
            const code = props.pane.exit.code;
            const sig = props.pane.exit.signalName;
            if (sig) {
                return `Processo encerrado pelo sinal ${sig}.`;
            }
            if (code !== null && code !== undefined) {
                return `Processo finalizado com código de saída ${code}.`;
            }
        }
        return 'O processo deste painel foi encerrado.';
    });

    function notifyInitialCommandReady(): void {
        if (props.pane.ptyId && props.pane.state === 'live') {
            emit('ready-for-initial-command', props.tabId, props.pane.id, props.pane.generation);
        }
    }

    onMounted(() => {
        notifyInitialCommandReady();
    });

    watch(
        () => [props.pane.ptyId, props.pane.generation, props.pane.state] as const,
        () => {
            notifyInitialCommandReady();
        }
    );

    function onMouseDown(): void {
        if (!props.focused) {
            emit('focus', props.pane.id);
        }
    }
</script>

<style lang="scss" scoped>
    .pane-view {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        background: var(--mt-ui-surface, var(--background-0));
        border: 1px solid var(--mt-ui-border, var(--background-100));
        overflow: hidden;

        &.is-focused {
            border-color: color-mix(in srgb, var(--max-primary-500) 65%, transparent);
        }
    }

    .pane-view__toolbar {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        height: 1.5rem;
        padding: 0 0.5rem;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
        flex-shrink: 0;
    }

    .pane-view__cwd {
        font-size: 0.6875rem;
        font-family: var(--mt-font-mono);
        color: var(--mt-ui-text-muted, var(--background-400));
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .pane-view__actions {
        display: flex;
        align-items: center;
        gap: 0.1875rem;
    }

    .pane-view__action-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.25rem;
        height: 1.25rem;
        padding: 0;
        border: none;
        border-radius: 0.25rem;
        background: transparent;
        color: var(--mt-ui-text-muted, var(--background-400));
        cursor: pointer;

        &:disabled {
            opacity: 0.4;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            background: var(--mt-ui-surface-hover, var(--background-75));
            color: var(--mt-ui-text, var(--background-800));
        }

        &--danger:hover:not(:disabled) {
            color: var(--max-danger-400);
        }
    }

    .pane-view__body {
        flex: 1;
        min-width: 0;
        min-height: 0;
        position: relative;
        display: flex;
        flex-direction: column;
    }

    .pane-view__exited {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 0.75rem;
        padding: 1.5rem;
        text-align: center;
        background: var(--mt-ui-surface, var(--background-0));
    }

    .pane-view__exited-title {
        margin: 0;
        font-size: 0.8125rem;
        color: var(--mt-ui-text, var(--background-800));
    }

    .pane-view__exited-actions {
        display: flex;
        gap: 0.5rem;
    }

    .pane-view__restart-btn {
        padding: 0.375rem 0.75rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--max-primary-500);
        background: rgb(20 184 166 / 15%);
        color: var(--max-primary-400);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
    }

    .pane-view__close-btn {
        padding: 0.375rem 0.75rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: transparent;
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
    }
</style>
