import type {
    BackgroundRamp,
    ColorScale11,
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
 * Converte `MaxUiTokens` em um dicionário plano de variáveis CSS (`--max-*`, `--background-*`, `--mt-term-*`).
 */
export function buildCssVarsRecord(tokens: MaxUiTokens): Record<string, string> {
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

    // Aliases semânticos locais de UI derivados da rampa canônica
    out['--mt-ui-surface'] = tokens.background.b0;
    out['--mt-ui-surface-panel'] = tokens.background.b25;
    out['--mt-ui-surface-elevated'] = tokens.background.b50;
    out['--mt-ui-surface-hover'] = tokens.background.b75;
    out['--mt-ui-surface-active'] = tokens.background.b100;
    out['--mt-ui-border-subtle'] = tokens.background.b75;
    out['--mt-ui-border'] = tokens.background.b100;
    out['--mt-ui-border-strong'] = tokens.background.b150;
    out['--mt-ui-text-muted'] = tokens.background.b400;
    out['--mt-ui-text-secondary'] = tokens.background.b600;
    out['--mt-ui-text'] = tokens.background.b800;
    out['--mt-ui-text-strong'] = tokens.background.b950;

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
    rootEl?: HTMLElement | null
): Record<string, string> {
    const vars = buildCssVarsRecord(tokens);
    const target =
        rootEl ?? (typeof document !== 'undefined' ? document.documentElement : null);
    if (target) {
        applyCssVarsRecordToElement(target, tokens.polarity, vars);
    }
    return vars;
}
