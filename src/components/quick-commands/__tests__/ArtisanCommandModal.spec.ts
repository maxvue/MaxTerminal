import { beforeEach, describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import ArtisanCommandModal from '../ArtisanCommandModal.vue';
import { useQuickCommandsStore } from '@/stores/quickCommands';
import { setQuickCommandsAdaptersForTest } from '@/composables/quick-commands/adapters';
import * as ipc from '@/ipc/quickCommands';

vi.mock('@/ipc/quickCommands', () => ({
    quickCommandsIpc: {
        resolveRoot: vi.fn(),
        catalogGet: vi.fn(),
        catalogRefresh: vi.fn(),
        shortcutList: vi.fn(),
        validateForm: vi.fn(),
        preview: vi.fn(),
        create: vi.fn(),
        maxcodePrompt: vi.fn(),
        formSchema: vi.fn()
    },
    onCatalogUpdated: vi.fn(async () => () => {}),
    onDiscoveryFailed: vi.fn(async () => () => {}),
    onCommandCreated: vi.fn(async () => () => {})
}));

describe('ArtisanCommandModal', () => {
    const openMaxCodePlanMock = vi.fn(async () => {});

    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        setQuickCommandsAdaptersForTest({
            openMaxCodePlan: openMaxCodePlanMock,
            notifyToast: vi.fn(),
            focusPaneInput: vi.fn()
        });
        vi.mocked(ipc.quickCommandsIpc.preview).mockResolvedValue({
            path: 'app/Console/Commands/BillingSync.php',
            signature: 'billing:sync',
            php: '<?php\nclass BillingSync {}',
            report: { valid: true, issues: [] },
            consolePatch: null,
            manualSteps: [],
            revision: 'rev-1'
        });
    });

    it('inicializa com seedSignature e aciona Criar e Criar com MaxCode', async () => {
        vi.mocked(ipc.quickCommandsIpc.create).mockResolvedValue({
            path: 'app/Console/Commands/BillingSync.php',
            consoleRoutesPatched: false,
            manualSteps: [],
            catalogRefreshPending: true
        });
        vi.mocked(ipc.quickCommandsIpc.maxcodePrompt).mockResolvedValue(
            'Criar comando artisan:\n- Nome da Classe: BillingSync'
        );

        const store = useQuickCommandsStore();
        store.openCreateModal('/tmp/proj', { signatureName: 'billing:sync' });
        await flushPromises();

        const wrapper = mount(ArtisanCommandModal, {
            props: {
                open: true,
                root: '/tmp/proj'
            }
        });

        const classInput = wrapper.find('[data-testid="input-class-name"] input').exists()
            ? wrapper.find('[data-testid="input-class-name"] input')
            : wrapper.find('[data-testid="input-class-name"]');
        expect((classInput.element as HTMLInputElement).value).toBe('BillingSync');

        await wrapper.find('[data-testid="btn-create-maxcode"]').trigger('click');
        await flushPromises();
        expect(ipc.quickCommandsIpc.maxcodePrompt).toHaveBeenCalledTimes(1);
        expect(openMaxCodePlanMock).toHaveBeenCalledTimes(1);

        await wrapper.find('[data-testid="btn-create-artisan"]').trigger('click');
        await flushPromises();
        expect(ipc.quickCommandsIpc.create).toHaveBeenCalledTimes(1);
    });
});
