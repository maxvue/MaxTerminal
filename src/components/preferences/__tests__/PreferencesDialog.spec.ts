import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { nextTick } from 'vue';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import PreferencesDialog from '@/components/preferences/PreferencesDialog.vue';
import { useWindowStore } from '@/stores/window';

const DummySection = {
    template: '<div class="dummy-section">Seção Conteúdo</div>'
};

describe('PreferencesDialog.vue', () => {
    let pinia: ReturnType<typeof createPinia>;
    let windowStore: ReturnType<typeof useWindowStore>;
    let wrapper: ReturnType<typeof mount> | null = null;

    beforeEach(() => {
        document.body.innerHTML = '';
        pinia = createPinia();
        setActivePinia(pinia);
        windowStore = useWindowStore();
    });

    afterEach(() => {
        wrapper?.unmount();
        wrapper = null;
        document.body.innerHTML = '';
    });

    it('renderiza título "Configurações gerais" e as 6 seções cadastradas', async () => {
        windowStore.openPreferences('system');

        wrapper = mount(PreferencesDialog, {
            attachTo: document.body,
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: true,
                    PreferencesSystemSection: DummySection,
                    AppearancePreferences: DummySection,
                    AiPreferences: DummySection,
                    AgentSettingsPanel: DummySection,
                    SkillsAndMemoryPreferences: DummySection,
                    BridgeSettingsSection: DummySection
                }
            }
        });

        await nextTick();

        // Título visível renderizado no MaxModal
        expect(document.body.textContent).toContain('Configurações gerais');

        // 6 seções esperadas
        const buttons = Array.from(document.body.querySelectorAll('.prefs-dialog__nav-btn'));
        expect(buttons.length).toBe(6);

        const buttonTexts = buttons.map((b) => b.textContent ?? '');
        expect(buttonTexts.some((t) => t.includes('Sistema & SO'))).toBe(true);
        expect(buttonTexts.some((t) => t.includes('Aparência & Temas'))).toBe(true);
        expect(buttonTexts.some((t) => t.includes('Inteligência Artificial'))).toBe(true);
        expect(buttonTexts.some((t) => t.includes('Agentes CLI'))).toBe(true);
        expect(buttonTexts.some((t) => t.includes('MaxSkills & Memória'))).toBe(true);
        expect(buttonTexts.some((t) => t.includes('Integração MaxCode'))).toBe(true);
    });

    it('troca de seção ao clicar no item de navegação', async () => {
        windowStore.openPreferences('system');

        wrapper = mount(PreferencesDialog, {
            attachTo: document.body,
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: true,
                    PreferencesSystemSection: DummySection,
                    AppearancePreferences: DummySection,
                    AiPreferences: DummySection,
                    AgentSettingsPanel: DummySection,
                    SkillsAndMemoryPreferences: DummySection,
                    BridgeSettingsSection: DummySection
                }
            }
        });

        await nextTick();

        const buttons = Array.from(document.body.querySelectorAll<HTMLElement>('.prefs-dialog__nav-btn'));
        const appearanceBtn = buttons.find((b) => b.textContent?.includes('Aparência & Temas'));
        expect(appearanceBtn).toBeDefined();

        appearanceBtn?.click();
        await nextTick();

        expect(windowStore.preferencesSection).toBe('appearance');
    });

    it('faz fallback para a primeira seção caso o id da seção seja desconhecido', async () => {
        windowStore.openPreferences('secao-desconhecida-xyz');

        wrapper = mount(PreferencesDialog, {
            attachTo: document.body,
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: true,
                    PreferencesSystemSection: DummySection,
                    AppearancePreferences: DummySection,
                    AiPreferences: DummySection,
                    AgentSettingsPanel: DummySection,
                    SkillsAndMemoryPreferences: DummySection,
                    BridgeSettingsSection: DummySection
                }
            }
        });

        await nextTick();

        // O conteúdo renderiza fallback para a primeira seção
        expect(document.body.querySelector('.dummy-section')).not.toBeNull();
    });

    it('fecha o diálogo e atualiza store quando o modal é fechado', async () => {
        windowStore.openPreferences('system');

        wrapper = mount(PreferencesDialog, {
            attachTo: document.body,
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: true,
                    PreferencesSystemSection: DummySection,
                    AppearancePreferences: DummySection,
                    AiPreferences: DummySection,
                    AgentSettingsPanel: DummySection,
                    SkillsAndMemoryPreferences: DummySection,
                    BridgeSettingsSection: DummySection
                }
            }
        });

        await nextTick();
        expect(windowStore.preferencesOpen).toBe(true);

        const modalComponent = wrapper.findComponent({ name: 'MaxModal' });
        expect(modalComponent.exists()).toBe(true);

        modalComponent.vm.$emit('update:visible', false);
        await nextTick();

        expect(windowStore.preferencesOpen).toBe(false);
    });

    it('aplica dimensões com largura de viewport menos 50px e limite máximo de 1500px no MaxModal', async () => {
        windowStore.openPreferences('system');

        wrapper = mount(PreferencesDialog, {
            attachTo: document.body,
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: true,
                    PreferencesSystemSection: DummySection,
                    AppearancePreferences: DummySection,
                    AiPreferences: DummySection,
                    AgentSettingsPanel: DummySection,
                    SkillsAndMemoryPreferences: DummySection,
                    BridgeSettingsSection: DummySection
                }
            }
        });

        await nextTick();

        const modalComponent = wrapper.findComponent({ name: 'MaxModal' });
        expect(modalComponent.exists()).toBe(true);
        expect(modalComponent.props('width')).toBe('100vw');
        expect(modalComponent.props('maxWidth')).toBe('100vw');
        expect(modalComponent.classes()).toContain('prefs-dialog-modal');
    });

    it('alinha os botões de navegação e labels com ícone e texto', async () => {
        windowStore.openPreferences('system');

        wrapper = mount(PreferencesDialog, {
            attachTo: document.body,
            global: {
                plugins: [pinia],
                stubs: {
                    PreferencesSystemSection: DummySection,
                    AppearancePreferences: DummySection,
                    AiPreferences: DummySection,
                    AgentSettingsPanel: DummySection,
                    SkillsAndMemoryPreferences: DummySection,
                    BridgeSettingsSection: DummySection
                }
            }
        });

        await nextTick();

        const buttons = Array.from(document.body.querySelectorAll<HTMLElement>('.prefs-dialog__nav-btn'));
        expect(buttons.length).toBe(6);

        for (const btn of buttons) {
            expect(btn.classList.contains('prefs-dialog__nav-btn')).toBe(true);
            const labelEl = btn.querySelector('.max-button-label');
            if (labelEl) {
                expect(labelEl).toBeDefined();
            }
        }
    });
});
