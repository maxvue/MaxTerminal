import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as credentialsIpc from '@/ipc/credentials';
import { useVaultStore } from '@/stores/vault';

vi.mock('@/ipc/credentials', () => ({
    vaultGetState: vi.fn(),
    vaultSetup: vi.fn(),
    vaultUnlock: vi.fn(),
    vaultLock: vi.fn(),
    vaultChangePassword: vi.fn()
}));

describe('useVaultStore (R4 - Cofre de Credenciais)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('inicializa como unconfigured e atualiza o estado ao buscar do backend', async () => {
        const store = useVaultStore();
        expect(store.status).toBe('unconfigured');
        expect(store.isConfigured).toBe(false);
        expect(store.isUnlocked).toBe(false);

        vi.mocked(credentialsIpc.vaultGetState).mockResolvedValueOnce('locked');
        const st = await store.fetchState();
        expect(st).toBe('locked');
        expect(store.status).toBe('locked');
        expect(store.isConfigured).toBe(true);
        expect(store.isLocked).toBe(true);
    });

    it('realiza setup de senha mestra e marca como desbloqueado', async () => {
        const store = useVaultStore();
        vi.mocked(credentialsIpc.vaultSetup).mockResolvedValueOnce('unlocked');

        await store.setup('senha-mestra-123');
        expect(credentialsIpc.vaultSetup).toHaveBeenCalledWith('senha-mestra-123');
        expect(store.status).toBe('unlocked');
        expect(store.isUnlocked).toBe(true);
    });

    it('realiza unlock com senha mestra e dispara callback pendente', async () => {
        const store = useVaultStore();
        store.status = 'locked';

        const callback = vi.fn();
        store.openUnlockModal(callback);
        expect(store.unlockModalOpen).toBe(true);

        vi.mocked(credentialsIpc.vaultUnlock).mockResolvedValueOnce('unlocked');
        await store.unlock('minha-senha');
        expect(credentialsIpc.vaultUnlock).toHaveBeenCalledWith('minha-senha');
        expect(store.status).toBe('unlocked');

        store.handleUnlocked();
        expect(callback).toHaveBeenCalledTimes(1);
    });

    it('abre modal de setup se o cofre ainda não estiver configurado', () => {
        const store = useVaultStore();
        store.status = 'unconfigured';

        const callback = vi.fn();
        store.openUnlockModal(callback);
        expect(store.setupModalOpen).toBe(true);
        expect(store.unlockModalOpen).toBe(false);

        store.handleConfigured();
        expect(callback).toHaveBeenCalledTimes(1);
    });

    it('bloqueia o cofre via lock', async () => {
        const store = useVaultStore();
        store.status = 'unlocked';
        vi.mocked(credentialsIpc.vaultLock).mockResolvedValueOnce('locked');

        await store.lock();
        expect(credentialsIpc.vaultLock).toHaveBeenCalled();
        expect(store.status).toBe('locked');
        expect(store.isLocked).toBe(true);
    });

    it('altera a senha mestra com sucesso', async () => {
        const store = useVaultStore();
        vi.mocked(credentialsIpc.vaultChangePassword).mockResolvedValueOnce('unlocked');

        await store.changePassword('antiga', 'nova-senha-123');
        expect(credentialsIpc.vaultChangePassword).toHaveBeenCalledWith('antiga', 'nova-senha-123');
        expect(store.status).toBe('unlocked');
    });
});
