/**
 * E2E Desktop (Camada B — CA-28): Criação de abas, split panes e persistência de sessão.
 */
export const DESKTOP_TABS_E2E_SPEC = {
    suite: 'tabs',
    steps: [
        'Adiciona nova aba com Template Padrão do projeto ativo',
        'Divide o painel horizontal e verticalmente via splitpanes',
        'Inicia nova sessão arquivando a anterior no SQLite'
    ]
} as const;
