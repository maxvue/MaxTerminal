import { describe, expect, it, vi, beforeEach } from 'vitest';
import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import InspectButton from '@/components/bridge/InspectButton.vue';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';

describe('InspectButton.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('oculta o botão quando exitCode é null, undefined ou 0 e exibe quando != 0 (CA-28)', () => {
        const wNull = mount(InspectButton, {
            props: { block: { id: 'b-1', exitCode: null } }
        });
        expect(wNull.find('button').exists()).toBe(false);

        const wUndef = mount(InspectButton, {
            props: { block: { id: 'b-2', exitCode: undefined } }
        });
        expect(wUndef.find('button').exists()).toBe(false);

        const wZero = mount(InspectButton, {
            props: { block: { id: 'b-3', exitCode: 0 } }
        });
        expect(wZero.find('button').exists()).toBe(false);

        const wErr = mount(InspectButton, {
            props: { block: { id: 'b-4', exitCode: 2 } }
        });
        expect(wErr.find('button').exists()).toBe(true);
        expect(wErr.text()).toContain('Inspecionar');

        const wNeg = mount(InspectButton, {
            props: { block: { id: 'b-5', exitCode: -1 } }
        });
        expect(wNeg.find('button').exists()).toBe(true);
    });

    it('dispara inspectCommandError e emite evento inspected ao concluir (CA-32)', async () => {
        const store = useMaxCodeBridgeStore();
        vi.spyOn(store, 'inspectCommandError').mockResolvedValueOnce({
            transport: 'loopback',
            requestId: 'req-42',
            conversationId: 'conv-42',
            formattedPrompt: 'Ao executar o comando "npm test" tive o seguinte erro: "FAIL"'
        });

        const wrapper = mount(InspectButton, {
            props: { block: { id: 'b-err', exitCode: 1 } }
        });

        await wrapper.find('button').trigger('click');
        expect(store.inspectCommandError).toHaveBeenCalledWith('b-err');
        expect(wrapper.emitted('inspected')?.[0]?.[0]).toMatchObject({
            requestId: 'req-42'
        });
        expect(wrapper.text()).toContain('Enviado ao MaxCode');
    });
});
