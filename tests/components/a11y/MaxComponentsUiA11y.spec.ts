import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import AgentLauncherMenu from '@/components/agents/AgentLauncherMenu.vue';
import AiPromptInput from '@/components/ai/AiPromptInput.vue';
import AiPreferences from '@/components/preferences/AiPreferences.vue';
import TabBar from '@/components/workspace/TabBar.vue';
import { DEFAULT_AI_SETTINGS, useAiStore } from '@/stores/ai';

describe('Acessibilidade e Padronização MaxComponentsUi (Etapa 3 & 4)', () => {
    let pinia: ReturnType<typeof createPinia>;

    beforeEach(() => {
        pinia = createPinia();
        setActivePinia(pinia);
    });

    it('AiPromptInput define aria-label explícito no campo de prompt', () => {
        const aiStore = useAiStore();
        aiStore.openPanePrompt('pane-test', {
            kind: 'suggest',
            trigger: 'question',
            initialPrompt: 'ls -la',
            selection: null
        });

        const wrapper = mount(AiPromptInput, {
            props: { paneId: 'pane-test' },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: true
                }
            }
        });

        const promptInput = wrapper.findComponent({ name: 'MaxInputText' });
        expect(promptInput.exists()).toBe(true);
        const resolvedLabel =
            (promptInput.props() as Record<string, unknown>)['ariaLabel'] ??
            promptInput.attributes('aria-label') ??
            wrapper.find('[data-testid="ai-prompt-field"] input').attributes('aria-label');
        expect(resolvedLabel).toBe('Prompt de inteligência artificial');
    });

    it('AgentLauncherMenu define aria-label no container MaxPopover', () => {
        const wrapper = mount(AgentLauncherMenu, {
            props: { projectId: 'project-1' },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: true
                }
            }
        });

        const popover = wrapper.findComponent({ name: 'MaxPopover' });
        expect(popover.exists()).toBe(true);
        const resolvedLabel =
            (popover.props() as Record<string, unknown>)['ariaLabel'] ??
            popover.attributes('aria-label');
        expect(resolvedLabel).toBe('Menu de Agentes CLI');
    });

    it('AiPreferences define aria-labels descritivos nos controles MaxInputNumber', () => {
        const store = useAiStore();
        store.settings = {
            ...DEFAULT_AI_SETTINGS,
            diagnosis: {
                ...DEFAULT_AI_SETTINGS.diagnosis,
                debounceMs: 500
            },
            rag: {
                ...DEFAULT_AI_SETTINGS.rag,
                topK: 5
            }
        };

        const wrapper = mount(AiPreferences, {
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: true,
                    HarnessPicker: true
                }
            }
        });

        const debounceNative = wrapper.find('[data-testid="input-diagnosis-debounce"] input');
        expect(debounceNative.exists()).toBe(true);
        expect(debounceNative.attributes('aria-label')).toBe('Atraso de debounce do diagnóstico em milissegundos');

        const topKNative = wrapper.find('[data-testid="input-rag-topk"] input');
        expect(topKNative.exists()).toBe(true);
        expect(topKNative.attributes('aria-label')).toBe('Quantidade de trechos RAG');
    });

    it('TabBar possui controle acessível para adicionar aba e emite add-default-tab', async () => {
        const wrapper = mount(TabBar, {
            props: {
                tabs: [],
                activeTabId: null,
                templates: []
            },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: true,
                    teleport: true
                }
            }
        });

        const addBtn = wrapper.find('[aria-label="Adicionar nova aba"]');
        expect(addBtn.exists()).toBe(true);
        await addBtn.trigger('click');
        expect(wrapper.emitted('add-default-tab')).toHaveLength(1);
    });
});
