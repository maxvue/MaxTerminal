import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
    _resetNativeWindowStateForTests,
    applyNativeWindowTheme,
    isNativeWindowAvailable
} from '@/theme/nativeWindow';

const mockSetTheme = vi.fn().mockResolvedValue(undefined);
const mockGetCurrentWindow = vi.fn(() => ({
    setTheme: mockSetTheme
}));

vi.mock('@tauri-apps/api/window', () => ({
    getCurrentWindow: () => mockGetCurrentWindow()
}));

let mockIsTauriValue = false;
vi.mock('@tauri-apps/api/core', () => ({
    isTauri: () => mockIsTauriValue
}));

describe('nativeWindow theme adapter', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        _resetNativeWindowStateForTests();
        mockIsTauriValue = false;
        delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    });

    it('retorna false e não chama setTheme quando executado em navegador puro', async () => {
        expect(isNativeWindowAvailable()).toBe(false);
        const result = await applyNativeWindowTheme('dark');
        expect(result).toBe(false);
        expect(mockSetTheme).not.toHaveBeenCalled();
    });

    it('aplica tema dark e light quando executado no ambiente Tauri', async () => {
        mockIsTauriValue = true;
        expect(isNativeWindowAvailable()).toBe(true);

        const resDark = await applyNativeWindowTheme('dark');
        expect(resDark).toBe(true);
        expect(mockSetTheme).toHaveBeenCalledWith('dark');

        const resLight = await applyNativeWindowTheme('light');
        expect(resLight).toBe(true);
        expect(mockSetTheme).toHaveBeenCalledWith('light');
        expect(mockSetTheme).toHaveBeenCalledTimes(2);
    });

    it('deduplica chamadas consecutivas para a mesma polaridade', async () => {
        mockIsTauriValue = true;

        await applyNativeWindowTheme('dark');
        expect(mockSetTheme).toHaveBeenCalledTimes(1);

        const resRepeat = await applyNativeWindowTheme('dark');
        expect(resRepeat).toBe(true);
        expect(mockSetTheme).toHaveBeenCalledTimes(1);
    });

    it('reconcilia e serializa chamadas rápidas concorrentes garantindo que a última polaridade vença', async () => {
        mockIsTauriValue = true;

        let resolveFirstCall: (() => void) | undefined;
        mockSetTheme.mockImplementationOnce(
            () =>
                new Promise<void>((resolve) => {
                    resolveFirstCall = resolve;
                })
        );

        const p1 = applyNativeWindowTheme('dark');
        const p2 = applyNativeWindowTheme('light');

        expect(mockSetTheme).toHaveBeenCalledWith('dark');
        expect(mockSetTheme).toHaveBeenCalledTimes(1);

        // Libera a primeira chamada
        if (resolveFirstCall) {
            (resolveFirstCall as () => void)();
        }
        await p1;
        await p2;

        // A segunda chamada foi serializada e executada com light
        expect(mockSetTheme).toHaveBeenCalledWith('light');
        expect(mockSetTheme).toHaveBeenCalledTimes(2);
    });

    it('trata falha de IPC sem quebrar a aplicação e permite retentativa', async () => {
        mockIsTauriValue = true;
        mockSetTheme.mockRejectedValueOnce(new Error('Permission denied'));

        const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
        const result = await applyNativeWindowTheme('dark');

        expect(result).toBe(false);
        expect(warnSpy).toHaveBeenCalled();
        warnSpy.mockRestore();

        // Como falhou, a próxima chamada para dark não é bloqueada pela deduplicação
        mockSetTheme.mockResolvedValueOnce(undefined);
        const retryResult = await applyNativeWindowTheme('dark');
        expect(retryResult).toBe(true);
        expect(mockSetTheme).toHaveBeenCalledTimes(2);
    });
});
