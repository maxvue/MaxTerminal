/**
 * E2E Desktop (Camada B — CA-28): Ativação via deep link maxterminal:// e single-instance.
 */
export const DESKTOP_DEEPLINK_E2E_SPEC = {
    suite: 'deeplink',
    steps: [
        'Envia maxterminal://open-project e maxterminal://open-terminal para instância ativa',
        'Rejeita parâmetros proibidos (cmd, shell, initial_command) e foca projeto autorizado'
    ]
} as const;
