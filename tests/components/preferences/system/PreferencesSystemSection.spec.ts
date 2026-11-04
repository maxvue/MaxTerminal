import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PreferencesSystemSection from '@/components/preferences/system/PreferencesSystemSection.vue';
import { useOsSettingsStore } from '@/stores/osSettings';
import type { OsSettings_Serialize } from '@/types/bindings';

vi.mock('@/ipc/backup', () => ({
    listDbBackups: vi.fn(async () => []),
    createDbBackup: vi.fn(async () => ({ fileName: 'manual.db', sizeBytes: 1024, createdAt: 1 })),
    restoreDbBackup: vi.fn(async () => ({ restartRequired: false }))
}));

const sampleOsSettings: OsSettings_Serialize = {
    schemaVersion: 1,
    closeBehavior: 'always_tray',
    quake: {
        enabled: true,
        accelerator: 'F12',
        mode: 'drop_down',
        dropDownHeightPct: 40,
        alwaysOnTop: true,
        hideOnBlur: true,
        preferredBackend: 'portal'
    },
    editor: {
        kind: 'vs_code',
        customArgv: []
    },
    notifications: {
        enabled: true,
        longCommandEnabled: true,
        longCommandThresholdSecs: 10,
        onlyWhenUnfocused: true
    },
    autostart: true,
    startMinimized: false
};

describe('PreferencesSystemSection.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('aplica limites numéricos estritos (clamping) em altura Quake e limiar de comando longo', async () => {
        const store = useOsSettingsStore();
        store.settings = { ...sampleOsSettings };
        const updateSpy = vi.spyOn(store, 'update').mockResolvedValue(store.settings);

        const wrapper = mount(PreferencesSystemSection);

        const numberInputs = wrapper.findAllComponents({ name: 'MaxInputNumber' });
        expect(numberInputs.length).toBe(2);

        const quakeHeightComponent = numberInputs[0]!;

        // Abaixo do mínimo de 20% -> clamp para 20
        quakeHeightComponent.vm.$emit('update:modelValue', 10);
        expect(updateSpy).toHaveBeenCalledWith({ quake: { dropDownHeightPct: 20 } });

        // Acima do máximo de 100% -> clamp para 100
        quakeHeightComponent.vm.$emit('update:modelValue', 150);
        expect(updateSpy).toHaveBeenCalledWith({ quake: { dropDownHeightPct: 100 } });

        // Valor intermediário válido
        quakeHeightComponent.vm.$emit('update:modelValue', 55);
        expect(updateSpy).toHaveBeenCalledWith({ quake: { dropDownHeightPct: 55 } });

        const longCmdComponent = numberInputs[1]!;

        // Abaixo do mínimo de 3 segundos -> clamp para 3
        longCmdComponent.vm.$emit('update:modelValue', 1);
        expect(updateSpy).toHaveBeenCalledWith({ notifications: { longCommandThresholdSecs: 3 } });

        // Acima do máximo de 3600 segundos -> clamp para 3600
        longCmdComponent.vm.$emit('update:modelValue', 5000);
        expect(updateSpy).toHaveBeenCalledWith({ notifications: { longCommandThresholdSecs: 3600 } });

        // Valor intermediário válido
        longCmdComponent.vm.$emit('update:modelValue', 45);
        expect(updateSpy).toHaveBeenCalledWith({ notifications: { longCommandThresholdSecs: 45 } });
    });
});
