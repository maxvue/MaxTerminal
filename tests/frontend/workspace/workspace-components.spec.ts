import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import SessionHistoryDrawer from '@/components/workspace/SessionHistoryDrawer.vue';
import TabBar from '@/components/workspace/TabBar.vue';
import TemplateEditorModal from '@/components/workspace/TemplateEditorModal.vue';

describe('Componentes do Workspace de Abas e Sessões (Fase 07)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('TabBar exibe botões "Adicionar nova aba" e "Iniciar Aba Template" e emite eventos', async () => {
        const wrapper = mount(TabBar, {
            props: {
                tabs: [],
                activeTabId: null,
                templates: []
            },
            global: {
                stubs: {
                    MaxIcon: true,
                    teleport: true
                }
            }
        });

        expect(wrapper.text()).toContain('Adicionar nova aba');
        expect(wrapper.text()).toContain('Iniciar Aba Template');

        const addBtn = wrapper.find('[aria-label="Adicionar nova aba"]');
        await addBtn.trigger('click');
        expect(wrapper.emitted('add-default-tab')).toHaveLength(1);
    });

    it('SessionHistoryDrawer renderiza snippets de busca FTS5 como texto puro escapado (sem v-html)', () => {
        const maliciousSnippet = '<img src=x onerror=alert(1)> segredo [REDACTED:BEARER]';
        const wrapper = mount(SessionHistoryDrawer, {
            props: {
                open: true,
                sessions: [],
                searchHits: [
                    {
                        tabSessionId: 'ts-1',
                        sessionId: 's-1',
                        paneId: 'p-1',
                        seq: 1,
                        snippet: maliciousSnippet
                    }
                ],
                selectedDetail: null,
                selectedChunks: []
            },
            global: {
                stubs: {
                    MaxIcon: true,
                    teleport: true
                }
            }
        });

        const input = wrapper.find('input[type="search"]');
        void input.setValue('segredo');
        return wrapper.vm.$nextTick().then(() => {
            expect(wrapper.find('img').exists()).toBe(false);
            expect(wrapper.find('.session-drawer__hit-snippet').text()).toBe(maliciousSnippet);
        });
    });

    it('TemplateEditorModal valida nome obrigatório e emite payload TabTemplateInput', async () => {
        const wrapper = mount(TemplateEditorModal, {
            props: {
                open: true,
                template: null
            },
            global: {
                stubs: {
                    MaxIcon: true,
                    teleport: true
                }
            }
        });

        const inputs = wrapper.findAll('input[type="text"]');
        await inputs[0]?.setValue('Servidor Vite');
        await inputs[3]?.setValue('npm run dev');

        await wrapper.find('form').trigger('submit.prevent');
        const saved = wrapper.emitted('save');
        expect(saved).toBeTruthy();
        expect(saved?.[0]?.[0]).toMatchObject({
            name: 'Servidor Vite',
            initialCommand: 'npm run dev',
            environment: { kind: 'shell', agentId: null }
        });
    });
});
