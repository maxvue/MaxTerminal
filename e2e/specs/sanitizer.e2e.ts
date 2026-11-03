/**
 * E2E Desktop (Camada B — CA-28): Sanitização E2E antes da gravação em disco e inspeção externa.
 */
export const DESKTOP_SANITIZER_E2E_SPEC = {
    suite: 'sanitizer',
    steps: [
        'Executa comando com Bearer, APP_KEY, cookie e senha em URL',
        'Confirma redação em maxterminal.db, maxterminal.db-wal, índice FTS5 e botão Inspecionar'
    ]
} as const;
