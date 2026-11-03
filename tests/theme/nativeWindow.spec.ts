import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
    _resetNativeWindowStateForTests,
    applyNativeWindowTheme,
    isNativeWindowAvailable,
    resolveNativeWindowThemeTarget
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

describe('nativeWindow theme adapter & policy', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        _resetNativeWindowStateForTests();
        mockIsTauriValue = false;
        delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    });

    describe('resolveNativeWindowThemeTarget policy', () => {
        it('retorna null (sistema) no caminho padrão para manter cores do SO', () => {
            expect(resolveNativeWindowThemeTarget({ contentPolarity: 'dark' })).toBeNull();
            expect(resolveNativeWindowThemeTarget({ contentPolarity: 'light' })).toBeNull();
            expect(
                resolveNativeWindowThemeTarget({
                    contentPolarity: 'dark',
                    nativeIntegrationAvailable: true
                })
            ).toBeNull();
        });

        it('aplica fallback escuro ("dark") quando integração é indisponível e conteúdo for escuro', () => {
            expect(
                resolveNativeWindowThemeTarget({
                    contentPolarity: 'dark',
                    nativeIntegrationAvailable: false
                })
            ).toBe('dark');
        });

        it('retorna null (sistema) ao sair do fallback escuro para conteúdo claro', () => {
            expect(
                resolveNativeWindowThemeTarget({
                    contentPolarity: 'light',
                    nativeIntegrationAvailable: false
                })
            ).toBeNull();
        });
    });

    describe('applyNativeWindowTheme', () => {
        it('retorna false e não chama setTheme quando executado em navegador puro', async () => {
            expect(isNativeWindowAvailable()).toBe(false);
            const result = await applyNativeWindowTheme('dark');
            expect(result).toBe(false);
            expect(mockSetTheme).not.toHaveBeenCalled();
        });

        it('aplica tema null (sistema) quando executado no ambiente Tauri', async () => {
            mockIsTauriValue = true;
            expect(isNativeWindowAvailable()).toBe(true);

            const resSystem = await applyNativeWindowTheme(null);
            expect(resSystem).toBe(true);
            expect(mockSetTheme).toHaveBeenCalledWith(null);
            expect(mockSetTheme).toHaveBeenCalledTimes(1);
        });

        it('aplica tema dark e light quando solicitado', async () => {
            mockIsTauriValue = true;

            const resDark = await applyNativeWindowTheme('dark');
            expect(resDark).toBe(true);
            expect(mockSetTheme).toHaveBeenCalledWith('dark');

            const resLight = await applyNativeWindowTheme('light');
            expect(resLight).toBe(true);
            expect(mockSetTheme).toHaveBeenCalledWith('light');
            expect(mockSetTheme).toHaveBeenCalledTimes(2);
        });

        it('deduplica chamadas consecutivas para o mesmo alvo (inclusive null)', async () => {
            mockIsTauriValue = true;

            await applyNativeWindowTheme(null);
            expect(mockSetTheme).toHaveBeenCalledTimes(1);

            const resRepeatNull = await applyNativeWindowTheme(null);
            expect(resRepeatNull).toBe(true);
            expect(mockSetTheme).toHaveBeenCalledTimes(1);

            await applyNativeWindowTheme('dark');
            expect(mockSetTheme).toHaveBeenCalledTimes(2);

            const resRepeatDark = await applyNativeWindowTheme('dark');
            expect(resRepeatDark).toBe(true);
            expect(mockSetTheme).toHaveBeenCalledTimes(2);
        });

        it('reconcilia e serializa concorrência de alvos garantindo que o último vença', async () => {
            mockIsTauriValue = true;

            let resolveFirstCall: (() => void) | undefined;
            mockSetTheme.mockImplementationOnce(
                () =>
                    new Promise<void>((resolve) => {
                        resolveFirstCall = resolve;
                    })
            );

            const p1 = applyNativeWindowTheme('dark');
            const p2 = applyNativeWindowTheme(null);

            expect(mockSetTheme).toHaveBeenCalledWith('dark');
            expect(mockSetTheme).toHaveBeenCalledTimes(1);

            if (resolveFirstCall) {
                (resolveFirstCall as () => void)();
            }
            await p1;
            await p2;

            expect(mockSetTheme).toHaveBeenCalledWith(null);
            expect(mockSetTheme).toHaveBeenCalledTimes(2);
        });

        it('trata alternância rápida dark -> null -> dark sob concorrência', async () => {
            mockIsTauriValue = true;

            let resolveFirstCall: (() => void) | undefined;
            mockSetTheme.mockImplementationOnce(
                () =>
                    new Promise<void>((resolve) => {
                        resolveFirstCall = resolve;
                    })
            );

            const p1 = applyNativeWindowTheme('dark');
            const p2 = applyNativeWindowTheme(null);
            const p3 = applyNativeWindowTheme('dark');

            if (resolveFirstCall) {
                (resolveFirstCall as () => void)();
            }
            await Promise.all([p1, p2, p3]);

            // Como p3 substituiu p2 enquanto p1 estava em voo, o alvo final aplicado é 'dark'
            expect(mockSetTheme).toHaveBeenLastCalledWith('dark');
        });

        it('trata falha de IPC sem quebrar a aplicação e permite retentativa', async () => {
            mockIsTauriValue = true;
            mockSetTheme.mockRejectedValueOnce(new Error('Permission denied'));

            const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
            const result = await applyNativeWindowTheme(null);

            expect(result).toBe(false);
            expect(warnSpy).toHaveBeenCalled();
            warnSpy.mockRestore();

            // Como falhou, a próxima chamada para null não é bloqueada pela deduplicação
            mockSetTheme.mockResolvedValueOnce(undefined);
            const retryResult = await applyNativeWindowTheme(null);
            expect(retryResult).toBe(true);
            expect(mockSetTheme).toHaveBeenCalledTimes(2);
        });
    });
});
