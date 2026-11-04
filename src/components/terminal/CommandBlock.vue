<template>
    <div class="command-block" :class="[`status-${block.status}`, { 'is-compact': compact }]" :data-block-id="block.id">
        <div class="block-header">
            <div class="block-meta-left">
                <CommandBlockStatus :status="block.status" :backend-status="metaBackendStatus" :exit-code="metaExitCode" :duration-ms="entry.durationMs" :continuous="metaContinuous" :persistence="metaPersistence" :output-truncated="metaOutputTruncated" :confidence="metaConfidence" />
                <code class="block-command">{{ entry.command }}</code>
                <span v-if="block.cwd" class="block-cwd" :title="block.cwd">{{ block.cwd }}</span>
            </div>
            <div class="block-meta-right">
                <span class="block-duration">{{ formattedDuration }}</span>
                <span v-if="block.exitCode !== null && block.exitCode !== 0" class="block-exit-code">
                    exit {{ block.exitCode }}
                </span>
                <div class="block-actions">
                    <MaxButton v-for="act in actions" :key="act.id" :label="act.label" :icon="act.icon" variant="text" severity="secondary" size="small" class="block-action-btn" :title="act.label" :aria-label="act.label" @click="emit('action', act.id)" />
                    <InspectButton :block="block" />
                    <slot name="actions" :block="block" />
                </div>
            </div>
        </div>
        <InlineDiagnosis :block="block" />
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxButton } from '@maxvue/max-components-ui';
    import InlineDiagnosis from '@/components/ai/InlineDiagnosis.vue';
    import InspectButton from '@/components/bridge/InspectButton.vue';
    import CommandBlockStatus from '@/components/shell/CommandBlockStatus.vue';
    import type { BlockStatus, ObservationConfidence, PersistenceState } from '@/types/bindings';
    import type { BlockAction, CommandBlockModel } from '@/types/terminal';
    import type { VueWebTerminalProjectedEntry } from '@/terminal/blocks/vueWebTerminalAdapter';

    export interface CommandBlockProps {
        block: CommandBlockModel;
        entry: VueWebTerminalProjectedEntry;
        actions: BlockAction[];
        compact?: boolean;
    }

    const props = withDefaults(defineProps<CommandBlockProps>(), {
        compact: false
    });

    const emit = defineEmits<{
        action: [actionId: string];
    }>();

    const metaBackendStatus = computed<BlockStatus | null>(() => {
        const val = props.block.meta?.backendStatus;
        return typeof val === 'string' ? (val as BlockStatus) : null;
    });

    const metaExitCode = computed<number | null>(() => {
        if (typeof props.block.meta?.rawExitCode === 'number') {
            return props.block.meta.rawExitCode;
        }
        return props.block.exitCode;
    });

    const metaContinuous = computed<boolean>(() => Boolean(props.block.meta?.isContinuous));

    const metaPersistence = computed<PersistenceState | null>(() => {
        const val = props.block.meta?.persistence;
        return typeof val === 'string' ? (val as PersistenceState) : null;
    });

    const metaOutputTruncated = computed<boolean>(() => Boolean(props.block.meta?.outputTruncated));

    const metaConfidence = computed<ObservationConfidence | null>(() => {
        const val = props.block.meta?.confidence;
        return typeof val === 'string' ? (val as ObservationConfidence) : null;
    });

    const formattedDuration = computed<string>(() => {
        const ms = props.entry.durationMs;
        if (ms === null) {
            return 'Em execução...';
        }
        if (ms < 1000) {
            return `${ms} ms`;
        }
        const seconds = ms / 1000;
        if (seconds < 60) {
            return `${seconds.toFixed(1).replace('.', ',')} s`;
        }
        const mins = Math.floor(seconds / 60);
        const remSecs = Math.round(seconds % 60);
        return `${mins} min ${remSecs} s`;
    });
</script>

<style lang="scss" scoped>
    .command-block {
        pointer-events: auto;
        width: 100%;
        background: var(--mt-block-background, var(--mt-ui-surface-panel, var(--background-25)));
        border: 1px solid var(--mt-block-border, var(--mt-ui-border, var(--background-100)));
        border-radius: 6px;
        overflow: hidden;

        &.status-error {
            border-color: var(--max-danger-500);
        }

        &.status-aborted {
            border-color: var(--max-warning-500);
        }
    }

    .block-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 10px;
        padding: 4px 10px;
        background: var(--mt-block-header-background, var(--mt-ui-surface-elevated, var(--background-50)));
        font-size: 11px;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .block-meta-left,
    .block-meta-right,
    .block-actions {
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .block-meta-left {
        min-width: 0;
        flex: 1;
        overflow: hidden;
    }

    .status-badge {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 1px 6px;
        border-radius: 999px;
        font-size: 10px;
        font-weight: 600;
        flex-shrink: 0;

        &.badge-running {
            background: var(--mt-ui-success-subtle, color-mix(in srgb, var(--max-success-500) 15%, transparent));
            color: var(--max-success-500);
            border: 1px solid var(--max-success-500);
        }

        &.badge-success {
            background: transparent;
            color: var(--max-success-500);
            border: 1px solid var(--max-success-500);
        }

        &.badge-error {
            background: var(--mt-ui-danger-subtle, color-mix(in srgb, var(--max-danger-500) 15%, transparent));
            color: var(--max-danger-500);
            border: 1px solid var(--max-danger-500);
        }

        &.badge-aborted {
            background: var(--mt-ui-warning-subtle, color-mix(in srgb, var(--max-warning-500) 15%, transparent));
            color: var(--max-warning-500);
            border: 1px solid var(--max-warning-500);
        }
    }

    .status-dot {
        width: 6px;
        height: 6px;
        border-radius: 50%;
        background: currentcolor;
    }

    .block-command {
        font-family: var(--mt-font-mono);
        color: var(--mt-ui-text-strong, var(--background-950));
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .block-cwd {
        color: var(--mt-ui-text-muted, var(--background-400));
        font-family: var(--mt-font-mono);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .block-duration {
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-family: var(--mt-font-mono);
        white-space: nowrap;
    }

    .block-exit-code {
        padding: 1px 5px;
        background: color-mix(in srgb, var(--max-danger-500) 20%, transparent);
        border: 1px solid var(--max-danger-500);
        border-radius: 4px;
        color: var(--max-danger-400);
        font-family: var(--mt-font-mono);
        font-weight: 600;
    }

    .block-action-btn {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 2px 6px;
        background: var(--mt-ui-surface, var(--background-0));
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 4px;
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 11px;
        cursor: pointer;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
            color: var(--mt-ui-text-strong, var(--background-950));
        }
    }

    .action-icon {
        font-size: 13px;
    }
</style>
