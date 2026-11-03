/**
 * Suíte E2E Frontend (Camada A — CA-27):
 * Valida abertura da Paleta de Busca Global via Ctrl+Shift+F, debounce,
 * cancelamento de busca em andamento, navegação por teclado, chips de filtro
 * e renderização de snippets com `<mark>` sanitizado.
 */
export interface FrontendSearchScenario {
    readonly id: string;
    readonly description: string;
    readonly shortcut: string;
    readonly expectedSelectors: readonly string[];
}

export const FRONTEND_SEARCH_E2E_SCENARIOS: readonly FrontendSearchScenario[] = [
    {
        id: 'open-global-search-shortcut',
        description: 'Abre a paleta de busca global via atalho Ctrl+Shift+F e foca o input combobox',
        shortcut: 'Ctrl+Shift+F',
        expectedSelectors: ['[role="dialog"]', '[role="combobox"]', '[role="listbox"]']
    },
    {
        id: 'filter-chips-and-keyboard-navigation',
        description: 'Alterna chips de escopo, estado de saída e período, navegando por ArrowDown/ArrowUp/Enter',
        shortcut: 'ArrowDown',
        expectedSelectors: ['[role="option"]', 'mark.global-search-item__mark']
    },
    {
        id: 'cancel-inflight-and-escape-layered',
        description: 'Cancela busca anterior ao digitar novo termo e fecha apenas o modal em aberto no Escape',
        shortcut: 'Escape',
        expectedSelectors: ['[role="dialog"]']
    }
];
