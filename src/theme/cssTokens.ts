import { getColorFromVar } from '@maxvue/max-use';
import type {
    BackgroundRamp,
    ColorScale11,
    CssVarsContext,
    MaxUiTokens
} from '@/types/theme';

const SCALE_STEPS = [
    ['50', 'p50'],
    ['100', 'p100'],
    ['200', 'p200'],
    ['300', 'p300'],
    ['400', 'p400'],
    ['500', 'p500'],
    ['600', 'p600'],
    ['700', 'p700'],
    ['800', 'p800'],
    ['900', 'p900'],
    ['950', 'p950']
] as const;

const BACKGROUND_STEPS: ReadonlyArray<readonly [number, keyof BackgroundRamp]> = [
    [0, 'b0'],
    [25, 'b25'],
    [50, 'b50'],
    [75, 'b75'],
    [100, 'b100'],
    [125, 'b125'],
    [150, 'b150'],
    [175, 'b175'],
    [200, 'b200'],
    [225, 'b225'],
    [250, 'b250'],
    [275, 'b275'],
    [300, 'b300'],
    [325, 'b325'],
    [350, 'b350'],
    [375, 'b375'],
    [400, 'b400'],
    [425, 'b425'],
    [450, 'b450'],
    [475, 'b475'],
    [500, 'b500'],
    [525, 'b525'],
    [550, 'b550'],
    [575, 'b575'],
    [600, 'b600'],
    [625, 'b625'],
    [650, 'b650'],
    [675, 'b675'],
    [700, 'b700'],
    [725, 'b725'],
    [750, 'b750'],
    [775, 'b775'],
    [800, 'b800'],
    [825, 'b825'],
    [850, 'b850'],
    [875, 'b875'],
    [900, 'b900'],
    [925, 'b925'],
    [950, 'b950']
] as const;

/** Cores magenta canônicas do tema padrão Max Navy/Teal para fallback síncrono. */
const DEFAULT_CANONICAL_MAGENTA = '#a855f7';
const DEFAULT_CANONICAL_BRIGHT_MAGENTA = '#c084fc';

function appendScaleVars(
    target: Record<string, string>,
    prefix: string,
    scale: ColorScale11
): void {
    for (const [step, prop] of SCALE_STEPS) {
        target[`--max-${prefix}-${step}`] = scale[prop];
    }
    target[`--max-${prefix}-content`] = scale.content;
}

/**
 * Seleciona cor da rampa que atende à razão mínima de contraste WCAG em relação à superfície.
 */
function getSafeContrastColor(
    fgHex: string,
    bgHex: string,
    minRatio: number,
    candidateHexes: readonly string[]
): string {
    try {
        const bg = getColorFromVar(bgHex);
        const fg = getColorFromVar(fgHex);
        if (fg.contrast(bg) >= minRatio) {
            return fgHex;
        }
        for (const candidate of candidateHexes) {
            const cand = getColorFromVar(candidate);
            if (cand.contrast(bg) >= minRatio) {
                return candidate;
            }
        }
        return bg.isDark() ? '#ffffff' : '#000000';
    } catch {
        return fgHex;
    }
}

/**
 * Converte `MaxUiTokens` em um dicionário plano de variáveis CSS (`--max-*`, `--background-*`, `--mt-term-*`, `--mt-ui-*`).
 */
export function buildCssVarsRecord(
    tokens: MaxUiTokens,
    context?: CssVarsContext
): Record<string, string> {
    const out: Record<string, string> = {};

    appendScaleVars(out, 'primary', tokens.primary);
    appendScaleVars(out, 'success', tokens.success);
    appendScaleVars(out, 'warning', tokens.warning);
    appendScaleVars(out, 'danger', tokens.danger);
    appendScaleVars(out, 'info', tokens.info);

    for (const [stepNum, prop] of BACKGROUND_STEPS) {
        out[`--background-${stepNum}`] = tokens.background[prop];
    }

    out['--mt-term-bg'] = tokens.terminal.bg;
    out['--mt-term-fg'] = tokens.terminal.fg;
    out['--mt-term-cursor'] = tokens.terminal.cursor;
    out['--mt-term-cursor-accent'] = tokens.terminal.cursorAccent;
    out['--mt-term-selection-bg'] = tokens.terminal.selectionBg;
    out['--mt-term-selection-fg'] = tokens.terminal.selectionFg ?? '';
    out['--mt-term-tab-active-accent'] = tokens.terminal.tabActiveAccent;
    out['--mt-term-scrollbar-thumb'] = tokens.terminal.scrollbarThumb;
    out['--mt-term-find-match-bg'] = tokens.terminal.findMatchBg;
    out['--mt-term-block-bg'] = tokens.terminal.blockBg;
    out['--mt-term-block-border'] = tokens.terminal.blockBorder;
    out['--mt-term-block-error-bg'] = tokens.terminal.blockErrorBg;

    const isDark = tokens.polarity === 'dark';
    const bgSurface = tokens.background.b0;

    // Aliases semânticos locais de UI revisados para polaridade e contraste nas duas orientações
    if (isDark) {
        out['--mt-ui-surface'] = tokens.background.b0;
        out['--mt-ui-surface-panel'] = tokens.background.b25;
        out['--mt-ui-surface-elevated'] = tokens.background.b50;
        out['--mt-ui-surface-hover'] = tokens.background.b75;
        out['--mt-ui-surface-active'] = tokens.background.b100;
        out['--mt-ui-border-subtle'] = tokens.background.b75;
        out['--mt-ui-border'] = tokens.background.b100;
        out['--mt-ui-border-strong'] = tokens.background.b150;
        out['--mt-ui-text-muted'] = getSafeContrastColor(
            tokens.background.b400,
            bgSurface,
            3.0,
            [tokens.background.b500, tokens.background.b600]
        );
        out['--mt-ui-text-secondary'] = getSafeContrastColor(
            tokens.background.b600,
            bgSurface,
            4.5,
            [tokens.background.b700, tokens.background.b800]
        );
        out['--mt-ui-text'] = getSafeContrastColor(
            tokens.background.b800,
            bgSurface,
            4.5,
            [tokens.background.b900, tokens.background.b950]
        );
        out['--mt-ui-text-strong'] = tokens.background.b950;
    } else {
        out['--mt-ui-surface'] = tokens.background.b0;
        out['--mt-ui-surface-panel'] = tokens.background.b25;
        out['--mt-ui-surface-elevated'] = tokens.background.b50;
        out['--mt-ui-surface-hover'] = tokens.background.b150;
        out['--mt-ui-surface-active'] = tokens.background.b200;
        out['--mt-ui-border-subtle'] = tokens.background.b200;
        out['--mt-ui-border'] = tokens.background.b300;
        out['--mt-ui-border-strong'] = tokens.background.b450;
        out['--mt-ui-text-muted'] = getSafeContrastColor(
            tokens.background.b700,
            bgSurface,
            3.0,
            [tokens.background.b750, tokens.background.b800]
        );
        out['--mt-ui-text-secondary'] = getSafeContrastColor(
            tokens.background.b800,
            bgSurface,
            4.5,
            [tokens.background.b850, tokens.background.b900, tokens.background.b950]
        );
        out['--mt-ui-text'] = getSafeContrastColor(
            tokens.background.b900,
            bgSurface,
            4.5,
            [tokens.background.b950, tokens.background.b925]
        );
        out['--mt-ui-text-strong'] = tokens.background.b950;
    }

    // Categoria de Issues e Roxo do tema (Magenta ANSI do esquema com fallback canônico)
    const magenta =
        context?.scheme?.ansi?.magenta ?? context?.ansi?.magenta ?? DEFAULT_CANONICAL_MAGENTA;
    const brightMagenta =
        context?.scheme?.ansi?.brightMagenta ??
        context?.ansi?.brightMagenta ??
        DEFAULT_CANONICAL_BRIGHT_MAGENTA;

    let purple300 = brightMagenta;
    let purple100 = isDark ? '#f3e8ff' : '#581c87';
    try {
        const mColor = getColorFromVar(brightMagenta);
        purple300 = isDark ? mColor.lighten(0.12).hex() : mColor.darken(0.12).hex();
        purple100 = isDark ? mColor.lighten(0.35).hex() : mColor.darken(0.35).hex();
    } catch {
        // Mantém fallback seguro
    }

    out['--max-purple-500'] = magenta;
    out['--max-purple-400'] = brightMagenta;
    out['--max-purple-300'] = purple300;
    out['--max-purple-100'] = purple100;
    out['--mt-ui-issue-category'] = magenta;
    out['--mt-ui-issue-category-hover'] = brightMagenta;
    out['--mt-ui-issue-subtle'] = `color-mix(in srgb, ${magenta} 18%, transparent)`;

    // Tokens semânticos e formulários da biblioteca MaxComponentsUi
    out['--max-inputtext-border-color'] = out['--mt-ui-border'];
    out['--max-inputtext-disabled-background'] = isDark ? tokens.background.b75 : tokens.background.b150;
    out['--max-inputtext-focus-border-color'] = tokens.primary.p500;
    out['--max-form-field-disabled-background'] = isDark ? tokens.background.b75 : tokens.background.b150;
    out['--max-floatlabel-on-active-background'] = tokens.background.b0;
    out['--max-surface-400'] = out['--mt-ui-text-muted'];

    // Cores de borda dos botões MaxButton
    out['--max-button-primary-border-color'] = tokens.primary.p500;
    out['--max-button-secondary-border-color'] = out['--mt-ui-border'];
    out['--max-button-info-border-color'] = tokens.info.p500;
    out['--max-button-success-border-color'] = tokens.success.p500;
    out['--max-button-warn-border-color'] = tokens.warning.p500;
    out['--max-button-help-border-color'] = magenta;
    out['--max-button-danger-border-color'] = tokens.danger.p500;
    out['--max-button-contrast-border-color'] = isDark ? '#ffffff' : '#020617';

    // Foco canônico acessível (WCAG 2.4.7 / 2.4.11)
    out['--max-focus-ring-color'] = tokens.primary.p400;
    out['--max-focus-ring-offset-color'] = tokens.background.b0;
    out['--max-focus-ring'] = `0 0 0 2px ${tokens.background.b0}, 0 0 0 4px ${tokens.primary.p400}`;
    out['--max-focus-outline'] = `2px solid ${tokens.primary.p400}`;

    // Conteúdo / foreground de ações sólidas
    out['--max-primary-content'] = tokens.primary.content;
    out['--max-secondary-content'] = isDark ? '#ffffff' : '#00152a';
    out['--max-success-content'] = tokens.success.content;
    out['--max-info-content'] = tokens.info.content;
    out['--max-warning-content'] = tokens.warning.content;
    out['--max-danger-content'] = tokens.danger.content;
    out['--max-help-content'] = '#ffffff';
    out['--max-contrast-content'] = isDark ? '#020617' : '#ffffff';

    // Hierarquia textual por papel semântico
    out['--max-content-secondary'] = out['--mt-ui-text-secondary'];
    out['--max-content-placeholder'] = out['--mt-ui-text-muted'];
    out['--max-content-help'] = out['--mt-ui-text-muted'];
    out['--max-content-disabled'] = out['--mt-ui-border-strong'];

    // Tokens semânticos de seleção acessível
    out['--max-selection-background'] = tokens.terminal.selectionBg || tokens.primary.p600;
    out['--max-selection-content'] = tokens.terminal.selectionFg || '#ffffff';
    out['--max-selection-hover-background'] = tokens.primary.p500;
    out['--max-selection-hover-content'] = '#ffffff';

    // Tokens semânticos de tabela
    out['--max-table-header-bg'] = isDark ? tokens.background.b50 : tokens.background.b100;
    out['--max-table-header-text'] = out['--mt-ui-text-strong'];
    out['--max-table-row-even-bg'] = tokens.background.b0;
    out['--max-table-row-odd-bg'] = tokens.background.b25;
    out['--max-table-row-selected-bg'] = isDark ? tokens.background.b75 : tokens.background.b150;
    out['--max-table-border-color'] = out['--mt-ui-border-subtle'];

    // Papéis de destaque e realce suave
    out['--mt-ui-highlight-bg'] = `color-mix(in srgb, ${tokens.primary.p500} 24%, transparent)`;
    out['--mt-ui-highlight-text'] = isDark ? tokens.primary.p300 : tokens.primary.p700;
    out['--mt-ui-primary-subtle'] = `color-mix(in srgb, ${tokens.primary.p500} 15%, transparent)`;
    out['--mt-ui-success-subtle'] = `color-mix(in srgb, ${tokens.success.p500} 15%, transparent)`;
    out['--mt-ui-warning-subtle'] = `color-mix(in srgb, ${tokens.warning.p500} 15%, transparent)`;
    out['--mt-ui-danger-subtle'] = `color-mix(in srgb, ${tokens.danger.p500} 15%, transparent)`;
    out['--mt-ui-info-subtle'] = `color-mix(in srgb, ${tokens.info.p500} 15%, transparent)`;

    // Sombra e overlays modais
    out['--mt-ui-overlay'] = isDark ? 'rgba(0, 0, 0, 0.65)' : 'rgba(15, 23, 42, 0.45)';
    out['--mt-ui-shadow'] = isDark ? '0 8px 24px rgba(0, 0, 0, 0.45)' : '0 8px 24px rgba(15, 23, 42, 0.12)';
    out['--mt-ui-shadow-lg'] = isDark ? '0 18px 48px rgba(0, 0, 0, 0.55)' : '0 18px 48px rgba(15, 23, 42, 0.18)';

    return out;
}

/**
 * Aplica em lote um mapa de variáveis CSS e a polaridade (`dark` / `light`) ao elemento raiz (`document.documentElement`).
 */
export function applyCssVarsRecordToElement(
    el: HTMLElement,
    polarity: 'dark' | 'light',
    vars: Record<string, string>
): void {
    if (polarity === 'dark') {
        el.classList.add('dark');
    } else {
        el.classList.remove('dark');
    }
    el.dataset.themePolarity = polarity;
    el.style.colorScheme = polarity;

    for (const [name, value] of Object.entries(vars)) {
        if (value === '') {
            el.style.removeProperty(name);
        } else {
            el.style.setProperty(name, value);
        }
    }
}

/**
 * Aplica diretamente um `MaxUiTokens` ao elemento raiz informado (ou `document.documentElement`).
 */
export function applyMaxUiTokensToRoot(
    tokens: MaxUiTokens,
    rootEl?: HTMLElement | null,
    context?: CssVarsContext
): Record<string, string> {
    const vars = buildCssVarsRecord(tokens, context);
    const target =
        rootEl ?? (typeof document !== 'undefined' ? document.documentElement : null);
    if (target) {
        applyCssVarsRecordToElement(target, tokens.polarity, vars);
    }
    return vars;
}
