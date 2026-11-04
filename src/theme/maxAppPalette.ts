import type { PartialColorPalette } from '@maxvue/max-components-ui';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import type {
    ColorScheme,
    MaxUiTokens,
    ResolvedTheme,
    ThemeBootstrapSnapshot
} from '@/types/theme';

/**
 * Converte `MaxUiTokens` e opcionalmente o `ColorScheme` canônico em `PartialColorPalette`
 * (cores-base) compatível com `<MaxApp controlled-theme :colors="...">`.
 */
export function buildMaxAppPaletteFromUiTokens(
    tokens: MaxUiTokens,
    scheme?: ColorScheme | null
): PartialColorPalette {
    const palette: PartialColorPalette = {
        primary: tokens.primary.p500,
        secondary: tokens.info.p500,
        success: tokens.success.p500,
        danger: tokens.danger.p500,
        info: tokens.info.p500,
        warn: tokens.warning.p500,
        error: tokens.danger.p500,
        background: scheme?.background ?? tokens.background.b900
    };

    if (scheme?.ansi) {
        palette.red = scheme.ansi.red;
        palette.green = scheme.ansi.green;
        palette.yellow = scheme.ansi.yellow;
        palette.blue = scheme.ansi.blue;
        palette.violet = scheme.ansi.magenta;
        palette.black = scheme.ansi.black;
        palette.white = scheme.ansi.white;
    }

    return palette;
}

/**
 * Converte um `ThemeBootstrapSnapshot` persistido em `localStorage` em `PartialColorPalette`
 * para evitar FOUC enquanto o IPC inicial ainda não retornou.
 */
export function buildMaxAppPaletteFromBootstrap(
    snapshot: ThemeBootstrapSnapshot | null | undefined
): PartialColorPalette | undefined {
    if (!snapshot?.cssVars) {
        return undefined;
    }
    const { cssVars } = snapshot;
    const palette: PartialColorPalette = {};

    const primary = cssVars['--max-primary-500'];
    if (primary) {
        palette.primary = primary;
    }
    const info = cssVars['--max-info-500'];
    if (info) {
        palette.info = info;
        palette.secondary = info;
    }
    const success = cssVars['--max-success-500'];
    if (success) {
        palette.success = success;
    }
    const warning = cssVars['--max-warning-500'];
    if (warning) {
        palette.warn = warning;
    }
    const danger = cssVars['--max-danger-500'];
    if (danger) {
        palette.danger = danger;
        palette.error = danger;
    }
    const background = cssVars['--mt-term-bg'] ?? cssVars['--background-900'] ?? cssVars['--background-50'];
    if (background) {
        palette.background = background;
    }
    const text = cssVars['--mt-term-fg'] ?? cssVars['--mt-ui-text'];
    if (text) {
        palette.text = text;
    }
    const border = cssVars['--mt-ui-border'];
    if (border) {
        palette.border = border;
    }
    const violet = cssVars['--max-purple-500'];
    if (violet) {
        palette.violet = violet;
    }

    return Object.keys(palette).length > 0 ? palette : undefined;
}

/**
 * Resolve o `PartialColorPalette` efetivo para `<MaxApp :colors="...">` priorizando:
 * 1. `effectiveResolved` (`uiTokens` + `scheme` do tema ativo/preview)
 * 2. Snapshot síncrono de bootstrap (`localStorage`)
 * 3. Tokens canônicos de fallback (`Max Navy/Teal`)
 */
export function resolveMaxAppColors(
    resolved: ResolvedTheme | null | undefined,
    bootstrapSnapshot?: ThemeBootstrapSnapshot | null
): PartialColorPalette {
    if (resolved?.uiTokens) {
        return buildMaxAppPaletteFromUiTokens(resolved.uiTokens, resolved.scheme);
    }
    const fromBootstrap = buildMaxAppPaletteFromBootstrap(bootstrapSnapshot);
    if (fromBootstrap) {
        return {
            ...buildMaxAppPaletteFromUiTokens(FALLBACK_MAX_NAVY_TEAL_TOKENS),
            ...fromBootstrap
        };
    }
    return buildMaxAppPaletteFromUiTokens(FALLBACK_MAX_NAVY_TEAL_TOKENS);
}
