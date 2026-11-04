import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import HorizonModal from '@/components/horizon/HorizonModal.vue';
import { useHorizonStore } from '@/stores/horizon';
import type { HorizonSnapshot_Serialize } from '@/types/bindings';

vi.mock('@/components/terminal/TerminalView.vue', () => ({
    default: {
        name: 'TerminalView',
        props: ['paneId', 'ptyId', 'visible'],
        template: '<div class="mock-terminal-view" :data-pty-id="ptyId" :data-pane-id="paneId" />'
    }
}));

describe('HorizonModal.vue', () => {
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
            environment: 'local',
            queues: [{ connection: 'redis', queue: 'default' }],
            status: 'running',
            pending: 15,
            running: 4,
            failed: 2,
            sampledAtMs: 1700000000000,
            revision: 1,
            diagnostic: null,
            canStart: false,
            canStop: true,
            canClearPending: true,
            session: {
                ptyId: 'pty-100',
                paneId: 'pane-200',
                generation: 1
            },
            ...overrides
        };
    }

    it('renderiza metadados, métricas e terminal supervisionado quando aberto com sessão ativa', () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot();
        store.isModalOpen = true;

        const wrapper = mount(HorizonModal, {
            props: { modelValue: true }
        });

        expect(wrapper.text()).toContain('Em Execução');
        expect(wrapper.text()).toContain('Horizon 5.48.3');
        expect(wrapper.text()).toContain('local');
        expect(wrapper.text()).toContain('15');
        expect(wrapper.text()).toContain('4');
        expect(wrapper.text()).toContain('2');
        expect(wrapper.text()).toContain('redis:default');

        const terminal = wrapper.findComponent({ name: 'TerminalView' });
        expect(terminal.exists()).toBe(true);
        expect(terminal.props('ptyId')).toBe('pty-100');
        expect(terminal.props('paneId')).toBe('pane-200');
    });

    it('renderiza o painel de processo inativo quando não há sessão PTY', () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot({
            status: 'stopped',
            canStart: true,
            canStop: false,
            session: null
        });
        store.isModalOpen = true;

        const wrapper = mount(HorizonModal, {
            props: { modelValue: true }
        });

        expect(wrapper.find('.horizon-modal__empty').exists()).toBe(true);
        expect(wrapper.findComponent({ name: 'TerminalView' }).exists()).toBe(false);
        expect(wrapper.text()).toContain('Processo do Horizon Inativo');
    });

    it('renderiza o painel de processo externo quando o status for external', () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot({
            status: 'external',
            canStart: true,
            canStop: false,
            session: null
        });
        store.isModalOpen = true;

        const wrapper = mount(HorizonModal, {
            props: { modelValue: true }
        });

        expect(wrapper.find('.horizon-modal__external-panel').exists()).toBe(true);
        expect(wrapper.findComponent({ name: 'TerminalView' }).exists()).toBe(false);
        expect(wrapper.text()).toContain('Horizon em Execução Externa');
        expect(wrapper.text()).toContain('Externo (Ativo)');
        expect(wrapper.text()).toContain('Iniciar Terminal Supervisionado no MaxTerminal');
    });

    it('dispara a ação de iniciar ao clicar no botão Iniciar Horizon', async () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot({
            status: 'stopped',
            canStart: true,
            canStop: false,
            session: null
        });
        store.isModalOpen = true;

        const startSpy = vi.spyOn(store, 'start').mockResolvedValue();

        const wrapper = mount(HorizonModal, {
            props: { modelValue: true }
        });

        const startBtn = wrapper
            .findAllComponents({ name: 'MaxButton' })
            .find((b) => b.props('label') === 'Iniciar Horizon');
        expect(startBtn).toBeDefined();

        await startBtn!.trigger('click');
        expect(startSpy).toHaveBeenCalled();
    });

    it('dispara a ação de parar ao clicar no botão Parar Horizon', async () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot({
            status: 'running',
            canStart: false,
            canStop: true
        });
        store.isModalOpen = true;

        const stopSpy = vi.spyOn(store, 'stop').mockResolvedValue();

        const wrapper = mount(HorizonModal, {
            props: { modelValue: true }
        });

        const stopBtn = wrapper
            .findAllComponents({ name: 'MaxButton' })
            .find((b) => b.props('label') === 'Parar Horizon');
        expect(stopBtn).toBeDefined();

        await stopBtn!.trigger('click');
        expect(stopSpy).toHaveBeenCalled();
    });

    it('abre modal de confirmação e limpa exclusivamente tarefas pendentes ao confirmar', async () => {
        const store = useHorizonStore();
        store.data = createMockSnapshot({
            canClearPending: true
        });
        store.isModalOpen = true;

        const previewSpy = vi.spyOn(store, 'previewClear').mockResolvedValue({
            token: 'uuid-token-123',
            contextRevision: 'r1',
            expiresAtMs: 1700000030000,
            scopeId: 'scope-1',
            queues: [
                { connection: 'redis', queue: 'default' }
            ],
            estimatedPending: 15
        });

        const confirmSpy = vi.spyOn(store, 'confirmClear').mockResolvedValue({
            clearedPerQueue: [
                { connection: 'redis', queue: 'default', cleared: 15 }
            ],
            totalCleared: 15,
            isPartial: false,
            warning: null
        });

        const wrapper = mount(HorizonModal, {
            props: { modelValue: true }
        });

        const clearBtn = wrapper
            .findAllComponents({ name: 'MaxButton' })
            .find((b) => b.props('label') === 'Limpar Pendentes');
        expect(clearBtn).toBeDefined();

        await clearBtn!.trigger('click');
        await flushPromises();

        expect(previewSpy).toHaveBeenCalled();

        // O modal de confirmação secundário agora está aberto
        expect(wrapper.text()).toContain('Confirmar Limpeza Exclusiva de Tarefas Pendentes');
        expect(wrapper.text()).toContain('Tarefas reservadas (atualmente em execução pelos workers)');
        expect(wrapper.text()).toContain('estritamente preservados');

        const confirmBtn = wrapper
            .findAllComponents({ name: 'MaxButton' })
            .find((b) => b.props('label') === 'Confirmar Limpeza');
        expect(confirmBtn).toBeDefined();

        await confirmBtn!.trigger('click');
        await flushPromises();

        expect(confirmSpy).toHaveBeenCalledWith('uuid-token-123');
    });
});
