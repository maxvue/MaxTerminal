import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import CommandGroupModal from '@/components/commandGroups/CommandGroupModal.vue';
import { MtIpcError } from '@/ipc/core';
import type {
    CommandGroup,
    CommandGroupInput,
    GroupSuggestRequest,
    GroupSuggestResponse,
    RecurringCommandsResponse
} from '@/types/commandGroups';

const mockCreate = vi.fn<(input: CommandGroupInput) => Promise<CommandGroup>>();
const mockUpdate = vi.fn<
    (groupId: string, expectedRevision: number, input: CommandGroupInput) => Promise<CommandGroup>
>();
const mockGet = vi.fn<(groupId: string) => Promise<CommandGroup>>();
const mockSuggest = vi.fn<(req: GroupSuggestRequest) => Promise<GroupSuggestResponse>>();
const mockRecurring = vi.fn<
    (projectId: string, limit?: number) => Promise<RecurringCommandsResponse>
>();

vi.mock('@/ipc/commandGroups', () => ({
    commandGroupsIpc: {
        list: vi.fn(),
        get: (groupId: string) => mockGet(groupId),
        create: (input: CommandGroupInput) => mockCreate(input),
        update: (groupId: string, expectedRevision: number, input: CommandGroupInput) =>
            mockUpdate(groupId, expectedRevision, input),
        remove: vi.fn(),
        reorder: vi.fn(),
        run: vi.fn(),
        cancelRun: vi.fn(),
        getRun: vi.fn(),
        listRuns: vi.fn(),
        attachItemOutput: vi.fn(),
        detachItemOutput: vi.fn(),
        suggest: (req: GroupSuggestRequest) => mockSuggest(req),
        recurringCommands: (projectId: string, limit?: number) => mockRecurring(projectId, limit),
        onGroupChanged: vi.fn(async () => () => {}),
        onRunStarted: vi.fn(async () => () => {}),
        onRunItemChanged: vi.fn(async () => () => {}),
        onRunFinished: vi.fn(async () => () => {})
    }
}));

vi.mock('@/ipc/ai', () => ({
    aiIpc: {
        cancel: vi.fn(async () => {})
    }
}));

function makeGroup(overrides: Partial<CommandGroup> = {}): CommandGroup {
    return {
        id: '018f0000-0000-7000-8000-000000000001',
        scope: 'local',
        projectId: 'proj-1',
        name: 'Ambiente Dev',
        description: 'Subir stack local',
        labels: ['dev'],
        mode: 'sequential',
        separator: 'and',
        cwd: null,
        env: {},
        items: [
            {
                id: 'item-1',
                groupId: '018f0000-0000-7000-8000-000000000001',
                position: 0,
                title: 'Servidor Vite',
                command: 'npm run dev',
                cwd: null,
                env: {},
                continuous: false
            },
            {
                id: 'item-2',
                groupId: '018f0000-0000-7000-8000-000000000001',
                position: 1,
                title: 'Testes Watch',
                command: 'npm run test:unit',
                cwd: null,
                env: {},
                continuous: false
            }
        ],
        sortOrder: 0,
        revision: 1,
        createdAt: 1700000000000,
        updatedAt: 1700000000000,
        ...overrides
    };
}

function getFieldInput(wrapper: ReturnType<typeof mount>, testId: string) {
    const input = wrapper.find(`[data-testid="${testId}"] textarea, [data-testid="${testId}"] input`);
    return input.exists() ? input : wrapper.get(`[data-testid="${testId}"]`);
}

describe('CommandGroupModal.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        mockRecurring.mockResolvedValue({
            available: false,
            items: []
        });
    });

    it('modal_supports_reorder_via_buttons_and_alt_arrows_with_aria_live', async () => {
        const group = makeGroup();
        const wrapper = mount(CommandGroupModal, {
            props: {
                modelValue: true,
                group,
                scope: 'local',
                projectId: 'proj-1'
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        await flushPromises();

        expect((getFieldInput(wrapper, 'item-title-0').element as HTMLInputElement).value).toBe(
            'Servidor Vite'
        );
        expect((getFieldInput(wrapper, 'item-title-1').element as HTMLInputElement).value).toBe(
            'Testes Watch'
        );

        await wrapper.get('[data-testid="item-move-down-0"]').trigger('click');
        expect((getFieldInput(wrapper, 'item-title-0').element as HTMLInputElement).value).toBe(
            'Testes Watch'
        );
        expect(wrapper.get('[data-testid="reorder-live-region"]').text()).toContain(
            'Item movido para a posição 2 de 2'
        );

        await wrapper.get('[data-testid="group-item-row-1"]').trigger('keydown', {
            key: 'ArrowUp',
            altKey: true
        });
        expect((getFieldInput(wrapper, 'item-title-0').element as HTMLInputElement).value).toBe(
            'Servidor Vite'
        );
        expect(wrapper.get('[data-testid="reorder-live-region"]').text()).toContain(
            'Item movido para a posição 1 de 2'
        );
    });

    it('modal_displays_field_error_on_secret_detected_or_invalid_cwd', async () => {
        const group = makeGroup();
        const wrapper = mount(CommandGroupModal, {
            props: {
                modelValue: true,
                group,
                scope: 'local',
                projectId: 'proj-1'
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        await flushPromises();

        await getFieldInput(wrapper, 'input-group-cwd').setValue('../fora-do-projeto');
        await wrapper.get('[data-testid="btn-save-group"]').trigger('click');
        await flushPromises();

        expect(wrapper.get('[data-testid="error-group-cwd"]').text()).toContain(
            'relativo à raiz do projeto'
        );
        expect(mockUpdate).not.toHaveBeenCalled();

        await getFieldInput(wrapper, 'input-group-cwd').setValue('packages/app');
        mockUpdate.mockRejectedValueOnce(
            new MtIpcError(
                {
                    code: 'commands.group.secret_detected',
                    message: 'Segredo literal detectado na definição do grupo.',
                    details: {
                        fields: ['items.0.command']
                    }
                },
                'command_group_update'
            )
        );

        await wrapper.get('[data-testid="btn-save-group"]').trigger('click');
        await flushPromises();

        expect(wrapper.get('[data-testid="item-command-error-0"]').text()).toContain(
            'Segredo literal detectado neste campo'
        );
    });

    it('modal_resolves_revision_conflict_preserving_local_edits', async () => {
        const group = makeGroup({ revision: 2 });
        const serverLatest = makeGroup({
            revision: 5,
            name: 'Ambiente Dev Remoto'
        });

        mockUpdate.mockRejectedValueOnce(
            new MtIpcError(
                {
                    code: 'commands.group.revision_conflict',
                    message: 'Este grupo foi alterado por outra operação.',
                    details: null
                },
                'command_group_update'
            )
        );
        mockGet.mockResolvedValueOnce(serverLatest);

        const wrapper = mount(CommandGroupModal, {
            props: {
                modelValue: true,
                group,
                scope: 'local',
                projectId: 'proj-1'
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        await flushPromises();

        await getFieldInput(wrapper, 'input-group-name').setValue('Minha Edição Local');
        await wrapper.get('[data-testid="btn-save-group"]').trigger('click');
        await flushPromises();

        expect(wrapper.find('[data-testid="revision-conflict-banner"]').exists()).toBe(true);
        expect((getFieldInput(wrapper, 'input-group-name').element as HTMLInputElement).value).toBe(
            'Minha Edição Local'
        );

        mockUpdate.mockResolvedValueOnce({
            ...serverLatest,
            name: 'Minha Edição Local',
            revision: 6
        });

        await wrapper.get('[data-testid="btn-conflict-force-save"]').trigger('click');
        await flushPromises();

        expect(mockUpdate).toHaveBeenLastCalledWith(
            group.id,
            5,
            expect.objectContaining({
                name: 'Minha Edição Local'
            })
        );
        expect(wrapper.emitted('saved')).toBeTruthy();
    });

    it('suggest_panel_applies_preview_explicitly_and_supports_undo', async () => {
        const group = makeGroup();
        mockSuggest.mockResolvedValueOnce({
            requestId: 'req-ai-1',
            draftRevision: 2,
            suggestion: {
                scope: 'local',
                projectId: 'proj-1',
                name: 'Stack IA Completa',
                description: 'Gerado pela IA',
                labels: ['ia'],
                mode: 'multi_tabs',
                separator: 'and',
                cwd: null,
                env: {},
                items: [
                    {
                        id: null,
                        title: 'Vite Dev',
                        command: 'npm run dev',
                        cwd: null,
                        env: {},
                        continuous: true
                    },
                    {
                        id: null,
                        title: 'Fila Horizon',
                        command: 'php artisan horizon',
                        cwd: null,
                        env: {},
                        continuous: true
                    }
                ]
            }
        });

        const wrapper = mount(CommandGroupModal, {
            props: {
                modelValue: true,
                group,
                scope: 'local',
                projectId: 'proj-1'
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        await flushPromises();

        await getFieldInput(wrapper, 'suggest-goal-input').setValue('Subir Vite e Horizon');
        await wrapper.get('[data-testid="btn-suggest-ai"]').trigger('click');
        await flushPromises();

        expect(wrapper.find('[data-testid="suggest-preview-box"]').exists()).toBe(true);
        expect((getFieldInput(wrapper, 'input-group-name').element as HTMLInputElement).value).toBe(
            'Ambiente Dev'
        );

        await wrapper.get('[data-testid="btn-apply-suggestion"]').trigger('click');
        await flushPromises();

        expect((getFieldInput(wrapper, 'input-group-name').element as HTMLInputElement).value).toBe(
            'Stack IA Completa'
        );

        await wrapper.get('[data-testid="btn-undo-suggestion"]').trigger('click');
        await flushPromises();

        expect((getFieldInput(wrapper, 'input-group-name').element as HTMLInputElement).value).toBe(
            'Ambiente Dev'
        );
    });
});
