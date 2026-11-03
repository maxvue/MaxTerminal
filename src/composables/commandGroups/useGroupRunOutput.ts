import { getCurrentScope, onScopeDispose, ref, type Ref } from 'vue';
import { commandGroupsIpc } from '@/ipc/commandGroups';
import { isMtIpcError } from '@/ipc/core';
import type {
    GroupOutputFrame,
    GroupRunId,
    GroupRunItemId,
    OutputAttachReceipt,
    OutputSubscriptionId
} from '@/types/commandGroups';

const MAX_REPLAY_BYTES = 256 * 1024;

function trimToMaxBytesUtf8(text: string, maxBytes: number): { text: string; truncated: boolean } {
    const encoder = new TextEncoder();
    const bytes = encoder.encode(text);
    if (bytes.length <= maxBytes) {
        return { text, truncated: false };
    }
    const slice = bytes.slice(bytes.length - maxBytes);
    const decoder = new TextDecoder('utf-8', { fatal: false });
    return {
        text: decoder.decode(slice),
        truncated: true
    };
}

export interface UseGroupRunOutputOptions {
    onFrame?: (frame: GroupOutputFrame) => void;
    onTextChunk?: (chunk: string, frame: GroupOutputFrame) => void;
}

export interface UseGroupRunOutputReturn {
    frames: Ref<GroupOutputFrame[]>;
    outputText: Ref<string>;
    subscriptionId: Ref<OutputSubscriptionId | null>;
    attached: Ref<boolean>;
    attaching: Ref<boolean>;
    ended: Ref<boolean>;
    isTruncated: Ref<boolean>;
    droppedBytesTotal: Ref<number>;
    lastSeq: Ref<number | null>;
    error: Ref<string | null>;
    attach: (
        runId: GroupRunId,
        itemId: GroupRunItemId,
        initialTruncated?: boolean
    ) => Promise<OutputAttachReceipt | null>;
    detach: () => Promise<void>;
}

/**
 * Composable de anexo e desanexo de stream de saída (`Channel<GroupOutputFrame>`) para um item de execução.
 * Remove automaticamente a assinatura no `onScopeDispose` / `onBeforeUnmount` sem encerrar o processo em background (`CA-18`).
 */
export function useGroupRunOutput(
    options: UseGroupRunOutputOptions = {}
): UseGroupRunOutputReturn {
    const frames = ref<GroupOutputFrame[]>([]);
    const outputText = ref<string>('');
    const subscriptionId = ref<OutputSubscriptionId | null>(null);
    const attached = ref<boolean>(false);
    const attaching = ref<boolean>(false);
    const ended = ref<boolean>(false);
    const isTruncated = ref<boolean>(false);
    const droppedBytesTotal = ref<number>(0);
    const lastSeq = ref<number | null>(null);
    const error = ref<string | null>(null);

    let activeGeneration = 0;

    function handleIncomingFrame(frame: GroupOutputFrame, gen: number): void {
        if (gen !== activeGeneration) {
            return;
        }
        frames.value.push(frame);
        lastSeq.value = frame.seq;

        if (frame.kind === 'replay' || frame.kind === 'data') {
            const chunk = frame.text ?? '';
            if (chunk.length > 0) {
                const combined = outputText.value + chunk;
                const trimmed = trimToMaxBytesUtf8(combined, MAX_REPLAY_BYTES);
                outputText.value = trimmed.text;
                if (trimmed.truncated) {
                    isTruncated.value = true;
                }
                options.onTextChunk?.(chunk, frame);
            }
        } else if (frame.kind === 'gap') {
            const dropped = frame.droppedBytes ?? 0;
            droppedBytesTotal.value += dropped;
            isTruncated.value = true;
        } else if (frame.kind === 'end') {
            ended.value = true;
        }

        options.onFrame?.(frame);
    }

    async function detach(): Promise<void> {
        activeGeneration += 1;
        const subId = subscriptionId.value;
        subscriptionId.value = null;
        attached.value = false;
        attaching.value = false;

        if (!subId) {
            return;
        }

        try {
            await commandGroupsIpc.detachItemOutput(subId);
        } catch {
            // Detach é idempotente; ignora se já encerrado no backend.
        }
    }

    async function attach(
        runId: GroupRunId,
        itemId: GroupRunItemId,
        initialTruncated = false
    ): Promise<OutputAttachReceipt | null> {
        await detach();

        const gen = activeGeneration;
        attaching.value = true;
        error.value = null;
        ended.value = false;
        frames.value = [];
        outputText.value = '';
        droppedBytesTotal.value = 0;
        lastSeq.value = null;
        isTruncated.value = initialTruncated;

        try {
            const receipt = await commandGroupsIpc.attachItemOutput(
                runId,
                itemId,
                null,
                (frame) => handleIncomingFrame(frame, gen)
            );

            if (gen !== activeGeneration) {
                await commandGroupsIpc
                    .detachItemOutput(receipt.subscriptionId)
                    .catch(() => {});
                return null;
            }

            subscriptionId.value = receipt.subscriptionId;
            attached.value = true;
            if (receipt.truncated) {
                isTruncated.value = true;
            }
            lastSeq.value = receipt.replayEndSeq;
            return receipt;
        } catch (err: unknown) {
            if (isMtIpcError(err)) {
                error.value = err.message;
            } else if (err instanceof Error) {
                error.value = err.message;
            } else {
                error.value = 'Falha ao anexar saída do item.';
            }
            return null;
        } finally {
            if (gen === activeGeneration) {
                attaching.value = false;
            }
        }
    }

    if (getCurrentScope()) {
        onScopeDispose(() => {
            void detach();
        });
    }

    return {
        frames,
        outputText,
        subscriptionId,
        attached,
        attaching,
        ended,
        isTruncated,
        droppedBytesTotal,
        lastSeq,
        error,
        attach,
        detach
    };
}
