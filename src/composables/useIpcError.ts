import { isMtIpcError } from '@/ipc/core';
import { errorMessages } from '@/types/errors';

export interface UseIpcErrorReturn {
    message: (err: unknown) => string;
    code: (err: unknown) => string | null;
}

/** Traduz erros IPC (`MtIpcError`) em mensagens seguras em pt-BR para exibição na interface. */
export function useIpcError(): UseIpcErrorReturn {
    function code(err: unknown): string | null {
        if (isMtIpcError(err)) {
            return err.code;
        }
        return null;
    }

    function message(err: unknown): string {
        if (isMtIpcError(err)) {
            const mapped = errorMessages[err.code];
            if (mapped) {
                return mapped;
            }
            if (err.message.trim().length > 0) {
                return err.message;
            }
        }
        return 'Erro inesperado ao processar a operação.';
    }

    return {
        message,
        code
    };
}
