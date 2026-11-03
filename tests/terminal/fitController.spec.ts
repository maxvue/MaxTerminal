import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Terminal } from '@xterm/xterm';
import type { PtySize } from '@/types/bindings';
import { ptyClient } from '@/terminal/stream/ptyClient';
import { FitController } from '@/terminal/engine/fitController';
import { MockXtermTerminal } from './mocks/xtermMock';

describe('FitController', () => {
    let resizedCalls: PtySize[] = [];

    beforeEach(() => {
        vi.useFakeTimers();
        resizedCalls = [];
        vi.restoreAllMocks();
        vi.spyOn(ptyClient, 'resize').mockImplementation(async (_ptyId, size) => {
            resizedCalls.push(size);
        });
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('aplica debounce (10 resizes rápidos -> 1 pty_resize), ignora 0x0 e deduplica dimensões já confirmadas', async () => {
        const mockTerm = new MockXtermTerminal();
        const host = document.createElement('div');
        Object.defineProperty(host, 'clientWidth', { value: 800, configurable: true });
        Object.defineProperty(host, 'clientHeight', { value: 480, configurable: true });
        document.body.appendChild(host);

        let proposed = { cols: 110, rows: 35 };

        const fit = new FitController({
            ptyId: 'pty-fit',
            xterm: mockTerm as unknown as Terminal,
            fitAddon: {
                proposeDimensions: () => proposed
            },
            debounceMs: 50,
            maxWaitMs: 100
        });

        fit.observe(host);

        // Dispara 10 solicitações rápidas em 30 ms.
        for (let i = 0; i < 10; i++) {
            proposed = { cols: 100 + i, rows: 30 + i };
            void fit.requestFit(false);
            vi.advanceTimersByTime(3);
        }

        expect(resizedCalls).toHaveLength(0);
        vi.advanceTimersByTime(60);
        await Promise.resolve();
        await Promise.resolve();

        expect(resizedCalls).toHaveLength(1);
        expect(resizedCalls[0]).toMatchObject({ cols: 109, rows: 39 });

        // Solicitar novamente com as mesmas dimensões não chama ptyClient.resize.
        await fit.requestFit(true);
        expect(resizedCalls).toHaveLength(1);

        // Container 0x0 é ignorado.
        Object.defineProperty(host, 'clientWidth', { value: 0, configurable: true });
        Object.defineProperty(host, 'clientHeight', { value: 0, configurable: true });
        proposed = { cols: 120, rows: 40 };
        await fit.requestFit(true);
        expect(resizedCalls).toHaveLength(1);

        fit.dispose();
        host.remove();
    });

    it('retenta pty_resize no próximo requestFit se a chamada anterior falhar transitoriamente', async () => {
        const mockTerm = new MockXtermTerminal();
        const host = document.createElement('div');
        Object.defineProperty(host, 'clientWidth', { value: 640, configurable: true });
        Object.defineProperty(host, 'clientHeight', { value: 400, configurable: true });
        document.body.appendChild(host);

        let shouldFail = true;
        vi.spyOn(ptyClient, 'resize').mockImplementation(async (_ptyId, size) => {
            if (shouldFail) {
                shouldFail = false;
                throw { code: 'core.io', message: 'Erro transitório' };
            }
            resizedCalls.push(size);
        });

        const fit = new FitController({
            ptyId: 'pty-retry-fit',
            xterm: mockTerm as unknown as Terminal,
            fitAddon: {
                proposeDimensions: () => ({ cols: 95, rows: 28 })
            }
        });

        fit.observe(host);
        await fit.requestFit(true);
        expect(fit.getConfirmedSize()).toBeNull();

        // Mesmo que xterm.cols/rows já sejam 95x28, o próximo requestFit reenvia porque confirmedSize ainda é null.
        await fit.requestFit(true);
        expect(fit.getConfirmedSize()).toEqual({ cols: 95, rows: 28 });
        expect(resizedCalls).toHaveLength(1);

        fit.dispose();
        host.remove();
    });
});
