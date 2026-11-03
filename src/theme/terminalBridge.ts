import type { ITheme } from '@xterm/xterm';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { themeService } from '@/terminal/theme/themeService';
import type { TerminalTheme } from '@/types/terminal';
import type {
    AppearanceSettings,
    CursorStyle,
    ResolvedTheme,
    XtermThemePayload
} from '@/types/theme';

/**
 * Converte `XtermThemePayload` vindo do Rust para o objeto `ITheme` do `@xterm/xterm`.
 */
export function toXtermITheme(payload: XtermThemePayload): ITheme {
    const out: ITheme = {
        foreground: payload.foreground,
        background: payload.background,
        cursor: payload.cursor,
        cursorAccent: payload.cursorAccent,
        selectionBackground: payload.selectionBackground,
        selectionInactiveBackground: payload.selectionInactiveBackground,
        black: payload.black,
        red: payload.red,
        green: payload.green,
        yellow: payload.yellow,
        blue: payload.blue,
        magenta: payload.magenta,
        cyan: payload.cyan,
        white: payload.white,
        brightBlack: payload.brightBlack,
        brightRed: payload.brightRed,
        brightGreen: payload.brightGreen,
        brightYellow: payload.brightYellow,
        brightBlue: payload.brightBlue,
        brightMagenta: payload.brightMagenta,
        brightCyan: payload.brightCyan,
        brightWhite: payload.brightWhite
    };

    if (payload.selectionForeground) {
        out.selectionForeground = payload.selectionForeground;
    }

    if (payload.extendedAnsi) {
        out.extendedAnsi = Object.entries(payload.extendedAnsi)
            .sort(([a], [b]) => Number(a) - Number(b))
            .map(([, color]) => color ?? '#000000');
    }

    return out;
}

/**
 * Converte `ResolvedTheme` para `TerminalTheme` consumido pelo `themeService` da Fase 04.
 */
export function toTerminalTheme(resolved: ResolvedTheme): TerminalTheme {
    const xterm = resolved.xterm;
    const ansi: TerminalTheme['colors']['ansi'] = [
        xterm.black,
        xterm.red,
        xterm.green,
        xterm.yellow,
        xterm.blue,
        xterm.magenta,
        xterm.cyan,
        xterm.white,
        xterm.brightBlack,
        xterm.brightRed,
        xterm.brightGreen,
        xterm.brightYellow,
        xterm.brightBlue,
        xterm.brightMagenta,
        xterm.brightCyan,
        xterm.brightWhite
    ];

    const colors: TerminalTheme['colors'] = {
        foreground: xterm.foreground,
        background: xterm.background,
        cursor: xterm.cursor,
        cursorAccent: xterm.cursorAccent,
        selectionBackground: xterm.selectionBackground,
        selectionInactiveBackground: xterm.selectionInactiveBackground,
        ansi
    };

    if (xterm.selectionForeground) {
        colors.selectionForeground = xterm.selectionForeground;
    }

    return {
        id: resolved.scheme.id,
        name: resolved.scheme.name,
        source:
            resolved.scheme.origin === 'builtin'
                ? 'builtin'
                : resolved.scheme.origin === 'imported'
                    ? 'tabby'
                    : 'custom',
        colors,
        ui: {
            blockBackground: resolved.uiTokens.background.b0,
            blockBorder: resolved.uiTokens.background.b100,
            blockHeaderBackground: resolved.uiTokens.background.b25,
            richInputBackground: resolved.uiTokens.background.b25,
            richInputBorder: resolved.uiTokens.background.b100
        }
    };
}

/**
 * Resolve o estilo efetivo do cursor combinando `appearance.cursorStyleOverride` e `scheme.cursorStyle`.
 */
export function resolveEffectiveCursorStyle(
    resolved: ResolvedTheme,
    appearance: AppearanceSettings | null
): 'block' | 'underline' | 'bar' {
    const style: CursorStyle =
        appearance?.cursorStyleOverride ?? resolved.scheme.cursorStyle;
    switch (style) {
        case 'underline':
            return 'underline';
        case 'bar':
            return 'bar';
        default:
            return 'block';
    }
}

let lastTypographySignature = '';

/**
 * Propaga ao vivo um `ResolvedTheme` e as configurações tipográficas (`AppearanceSettings`)
 * para todas as instâncias ativas de `@xterm/xterm`, sem recriar PTYs ou perder scrollback.
 */
export function applyThemeAndTypographyToTerminals(
    resolved: ResolvedTheme,
    appearance: AppearanceSettings | null
): void {
    const terminalTheme = toTerminalTheme(resolved);
    themeService.notifyThemeChanged(terminalTheme);

    const effectiveCursorStyle = resolveEffectiveCursorStyle(resolved, appearance);
    const typographySignature = appearance
        ? `${appearance.fontFamily}|${appearance.fontSize}|${appearance.lineHeight}|${appearance.fontLigatures}`
        : '';
    const typographyChanged =
        Boolean(appearance) && typographySignature !== lastTypographySignature;
    if (appearance) {
        lastTypographySignature = typographySignature;
    }

    terminalInstanceRegistry.forEach((inst) => {
        inst.applyTheme(terminalTheme);
        if (appearance) {
            inst.applyPreferences({
                fontFamily: appearance.fontFamily,
                fontSize: appearance.fontSize,
                lineHeight: appearance.lineHeight ?? 1.2,
                cursorStyle: effectiveCursorStyle,
                cursorBlink: appearance.cursorBlink
            });
            inst.xterm.options.minimumContrastRatio =
                appearance.minimumContrastRatio ?? 1;
        }
        if (typographyChanged) {
            inst.xterm.clearTextureAtlas?.();
        }
    });
}
