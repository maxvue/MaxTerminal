import { mount } from '@vue/test-utils';
import { defineComponent, h } from 'vue';
import { describe, expect, it } from 'vitest';
import ProjectItem from '@/components/projects/ProjectItem.vue';
import { ZERO_PROJECT_COUNTERS } from '@/stores/projects';
import type { Project_Serialize } from '@/types/bindings';

const MaxIconStub = defineComponent({
    name: 'MaxIcon',
    props: {
        icon: {
            type: String,
            required: true
        }
    },
    setup(props) {
        return () => h('i', { class: 'max-icon-stub', 'data-icon': props.icon });
    }
});

function makeSampleProject(overrides: Partial<Project_Serialize> = {}): Project_Serialize {
    return {
        id: 'proj-laravel',
        title: 'Engeapp',
        icon: 'logos:laravel',
        color: '#FF2D20',
        path: '/home/user/GitHub/engeapp',
        rootId: null,
        stack: 'laravel',
        identitySource: 'auto',
        sortOrder: 0,
        missing: false,
        createdAt: 1000,
        updatedAt: 1000,
        ...overrides
    };
}

describe('ProjectItem.vue', () => {
    it('passa project.icon ao MaxIcon, aplica --project-color, exibe estado missing e emite eventos do menu de contexto incluindo configure', async () => {
        const project = makeSampleProject({ missing: true });

        const wrapper = mount(ProjectItem, {
            props: {
                project,
                counters: ZERO_PROJECT_COUNTERS,
                selected: true
            },
            global: {
                stubs: {
                    MaxIcon: MaxIconStub
                }
            }
        });

        // Verifica MaxIcon com project.icon
        const avatarIcon = wrapper.find('.project-item__avatar .max-icon-stub');
        expect(avatarIcon.exists()).toBe(true);
        expect(avatarIcon.attributes('data-icon')).toBe('logos:laravel');

        // Verifica custom property --project-color e classes de estado
        expect(wrapper.attributes('style')).toContain('--project-color: #FF2D20');
        expect(wrapper.classes()).toContain('project-item--selected');
        expect(wrapper.classes()).toContain('project-item--missing');
        expect(wrapper.text()).toContain('Pasta não encontrada no disco');

        // Clicar no item principal emite select
        await wrapper.find('.project-item__main').trigger('click');
        expect(wrapper.emitted('select')?.[0]).toEqual(['proj-laravel']);

        // Abre menu de ações e testa emits: edit, configure, redetect, remove
        await wrapper.find('.project-item__menu-trigger').trigger('click');
        expect(wrapper.find('.project-item__menu').exists()).toBe(true);

        await wrapper.find('[data-action="edit"]').trigger('click');
        expect(wrapper.emitted('edit')?.[0]).toEqual(['proj-laravel']);
        expect(wrapper.find('.project-item__menu').exists()).toBe(false);

        // Reabre menu e testa configure
        await wrapper.find('.project-item__menu-trigger').trigger('click');
        const selectCountBeforeConfigure = wrapper.emitted('select')?.length ?? 0;
        await wrapper.find('[data-action="configure"]').trigger('click');
        expect(wrapper.emitted('configure')?.[0]).toEqual(['proj-laravel']);
        expect(wrapper.find('.project-item__menu').exists()).toBe(false);
        // Garantir que configure não emite select acidental
        expect(wrapper.emitted('select')?.length ?? 0).toBe(selectCountBeforeConfigure);

        await wrapper.find('.project-item__menu-trigger').trigger('click');
        await wrapper.find('[data-action="redetect"]').trigger('click');
        expect(wrapper.emitted('redetect')?.[0]).toEqual(['proj-laravel']);

        await wrapper.find('.project-item__menu-trigger').trigger('click');
        await wrapper.find('[data-action="remove"]').trigger('click');
        expect(wrapper.emitted('remove')?.[0]).toEqual(['proj-laravel']);
    });

    it('renderiza botão Ocultar por padrão (hidden=false), emite hide com project.id e não emite select', async () => {
        const project = makeSampleProject();

        const wrapper = mount(ProjectItem, {
            props: {
                project,
                counters: ZERO_PROJECT_COUNTERS,
                selected: false,
                hidden: false
            },
            global: {
                stubs: {
                    MaxIcon: MaxIconStub
                }
            }
        });

        const btn = wrapper.find('.project-item__visibility-btn');
        expect(btn.exists()).toBe(true);
        expect(btn.text()).toContain('Ocultar');

        await btn.trigger('click');
        expect(wrapper.emitted('hide')?.[0]).toEqual(['proj-laravel']);
        expect(wrapper.emitted('show')).toBeUndefined();
        expect(wrapper.emitted('select')).toBeUndefined();
    });

    it('renderiza botão Mostrar quando hidden=true, emite show com project.id e não emite select', async () => {
        const project = makeSampleProject();

        const wrapper = mount(ProjectItem, {
            props: {
                project,
                counters: ZERO_PROJECT_COUNTERS,
                selected: false,
                hidden: true
            },
            global: {
                stubs: {
                    MaxIcon: MaxIconStub
                }
            }
        });

        const btn = wrapper.find('.project-item__visibility-btn');
        expect(btn.exists()).toBe(true);
        expect(btn.text()).toContain('Mostrar');

        await btn.trigger('click');
        expect(wrapper.emitted('show')?.[0]).toEqual(['proj-laravel']);
        expect(wrapper.emitted('hide')).toBeUndefined();
        expect(wrapper.emitted('select')).toBeUndefined();
    });

    it('desabilita o botão de visibilidade quando disabled=true ou pending=true e impede emissão de eventos', async () => {
        const project = makeSampleProject();

        const wrapperDisabled = mount(ProjectItem, {
            props: {
                project,
                counters: ZERO_PROJECT_COUNTERS,
                selected: false,
                disabled: true
            },
            global: {
                stubs: {
                    MaxIcon: MaxIconStub
                }
            }
        });

        const btnDisabled = wrapperDisabled.find('.project-item__visibility-btn');
        expect(btnDisabled.attributes('disabled')).toBeDefined();
        await btnDisabled.trigger('click');
        expect(wrapperDisabled.emitted('hide')).toBeUndefined();

        const wrapperPending = mount(ProjectItem, {
            props: {
                project,
                counters: ZERO_PROJECT_COUNTERS,
                selected: false,
                pending: true
            },
            global: {
                stubs: {
                    MaxIcon: MaxIconStub
                }
            }
        });

        const btnPending = wrapperPending.find('.project-item__visibility-btn');
        expect(btnPending.attributes('disabled')).toBeDefined();
        await btnPending.trigger('click');
        expect(wrapperPending.emitted('hide')).toBeUndefined();
    });
});
