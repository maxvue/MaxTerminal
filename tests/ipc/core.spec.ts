import { describe, expect, it } from 'vitest';
import { useIpcError } from '@/composables/useIpcError';
import { call, isMtIpcError, MtIpcError, unwrap, type IpcResult } from '@/ipc/core';
import type { MtError } from '@/types/bindings';
import { IPC_TRANSPORT_ERROR, IPC_TRANSPORT_MESSAGE } from '@/types/errors';

describe('Camada IPC - core e useIpcError', () => {
    describe('call e unwrap', () => {
        it('resolve data com sucesso quando o envelope retorna status ok', async () => {
            const expectedData = { name: 'MaxTerminal', version: '0.1.0' };
            const request = async (): Promise<IpcResult<typeof expectedData>> => ({
                status: 'ok',
                data: expectedData
            });

            const result = await call('test_command', request);
            expect(result).toEqual(expectedData);

            const unwrapResult = await unwrap(request(), 'test_command');
            expect(unwrapResult).toEqual(expectedData);
        });

        it('lança MtIpcError preservando code, message, details e command em erro de domínio', async () => {
            const domainError: MtError = {
                code: 'core.not_found',
                message: 'O recurso solicitado não foi encontrado.',
                details: { path: '/tmp/missing' }
            };

            const request = async (): Promise<IpcResult<string>> => ({
                status: 'error',
                error: domainError
            });

            await expect(call('test_domain_error', request)).rejects.toThrowError(MtIpcError);

            try {
                await call('test_domain_error', request);
            } catch (err: unknown) {
                expect(isMtIpcError(err)).toBe(true);
                if (isMtIpcError(err)) {
                    expect(err.code).toBe('core.not_found');
                    expect(err.message).toBe('O recurso solicitado não foi encontrado.');
                    expect(err.details).toEqual({ path: '/tmp/missing' });
                    expect(err.command).toBe('test_domain_error');
                    expect(err.name).toBe('MtIpcError');
                }
            }

            try {
                await unwrap(request(), 'test_domain_unwrap');
            } catch (err: unknown) {
                expect(isMtIpcError(err)).toBe(true);
                if (isMtIpcError(err)) {
                    expect(err.command).toBe('test_domain_unwrap');
                }
            }
        });

        it('re-lança a mesma instância de MtIpcError se a função de request já lançar um MtIpcError', async () => {
            const directError = new MtIpcError(
                { code: 'core.conflict', message: 'Conflito de estado' },
                'original_cmd'
            );

            const request = async (): Promise<IpcResult<string>> => {
                throw directError;
            };

            await expect(call('wrapped_cmd', request)).rejects.toBe(directError);
        });

        it('converte MtError lançado diretamente na promise em MtIpcError com o comando informado', async () => {
            const rawErrorPayload: MtError = {
                code: 'core.permission_denied',
                message: 'Permissão negada'
            };

            const request = async (): Promise<IpcResult<string>> => {
                throw rawErrorPayload;
            };

            try {
                await call('perm_cmd', request);
                expect.unreachable('Deveria ter lançado erro');
            } catch (err: unknown) {
                expect(isMtIpcError(err)).toBe(true);
                if (isMtIpcError(err)) {
                    expect(err.code).toBe('core.permission_denied');
                    expect(err.command).toBe('perm_cmd');
                }
            }
        });

        it('normaliza exceção nativa ou rejeição de transporte para ipc.transport sem vazar mensagem crua', async () => {
            const secretLeak = 'FATAL_INTERNAL_CRASH_WITH_SECRET_TOKEN_12345';
            const requestRejection = async (): Promise<IpcResult<string>> => {
                throw new Error(secretLeak);
            };

            try {
                await call('app_info', requestRejection);
                expect.unreachable('Deveria ter lançado erro');
            } catch (err: unknown) {
                expect(isMtIpcError(err)).toBe(true);
                if (isMtIpcError(err)) {
                    expect(err.code).toBe(IPC_TRANSPORT_ERROR);
                    expect(err.message).toBe(IPC_TRANSPORT_MESSAGE);
                    expect(err.message).not.toContain(secretLeak);
                    expect(err.command).toBe('app_info');
                }
            }
        });

        it('normaliza exceção síncrona lançada ao invocar a função de request', async () => {
            const requestSyncThrow = (): Promise<IpcResult<string>> => {
                throw new TypeError('Falha de tipo síncrona');
            };

            try {
                await call('sync_cmd', requestSyncThrow);
                expect.unreachable('Deveria ter lançado erro');
            } catch (err: unknown) {
                expect(isMtIpcError(err)).toBe(true);
                if (isMtIpcError(err)) {
                    expect(err.code).toBe(IPC_TRANSPORT_ERROR);
                    expect(err.message).toBe(IPC_TRANSPORT_MESSAGE);
                }
            }
        });

        it('normaliza payloads malformados (null, primitivos, status desconhecido, objeto vazio)', async () => {
            const malformedPayloads = [
                null,
                undefined,
                123,
                'string',
                {},
                { status: 'unknown' },
                { status: 'ok' }, // ausência do campo data
                { status: 'error' }, // ausência de error payload
                { status: 'error', error: { code: '', message: '' } }, // payload com campos vazios
                { status: 'error', error: null }
            ];

            for (const payload of malformedPayloads) {
                const request = async (): Promise<IpcResult<unknown>> => payload as unknown as IpcResult<unknown>;
                try {
                    await call('malformed_cmd', request);
                    expect.unreachable(`Payload deveria falhar: ${JSON.stringify(payload)}`);
                } catch (err: unknown) {
                    expect(isMtIpcError(err)).toBe(true);
                    if (isMtIpcError(err)) {
                        expect(err.code).toBe(IPC_TRANSPORT_ERROR);
                    }
                }
            }
        });
    });

    describe('useIpcError', () => {
        const ipcError = useIpcError();

        it('retorna mensagem mapeada em pt-BR pelo código de erro', () => {
            const err = new MtIpcError(
                { code: 'core.not_found', message: 'Não encontrado' },
                'test'
            );
            expect(ipcError.code(err)).toBe('core.not_found');
            expect(ipcError.message(err)).toBe('O recurso solicitado não foi encontrado.');
        });

        it('retorna fallback da mensagem do próprio MtIpcError quando o código não está no dicionário', () => {
            const customMsg = 'Mensagem de domínio específica não catalogada.';
            const err = new MtIpcError(
                { code: 'custom.code', message: customMsg },
                'test'
            );
            expect(ipcError.code(err)).toBe('custom.code');
            expect(ipcError.message(err)).toBe(customMsg);
        });

        it('retorna mensagem genérica para erros desconhecidos e null para código', () => {
            const nativeError = new Error('Erro nativo JS');
            expect(ipcError.code(nativeError)).toBeNull();
            expect(ipcError.message(nativeError)).toBe('Erro inesperado ao processar a operação.');

            expect(ipcError.code(null)).toBeNull();
            expect(ipcError.message(null)).toBe('Erro inesperado ao processar a operação.');
        });

        it('retorna a mensagem de transporte quando o código for ipc.transport', () => {
            const transportErr = new MtIpcError(
                { code: IPC_TRANSPORT_ERROR, message: 'qualquer coisa' },
                'test'
            );
            expect(ipcError.code(transportErr)).toBe(IPC_TRANSPORT_ERROR);
            expect(ipcError.message(transportErr)).toBe(IPC_TRANSPORT_MESSAGE);
        });
    });
});
