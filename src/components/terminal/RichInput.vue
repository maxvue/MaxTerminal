<template>
    <div class="rich-input-container">
        <div v-if="completion.open.value" class="rich-input-popup-anchor">
            <CompletionPopup :listbox-id="listboxId" :items="completion.items.value" :sections="completion.sections.value" :active-index="completion.activeIndex.value" :loading="completion.loading.value" :error="completion.error.value" :title="completion.title.value" :stage-label="completion.stageLabel.value" :empty-message="completion.emptyMessage.value" @hover="completion.setActiveIndex($event)" @select="onSelectFromPopup" />
        </div>
        <div class="rich-input-bar" :class="{ 'is-disabled': disabled }">
            <span class="prompt-chevron" aria-hidden="true">❯</span>
            <textarea ref="textareaRef" v-model="text" class="rich-textarea" role="combobox" aria-autocomplete="list" aria-multiline="true" aria-label="Entrada de comando do terminal" :aria-expanded="completion.open.value" :aria-controls="listboxId" :aria-activedescendant="activeDescendantId" :disabled="disabled" :rows="computedRows" placeholder="Digite um comando ou use >, >>, @, ?, /..." @input="onTextareaInput" @click="syncCursor" @keyup="syncCursor" @keydown="onTextareaKeydown" @paste="onPaste" />
            <div class="rich-input-actions">
                <InputModeToggle :pane-id="paneId" @click="emit('mode-toggle')" />
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, onBeforeUnmount, ref, toRef, watch } from 'vue';
    import { registerRichInputInsertHandler } from '@/composables/ai/adapters';
    import type { PaneId } from '@/types/bindings';
    import type { CompletionItem } from '@/types/terminal';
    import { useRichInput } from '@/composables/terminal/useRichInput';
    import CompletionPopup from './CompletionPopup.vue';
    import InputModeToggle from './InputModeToggle.vue';

    export interface RichInputProps {
        paneId: PaneId;
        disabled?: boolean;
    }

    const props = withDefaults(defineProps<RichInputProps>(), {
        disabled: false
    });

    const emit = defineEmits<{
        submit: [text: string];
        'mode-toggle': [];
    }>();

    const textareaRef = ref<HTMLTextAreaElement | null>(null);
    const paneIdRef = toRef(props, 'paneId');

    const {
        text,
        cursor,
        completion,
        activeDescendantId,
        listboxId,
        onKeydown,
        onPaste,
        selectCompletionItem
    } = useRichInput(paneIdRef);

    let unregisterInsertHandler: (() => void) | null = null;

    watch(
        () => props.paneId,
        (pid) => {
            unregisterInsertHandler?.();
            unregisterInsertHandler = registerRichInputInsertHandler(pid, (cmd) => {
                text.value = cmd;
                cursor.value = cmd.length;
                textareaRef.value?.focus();
            });
        },
        { immediate: true }
    );

    onBeforeUnmount(() => {
        unregisterInsertHandler?.();
        unregisterInsertHandler = null;
    });

    const computedRows = computed<number>(() => {
        const lineCount = text.value.split('\n').length;
        return Math.max(1, Math.min(8, lineCount));
    });

    function syncCursor(): void {
        if (textareaRef.value) {
            cursor.value = textareaRef.value.selectionStart ?? text.value.length;
        }
    }

    function onTextareaInput(): void {
        syncCursor();
    }

    async function onTextareaKeydown(event: KeyboardEvent): Promise<void> {
        syncCursor();
        const textBefore = text.value;
        const wasPopupOpen = completion.open.value;
        await onKeydown(event);
        if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && !wasPopupOpen && text.value === '') {
            emit('submit', textBefore);
        }
    }

    async function onSelectFromPopup(item: CompletionItem): Promise<void> {
        await selectCompletionItem(item);
        textareaRef.value?.focus();
    }

    function focus(): void {
        textareaRef.value?.focus();
    }

    defineExpose({
        focus
    });
</script>

<style lang="scss" scoped>
    .rich-input-container {
        position: relative;
        width: 100%;
    }

    .rich-input-popup-anchor {
        position: absolute;
        left: 8px;
        right: 8px;
        bottom: calc(100% + 6px);
        z-index: 30;
    }

    .rich-input-bar {
        display: flex;
        align-items: flex-start;
        gap: 8px;
        padding: 8px 10px;
        background: var(--mt-rich-input-background, var(--mt-ui-surface-panel, var(--background-25)));
        border-top: 1px solid var(--mt-rich-input-border, var(--mt-ui-border, var(--background-100)));
        color: var(--mt-ui-text-strong, var(--background-950));

        &.is-disabled {
            opacity: 0.6;
        }
    }

    .prompt-chevron {
        padding-top: 2px;
        color: var(--max-primary-400);
        font-family: var(--mt-font-mono);
        font-size: 13px;
        font-weight: 700;
        user-select: none;
    }

    .rich-textarea {
        flex: 1;
        min-height: 22px;
        max-height: 176px;
        padding: 2px 4px;
        background: transparent;
        border: none;
        color: var(--mt-ui-text-strong, var(--background-950));
        font-family: var(--mt-font-mono);
        font-size: 13px;
        line-height: 1.4;
        resize: none;
        outline: none;

        &::placeholder {
            color: var(--mt-ui-text-muted, var(--background-400));
        }
    }

    .rich-input-actions {
        display: flex;
        align-items: center;
        gap: 6px;
    }
</style>
