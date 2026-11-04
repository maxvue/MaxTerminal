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

    function createMockSnapshot(
        overrides: Partial<HorizonSnapshot_Serialize> = {}
    ): HorizonSnapshot_Serialize {
        return {
            context: {
                projectId: 'p1',
                root: '/app',
                contextRevision: 'r1',
                scopeId: 's1'
            },
            availability: 'available',
            horizonVersion: '5.48.3',
            environment: 'production',
            queues: [{ connection: 'redis', queue: 'default' }],
            status: 'running',
            pending: 5,
            running: 2,
            failed: 0,
            sampledAtMs: 1700000000000,
            revision: 1,
            diagnostic: null,
            canStart: false,
            canStop: true,
            canClearPending: true,
            session: null,
            ...overrides
        };
    }

    it('não renderiza o botão se o Horizon estiver indisponível no projeto', () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot({ availability: 'unavailable' });

        const wrapper = mount(HorizonBadgeButton);
        expect(wrapper.find('.horizon-badge-btn').exists()).toBe(false);
    });

    it('renderiza o botão com label HORIZON e 3 overlays estáveis quando disponível', () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot({
            pending: 42,
            running: 7,
            failed: 0
        });

        const wrapper = mount(HorizonBadgeButton);
        expect(wrapper.find('.horizon-badge-btn').exists()).toBe(true);

        const badgeButton = wrapper.findComponent({ name: 'MaxBadgeButton' });
        expect(badgeButton.exists()).toBe(true);
        expect(badgeButton.props('label')).toBe('HORIZON');

        const overlays = badgeButton.props('overlay');
        expect(overlays).toHaveLength(3);

        // 1. Pendentes: azul (--blue-b-500)
        expect(overlays[0]).toEqual({
            value: '42',
            color: 'var(--blue-b-500)'
        });

        // 2. Em Execução: verde (--max-success-500)
        expect(overlays[1]).toEqual({
            value: '7',
            color: 'var(--max-success-500)'
        });

        // 3. Falhas: vermelho (--max-danger-500) - nota: 0 deve ser preservado como string '0'!
        expect(overlays[2]).toEqual({
            value: '0',
            color: 'var(--max-danger-500)'
        });
    });

    it('renderiza travessão "—" quando as métricas forem nulas ou desconhecidas', () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot({
            pending: null,
            running: null,
            failed: null
        });

        const wrapper = mount(HorizonBadgeButton);
        const badgeButton = wrapper.findComponent({ name: 'MaxBadgeButton' });
        const overlays = badgeButton.props('overlay');

        expect(overlays[0].value).toBe('—');
        expect(overlays[1].value).toBe('—');
        expect(overlays[2].value).toBe('—');
    });

    it('define title e aria-label acessíveis descrevendo os contadores e status', () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot({
            pending: 12,
            running: 3,
            failed: 1,
            status: 'running'
        });

        const wrapper = mount(HorizonBadgeButton);
        const badgeButton = wrapper.findComponent({ name: 'MaxBadgeButton' });

        expect(badgeButton.attributes('title')).toContain('12 pendentes');
        expect(badgeButton.attributes('title')).toContain('3 em execução');
        expect(badgeButton.attributes('title')).toContain('1 falhas');
        expect(badgeButton.attributes('aria-label')).toContain('Painel do Laravel Horizon');
    });

    it('sincroniza o estado de abertura do modal via modelValue', async () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot();
        expect(store.isModalOpen).toBe(false);

        const wrapper = mount(HorizonBadgeButton);
        const badgeButton = wrapper.findComponent({ name: 'MaxBadgeButton' });

        // Simula a ativação do botão
        await badgeButton.vm.$emit('update:modelValue', true);
        expect(store.isModalOpen).toBe(true);

        await badgeButton.vm.$emit('update:modelValue', false);
        expect(store.isModalOpen).toBe(false);
    });

    it('chama openModal ao clicar no botão', async () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot();
        const openModalSpy = vi.spyOn(store, 'openModal');

        const wrapper = mount(HorizonBadgeButton);
        const badgeButton = wrapper.findComponent({ name: 'MaxBadgeButton' });
        await badgeButton.trigger('click');

        expect(openModalSpy).toHaveBeenCalled();
    });
});
