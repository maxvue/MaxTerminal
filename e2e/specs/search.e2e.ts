/**
 * E2E Desktop (Camada B — CA-28): Busca Global e navegação real sobre o binário Tauri.
 */
export const DESKTOP_SEARCH_E2E_SPEC = {
    suite: 'search',
    steps: [
        'Abre a Paleta de Busca Global via Ctrl+Shift+F na janela Tauri principal',
        'Executa search_global real no SQLite WAL e verifica agrupamento das 9 fontes',
        'Seleciona item de Bloco de Comando e navega para Projeto -> Aba -> Sessão -> Bloco'
    ]
} as const;
