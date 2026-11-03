import { beforeEach, describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import QuickCommandsHost from '../QuickCommandsHost.vue';
import { useQuickCommandsStore } from '@/stores/quickCommands';

vi.mock('@/ipc/quickCommands', () => ({
    quickCommandsIpc: {
        resolveRoot: vi.fn(),
        catalogGet: vi.fn(),
        catalogRefresh: vi.fn(),
        shortcutList: vi.fn(),
        validateForm: vi.fn(),
        preview: vi.fn(async () => ({
            path: 'app/Console/Commands/CacheWarm.php',
            signature: 'cache:warm',
            php: '<?php',
            report: { valid: true, issues: [] },
            consolePatch: null,
            manualSteps: [],
            revision: 'rev-1'
        })),
        create: vi.fn(),
        maxcodePrompt: vi.fn(),
        formSchema: vi.fn()
    },
    onCatalogUpdated: vi.fn(async () => () => {}),
    onDiscoveryFailed: vi.fn(async () => () => {}),
    onCommandCreated: vi.fn(async () => () => {})
}));

describe('QuickCommandsHost', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('monta o ArtisanCommandModal refletindo o estado da store', async () => {
        const store = useQuickCommandsStore();
        const wrapper = mount(QuickCommandsHost);
        await flushPromises();

        expect(wrapper.find('[data-testid="artisan-command-modal"]').exists()).toBe(false);

        store.openCreateModal('/tmp/proj', { signatureName: 'cache:warm' });
        await wrapper.vm.$nextTick();

        expect(wrapper.find('[data-testid="artisan-command-modal"]').exists()).toBe(true);
    });
});
