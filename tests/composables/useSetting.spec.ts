import { createPinia, setActivePinia } from 'pinia';
import { effectScope } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useSetting } from '@/composables/useSetting';
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

describe('composables/useSetting', () => {
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

    it('coalesceia múltiplas atribuições dentro da janela de debounce em uma única chamada IPC', async () => {
        const store = useSettingsStore();
        const setSpy = vi.spyOn(store, 'set');

        const scope = effectScope();
        const setting = scope.run(() => useSetting('terminal.fontSize', { debounceMs: 150 }))!;

        expect(setting.value.value).toBe(13);

        setting.value.value = 14;
        setting.value.value = 15;
        setting.value.value = 16;

        expect(setSpy).not.toHaveBeenCalled();

        await vi.advanceTimersByTimeAsync(150);

        expect(setSpy).toHaveBeenCalledTimes(1);
        expect(setSpy).toHaveBeenCalledWith('terminal.fontSize', 16);
        expect(setting.value.value).toBe(16);

        scope.stop();
    });

    it('permite flush() imediato antes de expirar o timer de debounce', async () => {
        const store = useSettingsStore();
        const setSpy = vi.spyOn(store, 'set');

        const scope = effectScope();
        const setting = scope.run(() => useSetting('ui.theme', { debounceMs: 300 }))!;

        setting.value.value = 'max-dark';
        expect(setSpy).not.toHaveBeenCalled();

        const flushed = await setting.flush();
        expect(flushed).toBe(true);
        expect(setSpy).toHaveBeenCalledTimes(1);
        expect(setSpy).toHaveBeenCalledWith('ui.theme', 'max-dark');

        // Avançar o relógio não deve disparar segunda chamada
        await vi.advanceTimersByTimeAsync(300);
        expect(setSpy).toHaveBeenCalledTimes(1);

        scope.stop();
    });

    it('cancela timer pendente ao descartar o escopo reativo (onScopeDispose)', async () => {
        const store = useSettingsStore();
        const setSpy = vi.spyOn(store, 'set');

        const scope = effectScope();
        const setting = scope.run(() => useSetting('terminal.scrollbackLines', { debounceMs: 200 }))!;

        setting.value.value = 25000;
        scope.stop();

        await vi.advanceTimersByTimeAsync(250);
        expect(setSpy).not.toHaveBeenCalled();
    });
});
