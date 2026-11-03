import { ref, type Ref } from 'vue';
import { createAiRequestId } from '@/composables/ai/adapters';
import { useIpcError } from '@/composables/useIpcError';
import { aiIpc } from '@/ipc/ai';
import type {
    AiRequestId,
    AiSelection_Deserialize,
    PaneId,
    ProjectId,
    Value
} from '@/types/bindings';

export interface UseAiGenerateOptions<T> {
    /** Identificador versionado do schema (ex.: `"artisan.command.v1"`, `"command-group.suggestion.v1"`). */
    schemaId: string;
    /** JSON Schema Draft 2020-12 que valida a resposta no backend Rust. */
    schema: Record<string, unknown>;
    /** Decodificador/transformador opcional no cliente após a validação do JSON Schema. */
    decode?: (raw: unknown) => T;
    /** Instrução de sistema adicional opcional. */
    system?: string;
}

export interface AiGenerateCallOptions {
    selection?: AiSelection_Deserialize | null;
    paneId?: PaneId | null;
    projectId?: ProjectId | null;
    system?: string | null;
}

export interface UseAiGenerateReturn<T> {
    loading: Ref<boolean>;
    error: Ref<string | null>;
    data: Ref<T | null>;
    repaired: Ref<boolean>;
    latencyMs: Ref<number | null>;
    requestId: Ref<AiRequestId | null>;
    generate: (prompt: string, callOpts?: AiGenerateCallOptions) => Promise<T>;
    cancel: () => Promise<boolean>;
    reset: () => void;
}

/**
 * Composable de geração assistida estruturada (JSON Schema) consumido pelos modais
 * de Comandos Artisan (Fase 14 — `C-05`) e Grupos de Comandos (Fase 15 — `C-06`).
 */
export function useAiGenerate<T = Record<string, unknown>>(
    options: UseAiGenerateOptions<T>
): UseAiGenerateReturn<T> {
    const ipcError = useIpcError();

    const loading = ref<boolean>(false);
    const error = ref<string | null>(null);
    const data = ref<T | null>(null) as Ref<T | null>;
    const repaired = ref<boolean>(false);
    const latencyMs = ref<number | null>(null);
    const requestId = ref<AiRequestId | null>(null);

    let activeSeq = 0;

    async function generate(prompt: string, callOpts: AiGenerateCallOptions = {}): Promise<T> {
        const trimmed = prompt.trim();
        if (!trimmed) {
            const msg = 'Informe uma descrição ou objetivo para gerar com IA.';
            error.value = msg;
            throw new Error(msg);
        }

        // Se houver uma requisição anterior ativa, cancela antes de iniciar a nova
        if (loading.value && requestId.value) {
            const prevId = requestId.value;
            void aiIpc.cancel(prevId).catch(() => {});
        }

        const seq = ++activeSeq;
        const reqId = createAiRequestId('ai-struct');
        requestId.value = reqId;
        loading.value = true;
        error.value = null;

        try {
            const response = await aiIpc.generateStructured({
                requestId: reqId,
                schemaId: options.schemaId,
                schema: options.schema as Value,
                prompt: trimmed,
                system: callOpts.system ?? options.system ?? null,
                selection: callOpts.selection ?? null,
                paneId: callOpts.paneId ?? null,
                projectId: callOpts.projectId ?? null
            });

            if (seq !== activeSeq) {
                throw new Error('Geração substituída por uma solicitação mais recente.');
            }

            const decoded = options.decode
                ? options.decode(response.value)
                : (response.value as unknown as T);

            data.value = decoded;
            repaired.value = response.repaired;
            latencyMs.value = response.latencyMs;
            return decoded;
        } catch (err: unknown) {
            if (seq === activeSeq) {
                error.value = ipcError.message(err);
            }
            throw err;
        } finally {
            if (seq === activeSeq) {
                loading.value = false;
            }
        }
    }

    async function cancel(): Promise<boolean> {
        const currentId = requestId.value;
        activeSeq += 1;
        loading.value = false;
        if (!currentId) {
            return false;
        }
        try {
            return await aiIpc.cancel(currentId);
        } catch {
            return false;
        }
    }

    function reset(): void {
        activeSeq += 1;
        loading.value = false;
        error.value = null;
        data.value = null;
        repaired.value = false;
        latencyMs.value = null;
        requestId.value = null;
    }

    return {
        loading,
        error,
        data,
        repaired,
        latencyMs,
        requestId,
        generate,
        cancel,
        reset
    };
}
