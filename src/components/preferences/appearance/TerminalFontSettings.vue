<template>
    <div class="font-settings">
        <div class="font-settings__grid">
            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-font-preset">Preset de fonte monoespaçada</label>
                <MaxInputSelect id="mt-font-preset" :model-value="selectedPresetId" :options="presetOptions" option-label="label" option-value="value" option-name="label" :disabled="disabled" class="font-settings__select" @change="onPresetChange" />
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-font-family">Pilha CSS font-family</label>
                <MaxInputText id="mt-font-family" :model-value="appearance.fontFamily" :disabled="disabled" class="font-settings__input" @update:model-value="onFontFamilyChange" @change="onFontFamilyChange" />
                <MaxAlert v-if="fontError" :message="fontError" severity="error" class="font-settings__error" />
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-font-size">Tamanho da fonte ({{ appearance.fontSize }}px)</label>
                <MaxInputNumber id="mt-font-size" :model-value="appearance.fontSize" :min="8" :max="36" :step="1" :disabled="disabled" class="font-settings__input" @update:model-value="onFontSizeChange" @change="onFontSizeChange" />
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-line-height">Altura da linha ({{ appearance.lineHeight }})</label>
                <MaxInputNumber id="mt-line-height" :model-value="appearance.lineHeight" :min="0.8" :max="2.5" :step="0.05" :disabled="disabled" class="font-settings__input" @update:model-value="onLineHeightChange" @change="onLineHeightChange" />
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-cursor-style">Estilo do cursor</label>
                <MaxInputSelect id="mt-cursor-style" :model-value="appearance.cursorStyleOverride ?? 'theme'" :options="cursorStyleOptions" option-label="label" option-value="value" option-name="label" :disabled="disabled" class="font-settings__select" @change="onCursorStyleChange" />
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-contrast-ratio">Contraste mínimo ({{ appearance.minimumContrastRatio }}:1)</label>
                <MaxInputNumber id="mt-contrast-ratio" :model-value="appearance.minimumContrastRatio" :min="1" :max="21" :step="0.5" :disabled="disabled" class="font-settings__input" @update:model-value="onMinimumContrastChange" @change="onMinimumContrastChange" />
            </div>
        </div>

        <div class="font-settings__toggles">
            <MaxInputCheckbox id="mt-font-ligatures" :model-value="appearance.fontLigatures" :disabled="disabled" label="Habilitar ligaduras tipográficas no terminal (ex.: =>, !==, ->)" class="font-settings__checkbox" @update:model-value="onLigaturesToggle" @change="onLigaturesToggle" />
            <MaxInputCheckbox id="mt-cursor-blink" :model-value="appearance.cursorBlink" :disabled="disabled" label="Cursor piscante nas sessões interativas" class="font-settings__checkbox" @update:model-value="onCursorBlinkToggle" @change="onCursorBlinkToggle" />
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import {
        MaxAlert,
        MaxInputCheckbox,
        MaxInputNumber,
        MaxInputSelect,
        MaxInputText
    } from '@maxvue/max-components-ui';
    import {
        isValidFontFamilyStack,
        MONOSPACE_FONT_PRESETS
    } from '@/theme/fonts';
    import type {
        AppearanceSettings,
        AppearanceSettingsPatch,
        CursorStyle
    } from '@/types/theme';

    export interface TerminalFontSettingsProps {
        appearance: AppearanceSettings;
        disabled?: boolean;
    }

    const props = withDefaults(defineProps<TerminalFontSettingsProps>(), {
        disabled: false
    });

    const emit = defineEmits<{
        update: [patch: AppearanceSettingsPatch];
    }>();

    const fontError = ref<string | null>(null);

    const presetOptions = computed(() => [
        ...MONOSPACE_FONT_PRESETS.map((p) => ({ label: p.label, value: p.id })),
        { label: 'Pilha personalizada', value: 'custom' }
    ]);

    const cursorStyleOptions = [
        { label: 'Seguir esquema de cores', value: 'theme' },
        { label: 'Bloco (█)', value: 'block' },
        { label: 'Barra vertical (▏)', value: 'bar' },
        { label: 'Sublinhado (_)', value: 'underline' }
    ];

    const selectedPresetId = computed(() => {
        const matched = MONOSPACE_FONT_PRESETS.find(
            (p) => p.family === props.appearance.fontFamily
        );
        return matched ? matched.id : 'custom';
    });

    function onPresetChange(valOrEv: unknown): void {
        const presetId = typeof valOrEv === 'string' ? valOrEv : (valOrEv as { target?: HTMLSelectElement })?.target?.value;
        const preset = MONOSPACE_FONT_PRESETS.find((p) => p.id === presetId);
        if (preset) {
            fontError.value = null;
            emit('update', { fontFamily: preset.family });
        }
    }

    function onFontFamilyChange(valOrEv: unknown): void {
        const raw = typeof valOrEv === 'string' ? valOrEv : ((valOrEv as { target?: HTMLInputElement })?.target?.value ?? '');
        const value = raw.trim();
        if (!isValidFontFamilyStack(value)) {
            fontError.value = 'Pilha de fontes contém caracteres inválidos.';
            return;
        }
        fontError.value = null;
        emit('update', { fontFamily: value });
    }

    function onFontSizeChange(valOrEv: unknown): void {
        const raw = typeof valOrEv === 'number' ? valOrEv : Number.parseInt((valOrEv as { target?: HTMLInputElement })?.target?.value ?? String(valOrEv), 10);
        if (Number.isFinite(raw)) {
            const fontSize = Math.min(36, Math.max(8, raw));
            emit('update', { fontSize });
        }
    }

    function onLineHeightChange(valOrEv: unknown): void {
        const raw = typeof valOrEv === 'number' ? valOrEv : Number.parseFloat((valOrEv as { target?: HTMLInputElement })?.target?.value ?? String(valOrEv));
        if (Number.isFinite(raw)) {
            const lineHeight = Math.min(2.5, Math.max(0.8, Number(raw.toFixed(2))));
            emit('update', { lineHeight });
        }
    }

    function onCursorStyleChange(valOrEv: unknown): void {
        const val = typeof valOrEv === 'string' ? valOrEv : (valOrEv as { target?: HTMLSelectElement })?.target?.value;
        if (val === 'theme') {
            emit('update', { cursorStyleOverride: null });
        } else if (val) {
            emit('update', { cursorStyleOverride: val as CursorStyle });
        }
    }

    function onMinimumContrastChange(valOrEv: unknown): void {
        const raw = typeof valOrEv === 'number' ? valOrEv : Number.parseFloat((valOrEv as { target?: HTMLInputElement })?.target?.value ?? String(valOrEv));
        if (Number.isFinite(raw)) {
            const minimumContrastRatio = Math.min(21, Math.max(1, raw));
            emit('update', { minimumContrastRatio });
        }
    }

    function onLigaturesToggle(valOrEv: unknown): void {
        const fontLigatures = typeof valOrEv === 'boolean' ? valOrEv : Boolean((valOrEv as { target?: HTMLInputElement })?.target?.checked);
        emit('update', { fontLigatures });
    }

    function onCursorBlinkToggle(valOrEv: unknown): void {
        const cursorBlink = typeof valOrEv === 'boolean' ? valOrEv : Boolean((valOrEv as { target?: HTMLInputElement })?.target?.checked);
        emit('update', { cursorBlink });
    }
</script>

<style lang="scss" scoped>
    .font-settings {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
    }

    .font-settings__grid {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 0.625rem;
    }

    .font-settings__field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .font-settings__label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--background-300);
    }

    .font-settings__select,
    .font-settings__input {
        height: 2rem;
        padding: 0 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-950);
        color: var(--background-100);
        font-size: 0.8125rem;

        &:disabled {
            opacity: 0.45;
            cursor: not-allowed;
        }
    }

    .font-settings__error {
        font-size: 0.75rem;
        color: var(--max-danger-400);
    }

    .font-settings__toggles {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .font-settings__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.8125rem;
        color: var(--background-200);
        cursor: pointer;

        input {
            accent-color: var(--max-primary-500);
        }
    }
</style>
