import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectSettingsDialog from '@/components/projects/ProjectSettingsDialog.vue';
import { MtIpcError } from '@/ipc/core';
import * as projectsIpc from '@/ipc/projects';
import * as themesIpc from '@/ipc/themes';
import { useProjectsStore } from '@/stores/projects';
import { useThemeStore } from '@/stores/theme';
import { useWindowStore } from '@/stores/window';
import type { Project_Serialize } from '@/types/bindings';
import type { ResolvedTheme, ThemeSummary } from '@/types/theme';

const MaxIconStub = {
    props: ['icon'],
    template: '<i class="max-icon-stub" :data-icon="icon"></i>'
};

const MaxModalStub = {
    props: ['visible', 'title', 'subTitle'],
    template: `
        <div v-if="visible" class="max-modal-stub">
            <h2 class="modal-title">{{ title }}</h2>
            <p class="modal-subtitle">{{ subTitle }}</p>
            <slot />
        </div>
    `
};

const MaxInputTextStub = {
    props: ['modelValue', 'label', 'disabled'],
    emits: ['update:modelValue', 'input'],
    template: '<input class="max-input-text-stub" :value="modelValue" :disabled="disabled" :aria-label="label" @input="$emit(\'update:modelValue\', $event.target.value); $emit(\'input\', $event)" />'
};

const MaxInputSelectStub = {
    props: ['modelValue', 'options', 'disabled', 'ariaLabel'],
    emits: ['update:modelValue', 'change'],
    template: `
        <select class="max-input-select-stub" :value="modelValue" :disabled="disabled" :aria-label="ariaLabel" @change="$emit('update:modelValue', $event.target.value); $emit('change', $event.target.value)">
            <option v-for="opt in (options || [])" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
        </select>
    `
};

vi.mock('@/ipc/projects', () => ({
    projectSnapshot: vi.fn(),
    projectUpdate: vi.fn()
}));

vi.mock('@/ipc/themes', () => ({
    themeList: vi.fn(),
    themeAppearanceGet: vi.fn(),
    themeAppearanceUpdate: vi.fn(),
    themeSetActive: vi.fn(),
    themeGet: vi.fn(),
    onThemeChanged: vi.fn().mockResolvedValue(vi.fn()),
    onThemeLibraryChanged: vi.fn().mockResolvedValue(vi.fn()),
    saveThemeBootstrapSnapshot: vi.fn()
}));

function makeProject(id: string, title = `Projeto ${id}`): Project_Serialize {
    return {
        id,
        title,
        icon: 'mdi:folder',
        color: '#14B8A6',
        path: `/home/user/GitHub/${id}`,
        rootId: null,
        stack: 'unknown',
        identitySource: 'auto',
        sortOrder: 0,
        missing: false,
        createdAt: 1000,
        updatedAt: 1000
    };
}

const sampleThemes = [
    { id: 'theme-nord', name: 'Nord', polarity: 'dark', origin: 'builtin' },
    { id: 'theme-dracula', name: 'Dracula', polarity: 'dark', origin: 'builtin' }
] as unknown as ThemeSummary[];

const mockResolved: ResolvedTheme = {
    scheme: {
        id: 'theme-nord',
        name: 'Nord',
        polarity: 'dark',
        accent: '#88C0D0',
        swatches: []
    } as unknown as ResolvedTheme['scheme'],
    uiTokens: {
        background0: '#000'
    } as unknown as ResolvedTheme['uiTokens'],
    xterm: {
        background: '#000'
    } as unknown as ResolvedTheme['xterm']
};

describe('ProjectSettingsDialog', () => {
    let pinia: ReturnType<typeof createPinia>;
    let projectsStore: ReturnType<typeof useProjectsStore>;
    let themeStore: ReturnType<typeof useThemeStore>;
    let windowStore: ReturnType<typeof useWindowStore>;

    const projA = makeProject('proj-a', 'Projeto Alpha');
    const projB = makeProject('proj-b', 'Projeto Beta');

    beforeEach(() => {
        pinia = createPinia();
        setActivePinia(pinia);
        projectsStore = useProjectsStore();
        themeStore = useThemeStore();
        windowStore = useWindowStore();

        vi.clearAllMocks();

        projectsStore.applySnapshot({
            epoch: 'ep1',
            revision: 1,
            projects: [projA, projB],
            roots: [],
            discovered: []
        });

        themeStore.themes = [...sampleThemes];
        themeStore.appearance = {
            activeThemeId: 'max-navy-teal',
            followSystem: false,
            preferredDarkThemeId: 'max-navy-teal',
            preferredLightThemeId: 'max-light',
            fontFamily: 'monospace',
            fontSize: 13,
            lineHeight: 1.2,
            fontLigatures: true,
            cursorBlink: true,
            cursorStyleOverride: null,
            minimumContrastRatio: 1,
            projectOverrides: {}
        };
    });

    it('isola persistência de identidade entre Projeto A e Projeto B', async () => {
        windowStore.openProjectSettings('proj-a');

        const wrapper = mount(ProjectSettingsDialog, {
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxModal: MaxModalStub,
                    MaxInputText: MaxInputTextStub,
                    MaxInputSelect: MaxInputSelectStub
                }
            }
        });

        expect(wrapper.find('.modal-title').text()).toContain('Configurações do projeto — Projeto Alpha');
        expect(wrapper.find('.modal-subtitle').text()).toBe('/home/user/GitHub/proj-a');

        vi.mocked(projectsIpc.projectUpdate).mockResolvedValue({
            ...projA,
            title: 'Alpha Atualizado'
        });

        // Salva alteração em A
        const titleInput = wrapper.find('input[aria-label="Título"]');
        await titleInput.setValue('Alpha Atualizado');
        await wrapper.find('form').trigger('submit');
        await nextTick();

        expect(projectsIpc.projectUpdate).toHaveBeenCalledWith('proj-a', expect.objectContaining({
            title: 'Alpha Atualizado'
        }));
        expect(projectsIpc.projectUpdate).not.toHaveBeenCalledWith('proj-b', expect.anything());

        // Agora muda para o projeto B
        windowStore.openProjectSettings('proj-b');
        await nextTick();

        expect(wrapper.find('.modal-title').text()).toContain('Configurações do projeto — Projeto Beta');
        expect(wrapper.find('.modal-subtitle').text()).toBe('/home/user/GitHub/proj-b');
    });

    it('aplica tema para projeto específico ou herda tema global', async () => {
        windowStore.openProjectSettings('proj-a');
        vi.mocked(themesIpc.themeSetActive).mockResolvedValue(mockResolved);

        const wrapper = mount(ProjectSettingsDialog, {
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxModal: MaxModalStub,
                    MaxInputText: MaxInputTextStub,
                    MaxInputSelect: MaxInputSelectStub
                }
            }
        });

        const select = wrapper.find('select.max-input-select-stub');
        expect(select.exists()).toBe(true);

        // Seleciona Nord para o projeto A
        await select.setValue('theme-nord');
        await flushPromises();

        expect(themesIpc.themeSetActive).toHaveBeenCalledWith('theme-nord', {
            kind: 'project',
            projectId: 'proj-a'
        });

        // Seleciona herança global ('')
        await select.setValue('');
        await flushPromises();

        expect(themesIpc.themeSetActive).toHaveBeenCalledWith('max-navy-teal', {
            kind: 'clear_project',
            projectId: 'proj-a'
        });
    });

    it('exibe erro de tema quando a operação falha', async () => {
        windowStore.openProjectSettings('proj-a');
        vi.mocked(themesIpc.themeSetActive).mockRejectedValue(
            new MtIpcError({ code: 'theme.failed', message: 'Falha no IPC de tema' }, 'themeSetActive')
        );

        const wrapper = mount(ProjectSettingsDialog, {
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxModal: MaxModalStub,
                    MaxInputText: MaxInputTextStub,
                    MaxInputSelect: MaxInputSelectStub
                }
            }
        });

        const select = wrapper.find('select.max-input-select-stub');
        await select.setValue('theme-dracula');
        await flushPromises();

        expect(wrapper.find('.project-settings-dialog__theme-error').text()).toContain('Falha no IPC de tema');
    });

    it('fecha painel automaticamente se projeto ativo for removido da store', async () => {
        windowStore.openProjectSettings('proj-a');

        mount(ProjectSettingsDialog, {
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxModal: MaxModalStub,
                    MaxInputText: MaxInputTextStub,
                    MaxInputSelect: MaxInputSelectStub
                }
            }
        });

        expect(windowStore.projectSettingsProjectId).toBe('proj-a');

        // Simula remoção de proj-a do catálogo
        projectsStore.applySnapshot({
            epoch: 'ep2',
            revision: 2,
            projects: [projB],
            roots: [],
            discovered: []
        });

        await nextTick();
        expect(windowStore.projectSettingsProjectId).toBeNull();
    });
});
