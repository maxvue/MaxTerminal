import type { App } from 'vue';
import { logFrontend } from '@/ipc/app';
import type { FrontendLogEvent } from '@/types/bindings';

/** Limite máximo de eventos pendentes na fila em memória do logger do frontend. */
export const MAX_FRONTEND_LOG_QUEUE = 64;

const VALID_EVENTS = new Set<FrontendLogEvent>([
    'vue_error',
    'window_error',
    'unhandled_rejection',
    'bootstrap_failed'
]);

export interface FrontendLogger {
    report: (event: FrontendLogEvent) => void;
}

/** Cria uma instância isolada de `FrontendLogger` (útil para testes determinísticos e para o singleton). */
export function createFrontendLogger(
    sender: (event: FrontendLogEvent) => Promise<void> = logFrontend
): FrontendLogger & { flushForTests: () => Promise<void> } {
    const queue: FrontendLogEvent[] = [];
    const microtaskSeen = new Set<FrontendLogEvent>();
    let microtaskScheduled = false;
    let inFlight: Promise<void> | null = null;

    function scheduleDrain(): void {
        if (inFlight !== null || queue.length === 0) {
            return;
        }
        const nextEvent = queue.shift();
        if (!nextEvent) {
            return;
        }

        inFlight = sender(nextEvent)
            .catch(() => {
                // Silencia falha de envio de log para impedir recursão infinita de IPC.
            })
            .finally(() => {
                inFlight = null;
                if (queue.length > 0) {
                    scheduleDrain();
                }
            });
    }

    function report(event: FrontendLogEvent): void {
        if (!VALID_EVENTS.has(event)) {
            return;
        }
        if (microtaskSeen.has(event)) {
            return;
        }
        if (queue.length >= MAX_FRONTEND_LOG_QUEUE) {
            return;
        }

        microtaskSeen.add(event);
        if (!microtaskScheduled) {
            microtaskScheduled = true;
            queueMicrotask(() => {
                microtaskSeen.clear();
                microtaskScheduled = false;
            });
        }

        queue.push(event);
        scheduleDrain();
    }

    async function flushForTests(): Promise<void> {
        while (inFlight !== null || queue.length > 0) {
            if (inFlight !== null) {
                await inFlight;
            } else {
                scheduleDrain();
            }
        }
    }

    return {
        report,
        flushForTests
    };
}

/** Instância padrão do logger do frontend. */
export const logger: FrontendLogger = createFrontendLogger();

/**
 * Instala os handlers globais (`app.config.errorHandler`, `window.error`, `unhandledrejection`)
 * descartando mensagens livres e stacks, e retorna uma função de limpeza idempotente.
 */
export function installGlobalErrorHandlers(
    app: App,
    targetLogger: FrontendLogger = logger
): () => void {
    const previousVueHandler = app.config.errorHandler;

    const vueErrorHandler: NonNullable<typeof app.config.errorHandler> = (
        _err,
        _instance,
        _info
    ) => {
        targetLogger.report('vue_error');
    };

    const onWindowError = (_event: Event): void => {
        targetLogger.report('window_error');
    };

    const onUnhandledRejection = (_event: PromiseRejectionEvent): void => {
        targetLogger.report('unhandled_rejection');
    };

    app.config.errorHandler = vueErrorHandler;
    window.addEventListener('error', onWindowError);
    window.addEventListener('unhandledrejection', onUnhandledRejection);

    let disposed = false;
    return () => {
        if (disposed) {
            return;
        }
        disposed = true;
        if (app.config.errorHandler === vueErrorHandler) {
            if (previousVueHandler) {
                app.config.errorHandler = previousVueHandler;
            } else {
                delete app.config.errorHandler;
            }
        }
        window.removeEventListener('error', onWindowError);
        window.removeEventListener('unhandledrejection', onUnhandledRejection);
    };
}
