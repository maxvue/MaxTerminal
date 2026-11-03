/**
 * E2E Desktop (Camada B — CA-28): System Tray e minimização com PTYs em execução.
 */
export const DESKTOP_TRAY_E2E_SPEC = {
    suite: 'tray',
    steps: [
        'Inicia processo contínuo em aba e solicita fechamento da janela principal',
        'Verifica minimização para a bandeja do sistema mantendo o PTY vivo e restauração via Quake toggle'
    ]
} as const;
