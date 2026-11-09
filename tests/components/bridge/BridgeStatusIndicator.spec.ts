import { beforeEach, describe, expect, it, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { MaxButton } from '@maxvue/max-components-ui';
import BridgeStatusIndicator from '@/components/bridge/BridgeStatusIndicator.vue';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
import { useWindowStore } from '@/stores/window';

describe('BridgeStatusIndicator.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('exibe classes semânticas, rótulos e tooltip de porta/conexões para os 4 estados (CA-33)', async () => {
        const store = useMaxCodeBridgeStore();
        const windowStore = useWindowStore();
        const openPrefsSpy = vi.spyOn(windowStore, 'openPreferences');

        store.settings.enabled = true;
        store.status = {
            state: 'connected',
            serverPort: 49100,
            peerInstanceId: 'mc-1',
            peerPid: 1234,
            peerVersion: '1.0.0',
            peerCapabilities: ['inspect_v1'],
            lastRttMs: 2,
            lastSeenMs: 1700000000000,
            lastError: null
        };

        const wrapper = mount(BridgeStatusIndicator);
        expect(wrapper.classes()).toContain('is-success');
        expect(wrapper.text()).toContain('MaxCode conectado');
        const title = wrapper.attributes('title') ?? wrapper.findComponent(MaxButton).props('title');
        expect(title).toContain('Porta 49100');
        expect(title).toContain('1 conexão');

        store.status.state = 'disconnected';
        await wrapper.vm.$nextTick();
        expect(wrapper.classes()).toContain('is-info');
        expect(wrapper.text()).toContain('Aguardando MaxCode');

        store.status.lastError = 'Falha ao abrir socket';
        await wrapper.vm.$nextTick();
        expect(wrapper.classes()).toContain('is-danger');
        expect(wrapper.text()).toContain('Ponte indisponível');

        store.settings.enabled = false;
        await wrapper.vm.$nextTick();
        expect(wrapper.classes()).toContain('is-neutral');
        expect(wrapper.text()).toContain('Ponte desativada');

        await wrapper.find('button').trigger('click');
        expect(openPrefsSpy).toHaveBeenCalledWith('bridge');
        expect(wrapper.emitted('openSettings')).toHaveLength(1);
    });
});
