<template>
    <div class="hybrid-terminal" :style="themeUiStyle" @keydown.capture="onContainerKeydown" @focusin="onFocusIn">
        <div v-if="hasGapWarning" class="hybrid-gap-banner" role="status">
            [MaxTerminal: {{ instanceState?.flow.droppedBytes }} bytes descartados antes da exibição — visualização parcial]
        </div>
        <div v-if="instanceState?.search.open" class="hybrid-search-anchor">
            <TerminalSearchBar :pane-id="paneId" />
        </div>
        <div class="hybrid-terminal-viewport">
            <TerminalView :pane-id="paneId" :pty-id="ptyId" :visible="visible" @ready="onTerminalReady" @exit="onTerminalExit" @title-change="emit('title-change', $event)" @modes-change="onModesChange" />
            <CommandBlockLayer v-if="isReady" :key="paneId" :pane-id="paneId" :get-context="getInstanceContext" :on-input-intent="handleBlockInputIntent">
                <template #block-actions="{ block }">
                    <slot name="block-actions" :block="block" />
                </template>
            </CommandBlockLayer>
        </div>
        <TerminalExitBanner v-if="instanceState?.exit" :exit="instanceState.exit" @restart-requested="emit('restart-requested')" />
        <AiPaneResults v-if="showRichInput" :pane-id="paneId" />
        <RichInput v-if="showRichInput" ref="richInputRef" :pane-id="paneId" :disabled="Boolean(instanceState?.exit)" />
    </div>
</template>

<script setup lang="ts">
    import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
    import AiPaneResults from '@/components/ai/AiPaneResults.vue';
    import { useAiTriggers } from '@/composables/ai/useAiTriggers';
    import { useEditorLinks } from '@/composables/useEditorLinks';
    import { useShellCommandBlocks } from '@/composables/useShellCommandBlocks';
    import type { ExitInfo, LinkContext, PaneId, PtyId, TabId } from '@/types/bindings';
    import type {
        Disposable,
        TerminalContext,
        TerminalInstanceHandle,
        TerminalModes
    } from '@/types/terminal';
    import { useTerminalStore } from '@/stores/terminal';
    import { useWindowStore } from '@/stores/window';
    import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
    import { resolveTerminalContext } from '@/terminal/extensions/terminalContext';
    import { isSearchShortcut, isToggleInputModeShortcut } from '@/terminal/keybindings';
    import { themeUiToCssVars } from '@/terminal/theme/themeUiVars';
    import CommandBlockLayer from './CommandBlockLayer.vue';
    import RichInput from './RichInput.vue';
    import TerminalExitBanner from './TerminalExitBanner.vue';
    import TerminalSearchBar from './TerminalSearchBar.vue';
    import TerminalView from './TerminalView.vue';

    export interface HybridTerminalProps {
        tabId?: TabId | undefined;
        paneId: PaneId;
        ptyId: PtyId;
        visible?: boolean;
        richInput?: boolean;
    }

    const props = withDefaults(defineProps<HybridTerminalProps>(), {
        tabId: undefined,
        visible: true,
        richInput: true
    });

    const emit = defineEmits<{
        exit: [exit: ExitInfo];
        'restart-requested': [];
        'title-change': [title: string];
        focus: [];
    }>();

    const terminalStore = useTerminalStore();
    const windowStore = useWindowStore();
    useAiTriggers();
    const isReady = ref<boolean>(false);
    const richInputRef = ref<InstanceType<typeof RichInput> | null>(null);
    const hadRichFocusBeforeAltScreen = ref<boolean>(false);
    let editorLinksSub: Disposable | null = null;
    let shellBlocksSub: ReturnType<typeof useShellCommandBlocks> | null = null;

    function startShellBlocksSync(paneId: PaneId, ptyId: PtyId): void {
        shellBlocksSub?.stop();
        shellBlocksSub = useShellCommandBlocks({
            paneId,
            ptyId
        });
        void shellBlocksSub.start().catch(() => {});
    }

    const instanceState = computed(() => terminalStore.instance(props.paneId));

    const themeUiStyle = computed<Record<string, string>>(() => {
        if (!instanceState.value) {
            return {};
        }
        return themeUiToCssVars(instanceState.value.themeUi);
    });

    const hasGapWarning = computed<boolean>(() => {
        const dropped = instanceState.value?.flow.droppedBytes ?? '0';
        try {
            return BigInt(dropped) > 0n;
        } catch {
            return false;
        }
    });

    const showRichInput = computed<boolean>(() => {
        if (!props.richInput) {
            return false;
        }
        return instanceState.value?.inputMode === 'rich';
    });

    watch(
        () => props.richInput,
        (enabled) => {
            if (!enabled) {
                terminalStore.setInputModeOverride(props.paneId, 'passthrough');
            } else if (instanceState.value?.inputModeOverride === 'passthrough') {
                terminalStore.setInputModeOverride(props.paneId, null);
            }
        },
        { immediate: true }
    );

    function resolvePaneLinkContext(): LinkContext | null {
        const tracked = windowStore.linkContextForPane(props.paneId);
        if (tracked) {
            return tracked;
        }
        if (props.tabId) {
            return {
                tabId: props.tabId,
                paneId: props.paneId,
                ptyId: props.ptyId,
                cwdEpoch: 1
            };
        }
        return null;
    }

    function onTerminalReady(handle: TerminalInstanceHandle): void {
        isReady.value = true;
        if (!props.richInput) {
            terminalStore.setInputModeOverride(props.paneId, 'passthrough');
        }
        editorLinksSub?.dispose();
        editorLinksSub = useEditorLinks(handle.xterm, {
            getContext: resolvePaneLinkContext
        });
        startShellBlocksSync(props.paneId, props.ptyId);
    }

    watch(
        () => [props.paneId, props.ptyId] as const,
        ([nextPaneId, nextPtyId]) => {
            if (isReady.value) {
                startShellBlocksSync(nextPaneId, nextPtyId);
            }
        }
    );

    onBeforeUnmount(() => {
        shellBlocksSub?.stop();
        shellBlocksSub = null;
        editorLinksSub?.dispose();
        editorLinksSub = null;
    });

    function onTerminalExit(exit: ExitInfo): void {
        emit('exit', exit);
    }

    function onModesChange(modes: TerminalModes): void {
        if (modes.altScreen) {
            const activeEl = typeof document !== 'undefined' ? document.activeElement : null;
            hadRichFocusBeforeAltScreen.value = Boolean(
                activeEl && activeEl.classList.contains('rich-textarea')
            );
            terminalInstanceRegistry.get(props.paneId)?.focus();
            return;
        }

        if (hadRichFocusBeforeAltScreen.value && showRichInput.value && !instanceState.value?.search.open) {
            hadRichFocusBeforeAltScreen.value = false;
            void nextTick(() => {
                richInputRef.value?.focus();
            });
        }
    }

    function getInstanceContext(): TerminalContext {
        const inst = terminalInstanceRegistry.get(props.paneId);
        if (inst) {
            return inst.getContext();
        }
        return resolveTerminalContext({
            paneId: props.paneId,
            ptyId: props.ptyId
        });
    }

    async function handleBlockInputIntent(commandText: string): Promise<void> {
        const inst = terminalInstanceRegistry.get(props.paneId);
        if (inst) {
            await inst.sendLine(commandText);
        }
    }

    function onContainerKeydown(event: KeyboardEvent): void {
        if (isSearchShortcut(event)) {
            event.preventDefault();
            event.stopPropagation();
            terminalStore.openSearch(props.paneId);
            return;
        }
        if (isToggleInputModeShortcut(event)) {
            event.preventDefault();
            event.stopPropagation();
            terminalStore.toggleInputMode(props.paneId);
        }
    }

    function onFocusIn(): void {
        terminalStore.setActive(props.paneId);
        emit('focus');
    }
</script>

<style lang="scss" scoped>
    .hybrid-terminal {
        position: relative;
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        overflow: hidden;
        background: var(--background-950);
    }

    .hybrid-gap-banner {
        padding: 4px 10px;
        background: rgb(245 158 11 / 18%);
        border-bottom: 1px solid var(--max-warning-500);
        color: var(--max-warning-300);
        font-family: var(--mt-font-mono);
        font-size: 11px;
    }

    .hybrid-search-anchor {
        position: absolute;
        top: 8px;
        right: 12px;
        z-index: 25;
    }

    .hybrid-terminal-viewport {
        position: relative;
        flex: 1;
        min-height: 0;
        width: 100%;
        overflow: hidden;
    }
</style>
