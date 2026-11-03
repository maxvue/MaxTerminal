import { createPinia, setActivePinia } from 'pinia';
import { effectScope, nextTick } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useLayoutPersistence } from '@/composables/useLayoutPersistence';
import * as dbIpc from '@/ipc/db';
import { useSettingsStore } from '@/stores/settings';

vi.mock('@/ipc/db', () => ({
    getSettings: vi.fn(),
    setSetting: vi.fn(),
    getSettingSnapshot: vi.fn(),
    getStatus: vi.fn(),
    recover: vi.fn(),
    onSettingsChanged: vi.fn().mockResolvedValue(() => {}),
    onDbRecovered: vi.fn().mockResolvedValue(() => {}),
    onMaintenanceFinished: vi.fn().mockResolvedValue(() => {}),
    onWriterLag: vi.fn().mockResolvedValue(() => {})
}));

describe('useLayoutPersistence (Fase 06)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.useFakeTimers();
        vi.clearAllMocks();

        vi.mocked(dbIpc.setSetting).mockImplementation(async (key, value) => ({
            ok: true,
            data: {
                key,
                value,
                revision: 1,
                updatedAt: 1000
            }
        }));
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('coalesceia 10 eventos de resize em 1 gravação (debounce 300ms), aplica limites 15..45 e aceita o formato de splitpanes 4.1.2', async () => {
        const settingsStore = useSettingsStore();
        const setSpy = vi.spyOn(settingsStore, 'set');

        const scope = effectScope();
        const layout = scope.run(() => useLayoutPersistence())!;

        expect(layout.leftSize.value).toBe(22);

        // 10 eventos no formato oficial do splitpanes 4.1.2 ({ panes: [{ min, max, size }, ...] })
        for (let i = 1; i <= 10; i++) {
            const candidateSize = 20 + i; // termina em 30
            layout.onResized({
                panes: [
                    { min: 15, max: 45, size: candidateSize },
                    { min: 0, max: 100, size: 100 - candidateSize }
                ]
            });
        }

        expect(layout.leftSize.value).toBe(30);
        expect(setSpy).not.toHaveBeenCalled();

        await vi.advanceTimersByTimeAsync(300);
        expect(setSpy).toHaveBeenCalledTimes(1);
        expect(setSpy).toHaveBeenCalledWith('layout.columns.left_size', 30);

        // Clamping abaixo de 15 e acima de 45
        layout.onResized({
            panes: [
                { min: 15, max: 45, size: 5 },
                { min: 0, max: 100, size: 95 }
            ]
        });
        expect(layout.leftSize.value).toBe(15);

        layout.onResized({
            panes: [
                { min: 15, max: 45, size: 80 },
                { min: 0, max: 100, size: 20 }
            ]
        });
        expect(layout.leftSize.value).toBe(45);

        await layout.flush();
        expect(setSpy).toHaveBeenLastCalledWith('layout.columns.left_size', 45);

        scope.stop();
    });

    it('ignora payloads inválidos/NaN sem persistir valor default e suporta leitura tardia ou falha sem travar o divisor', async () => {
        const settingsStore = useSettingsStore();
        const setSpy = vi.spyOn(settingsStore, 'set');

        const scope = effectScope();
        const layout = scope.run(() => useLayoutPersistence())!;

        layout.onResized({
            panes: [
                { min: 15, max: 45, size: 28 },
                { min: 0, max: 100, size: 72 }
            ]
        });
        await layout.flush();
        expect(layout.leftSize.value).toBe(28);
        setSpy.mockClear();

        // Payloads inválidos: null, {}, 1 pane, 3 panes, size NaN
        layout.onResized(null);
        layout.onResized({});
        layout.onResized({ panes: [{ size: 25 }] });
        layout.onResized({ panes: [{ size: 25 }, { size: 25 }, { size: 50 }] });
        layout.onResized({ panes: [{ size: Number.NaN }, { size: 70 }] });

        await vi.advanceTimersByTimeAsync(350);
        expect(layout.leftSize.value).toBe(28);
        expect(setSpy).not.toHaveBeenCalled();

        // Leitura tardia da store atualiza leftSize
        settingsStore.values['layout.columns.left_size'] = 36;
        await nextTick();
        expect(layout.leftSize.value).toBe(36);

        // Falha na persistência não trava novas gravações
        setSpy.mockResolvedValueOnce(false);
        layout.onResized({
            panes: [
                { min: 15, max: 45, size: 32 },
                { min: 0, max: 100, size: 68 }
            ]
        });
        await layout.flush();
        expect(layout.leftSize.value).toBe(32);

        scope.stop();
    });
});
