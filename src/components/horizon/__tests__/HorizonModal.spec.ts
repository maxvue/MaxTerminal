import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { nextTick } from 'vue';
import HorizonModal from '@/components/horizon/HorizonModal.vue';
import { useHorizonStore } from '@/stores/horizon';
import { useProjectsStore } from '@/stores/projects';
import type { HorizonSnapshot_Serialize, Project_Serialize } from '@/types/bindings';

vi.mock('@/components/terminal/TerminalView.vue', () => ({
    default: {
        name: 'TerminalView',
        props: ['paneId', 'ptyId', 'visible'],
        template: '<div class="mock-terminal-view" :data-pty="ptyId" :data-pane="paneId">Terminal Mock</div>'
    }
}));

describe('HorizonModal.vue', () => {
    let pinia: ReturnType<typeof createPinia>;

    const sampleProject: Project_Serialize = {
        id: 'proj-1',
        title: 'Laravel Project',
        icon: 'lucide:code',
        color: '#ff2d20',
        path: '/var/www/html',
        rootId: null,
        sortOrder: 0,
        createdAt: 1700000000000,
        updatedAt: 1700000000000,
        stack: 'laravel',
        identitySource: 'auto',
        missing: false
    };

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
        status: 'stopped',
        pending: 15,
        running: 2,
        failed: 1,
        sampledAtMs: 1700000000000,
        revision: 1,
        diagnostic: null,
        canStart: true,
        canStop: false,
        canClearPending: true,
        session: null
    };

    beforeEach(() => {
        document.body.innerHTML = '';
        pinia = createPinia();
        setActivePinia(pinia);
        vi.clearAllMocks();

        const projectsStore = useProjectsStore();
        projectsStore.projects = [sampleProject];
        projectsStore.selectedId = 'proj-1';
    });

    afterEach(() => {
        document.body.innerHTML = '';
    });

    it('renderiza título e métricas quando aberto no body', async () => {
        const store = useHorizonStore();
        store.data = sampleSnapshot;
        store.isConfirmedCurrentActivation = true;
        store.isModalOpen = true;

        mount(HorizonModal, {
            attachTo: document.body,
            props: {
                modelValue: true
            },
            global: {
                plugins: [pinia]
            }
        });

        await nextTick();

        expect(document.body.textContent).toContain('Laravel Horizon — Monitoramento e Supervisão');
        expect(document.body.textContent).toContain('Parado');
        expect(document.body.textContent).toContain('15');
        expect(document.body.textContent).toContain('2');
        expect(document.body.textContent).toContain('1');
    });

    it('renderiza o TerminalView com PTY e pane quando houver sessão ativa supervisionada', async () => {
        const store = useHorizonStore();
        store.data = {
            ...sampleSnapshot,
            status: 'running',
            session: {
                ptyId: 'pty-101',
                paneId: 'pane-202',
                generation: 1
            }
        };
        store.isConfirmedCurrentActivation = true;
        store.isModalOpen = true;

        mount(HorizonModal, {
            attachTo: document.body,
            props: {
                modelValue: true
            },
            global: {
                plugins: [pinia]
            }
        });

        await nextTick();

        const term = document.body.querySelector('.mock-terminal-view');
        expect(term).not.toBeNull();
        expect(term?.getAttribute('data-pty')).toBe('pty-101');
        expect(term?.getAttribute('data-pane')).toBe('pane-202');
    });

    it('exibe o painel de execução externa sem CTA de iniciar terminal duplicado quando status for external', async () => {
        const store = useHorizonStore();
        store.data = {
            ...sampleSnapshot,
            status: 'external',
            session: null
        };
        store.isConfirmedCurrentActivation = true;
        store.isModalOpen = true;

        mount(HorizonModal, {
            attachTo: document.body,
            props: {
                modelValue: true
            },
            global: {
                plugins: [pinia]
            }
        });

        await nextTick();

        expect(document.body.querySelector('.horizon-modal__external-panel')).not.toBeNull();
        expect(document.body.textContent).toContain('Horizon em Execução Externa');
        expect(document.body.textContent).not.toContain('Iniciar Terminal Supervisionado no MaxTerminal');
    });

    it('aciona store.start() ao clicar no botão de iniciar', async () => {
        const store = useHorizonStore();
        store.data = sampleSnapshot;
        store.isConfirmedCurrentActivation = true;
        store.isModalOpen = true;

        const startSpy = vi.spyOn(store, 'start').mockResolvedValue(undefined);

        const wrapper = mount(HorizonModal, {
            attachTo: document.body,
            props: {
                modelValue: true
            },
            global: {
                plugins: [pinia]
            }
        });

        await nextTick();

        const startBtn = wrapper.findAllComponents({ name: 'MaxButton' }).find((b) => b.text().includes('Iniciar Horizon'));
        expect(startBtn).toBeDefined();

        await startBtn!.trigger('click');
        expect(startSpy).toHaveBeenCalled();
    });

    it('aciona store.stop() ao clicar no botão de parar', async () => {
        const store = useHorizonStore();
        store.data = {
            ...sampleSnapshot,
            status: 'running',
            canStart: false,
            canStop: true
        };
        store.isConfirmedCurrentActivation = true;
        store.isModalOpen = true;

        const stopSpy = vi.spyOn(store, 'stop').mockResolvedValue(undefined);

        const wrapper = mount(HorizonModal, {
            attachTo: document.body,
            props: {
                modelValue: true
            },
            global: {
                plugins: [pinia]
            }
        });

        await nextTick();

        const stopBtn = wrapper.findAllComponents({ name: 'MaxButton' }).find((b) => b.text().includes('Parar Horizon'));
        expect(stopBtn).toBeDefined();

        await stopBtn!.trigger('click');
        expect(stopSpy).toHaveBeenCalled();
    });

    it('aciona previewClear() ao clicar em Limpar Pendentes', async () => {
        const store = useHorizonStore();
        store.data = sampleSnapshot;
        store.isConfirmedCurrentActivation = true;
        store.isModalOpen = true;

        vi.spyOn(store, 'previewClear').mockResolvedValue({
            token: 'test-token',
            contextRevision: 'rev-1',
            scopeId: 'scope-1',
            queues: [{ connection: 'redis', queue: 'default' }],
            estimatedPending: 15,
            expiresAtMs: Date.now() + 30000
        });

        const wrapper = mount(HorizonModal, {
            attachTo: document.body,
            props: {
                modelValue: true
            },
            global: {
                plugins: [pinia]
            }
        });

        await nextTick();

        const clearBtn = wrapper.findAllComponents({ name: 'MaxButton' }).find((b) => b.text().includes('Limpar Pendentes'));
        expect(clearBtn).toBeDefined();

        await clearBtn!.trigger('click');
        expect(store.previewClear).toHaveBeenCalled();
    });
});
