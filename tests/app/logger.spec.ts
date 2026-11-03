import { createApp } from 'vue';
import { describe, expect, it, vi } from 'vitest';
import {
    createFrontendLogger,
    installGlobalErrorHandlers,
    MAX_FRONTEND_LOG_QUEUE
} from '@/app/logger';
import type { FrontendLogEvent } from '@/types/bindings';

describe('Logger do Frontend - createFrontendLogger e installGlobalErrorHandlers', () => {
    describe('createFrontendLogger', () => {
        it('deduplica eventos idênticos emitidos sincronamente na mesma microtask', async () => {
            const sentEvents: FrontendLogEvent[] = [];
            const sender = vi.fn(async (event: FrontendLogEvent) => {
                sentEvents.push(event);
            });

            const loggerInstance = createFrontendLogger(sender);

            // Dispara 3 vezes o mesmo evento sincronamente
            loggerInstance.report('vue_error');
            loggerInstance.report('vue_error');
            loggerInstance.report('vue_error');

            await loggerInstance.flushForTests();

            expect(sentEvents).toEqual(['vue_error']);
            expect(sender).toHaveBeenCalledTimes(1);

            // Em uma microtask seguinte, o mesmo evento pode ser aceito novamente
            loggerInstance.report('vue_error');
            await loggerInstance.flushForTests();

            expect(sentEvents).toEqual(['vue_error', 'vue_error']);
            expect(sender).toHaveBeenCalledTimes(2);
        });

        it('ignora variantes de evento não catalogadas no conjunto fechado', async () => {
            const sender = vi.fn(async () => {});
            const loggerInstance = createFrontendLogger(sender);

            // Injeção de string inválida com coerção de tipo forçada
            loggerInstance.report('evento_desconhecido' as FrontendLogEvent);
            await loggerInstance.flushForTests();

            expect(sender).not.toHaveBeenCalled();
        });

        it('mantém no máximo 1 envio assíncrono em voo por vez, drenando a fila sequencialmente', async () => {
            let activeInFlight = 0;
            let maxConcurrent = 0;
            const executionTrail: FrontendLogEvent[] = [];

            const sender = vi.fn(async (event: FrontendLogEvent) => {
                activeInFlight++;
                if (activeInFlight > maxConcurrent) {
                    maxConcurrent = activeInFlight;
                }
                // Latência simulada para forçar enfileiramento
                await new Promise((resolve) => setTimeout(resolve, 10));
                executionTrail.push(event);
                activeInFlight--;
            });

            const loggerInstance = createFrontendLogger(sender);

            // Envia eventos distintos na mesma microtask para não serem deduplicados
            loggerInstance.report('vue_error');
            loggerInstance.report('window_error');
            loggerInstance.report('unhandled_rejection');
            loggerInstance.report('bootstrap_failed');

            await loggerInstance.flushForTests();

            expect(maxConcurrent).toBe(1);
            expect(executionTrail).toEqual([
                'vue_error',
                'window_error',
                'unhandled_rejection',
                'bootstrap_failed'
            ]);
        });

        it('silencia falhas de envio de log sem propagar exceção não tratada e continua drenando a fila', async () => {
            let attempts = 0;
            const sender = vi.fn(async (_event: FrontendLogEvent) => {
                attempts++;
                if (attempts === 1) {
                    throw new Error('Falha de IPC simulada');
                }
            });

            const loggerInstance = createFrontendLogger(sender);

            loggerInstance.report('vue_error');
            loggerInstance.report('window_error');

            // flushForTests não deve explodir
            await expect(loggerInstance.flushForTests()).resolves.toBeUndefined();
            expect(sender).toHaveBeenCalledTimes(2);
        });

        it('limita a fila em memória a no máximo 64 itens descartando excessos silenciosamente', async () => {
            let resolveBlocked: () => void = () => {};
            const blockedPromise = new Promise<void>((resolve) => {
                resolveBlocked = resolve;
            });

            const sentEvents: FrontendLogEvent[] = [];
            const sender = vi.fn(async (event: FrontendLogEvent) => {
                sentEvents.push(event);
                // Bloqueia a drenagem no primeiro evento
                await blockedPromise;
            });

            const loggerInstance = createFrontendLogger(sender);

            // Dispara o primeiro evento para prender o inFlight
            loggerInstance.report('vue_error');

            // Aguarda microtask para liberar deduplicação
            await new Promise<void>((resolve) => queueMicrotask(() => resolve()));

            // Dispara MAX_FRONTEND_LOG_QUEUE + 10 eventos em microtasks distintas
            for (let i = 0; i < MAX_FRONTEND_LOG_QUEUE + 10; i++) {
                // Alterna entre eventos permitidos
                const event: FrontendLogEvent = i % 2 === 0 ? 'window_error' : 'unhandled_rejection';
                loggerInstance.report(event);
                await new Promise<void>((resolve) => queueMicrotask(() => resolve()));
            }

            // Desbloqueia o sender e drena
            resolveBlocked();
            await loggerInstance.flushForTests();

            // Total de chamadas ao sender deve ser 1 (em voo) + máximo 64 (fila) = 65
            expect(sentEvents.length).toBeLessThanOrEqual(MAX_FRONTEND_LOG_QUEUE + 1);
        });
    });

    describe('installGlobalErrorHandlers', () => {
        it('captura erros de Vue, window.error e unhandledrejection enviando apenas a variante canônica sem vazar sentinelas', () => {
            const reportedEvents: FrontendLogEvent[] = [];
            const mockLogger = {
                report: vi.fn((event: FrontendLogEvent) => {
                    reportedEvents.push(event);
                })
            };

            const app = createApp({});
            const uninstall = installGlobalErrorHandlers(app, mockLogger);

            const secretToken = 'BEARER_SECRET_TOKEN_SUPER_CONFIDENCIAL_12345';

            // 1. Erro Vue
            expect(app.config.errorHandler).toBeDefined();
            app.config.errorHandler?.(new Error(`Erro com segredo: ${secretToken}`), null, 'render');
            expect(reportedEvents).toContain('vue_error');

            // 2. Erro de window
            const windowErrorEvent = new Event('error');
            window.dispatchEvent(windowErrorEvent);
            expect(reportedEvents).toContain('window_error');

            // 3. Unhandled rejection
            const unhandledRejectionEvent = new Event('unhandledrejection');
            window.dispatchEvent(unhandledRejectionEvent);
            expect(reportedEvents).toContain('unhandled_rejection');

            // Valida que nenhum argumento enviado ao logger continha o token secreto
            for (const callArgs of mockLogger.report.mock.calls) {
                expect(callArgs[0]).not.toContain(secretToken);
                expect([
                    'vue_error',
                    'window_error',
                    'unhandled_rejection',
                    'bootstrap_failed'
                ]).toContain(callArgs[0]);
            }

            // 4. Limpeza idempotente
            uninstall();
            uninstall(); // chamada duplicada segura

            mockLogger.report.mockClear();

            // Dispara eventos novamente após cleanup; nenhum deve ser reportado
            window.dispatchEvent(new Event('error'));
            window.dispatchEvent(new Event('unhandledrejection'));
            expect(mockLogger.report).not.toHaveBeenCalled();
        });
    });
});
