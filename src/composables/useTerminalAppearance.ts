import { computed } from 'vue';
import { useThemeStore } from '@/stores/theme';
import { toXtermITheme } from '@/theme/terminalBridge';

/**
 * Composable que expõe reativamente o tema xterm.js e opções tipográficas/cursor
 * sincronizadas com a `useThemeStore`.
 */
export function useTerminalAppearance() {
    const themeStore = useThemeStore();

    const xtermTheme = computed(() => {
        const resolved = themeStore.effectiveResolved;
        return resolved ? toXtermITheme(resolved.xterm) : null;
    });

    const fontOptions = computed(() => {
        const app = themeStore.appearance;
        const resolved = themeStore.effectiveResolved;
        return {
            fontFamily: app.fontFamily,
            fontSize: app.fontSize,
            lineHeight: app.lineHeight,
            fontLigatures: app.fontLigatures,
            cursorBlink: app.cursorBlink,
            cursorStyle: app.cursorStyleOverride ?? resolved?.scheme.cursorStyle ?? 'block',
            minimumContrastRatio: app.minimumContrastRatio
        };
    });

    return {
        xtermTheme,
        fontOptions
    };
}
