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
                    MaxIcon: MaxIconStub,
                    teleport: true
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
        expect(wrapper.find('[role="menu"]').exists()).toBe(true);

        await wrapper.find('[data-action="edit"]').trigger('click');
        expect(wrapper.emitted('edit')?.[0]).toEqual(['proj-laravel']);
        expect(wrapper.find('[role="menu"]').exists()).toBe(false);

        // Reabre menu e testa configure
        await wrapper.find('.project-item__menu-trigger').trigger('click');
        const selectCountBeforeConfigure = wrapper.emitted('select')?.length ?? 0;
        await wrapper.find('[data-action="configure"]').trigger('click');
        expect(wrapper.emitted('configure')?.[0]).toEqual(['proj-laravel']);
        expect(wrapper.find('[role="menu"]').exists()).toBe(false);
        // Garantir que configure não emite select acidental
        expect(wrapper.emitted('select')?.length ?? 0).toBe(selectCountBeforeConfigure);

        await wrapper.find('.project-item__menu-trigger').trigger('click');
        await wrapper.find('[data-action="redetect"]').trigger('click');
        expect(wrapper.emitted('redetect')?.[0]).toEqual(['proj-laravel']);

        await wrapper.find('.project-item__menu-trigger').trigger('click');
        await wrapper.find('[data-action="remove"]').trigger('click');
        expect(wrapper.emitted('remove')?.[0]).toEqual(['proj-laravel']);
    });
});
