import { mockIPC } from '@tauri-apps/api/mocks';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { logger } from '@/app/logger';
import { useAppStore, type BootstrapStep } from '@/stores/app';
import type { AppInfo } from '@/types/bindings';

const mockAppInfo: AppInfo = {
    name: 'MaxTerminal',
    version: '0.1.0',
    buildProfile: 'debug',
    tauriVersion: '2.0.0',
    os: 'linux',
    arch: 'x86_64',
    paths: {
        configDir: '/home/user/.config/maxterminal',
        dataDir: '/home/user/.local/share/maxterminal',
        stateDir: '/home/user/.local/state/maxterminal',
        cacheDir: '/home/user/.cache/maxterminal',
        logDir: '/home/user/.local/state/maxterminal/logs',
        dbPath: '/home/user/.local/share/maxterminal/maxterminal.db'
    }
};

describe('useAppStore', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('inicializa em estado idle com info nula e isReady falso', () => {
        const store = useAppStore();
        expect(store.status).toBe('idle');
        expect(store.info).toBeNull();
        expect(store.error).toBeNull();
        expect(store.isReady).toBe(false);
        expect(store.versionLabel).toBe('');
        expect(store.isDebug).toBe(false);
    });

    it('bootstrap com mockIPC preenche info, status ready, isReady, versionLabel e isDebug', async () => {
        let ipcCalls = 0;
        mockIPC((cmd) => {
            if (cmd === 'app_info') {
                ipcCalls++;
                return mockAppInfo;
            }
        });

        const store = useAppStore();
        await store.bootstrap();

        expect(ipcCalls).toBe(1);
        expect(store.status).toBe('ready');
        expect(store.isReady).toBe(true);
        expect(store.info).toEqual(mockAppInfo);
        expect(store.versionLabel).toBe('v0.1.0');
        expect(store.isDebug).toBe(true);
        expect(store.error).toBeNull();
    });

    it('chamadas concorrentes a bootstrap durante loading compartilham a mesma promessa em voo e disparam exatamente 1 IPC', async () => {
        let ipcCalls = 0;
        mockIPC(async (cmd) => {
            if (cmd === 'app_info') {
                ipcCalls++;
                // Pequena latência simulada para assegurar concorrência real
                await new Promise((resolve) => setTimeout(resolve, 20));
                return mockAppInfo;
            }
        });

        const store = useAppStore();
        const p1 = store.bootstrap();
        const p2 = store.bootstrap();
        const p3 = store.bootstrap();

        await Promise.all([p1, p2, p3]);

        expect(ipcCalls).toBe(1);
        expect(store.status).toBe('ready');
    });

    it('bootstrap é no-op quando o status já for ready', async () => {
        let ipcCalls = 0;
        mockIPC((cmd) => {
            if (cmd === 'app_info') {
                ipcCalls++;
                return mockAppInfo;
            }
        });

        const store = useAppStore();
        await store.bootstrap();
        expect(ipcCalls).toBe(1);

        await store.bootstrap();
        expect(ipcCalls).toBe(1);
    });

    it('executa os passos de bootstrap registrados na ordem estrita antes de transitar para ready', async () => {
        mockIPC((cmd) => {
            if (cmd === 'app_info') {
                return mockAppInfo;
            }
        });

        const store = useAppStore();
        const executionOrder: string[] = [];

        const step1: BootstrapStep = {
            name: 'database_init',
            run: async () => {
                executionOrder.push('step1');
            }
        };

        const step2: BootstrapStep = {
            name: 'pty_runtime',
            run: async () => {
                executionOrder.push('step2');
            }
        };

        store.registerBootstrapStep(step1);
        store.registerBootstrapStep(step2);

        await store.bootstrap();

        expect(executionOrder).toEqual(['step1', 'step2']);
        expect(store.status).toBe('ready');
    });

    it('rejeita passo de bootstrap com nome vazio, duplicado ou registrado após o início do bootstrap', async () => {
        mockIPC((cmd) => {
            if (cmd === 'app_info') {
                return mockAppInfo;
            }
        });

        const store = useAppStore();

        expect(() => {
            store.registerBootstrapStep({ name: '   ', run: async () => {} });
        }).toThrowError('O nome do passo de bootstrap não pode ser vazio.');

        store.registerBootstrapStep({ name: 'step_a', run: async () => {} });

        expect(() => {
            store.registerBootstrapStep({ name: 'step_a', run: async () => {} });
        }).toThrowError("Passo de bootstrap duplicado: 'step_a'.");

        await store.bootstrap();

        expect(() => {
            store.registerBootstrapStep({ name: 'step_late', run: async () => {} });
        }).toThrowError("Não é permitido registrar o passo de bootstrap 'step_late' após o início do bootstrap.");
    });

    it('em falha de app_info, transita para error, reporta bootstrap_failed e permite retry bem-sucedido', async () => {
        const loggerSpy = vi.spyOn(logger, 'report');
        let shouldFail = true;

        mockIPC((cmd) => {
            if (cmd === 'app_info') {
                if (shouldFail) {
                    throw new Error('Falha de conexão com processo nativo');
                }
                return mockAppInfo;
            }
        });

        const store = useAppStore();
        await store.bootstrap();

        expect(store.status).toBe('error');
        expect(store.isReady).toBe(false);
        expect(store.error).not.toBeNull();
        expect(loggerSpy).toHaveBeenCalledWith('bootstrap_failed');

        // Testa recuperação via retry
        shouldFail = false;
        await store.retry();

        expect(store.status).toBe('ready');
        expect(store.isReady).toBe(true);
        expect(store.error).toBeNull();
        expect(store.info).toEqual(mockAppInfo);

        loggerSpy.mockRestore();
    });
});
