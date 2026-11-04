<template>
    <div class="font-settings">
        <div class="font-settings__grid">
            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-font-preset">Preset de fonte monoespaçada</label>
                <MaxInputSelect id="mt-font-preset" aria-label="Preset de fonte monoespaçada" :model-value="selectedPresetId" :options="presetOptions" option-label="label" option-value="value" option-name="label" :disabled="disabled" class="font-settings__select" @change="onPresetChange" />
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-font-family">Pilha CSS font-family</label>
                <MaxInputText id="mt-font-family" :model-value="appearance.fontFamily" :disabled="disabled" class="font-settings__input" @update:model-value="onFontFamilyChange" @change="onFontFamilyChange" />
                <MaxAlert v-if="fontError" :message="fontError" severity="danger" class="font-settings__error" />
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-font-size">Tamanho da fonte ({{ appearance.fontSize }}px)</label>
                <MaxInputNumber id="mt-font-size" :model-value="appearance.fontSize" :min="8" :max="36" :step="1" :disabled="disabled" class="font-settings__input" @update:model-value="onFontSizeChange" />
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-line-height">Altura da linha ({{ appearance.lineHeight }})</label>
                <MaxInputNumber id="mt-line-height" :model-value="appearance.lineHeight" :min="0.8" :max="2.5" :step="0.05" :disabled="disabled" class="font-settings__input" @update:model-value="onLineHeightChange" />
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-cursor-style">Estilo do cursor</label>
                <MaxInputSelect id="mt-cursor-style" aria-label="Estilo do cursor" :model-value="appearance.cursorStyleOverride ?? 'theme'" :options="cursorStyleOptions" option-label="label" option-value="value" option-name="label" :disabled="disabled" class="font-settings__select" @change="onCursorStyleChange" />
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-contrast-ratio">Contraste mínimo ({{ appearance.minimumContrastRatio }}:1)</label>
                <MaxInputNumber id="mt-contrast-ratio" :model-value="appearance.minimumContrastRatio" :min="1" :max="21" :step="0.5" :disabled="disabled" class="font-settings__input" @update:model-value="onMinimumContrastChange" />
            </div>
        </div>

        <div class="font-settings__toggles">
            <MaxInputCheckbox :model-value="appearance.fontLigatures" :disabled="Boolean(disabled)" label="Habilitar ligaduras tipográficas no terminal (ex.: =>, !==, ->)" class="font-settings__checkbox" @update:model-value="onLigaturesToggle" />
            <MaxInputCheckbox :model-value="appearance.cursorBlink" :disabled="Boolean(disabled)" label="Cursor piscante nas sessões interativas" class="font-settings__checkbox" @update:model-value="onCursorBlinkToggle" />
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

    function parseLocalizedNumber(valOrEv: unknown): number | null {
        if (typeof valOrEv === 'number') {
            return Number.isFinite(valOrEv) ? valOrEv : null;
        }
        const rawStr = typeof valOrEv === 'string'
            ? valOrEv
            : ((valOrEv as { target?: HTMLInputElement })?.target?.value ?? String(valOrEv ?? ''));
        const trimmed = rawStr.trim();
        if (!trimmed) {
            return null;
        }

        let sanitized = trimmed;
        if (sanitized.includes(',') && sanitized.includes('.')) {
            sanitized = sanitized.replace(/\./g, '').replace(',', '.');
        } else if (sanitized.includes(',')) {
            sanitized = sanitized.replace(',', '.');
        }

        const parsed = Number.parseFloat(sanitized);
        return Number.isFinite(parsed) ? parsed : null;
    }

    function onFontSizeChange(valOrEv: unknown): void {
        const raw = parseLocalizedNumber(valOrEv);
        if (raw !== null) {
            const rounded = Math.round(raw);
            const fontSize = Math.min(36, Math.max(8, rounded));
            emit('update', { fontSize });
        }
    }

    function onLineHeightChange(valOrEv: unknown): void {
        const raw = parseLocalizedNumber(valOrEv);
        if (raw !== null) {
            const clamped = Math.min(2.5, Math.max(0.8, raw));
            const snapped = Math.round(clamped / 0.05) * 0.05;
            const lineHeight = Number(snapped.toFixed(2));
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
        const raw = parseLocalizedNumber(valOrEv);
        if (raw !== null) {
            const clamped = Math.min(21, Math.max(1, raw));
            const snapped = Math.round(clamped / 0.5) * 0.5;
            const minimumContrastRatio = Number(snapped.toFixed(1));
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
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .font-settings__select,
    .font-settings__input {
        height: 2rem;
        padding: 0 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
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
        color: var(--mt-ui-text-secondary, var(--background-600));
        cursor: pointer;

        input {
            accent-color: var(--max-primary-500);
        }
    }
</style>
