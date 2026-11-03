<template>
    <div class="font-settings">
        <div class="font-settings__grid">
            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-font-preset">Preset de fonte monoespaçada</label>
                <select id="mt-font-preset" class="font-settings__select" :value="selectedPresetId" :disabled="disabled" @change="onPresetChange">
                    <option v-for="preset in MONOSPACE_FONT_PRESETS" :key="preset.id" :value="preset.id">
                        {{ preset.label }}
                    </option>
                    <option value="custom">Pilha personalizada</option>
                </select>
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-font-family">Pilha CSS font-family</label>
                <input id="mt-font-family" type="text" class="font-settings__input" :value="appearance.fontFamily" :disabled="disabled" @change="onFontFamilyChange">
                <span v-if="fontError" class="font-settings__error">{{ fontError }}</span>
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-font-size">Tamanho da fonte ({{ appearance.fontSize }}px)</label>
                <input id="mt-font-size" type="number" min="8" max="36" step="1" class="font-settings__input" :value="appearance.fontSize" :disabled="disabled" @change="onFontSizeChange">
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-line-height">Altura da linha ({{ appearance.lineHeight }})</label>
                <input id="mt-line-height" type="number" min="0.8" max="2.5" step="0.05" class="font-settings__input" :value="appearance.lineHeight" :disabled="disabled" @change="onLineHeightChange">
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-cursor-style">Estilo do cursor</label>
                <select id="mt-cursor-style" class="font-settings__select" :value="appearance.cursorStyleOverride ?? 'theme'" :disabled="disabled" @change="onCursorStyleChange">
                    <option value="theme">Seguir esquema de cores</option>
                    <option value="block">Bloco (█)</option>
                    <option value="bar">Barra vertical (▏)</option>
                    <option value="underline">Sublinhado (_)</option>
                </select>
            </div>

            <div class="font-settings__field">
                <label class="font-settings__label" for="mt-contrast-ratio">Contraste mínimo ({{ appearance.minimumContrastRatio }}:1)</label>
                <input id="mt-contrast-ratio" type="number" min="1" max="21" step="0.5" class="font-settings__input" :value="appearance.minimumContrastRatio" :disabled="disabled" @change="onMinimumContrastChange">
            </div>
        </div>

        <div class="font-settings__toggles">
            <label class="font-settings__checkbox">
                <input type="checkbox" :checked="appearance.fontLigatures" :disabled="disabled" @change="onLigaturesToggle">
                <span>Habilitar ligaduras tipográficas no terminal (ex.: =&gt;, !==, -&gt;)</span>
            </label>
            <label class="font-settings__checkbox">
                <input type="checkbox" :checked="appearance.cursorBlink" :disabled="disabled" @change="onCursorBlinkToggle">
                <span>Cursor piscante nas sessões interativas</span>
            </label>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
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

    const selectedPresetId = computed(() => {
        const matched = MONOSPACE_FONT_PRESETS.find(
            (p) => p.family === props.appearance.fontFamily
        );
        return matched ? matched.id : 'custom';
    });

    function onPresetChange(ev: Event): void {
        const presetId = (ev.target as HTMLSelectElement).value;
        const preset = MONOSPACE_FONT_PRESETS.find((p) => p.id === presetId);
        if (preset) {
            fontError.value = null;
            emit('update', { fontFamily: preset.family });
        }
    }

    function onFontFamilyChange(ev: Event): void {
        const value = (ev.target as HTMLInputElement).value.trim();
        if (!isValidFontFamilyStack(value)) {
            fontError.value = 'Pilha de fontes contém caracteres inválidos.';
            return;
        }
        fontError.value = null;
        emit('update', { fontFamily: value });
    }

    function onFontSizeChange(ev: Event): void {
        const raw = Number.parseInt((ev.target as HTMLInputElement).value, 10);
        if (Number.isFinite(raw)) {
            const fontSize = Math.min(36, Math.max(8, raw));
            emit('update', { fontSize });
        }
    }

    function onLineHeightChange(ev: Event): void {
        const raw = Number.parseFloat((ev.target as HTMLInputElement).value);
        if (Number.isFinite(raw)) {
            const lineHeight = Math.min(2.5, Math.max(0.8, Number(raw.toFixed(2))));
            emit('update', { lineHeight });
        }
    }

    function onCursorStyleChange(ev: Event): void {
        const val = (ev.target as HTMLSelectElement).value;
        if (val === 'theme') {
            emit('update', { cursorStyleOverride: null });
        } else {
            emit('update', { cursorStyleOverride: val as CursorStyle });
        }
    }

    function onMinimumContrastChange(ev: Event): void {
        const raw = Number.parseFloat((ev.target as HTMLInputElement).value);
        if (Number.isFinite(raw)) {
            const minimumContrastRatio = Math.min(21, Math.max(1, raw));
            emit('update', { minimumContrastRatio });
        }
    }

    function onLigaturesToggle(ev: Event): void {
        const fontLigatures = (ev.target as HTMLInputElement).checked;
        emit('update', { fontLigatures });
    }

    function onCursorBlinkToggle(ev: Event): void {
        const cursorBlink = (ev.target as HTMLInputElement).checked;
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
