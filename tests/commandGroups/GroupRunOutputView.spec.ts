import { flushPromises, mount } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import GroupRunOutputView from '@/components/commandGroups/GroupRunOutputView.vue';
import type {
    GroupOutputFrame,
    GroupRunItem,
    OutputAttachReceipt
} from '@/types/commandGroups';

let capturedOnFrame: ((frame: GroupOutputFrame) => void) | null = null;

const mockAttachItemOutput = vi.fn<
    (
        runId: string,
        itemId: string,
        afterSeq: number | null,
        onFrame: (frame: GroupOutputFrame) => void
    ) => Promise<OutputAttachReceipt>
>();
const mockDetachItemOutput = vi.fn<(subscriptionId: string) => Promise<void>>();
const mockCancelRun = vi.fn();

vi.mock('@/ipc/commandGroups', () => ({
    commandGroupsIpc: {
        attachItemOutput: (
            runId: string,
            itemId: string,
            afterSeq: number | null,
            onFrame: (frame: GroupOutputFrame) => void
        ) => {
            capturedOnFrame = onFrame;
            return mockAttachItemOutput(runId, itemId, afterSeq, onFrame);
        },
        detachItemOutput: (subscriptionId: string) => mockDetachItemOutput(subscriptionId),
        cancelRun: (...args: unknown[]) => mockCancelRun(...args)
    }
}));

function makeItem(overrides: Partial<GroupRunItem> = {}): GroupRunItem {
    return {
        id: 'item-bg-1',
        runId: 'run-bg-1',
        position: 0,
        snapshot: {
            sourceItemId: 'src-bg-1',
            position: 0,
            title: 'Worker Paralelo',
            command: 'npm run worker',
            definedCwd: null,
            resolvedCwd: '/home/user/proj',
            env: {},
            continuous: true,
            templateId: null,
            shellPath: '/bin/zsh',
            shellDialect: 'zsh'
        },
        status: 'running',
        version: 1,
        ptyId: null,
        tabId: null,
        paneId: null,
        sessionId: null,
        commandBlockId: null,
        exitCode: null,
        signal: null,
        errorCode: null,
        errorMessage: null,
        startedAt: 1700000000000,
        finishedAt: null,
        outputTruncated: false,
        ...overrides
    };
}

describe('GroupRunOutputView.vue', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        capturedOnFrame = null;
        mockAttachItemOutput.mockResolvedValue({
            subscriptionId: 'sub-123',
            truncated: false,
            replayStartSeq: 0,
            replayEndSeq: 1
        });
        mockDetachItemOutput.mockResolvedValue();
    });

    it('output_view_attaches_on_mount_and_detaches_on_unmount', async () => {
        const wrapper = mount(GroupRunOutputView, {
            props: {
                runId: 'run-bg-1',
                item: makeItem()
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        await flushPromises();

        expect(mockAttachItemOutput).toHaveBeenCalledTimes(1);
        expect(mockAttachItemOutput).toHaveBeenCalledWith(
            'run-bg-1',
            'item-bg-1',
            null,
            expect.any(Function)
        );

        capturedOnFrame?.({
            subscriptionId: 'sub-123',
            itemId: 'item-bg-1',
            seq: 2,
            kind: 'data',
            text: 'Servidor iniciado na porta 3000\n',
            droppedBytes: null
        });
        await flushPromises();

        expect(wrapper.get('[data-testid="group-run-output-terminal"]').text()).toContain(
            'Servidor iniciado na porta 3000'
        );

        wrapper.unmount();
        await flushPromises();

        expect(mockDetachItemOutput).toHaveBeenCalledWith('sub-123');
        expect(mockCancelRun).not.toHaveBeenCalled();
    });

    it('output_view_renders_truncation_and_gap_banners', async () => {
        mockAttachItemOutput.mockResolvedValueOnce({
            subscriptionId: 'sub-456',
            truncated: true,
            replayStartSeq: 1,
            replayEndSeq: 5
        });

        const wrapper = mount(GroupRunOutputView, {
            props: {
                runId: 'run-bg-1',
                item: makeItem({ outputTruncated: true })
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        await flushPromises();

        expect(wrapper.find('[data-testid="group-run-output-truncated-banner"]').exists()).toBe(
            true
        );

        capturedOnFrame?.({
            subscriptionId: 'sub-456',
            itemId: 'item-bg-1',
            seq: 6,
            kind: 'gap',
            text: null,
            droppedBytes: 4096
        });
        await flushPromises();

        const banner = wrapper.get('[data-testid="group-run-output-truncated-banner"]');
        expect(banner.text()).toContain('256 KiB');
        expect(banner.text()).toContain('4.0 KiB');

        await wrapper.get('[data-testid="group-run-output-terminal"]').trigger('keydown', {
            key: 'a'
        });
        expect(wrapper.find('[data-testid="group-run-output-blocked-notice"]').exists()).toBe(true);
    });
});
