# Implementação 04 — Camada IPC Frontend, Store Pinia, Composables e Gatilho de Input Rico (`spec.md`)

## 1. Contexto, Objetivo e Escopo

Esta implementação constrói toda a camada lógica do Frontend (TypeScript, wrappers IPC, Store Pinia, Composables de edição/execução/saída e o provedor do gatilho `>>` no `TriggerRegistry`) para a **Fase 15: Grupos de Comandos (`>>`)**.

### 1.1. Escopo Incluído
- **Itens da Fase 15 cobertos:** `I-11` (Provedor do gatilho `>>` em `commandGroupsTrigger.ts` com desambiguação exata frente ao `>` do Artisan da Fase 14, seções "Deste projeto" → "Globais", busca fuzzy por nome/descrição/labels, etapa 2 de ações e as duas opções fixas finais de criação), `I-12` (`useCommandGroupsStore` em `src/stores/commandGroups.ts` com cache por projeto + globais, reconciliação monotônica por `seq`/`version`, deduplicação single-flight e assinatura de eventos) e os composables `useCommandGroupEditor`, `useCommandGroupRun` e `useGroupRunOutput`.
- **Critérios de Aceite cobertos:** `CA-10`, `CA-11`, `CA-12`, `CA-13`, `CA-14`, `CA-15`, `CA-23`, `CA-25`, `CA-26`.

### 1.2. Escopo Excluído
- Componentes visuais Vue (`CommandGroupModal.vue`, `GroupRunPanel.vue`, `GroupRunOutputView.vue`, etc.) — cobertos na **Implementação 05**.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho Absoluto / Relativo ao Workspace | Descrição |
|------|------------------------------------------|-----------|
| Criar | `src/types/commandGroups.ts` | Reexportações dos tipos gerados em `src/types/bindings.ts` e tipos derivados exclusivos de UI (`GroupRunItemAction`, `CommandGroupDraft`, `GroupTriggerActionId`, `GroupEditorFieldErrors`). |
| Criar | `src/ipc/commandGroups.ts` | Wrappers tipados para os 14 comandos IPC, criação de `Channel<GroupOutputFrame>` e helpers `listen` para os 4 eventos `command-group://*`. |
| Criar | `src/stores/commandGroups.ts` | Store Pinia `useCommandGroupsStore` com cache de grupos locais por `ProjectId` e globais, tombstones de revisão, mapa de execuções ativas/históricas, bufferização de eventos antes de snapshot e reconciliação de saltos de `seq`. |
| Criar | `src/composables/commandGroups/useCommandGroupEditor.ts` | Composable de estado do editor modal (clone profundo, controle `dirty`, validação local por campo espelhando o backend, reordenação de itens, aplicar/desfazer sugestão IA e proteção contra sobrescrita por resposta tardia ou conflito de revisão). |
| Criar | `src/composables/commandGroups/useCommandGroupRun.ts` | Composable de lançamento e acompanhamento de execução (captura atômica de projeto/painel/geração no clique, `requestId` estável por tentativa, seleção de template compatível no modo Multi-Abas e cancelamento). |
| Criar | `src/composables/commandGroups/useGroupRunOutput.ts` | Composable de anexo/desanexo de stream de saída (`command_group_run_item_attach` / `detach`) com limpeza automática no `onBeforeUnmount`. |
| Criar | `src/terminal/triggers/providers/commandGroupsTrigger.ts` | Provedor do gatilho `>>` para o `TriggerRegistry` (Fase 04), incluindo desambiguação `>>` vs `>`, filtro fuzzy preservando ordem "Deste projeto" → "Globais", etapa 2 de ações (`Tab`/`→`) e itens fixados `pinned: 'bottom'`. |
| Modificar | `src/terminal/triggers/providers/index.ts` | Exportar e registrar `registerCommandGroupsTrigger` após os provedores da Fase 14. |
| Criar | `tests/commandGroups/commandGroupsStore.spec.ts` | Testes unitários Vitest da store cobrindo invalidação seletiva de cache, descarte de `seq` menor, reconciliação de salto de `seq` e deduplicação single-flight (`CA-11`, `CA-14`, `CA-26`). |
| Criar | `tests/commandGroups/commandGroupsTrigger.spec.ts` | Testes unitários Vitest do gatilho `>>` cobrindo precedência sobre `>`, transição `>>` ↔ `>` com `AbortSignal`, escapes `\>>` e ` >>`, query `>>>dev` → `>dev`, passthrough, ordenação de seções, fuzzy por nome/descrição/labels e ações fixas finais com e sem projeto ativo (`CA-11`, `CA-12`, `CA-13`, `CA-15`, `CA-25`). |

---

## 3. Contratos e Assinaturas TypeScript

### 3.1. `src/types/commandGroups.ts`

```ts
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

/** Item editável no formulário com chave local estável para reordenação e foco. */
export interface EditableGroupItem extends CommandGroupItemInput {
    localKey: string;
    envPairs: Array<{ key: string; value: string }>;
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
```

### 3.2. `src/ipc/commandGroups.ts`

```ts
import { Channel } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { commands } from '@/types/bindings';
import { unwrap } from '@/ipc/core';
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
    update(groupId: CommandGroupId, expectedRevision: number, input: CommandGroupInput): Promise<CommandGroup> {
        return unwrap(commands.commandGroupUpdate(groupId, expectedRevision, input), 'command_group_update');
    },
    remove(groupId: CommandGroupId, expectedRevision: number): Promise<DeleteGroupReceipt> {
        return unwrap(commands.commandGroupDelete(groupId, expectedRevision), 'command_group_delete');
    },
    reorder(scope: GroupScope, projectId: string | null, entries: ReorderGroupEntry[]): Promise<CommandGroupCatalog> {
        return unwrap(commands.commandGroupReorder(scope, projectId, entries), 'command_group_reorder');
    },
    run(request: GroupRunRequest): Promise<GroupRun> {
        return unwrap(commands.commandGroupRun(request), 'command_group_run');
    },
    cancelRun(runId: GroupRunId, itemId: GroupRunItemId | null = null): Promise<GroupRun> {
        return unwrap(commands.commandGroupRunCancel(runId, itemId), 'command_group_run_cancel');
    },
    listRuns(
        projectId: string,
        cursor: string | null = null,
        limit = 25,
        activeOnly: boolean | null = null
    ): Promise<GroupRunListPage> {
        return unwrap(commands.commandGroupRunList(projectId, cursor, limit, activeOnly), 'command_group_run_list');
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
        return unwrap(commands.commandGroupRunItemDetach(subscriptionId), 'command_group_run_item_detach');
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
```

### 3.3. `src/stores/commandGroups.ts` (`useCommandGroupsStore`)

A store gerencia:
- `globalGroups: CommandGroup[]`, `globalLoaded: boolean`, `globalLoading: boolean`, `globalError: MtError | null`.
- `localGroupsByProject: Record<string, CommandGroup[]>`, `localLoadedByProject: Record<string, boolean>`, `localLoadingByProject: Record<string, boolean>`, `localErrorByProject: Record<string, MtError | null>`.
- `deletedRevisionByGroupId: Record<string, number>` (tombstones para evitar que uma resposta de leitura atrasada ressuscite um grupo já excluído com revisão maior).
- `runsById: Record<string, GroupRun>` e `runSummariesByProject: Record<string, GroupRunSummary[]>`.
- `pendingEventsByRunId: Record<string, GroupRunEvent[]>` (buffer de eventos que chegam antes do retorno de `command_group_run` ou `command_group_run_get`).
- `editorModalState: { open: boolean; group: CommandGroup | null; scope: GroupScope; projectId: string | null; initialName: string }`.
- `activeRunPanelId: string | null`.
- Métodos obrigatórios:
  - `initListeners(): Promise<void>` e `disposeListeners(): void`.
  - `load(projectId: string | null, force?: boolean): Promise<CommandGroupCatalog>` (com deduplicação single-flight por chave).
  - `get(groupId: string): Promise<CommandGroup>`.
  - `create(input: CommandGroupInput): Promise<CommandGroup>`.
  - `update(groupId: string, expectedRevision: number, input: CommandGroupInput): Promise<CommandGroup>`.
  - `remove(groupId: string, expectedRevision: number): Promise<DeleteGroupReceipt>`.
  - `reorder(scope: GroupScope, projectId: string | null, entries: ReorderGroupEntry[]): Promise<CommandGroupCatalog>`.
  - `startRun(request: GroupRunRequest): Promise<GroupRun>`.
  - `cancelRun(runId: string, itemId?: string | null): Promise<GroupRun>`.
  - `loadRun(runId: string): Promise<GroupRun>`.
  - `loadHistory(projectId: string, cursor?: string | null, limit?: number, activeOnly?: boolean | null): Promise<GroupRunListPage>`.
  - `suggest(request: GroupSuggestRequest): Promise<GroupSuggestResponse>`.
  - `loadRecurring(projectId: string, limit?: number): Promise<RecurringCommandsResponse>`.
  - `openCreateModal(scope: GroupScope, projectId: string | null, initialName?: string): void`.
  - `openEditModal(group: CommandGroup): void`.
  - `closeEditorModal(): void`.

### 3.4. `src/terminal/triggers/providers/commandGroupsTrigger.ts`

Regras exatas do provedor `>>`:
1. **Ativação e Precedência (`matchTrigger`):**
   - Só ativa quando `context.inputMode === 'rich'`. Em `passthrough`, nunca ativa (`CA-12`).
   - Rejeita se a linha iniciar com espaço (`" >>"`) ou com barra invertida de escape (`"\\>>"`).
   - Exige que os dois primeiros caracteres do buffer sejam exatamente `">>"`.
   - Tudo após os dois primeiros caracteres `">>"` constitui a `query` bruta: portanto, para `">>>dev"`, o prefixo consumido é `">>"` e a `query` é `">dev"` (`CA-12`).
   - Como o `TriggerRegistry` da Fase 04 resolve por prefixo mais longo (`>>` comprimento 2 > `>` comprimento 1), digitar o segundo `>` em `>` aborta o `AbortSignal` da sessão Artisan e inicia a sessão de Grupos; apagar o segundo `>` com `Backspace` (voltando para `>`) aborta o `AbortSignal` da sessão de Grupos e reativa o provedor Artisan (`CA-12`).
2. **Busca Fuzzy e Ordenação Estrita por Escopo (`CA-13`, `CA-25`):**
   - Carrega os grupos do projeto ativo (`localGroups`) e os globais (`globalGroups`) via `useCommandGroupsStore`, respeitando `context.signal` (`AbortSignal`) para descartar resultados caso a sessão ou o projeto mude.
   - Aplica filtro e pontuação fuzzy sobre `name`, `description` e `labels` (normalizados Unicode NFC + lowercase), concedendo bônus para match de prefixo no `name`.
   - **Nunca mistura escopos por score:** primeiro filtra e ordena os grupos locais (desempate por `sortOrder`, `name`, `id`) sob a seção `"Deste projeto"`, depois filtra e ordena os grupos globais sob a seção `"Globais"`. Sem query, preserva a ordem de `sortOrder`.
3. **Itens Fixados no Final (`pinned: 'bottom'`, `CA-13`):**
   - Independentemente da query, de erro de carregamento ou de haver 0 grupos correspondentes, sempre retorna no final da lista:
     1. `id: 'action:create-local'`, `label: 'Criar grupo de comandos local'`, `pinned: 'bottom'`, `disabled: !context.projectId`, `disabledReason: !context.projectId ? 'Selecione um projeto para criar um grupo local' : undefined`.
     2. `id: 'action:create-global'`, `label: 'Criar grupo de comandos global'`, `pinned: 'bottom'`, `disabled: false`.
   - Ao acionar qualquer uma das duas opções com `Enter`, abre o `CommandGroupModal` pré-selecionando o escopo (`local` ou `global`) e aproveitando a `query` atual (se não vazia) como sugestão para o campo **Nome** do rascunho — **nunca** como comando shell (`spec §8.2`).
4. **Etapa 2 de Ações do Grupo (`Tab` / `→`, `CA-15`):**
   - Pressionar `Enter` em um grupo na Etapa 1 executa imediatamente no modo padrão salvo do grupo (gerando um `requestId` estável por gesto de submissão para evitar duplicatas com `Enter` segurado).
   - Pressionar `Tab` ou `→` sobre um grupo abre a Etapa 2 listando:
     1. `Executar em modo Sequencial`
     2. `Executar em modo Paralelo`
     3. `Executar em modo Multi-Abas`
     4. `Editar grupo`
     5. `Excluir grupo`
   - Pressionar `←` retorna à Etapa 1; pressionar `Escape` fecha o popup mantendo o texto literal `>>...` no input e suprime reabertura automática até que o texto seja editado novamente.

---

## 4. Regras de Negócio e Casos de Borda Desta Implementação

1. **Reconciliação de Eventos Fora de Ordem ou Perdidos (`CA-14`, `CA-26`):**
   - Cada `GroupRunEvent` possui `seq` monotônico da execução (`GroupRun.version`).
   - Se a store receber um `GroupRunEvent` com `event.seq <= currentRun.version`, o evento é descartado silenciosamente (`CA-14`).
   - Se `event.seq == currentRun.version + 1` e o evento contiver `item`, a store aplica o delta do item e atualiza `status`, `counts`, `version = event.seq`, `lastExitCode`, `startedAt`, `cancelRequestedAt` e `finishedAt`.
   - Se `event.seq > currentRun.version + 1` (salto de sequência indicando perda de evento intermediário que pode ter afetado outro item), a store dispara imediatamente `loadRun(event.runId)` para reconciliar o snapshot completo do backend (`spec §7`).
2. **Invalidação Seletiva do Cache de Grupos (`CA-14`):**
   - Quando chega `command-group://changed` com `scope === 'local'` e `projectId: P`, somente o cache de `P` (`localGroupsByProject[P]`) é invalidado/atualizado; caches de outros projetos e o cache global permanecem intactos.
   - Quando chega `command-group://changed` com `scope === 'global'`, apenas `globalGroups` é atualizado, refletindo reativamente em todas as projeções sem recarregar os grupos locais de nenhum projeto.
3. **Proteção Contra Duplo Lançamento (`requestId` por Gesto):**
   - Tanto no gatilho `>>` quanto em `useCommandGroupRun`, cada tentativa de lançamento cria um único `requestId` UUID v7 e bloqueia novas submissões da mesma ação enquanto a chamada IPC estiver pendente. Se houver retry de transporte da mesma tentativa, o mesmo `requestId` é reutilizado.
