import { Channel } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import { commands } from '@/types/bindings';
import type {
    CommandGroup,
    CommandGroupCatalog,
    CommandGroupId,
    CommandGroupInput,
    DeleteGroupReceipt,
    GroupChangedEvent,
    GroupOutputFrame,
    GroupRun,
    GroupRunEvent,
    GroupRunId,
    GroupRunItemId,
    GroupRunListPage,
    GroupRunRequest,
    GroupScope,
    GroupSuggestRequest,
    GroupSuggestResponse,
    OutputAttachReceipt,
    OutputSubscriptionId,
    RecurringCommandsResponse,
    ReorderGroupEntry
} from '@/types/commandGroups';

export const commandGroupsIpc = {
    list(projectId: string | null): Promise<CommandGroupCatalog> {
        return unwrap(commands.commandGroupList(projectId), 'command_group_list');
    },
    get(groupId: CommandGroupId): Promise<CommandGroup> {
        return unwrap(commands.commandGroupGet(groupId), 'command_group_get');
    },
    create(input: CommandGroupInput): Promise<CommandGroup> {
        return unwrap(commands.commandGroupCreate(input), 'command_group_create');
    },
    update(
        groupId: CommandGroupId,
        expectedRevision: number,
        input: CommandGroupInput
    ): Promise<CommandGroup> {
        return unwrap(
            commands.commandGroupUpdate(groupId, expectedRevision, input),
            'command_group_update'
        );
    },
    remove(groupId: CommandGroupId, expectedRevision: number): Promise<DeleteGroupReceipt> {
        return unwrap(
            commands.commandGroupDelete(groupId, expectedRevision),
            'command_group_delete'
        );
    },
    reorder(
        scope: GroupScope,
        projectId: string | null,
        entries: ReorderGroupEntry[]
    ): Promise<CommandGroupCatalog> {
        return unwrap(
            commands.commandGroupReorder(scope, projectId, entries),
            'command_group_reorder'
        );
    },
    run(request: GroupRunRequest): Promise<GroupRun> {
        return unwrap(commands.commandGroupRun(request), 'command_group_run');
    },
    cancelRun(runId: GroupRunId, itemId: GroupRunItemId | null = null): Promise<GroupRun> {
        return unwrap(
            commands.commandGroupRunCancel(runId, itemId),
            'command_group_run_cancel'
        );
    },
    listRuns(
        projectId: string,
        cursor: string | null = null,
        limit = 25,
        activeOnly: boolean | null = null
    ): Promise<GroupRunListPage> {
        return unwrap(
            commands.commandGroupRunList(projectId, cursor, limit, activeOnly),
            'command_group_run_list'
        );
    },
    getRun(runId: GroupRunId): Promise<GroupRun> {
        return unwrap(commands.commandGroupRunGet(runId), 'command_group_run_get');
    },
    attachItemOutput(
        runId: GroupRunId,
        itemId: GroupRunItemId,
        afterSeq: number | null,
        onFrame: (frame: GroupOutputFrame) => void
    ): Promise<OutputAttachReceipt> {
        const channel = new Channel<GroupOutputFrame>();
        channel.onmessage = onFrame;
        return unwrap(
            commands.commandGroupRunItemAttach(runId, itemId, afterSeq, channel),
            'command_group_run_item_attach'
        );
    },
    detachItemOutput(subscriptionId: OutputSubscriptionId): Promise<null> {
        return unwrap(
            commands.commandGroupRunItemDetach(subscriptionId),
            'command_group_run_item_detach'
        );
    },
    suggest(request: GroupSuggestRequest): Promise<GroupSuggestResponse> {
        return unwrap(commands.commandGroupSuggest(request), 'command_group_suggest');
    },
    recurringCommands(projectId: string, limit = 15): Promise<RecurringCommandsResponse> {
        return unwrap(
            commands.commandGroupRecurringCommands(projectId, limit),
            'command_group_recurring_commands'
        );
    },
    onGroupChanged(handler: (payload: GroupChangedEvent) => void): Promise<UnlistenFn> {
        return listen<GroupChangedEvent>('command-group://changed', (e) => handler(e.payload));
    },
    onRunStarted(handler: (payload: GroupRunEvent) => void): Promise<UnlistenFn> {
        return listen<GroupRunEvent>('command-group://run-started', (e) => handler(e.payload));
    },
    onRunItemChanged(handler: (payload: GroupRunEvent) => void): Promise<UnlistenFn> {
        return listen<GroupRunEvent>('command-group://run-item-changed', (e) => handler(e.payload));
    },
    onRunFinished(handler: (payload: GroupRunEvent) => void): Promise<UnlistenFn> {
        return listen<GroupRunEvent>('command-group://run-finished', (e) => handler(e.payload));
    }
};
