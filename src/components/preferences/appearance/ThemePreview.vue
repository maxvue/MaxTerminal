<template>
    <div v-if="resolved" class="theme-preview" :style="terminalStyle" aria-label="Pré-visualização do tema de terminal">
        <div class="theme-preview__header">
            <span class="theme-preview__title">{{ resolved.scheme.name }}</span>
            <span class="theme-preview__info">{{ resolved.scheme.polarity === 'dark' ? 'Modo Escuro' : 'Modo Claro' }} · {{ appearance.fontFamily.split(',')[0] }} {{ appearance.fontSize }}px</span>
        </div>
        <div class="theme-preview__body">
            <div class="theme-preview__line">
                <span :style="{ color: resolved.xterm.green }">johnattas@cachyos</span>
                <span :style="{ color: resolved.xterm.foreground }">:</span>
                <span :style="{ color: resolved.xterm.blue }">~/GitHub/MaxTerminal</span>
                <span :style="{ color: resolved.xterm.magenta }"> (main)</span>
                <span :style="{ color: resolved.xterm.cyan }"> ❯ </span>
                <span :style="{ color: resolved.xterm.foreground }">cargo test --workspace</span>
            </div>
            <div class="theme-preview__line">
                <span :style="{ color: resolved.xterm.brightBlack }">   Compiling </span>
                <span :style="{ color: resolved.xterm.foreground }">mt-themes v0.1.0 (src-tauri/crates/mt-themes)</span>
            </div>
            <div class="theme-preview__line">
                <span :style="{ color: resolved.xterm.green }">test result: ok.</span>
                <span :style="{ color: resolved.xterm.foreground }"> 78 passed; </span>
                <span :style="{ color: resolved.xterm.yellow }">0 warnings; </span>
                <span :style="{ color: resolved.xterm.red }">0 failed</span>
                <span class="theme-preview__cursor" :style="{ backgroundColor: resolved.xterm.cursor }" />
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import type { AppearanceSettings, ResolvedTheme } from '@/types/theme';

    export interface ThemePreviewProps {
        resolved: ResolvedTheme | null;
        appearance: AppearanceSettings;
    }

    const props = defineProps<ThemePreviewProps>();

    const terminalStyle = computed(() => {
        if (!props.resolved) {
            return {};
        }
        return {
            backgroundColor: props.resolved.xterm.background,
            color: props.resolved.xterm.foreground,
            fontFamily: props.appearance.fontFamily,
            fontSize: `${props.appearance.fontSize}px`,
            lineHeight: String(props.appearance.lineHeight)
        };
    });
</script>

<style lang="scss" scoped>
    .theme-preview {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 0.75rem 0.875rem;
        border-radius: 0.5rem;
        border: 1px solid var(--background-700);
        overflow: hidden;
    }

    .theme-preview__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding-bottom: 0.375rem;
        border-bottom: 1px solid var(--background-800);
        font-family: var(--mt-font-sans);
        font-size: 0.75rem;
    }

    .theme-preview__title {
        font-weight: 600;
    }

    .theme-preview__info {
        opacity: 0.75;
    }

    .theme-preview__body {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .theme-preview__line {
        white-space: pre-wrap;
        word-break: break-all;
    }

    .theme-preview__cursor {
        display: inline-block;
        width: 0.5rem;
        height: 0.9375rem;
        margin-left: 0.25rem;
        vertical-align: text-bottom;
    }
</style>
