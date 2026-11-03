import type {
    CommandGroup,
    CommandGroupCatalog,
    CommandGroupId,
    CommandGroupInput,
    CommandGroupItem,
    CommandGroupItemInput,
    DeleteGroupReceipt,
    ExecutionMode,
    GroupChangedEvent,
    GroupChangeOperation,
    GroupItemId,
    GroupOutputFrame,
    GroupOutputFrameKind,
    GroupRequestId,
    GroupRun,
    GroupRunCounts,
    GroupRunEvent,
    GroupRunId,
    GroupRunItem,
    GroupRunItemEventDelta,
    GroupRunItemId,
    GroupRunItemSnapshot,
    GroupRunListPage,
    GroupRunRequest,
    GroupRunSnapshot,
    GroupRunSummary,
    GroupScope,
    GroupSuggestRequest,
    GroupSuggestResponse,
    ItemStatus,
    OutputAttachReceipt,
    OutputSubscriptionId,
    RecurringCommandEntry,
    RecurringCommandsResponse,
    ReorderGroupEntry,
    RunStatus,
    SequentialSeparator,
    ShellDialect
} from '@/types/bindings';

export type {
    CommandGroup,
    CommandGroupCatalog,
    CommandGroupId,
    CommandGroupInput,
    CommandGroupItem,
    CommandGroupItemInput,
    DeleteGroupReceipt,
    ExecutionMode,
    GroupChangedEvent,
    GroupChangeOperation,
    GroupItemId,
    GroupOutputFrame,
    GroupOutputFrameKind,
    GroupRequestId,
    GroupRun,
    GroupRunCounts,
    GroupRunEvent,
    GroupRunId,
    GroupRunItem,
    GroupRunItemEventDelta,
    GroupRunItemId,
    GroupRunItemSnapshot,
    GroupRunListPage,
    GroupRunRequest,
    GroupRunSnapshot,
    GroupRunSummary,
    GroupScope,
    GroupSuggestRequest,
    GroupSuggestResponse,
    ItemStatus,
    OutputAttachReceipt,
    OutputSubscriptionId,
    RecurringCommandEntry,
    RecurringCommandsResponse,
    ReorderGroupEntry,
    RunStatus,
    SequentialSeparator,
    ShellDialect
};

/** União de ações disponíveis por item no painel de execução (spec §9.5). */
export type GroupRunItemAction = 'show_output' | 'focus_tab' | 'cancel';

/** Ações secundárias (etapa 2) de um grupo no popup do gatilho `>>`. */
export type GroupTriggerSecondaryAction =
    | 'run_default'
    | 'run_sequential'
    | 'run_parallel'
    | 'run_multi_tabs'
    | 'edit'
    | 'delete';

/** Alias compatível para identificadores de ação do gatilho `>>`. */
export type GroupTriggerActionId = GroupTriggerSecondaryAction | 'create_local' | 'create_global';

/** Mapa de erros de validação por caminho de campo no editor de grupo. */
export type GroupEditorFieldErrors = Record<string, string>;

/** Item editável no formulário com chave local estável para reordenação e foco. */
export interface EditableGroupItem {
    localKey: string;
    id?: GroupItemId | null;
    title: string;
    command: string;
    cwd?: string | null;
    env: Record<string, string>;
    envPairs: Array<{ key: string; value: string }>;
    continuous: boolean;
}

/** Rascunho completo do editor `CommandGroupModal.vue`. */
export interface EditableCommandGroupDraft {
    scope: GroupScope;
    projectId: string | null;
    name: string;
    description: string;
    labels: string[];
    mode: ExecutionMode;
    separator: SequentialSeparator;
    cwd: string;
    envPairs: Array<{ key: string; value: string }>;
    items: EditableGroupItem[];
}

/** Alias de rascunho para compatibilidade com contratos de UI. */
export type CommandGroupDraft = EditableCommandGroupDraft;
