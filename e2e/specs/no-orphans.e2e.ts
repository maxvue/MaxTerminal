/**
 * E2E Desktop (Camada B — CA-28): Garantia de zero processos órfãos ao fechar abas e painéis.
 */
export const DESKTOP_NO_ORPHANS_E2E_SPEC = {
    suite: 'no-orphans',
    steps: [
        'Spawna árvore de processos (nohup, setsid, double-fork) em PTY real',
        'Fecha a aba na UI e confirma via /proc que zero PIDs descendentes restam vivos ou zumbis'
    ]
} as const;
