import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MaxInputNumber } from '@maxvue/max-components-ui';
import BridgeSettingsSection from '@/components/bridge/BridgeSettingsSection.vue';
import TerminalFontSettings from '@/components/preferences/appearance/TerminalFontSettings.vue';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
import type { AppearanceSettings } from '@/types/theme';

describe('NumericInputContracts — Validação explícita de contratos numéricos (Etapa 4)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    describe('BridgeSettingsSection — preferredPort (1024..65535)', () => {
        it('aceita porta inteira válida e null/vazio, rejeitando portas fora da faixa ou fracionárias', async () => {
            const bridgeStore = useMaxCodeBridgeStore();
            const updateSpy = vi.spyOn(bridgeStore, 'updateSettings').mockResolvedValue(bridgeStore.settings);

            const wrapper = mount(BridgeSettingsSection);
            const inputComp = wrapper.findComponent('[data-testid="input-preferred-port"]');
            expect(inputComp.exists()).toBe(true);

            // Rejeita porta menor que 1024
            await (inputComp as any).vm.$emit('update:modelValue', 80);
            expect(updateSpy).not.toHaveBeenCalled();

            // Rejeita porta maior que 65535
            await (inputComp as any).vm.$emit('update:modelValue', 70000);
            expect(updateSpy).not.toHaveBeenCalled();

            // Rejeita valor decimal
            await (inputComp as any).vm.$emit('update:modelValue', 3000.5);
            expect(updateSpy).not.toHaveBeenCalled();

            // Aceita porta válida
            await (inputComp as any).vm.$emit('update:modelValue', 4000);
            expect(updateSpy).toHaveBeenCalledWith({ preferredPort: 4000 });

            // Aceita esvaziamento (null)
            await (inputComp as any).vm.$emit('update:modelValue', null);
            expect(updateSpy).toHaveBeenCalledWith({ preferredPort: null });
        });
    });

    describe('TerminalFontSettings — fontSize (8..36) e lineHeight (0.8..2.5)', () => {
        const baseAppearance: AppearanceSettings = {
            activeThemeId: 't1',
            preferredDarkThemeId: 't1',
            preferredLightThemeId: 't2',
            followSystem: false,
            fontFamily: 'JetBrains Mono',
            fontSize: 14,
            lineHeight: 1.2,
            fontLigatures: true,
            cursorBlink: true,
            cursorStyleOverride: null,
            minimumContrastRatio: 1,
            projectOverrides: {}
        };

        it('clampa fontSize nos limites 8 e 36', async () => {
            const wrapper = mount(TerminalFontSettings, {
                props: { appearance: baseAppearance }
            });

            const numberInputs = wrapper.findAllComponents(MaxInputNumber);
            const fontSizeInput = numberInputs.find((c) => c.attributes('id') === 'mt-font-size') ?? numberInputs[0];
            expect(fontSizeInput).toBeDefined();
            expect(fontSizeInput!.exists()).toBe(true);

            // Menor que 8 -> clampa para 8
            await (fontSizeInput as any).vm.$emit('update:modelValue', 4);
            expect(wrapper.emitted('update')?.[0]?.[0]).toEqual({ fontSize: 8 });

            // Maior que 36 -> clampa para 36
            await (fontSizeInput as any).vm.$emit('update:modelValue', 50);
            expect(wrapper.emitted('update')?.[1]?.[0]).toEqual({ fontSize: 36 });
        });

        it('clampa lineHeight nos limites 0.8 e 2.5', async () => {
            const wrapper = mount(TerminalFontSettings, {
                props: { appearance: baseAppearance }
            });

            const numberInputs = wrapper.findAllComponents(MaxInputNumber);
            const lineHeightInput = numberInputs.find((c) => c.attributes('id') === 'mt-line-height') ?? numberInputs[1];
            expect(lineHeightInput).toBeDefined();
            expect(lineHeightInput!.exists()).toBe(true);

            // Menor que 0.8 -> clampa para 0.8
            await (lineHeightInput as any).vm.$emit('update:modelValue', 0.2);
            expect(wrapper.emitted('update')?.[0]?.[0]).toEqual({ lineHeight: 0.8 });

            // Maior que 2.5 -> clampa para 2.5
            await (lineHeightInput as any).vm.$emit('update:modelValue', 5.0);
            expect(wrapper.emitted('update')?.[1]?.[0]).toEqual({ lineHeight: 2.5 });
        });
    });
});
