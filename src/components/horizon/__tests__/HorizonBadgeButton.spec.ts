import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import HorizonBadgeButton from '@/components/horizon/HorizonBadgeButton.vue';
import { useHorizonStore } from '@/stores/horizon';
import type { HorizonSnapshot_Serialize } from '@/types/bindings';

describe('HorizonBadgeButton.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    const sampleSnapshot: HorizonSnapshot_Serialize = {
        context: {
            projectId: 'proj-1',
            root: '/var/www/html',
            contextRevision: 'rev-1',
            scopeId: 'scope-1'
        },
        availability: 'available',
        horizonVersion: '5.48.0',
        environment: 'local',
        queues: [{ connection: 'redis', queue: 'default' }],
        status: 'running',
        pending: 10,
        running: 3,
        failed: 0,
        sampledAtMs: 1700000000000,
        revision: 1,
        diagnostic: null,
        canStart: false,
        canStop: true,
        canClearPending: true,
        session: null
    };

    it('não renderiza o botão se o Horizon estiver indisponível (checking ou unavailable)', () => {
        const store = useHorizonStore();
        store.data = null;

        const wrapper = mount(HorizonBadgeButton);
        expect(wrapper.find('.horizon-badge-btn').exists()).toBe(false);
    });

    it('renderiza o botão com contadores e runtimeStatus quando disponível', () => {
        const store = useHorizonStore();
        store.data = sampleSnapshot;
        store.isConfirmedCurrentActivation = true;

        const wrapper = mount(HorizonBadgeButton);
        expect(wrapper.find('.horizon-badge-btn').exists()).toBe(true);
        expect(wrapper.text()).toContain('HORIZON');
        expect(wrapper.attributes('title')).toContain('Status: running');
    });

    it('chama store.openModal() ao ser clicado', async () => {
        const store = useHorizonStore();
        store.data = sampleSnapshot;
        store.isConfirmedCurrentActivation = true;

        const wrapper = mount(HorizonBadgeButton);
        expect(store.isModalOpen).toBe(false);

        await wrapper.find('.horizon-badge-btn').trigger('click');
        expect(store.isModalOpen).toBe(true);
    });
});
