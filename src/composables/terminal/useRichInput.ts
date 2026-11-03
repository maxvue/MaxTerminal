import {
    computed,
    readonly,
    ref,
    unref,
    type ComputedRef,
    type Ref
} from 'vue';
import type { PaneId } from '@/types/bindings';
import type { CompletionItem, TriggerSelectOutcome } from '@/types/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { isInterruptShortcut } from '@/terminal/keybindings';
import { triggerRegistry } from '@/terminal/triggers/triggerRegistry';
import { useCompletion, type UseCompletionReturn } from './useCompletion';

/** Retorno do composable `useRichInput`. */
export interface UseRichInputReturn {
    text: Ref<string>;
    cursor: Ref<number>;
    history: Readonly<Ref<readonly string[]>>;
    historyIndex: Readonly<Ref<number>>;
    completion: UseCompletionReturn;
    activeDescendantId: ComputedRef<string | undefined>;
    listboxId: string;
    onKeydown: (event: KeyboardEvent) => Promise<void>;
    onPaste: (event: ClipboardEvent) => void;
    submit: () => Promise<void>;
    selectCompletionItem: (item: CompletionItem) => Promise<void>;
}

const MAX_HISTORY_ENTRIES = 200;

function isCursorOnFirstLine(text: string, cursor: number): boolean {
    const before = text.slice(0, Math.max(0, cursor));
    return !before.includes('\n');
}

function isCursorOnLastLine(text: string, cursor: number): boolean {
    const after = text.slice(Math.max(0, cursor));
    return !after.includes('\n');
}

/**
 * Gerencia o estado do `RichInput.vue`: texto, cursor, histórico local de 200 comandos,
 * integração com `useCompletion` e envio atômico via `InputPipeline`.
 */
export function useRichInput(paneId: Ref<PaneId> | PaneId): UseRichInputReturn {
    const text = ref<string>('');
    const cursor = ref<number>(0);
    const historyList = ref<string[]>([]);
    const historyIndex = ref<number>(-1);
    const draftBeforeHistory = ref<string>('');

    const listboxId = `mt-completion-listbox-${unref(paneId)}`;
    const completion = useCompletion(paneId, text, cursor);

    const activeDescendantId = computed<string | undefined>(() => {
        if (!completion.open.value || completion.activeIndex.value < 0) {
            return undefined;
        }
        return `${listboxId}-item-${completion.activeIndex.value}`;
    });

    function recordHistory(cmd: string): void {
        const trimmed = cmd.trim();
        if (trimmed.length === 0) {
            return;
        }
        const current = historyList.value;
        if (current[current.length - 1] === cmd) {
            return;
        }
        current.push(cmd);
        if (current.length > MAX_HISTORY_ENTRIES) {
            current.shift();
        }
        historyIndex.value = -1;
        draftBeforeHistory.value = '';
    }

    async function sendLineToPty(lineText: string, isComposing = false): Promise<void> {
        const instance = terminalInstanceRegistry.get(unref(paneId));
        if (instance) {
            await instance.sendLine(lineText, { isComposing });
        }
    }

    async function sendInterruptToPty(): Promise<void> {
        const instance = terminalInstanceRegistry.get(unref(paneId));
        if (instance) {
            await instance.sendInput('\x03');
        }
    }

    async function applyOutcome(outcome: TriggerSelectOutcome): Promise<void> {
        if (outcome.kind === 'replace-input') {
            text.value = outcome.text;
            cursor.value = outcome.cursor ?? outcome.text.length;
            if (!outcome.keepOpen) {
                completion.cancel();
            }
            return;
        }

        if (outcome.kind === 'submit') {
            recordHistory(outcome.text);
            text.value = '';
            cursor.value = 0;
            completion.cancel();
            await sendLineToPty(outcome.text);
            return;
        }

        if (outcome.kind === 'push-stage') {
            const matched = triggerRegistry.match(text.value);
            const prefix = matched?.def.prefix ?? '';
            const nextText = prefix + (outcome.initialQuery ?? '');
            text.value = nextText;
            cursor.value = nextText.length;
            return;
        }

        if (outcome.kind === 'handled') {
            text.value = '';
            cursor.value = 0;
            completion.cancel();
            return;
        }

        if (outcome.kind === 'close') {
            completion.cancel();
        }
    }

    async function selectCompletionItem(item: CompletionItem): Promise<void> {
        const idx = completion.items.value.findIndex((i) => i.id === item.id);
        if (idx !== -1) {
            completion.setActiveIndex(idx);
        }
        const outcome = await completion.confirm();
        if (outcome) {
            await applyOutcome(outcome);
        }
    }

    async function submit(isComposing = false): Promise<void> {
        if (isComposing) {
            return;
        }

        if (completion.open.value) {
            if (completion.activeIndex.value >= 0) {
                const outcome = await completion.confirm();
                if (outcome) {
                    await applyOutcome(outcome);
                    return;
                }
            } else {
                const outcome = await completion.submitWithoutSelection();
                if (outcome) {
                    await applyOutcome(outcome);
                    return;
                }
            }
        }

        const currentText = text.value;
        recordHistory(currentText);
        text.value = '';
        cursor.value = 0;
        completion.cancel();
        await sendLineToPty(currentText, false);
    }

    async function onKeydown(event: KeyboardEvent): Promise<void> {
        if (event.isComposing) {
            return;
        }

        // Ctrl+C com campo vazio (e sem seleção ativa no textarea) envia \x03 ao PTY.
        if (isInterruptShortcut(event) && text.value.length === 0) {
            event.preventDefault();
            await sendInterruptToPty();
            return;
        }

        if (completion.open.value) {
            if (event.key === 'ArrowDown') {
                event.preventDefault();
                completion.move(1);
                return;
            }
            if (event.key === 'ArrowUp') {
                event.preventDefault();
                completion.move(-1);
                return;
            }
            if (event.key === 'Home') {
                event.preventDefault();
                completion.selectFirst();
                return;
            }
            if (event.key === 'End') {
                event.preventDefault();
                completion.selectLast();
                return;
            }
            if (event.key === 'PageDown') {
                event.preventDefault();
                completion.move(5);
                return;
            }
            if (event.key === 'PageUp') {
                event.preventDefault();
                completion.move(-5);
                return;
            }
            if (event.key === 'Escape') {
                event.preventDefault();
                completion.suppress();
                return;
            }
            if (event.key === 'Backspace') {
                if (completion.handleBackspace()) {
                    event.preventDefault();
                    const matched = triggerRegistry.match(text.value);
                    if (matched) {
                        text.value = matched.def.prefix;
                        cursor.value = text.value.length;
                    }
                    return;
                }
            }
            if (event.key === 'Tab') {
                event.preventDefault();
                const outcome = await completion.confirm('secondary');
                if (outcome) {
                    await applyOutcome(outcome);
                }
                return;
            }
            if (event.key === 'ArrowRight' && cursor.value >= text.value.length) {
                const active = completion.activeItem.value;
                if (active && active.kind === 'value') {
                    event.preventDefault();
                    const outcome = await completion.confirm('secondary');
                    if (outcome) {
                        await applyOutcome(outcome);
                    }
                    return;
                }
            }
            if (event.key === 'ArrowLeft' && completion.stage.value > 0) {
                if (completion.popStage()) {
                    event.preventDefault();
                    const matched = triggerRegistry.match(text.value);
                    if (matched) {
                        text.value = matched.def.prefix;
                        cursor.value = text.value.length;
                    }
                    return;
                }
            }
            if (event.key === 'Enter' && !event.shiftKey) {
                event.preventDefault();
                await submit(event.isComposing);
                return;
            }
            return;
        }

        // Navegação no histórico local quando o popup está fechado.
        if (event.key === 'ArrowUp' && isCursorOnFirstLine(text.value, cursor.value)) {
            if (historyList.value.length > 0) {
                event.preventDefault();
                if (historyIndex.value === -1) {
                    draftBeforeHistory.value = text.value;
                    historyIndex.value = historyList.value.length - 1;
                } else if (historyIndex.value > 0) {
                    historyIndex.value -= 1;
                }
                const entry = historyList.value[historyIndex.value] ?? '';
                text.value = entry;
                cursor.value = entry.length;
            }
            return;
        }

        if (event.key === 'ArrowDown' && isCursorOnLastLine(text.value, cursor.value)) {
            if (historyIndex.value !== -1) {
                event.preventDefault();
                if (historyIndex.value < historyList.value.length - 1) {
                    historyIndex.value += 1;
                    const entry = historyList.value[historyIndex.value] ?? '';
                    text.value = entry;
                    cursor.value = entry.length;
                } else {
                    historyIndex.value = -1;
                    text.value = draftBeforeHistory.value;
                    cursor.value = text.value.length;
                }
            }
            return;
        }

        if (event.key === 'Enter' && !event.shiftKey) {
            event.preventDefault();
            await submit(event.isComposing);
        }
    }

    function onPaste(event: ClipboardEvent): void {
        const pasted = event.clipboardData?.getData('text/plain');
        if (typeof pasted === 'string' && pasted.length > 0) {
            event.preventDefault();
            const before = text.value.slice(0, cursor.value);
            const after = text.value.slice(cursor.value);
            text.value = `${before}${pasted}${after}`;
            cursor.value = before.length + pasted.length;
        }
    }

    return {
        text,
        cursor,
        history: readonly(historyList),
        historyIndex: readonly(historyIndex),
        completion,
        activeDescendantId,
        listboxId,
        onKeydown,
        onPaste,
        submit: () => submit(false),
        selectCompletionItem
    };
}
