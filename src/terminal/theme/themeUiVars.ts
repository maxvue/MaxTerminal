import type { TerminalTheme } from '@/types/terminal';

/**
 * Converte o bloco `ui` de um `TerminalTheme` em variáveis CSS locais do painel `.hybrid-terminal`.
 */
export function themeUiToCssVars(ui: TerminalTheme['ui']): Record<string, string> {
    return {
        '--mt-block-background': ui.blockBackground,
        '--mt-block-border': ui.blockBorder,
        '--mt-block-header-background': ui.blockHeaderBackground,
        '--mt-rich-input-background': ui.richInputBackground,
        '--mt-rich-input-border': ui.richInputBorder
    };
}
