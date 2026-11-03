import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import VaultUnlockModal from '@/components/credentials/VaultUnlockModal.vue';
import * as credentialsIpc from '@/ipc/credentials';

vi.mock('@/ipc/credentials', () => ({
    vaultGetState: vi.fn(),
    vaultSetup: vi.fn(),
    vaultUnlock: vi.fn(),
    vaultLock: vi.fn(),
    vaultChangePassword: vi.fn()
}));

describe('VaultUnlockModal.vue (R4 - Desbloqueio do Cofre)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('renderiza o formulário quando visible é true', () => {
        const wrapper = mount(VaultUnlockModal, {
            props: {
                visible: true
            }
        });

        expect(wrapper.find('input[type="password"]').exists()).toBe(true);
        expect(wrapper.find('button[type="submit"]').text()).toContain('Desbloquear');
    });

    it('executa o desbloqueio com a senha mestra digitada e emite unlocked', async () => {
        vi.mocked(credentialsIpc.vaultUnlock).mockResolvedValueOnce('unlocked');

        const wrapper = mount(VaultUnlockModal, {
            props: {
                visible: true
            }
        });

        const input = wrapper.find('input[type="password"]');
        await input.setValue('master-password-123');

        await wrapper.find('form').trigger('submit');
        await wrapper.vm.$nextTick();

        expect(credentialsIpc.vaultUnlock).toHaveBeenCalledWith('master-password-123');
        expect(wrapper.emitted('unlocked')).toBeTruthy();
        expect(wrapper.emitted('update:visible')?.[0]).toEqual([false]);
    });
});
