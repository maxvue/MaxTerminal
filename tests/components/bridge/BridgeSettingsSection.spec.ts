import { beforeEach, describe, expect, it, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import BridgeSettingsSection from '@/components/bridge/BridgeSettingsSection.vue';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';

type EmittableComponent = { vm: { $emit: (event: string, ...args: unknown[]) => void } };

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

        const fallbackSelect = wrapper.find('[data-testid="select-fallback-mode"] select');
        if (fallbackSelect.exists()) {
            await fallbackSelect.setValue('clipboard_only');
        } else {
            const fallbackComp = wrapper.findComponent('[data-testid="select-fallback-mode"]');
            (fallbackComp as unknown as EmittableComponent).vm.$emit('update:modelValue', 'clipboard_only');
        }
        expect(updateSpy).toHaveBeenCalledWith({ inspectFallbackMode: 'clipboard_only' });

        const portInput = wrapper.find('[data-testid="input-preferred-port"] input');
        if (portInput.exists()) {
            const portComp = wrapper.findComponent('[data-testid="input-preferred-port"]');
            if (portComp.exists()) {
                (portComp as unknown as EmittableComponent).vm.$emit('update:modelValue', 48080);
            } else {
                await portInput.setValue('48080');
            }
        } else {
            await wrapper.get('[data-testid="input-preferred-port"]').setValue('48080');
        }
        expect(updateSpy).toHaveBeenCalledWith({ preferredPort: 48080 });

        await wrapper.get('[data-testid="btn-reconnect"]').trigger('click');
        expect(reconnectSpy).toHaveBeenCalledTimes(1);
    });
});
