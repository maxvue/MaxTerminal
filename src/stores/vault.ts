import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { useIpcError } from '@/composables/useIpcError';
import * as credentialsIpc from '@/ipc/credentials';
import type { VaultStatus } from '@/types/bindings';

export const useVaultStore = defineStore('vault', () => {
    const ipcError = useIpcError();

    const status = ref<VaultStatus>('unconfigured');
    const loading = ref<boolean>(false);
    const error = ref<string | null>(null);

    const isConfigured = computed<boolean>(() => status.value !== 'unconfigured');
    const isUnlocked = computed<boolean>(() => status.value === 'unlocked');
    const isLocked = computed<boolean>(() => status.value === 'locked');

    const unlockModalOpen = ref<boolean>(false);
    const setupModalOpen = ref<boolean>(false);
    const pendingSuccessCallback = ref<(() => void) | null>(null);

    function openUnlockModal(onSuccess?: () => void): void {
        if (onSuccess) {
            pendingSuccessCallback.value = onSuccess;
        }
        if (status.value === 'unconfigured') {
            setupModalOpen.value = true;
        } else {
            unlockModalOpen.value = true;
        }
    }

    function closeUnlockModal(): void {
        unlockModalOpen.value = false;
        pendingSuccessCallback.value = null;
    }

    function openSetupModal(onSuccess?: () => void): void {
        if (onSuccess) {
            pendingSuccessCallback.value = onSuccess;
        }
        setupModalOpen.value = true;
    }

    function closeSetupModal(): void {
        setupModalOpen.value = false;
        pendingSuccessCallback.value = null;
    }

    function handleUnlocked(): void {
        const cb = pendingSuccessCallback.value;
        pendingSuccessCallback.value = null;
        cb?.();
    }

    function handleConfigured(): void {
        const cb = pendingSuccessCallback.value;
        pendingSuccessCallback.value = null;
        cb?.();
    }

    async function fetchState(): Promise<VaultStatus> {
        loading.value = true;
        error.value = null;
        try {
            const nextStatus = await credentialsIpc.vaultGetState();
            status.value = nextStatus;
            return nextStatus;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    async function setup(masterPassword: string): Promise<void> {
        loading.value = true;
        error.value = null;
        try {
            await credentialsIpc.vaultSetup(masterPassword);
            status.value = 'unlocked';
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    async function unlock(masterPassword: string): Promise<void> {
        loading.value = true;
        error.value = null;
        try {
            await credentialsIpc.vaultUnlock(masterPassword);
            status.value = 'unlocked';
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    async function lock(): Promise<void> {
        loading.value = true;
        error.value = null;
        try {
            await credentialsIpc.vaultLock();
            status.value = 'locked';
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    async function changePassword(
        oldMasterPassword: string,
        newMasterPassword: string
    ): Promise<void> {
        loading.value = true;
        error.value = null;
        try {
            await credentialsIpc.vaultChangePassword(oldMasterPassword, newMasterPassword);
            status.value = 'unlocked';
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    return {
        status,
        loading,
        error,
        isConfigured,
        isUnlocked,
        isLocked,
        unlockModalOpen,
        setupModalOpen,
        openUnlockModal,
        closeUnlockModal,
        openSetupModal,
        closeSetupModal,
        handleUnlocked,
        handleConfigured,
        fetchState,
        setup,
        unlock,
        lock,
        changePassword
    };
});
