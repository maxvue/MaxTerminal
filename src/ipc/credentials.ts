import { unwrap } from '@/ipc/core';
import { commands, type VaultStatus } from '@/types/bindings';

/** Obtém o estado atual de configuração e bloqueio do cofre de senhas (`vault_get_state`). */
export function vaultGetState(): Promise<VaultStatus> {
    return unwrap(commands.vaultGetState(), 'vault_get_state');
}

/** Configura a senha mestra inicial do cofre (`vault_setup`). */
export function vaultSetup(masterPassword: string): Promise<VaultStatus> {
    return unwrap(commands.vaultSetup(masterPassword), 'vault_setup');
}

/** Desbloqueia o cofre de credenciais fornecendo a senha mestra (`vault_unlock`). */
export function vaultUnlock(masterPassword: string): Promise<VaultStatus> {
    return unwrap(commands.vaultUnlock(masterPassword), 'vault_unlock');
}

/** Bloqueia imediatamente o cofre descartando as chaves em memória (`vault_lock`). */
export function vaultLock(): Promise<VaultStatus> {
    return unwrap(commands.vaultLock(), 'vault_lock');
}

/** Altera a senha mestra do cofre re-encriptando a DEK (`vault_change_password`). */
export function vaultChangePassword(
    oldMasterPassword: string,
    newMasterPassword: string
): Promise<VaultStatus> {
    return unwrap(
        commands.vaultChangePassword(oldMasterPassword, newMasterPassword),
        'vault_change_password'
    );
}
