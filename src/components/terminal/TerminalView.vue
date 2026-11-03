<template>
    <div ref="hostRef" class="terminal-view" role="application" aria-label="Terminal" tabindex="-1" @mousedown="onHostMouseDown" />
</template>

<script setup lang="ts">
    import { ref, toRef } from 'vue';
    import '@xterm/xterm/css/xterm.css';
    import type { ExitInfo, PaneId, PtyId } from '@/types/bindings';
    import type { TerminalInstanceHandle, TerminalModes } from '@/types/terminal';
    import { useTerminal } from '@/composables/terminal/useTerminal';

    export interface TerminalViewProps {
        paneId: PaneId;
        ptyId: PtyId;
        visible?: boolean | undefined;
    }

    const props = withDefaults(defineProps<TerminalViewProps>(), {
        visible: true
    });

    const emit = defineEmits<{
        ready: [handle: TerminalInstanceHandle];
        exit: [exit: ExitInfo];
        'title-change': [title: string];
        'modes-change': [modes: TerminalModes];
    }>();

    const hostRef = ref<HTMLElement | null>(null);
    const paneIdRef = toRef(props, 'paneId');
    const ptyIdRef = toRef(props, 'ptyId');
    const visibleRef = toRef(props, 'visible');

    const { focus } = useTerminal(paneIdRef, ptyIdRef, hostRef, visibleRef, {
        onReady: (handle) => {
            emit('ready', handle);
        },
        onExit: (exit) => {
            emit('exit', exit);
        },
        onTitleChange: (title) => {
            emit('title-change', title);
        },
        onModesChange: (modes) => {
            emit('modes-change', modes);
        }
    });

    function onHostMouseDown(): void {
        focus();
    }
</script>

<style lang="scss" scoped>
    .terminal-view {
        position: relative;
        width: 100%;
        height: 100%;
        overflow: hidden;
        contain: strict;
        background: var(--mt-ui-surface, var(--background-0));
    }
</style>
