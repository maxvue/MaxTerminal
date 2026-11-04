import { beforeEach, describe, expect, it, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import BridgeSettingsSection from '@/components/bridge/BridgeSettingsSection.vue';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';

describe('BridgeSettingsSection.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('renderiza controles da ponte e persiste alterações via updateSettings e reconnectNow', async () => {
        const store = useMaxCodeBridgeStore();
        const updateSpy = vi.spyOn(store, 'updateSettings').mockImplementation(async (patch) => {
            store.settings = {
                ...store.settings,
                enabled: patch.enabled ?? store.settings.enabled,
                autoSyncActiveProject:
                    patch.autoSyncActiveProject ?? store.settings.autoSyncActiveProject,
                autoRegisterUnknownProject:
                    patch.autoRegisterUnknownProject ?? store.settings.autoRegisterUnknownProject,
                focusWindowOnIncomingCommand:
                    patch.focusWindowOnIncomingCommand ??
                    store.settings.focusWindowOnIncomingCommand,
                inspectFallbackMode:
                    patch.inspectFallbackMode ?? store.settings.inspectFallbackMode,
                preferredPort:
                    patch.preferredPort !== undefined
                        ? patch.preferredPort
                        : store.settings.preferredPort
            };
            return store.settings;
        });
        const reconnectSpy = vi.spyOn(store, 'reconnectNow').mockResolvedValue(store.status);

        const wrapper = mount(BridgeSettingsSection);

        const syncToggle = wrapper.find('[data-testid="toggle-sync-project"] input').exists()
            ? wrapper.get('[data-testid="toggle-sync-project"] input')
            : wrapper.get('[data-testid="toggle-sync-project"]');
        await syncToggle.setValue(false);
        expect(updateSpy).toHaveBeenCalledWith({ autoSyncActiveProject: false });

        await wrapper.get('[data-testid="select-fallback-mode"]').setValue('clipboard_only');
        expect(updateSpy).toHaveBeenCalledWith({ inspectFallbackMode: 'clipboard_only' });

        await wrapper.get('[data-testid="input-preferred-port"]').setValue('48080');
        expect(updateSpy).toHaveBeenCalledWith({ preferredPort: 48080 });

        await wrapper.get('[data-testid="btn-reconnect"]').trigger('click');
        expect(reconnectSpy).toHaveBeenCalledTimes(1);
    });
});
