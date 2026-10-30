# Especificação Técnica — Implementação 04: Integração no Binário Tauri, Comandos IPC, Eventos Revisionados e Wrappers TypeScript

> **Fase:** 05 — Shell Integration OSC 133 e Máquina de Estados das Abas
> **Implementação:** `04 - Comandos IPC Tauri, Eventos Revisionados e Wrappers TypeScript`
> **Fontes da verdade:** [`../../AGENTS.md`](../../../AGENTS.md), [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§§1, 2.1, 8 e 9).

---

## 1. Contexto, Objetivo e Escopo

Esta implementação conecta o crate `mt-shell` e os repositórios de `mt-db` ao binário Tauri (`src-tauri/src/`) e expõe os contratos IPC tipados para o frontend Vue/TypeScript (`src/ipc/`), mantendo o isolamento arquitetural (`mt-pty` e `mt-shell` continuam livres de dependência do Tauri).

### Escopo Incluído
1. **Extensão do comando `pty_spawn` (`src-tauri/src/commands/pty.rs`):**
   - Adição do parâmetro opcional `integration: Option<ShellIntegrationRequest>` ao comando `pty_spawn`.
   - Compatibilidade retroativa total: se `integration` for `None` mas `spec.owner` contiver `(Some(tab_id), Some(pane_id))`, constrói `ShellIntegrationRequest` padrão (`enabled: true`, `continuous: Auto`); se `owner` estiver ausente e `integration` for `None`, faz spawn bruto da Fase 02 sem tracker de aba.
2. **Comandos IPC de Estados de Aba (`src-tauri/src/commands/tab_state.rs`):**
   - `tab_state_list`, `tab_state_get`, `tab_mark_seen`, `tab_set_focus` (extraindo o rótulo da janela diretamente de `tauri::WebviewWindow` por segurança) e `tab_set_continuous`.
3. **Comandos IPC de Shell Integration e Blocos (`src-tauri/src/commands/shell.rs`):**
   - `shell_integration_info`, `command_block_list`, `command_block_get`, `command_block_error_context` e `command_block_search`.
4. **Eventos Globais Revisionados (`src-tauri/src/events/shell.rs`):**
   - Implementação de `ShellEventSink` emitindo `tab://state-changed`, `tab://state-removed`, `shell://command-started`, `shell://command-finished`, `shell://cwd-changed` (com debounce de 100 ms por painel) e `shell://integration-mode`.
5. **Bootstrap, Capabilities, Specta e Wrappers TS (`src/ipc/shell.ts`, `src/ipc/tabState.ts`):**
   - Inicialização do `ShellService` no `lib.rs` (registrando `shell_fts_spec()` no `DbSetup`, executando `close_dangling` antes de aceitar spawns, retendo o `TapHandle` e drenando o `ShellService::shutdown` antes de `Db::shutdown`).
   - Geração de `src/types/bindings.ts` via `tauri-specta` e criação dos wrappers `src/ipc/shell.ts` e `src/ipc/tabState.ts`.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho do Arquivo | Responsabilidade |
|------|--------------------|------------------|
| Modificar | `src-tauri/Cargo.toml` | Adicionar `mt-shell` às dependências do crate binário `maxterminal` |
| Modificar | `src-tauri/src/commands/pty.rs` | Estender `pty_spawn` com `integration: Option<ShellIntegrationRequest>` usando `ShellService` + `spawn_with_lifecycle` |
| Criar | `src-tauri/src/commands/tab_state.rs` | Comandos Tauri `tab_state_list`, `tab_state_get`, `tab_mark_seen`, `tab_set_focus`, `tab_set_continuous` |
| Criar | `src-tauri/src/commands/shell.rs` | Comandos Tauri `shell_integration_info`, `command_block_list`, `command_block_get`, `command_block_error_context`, `command_block_search` |
| Modificar | `src-tauri/src/commands/mod.rs` | Exportar módulos `shell` e `tab_state` |
| Criar | `src-tauri/src/events/shell.rs` | Eventos `tauri_specta::Event` e adaptador `TauriShellEventSink` (com debounce de 100 ms para `shell://cwd-changed`) |
| Modificar | `src-tauri/src/events/mod.rs` | Exportar módulo `shell` |
| Modificar | `src-tauri/src/lib.rs` | Registrar `shell_fts_spec`, inicializar `ShellService`, rodar `close_dangling`, registrar comandos/eventos no `tauri_specta` e hook de shutdown |
| Modificar | `src-tauri/capabilities/default.json` | Garantir permissões da janela principal para os comandos e eventos de `tab_state` e `shell` |
| Modificar | `src/types/bindings.ts` | Tipos e comandos gerados automaticamente pelo `tauri-specta` |
| Modificar | `src/ipc/pty.ts` | Permitir argumento opcional `integration?: ShellIntegrationRequest` em `ptySpawn` |
| Modificar | `src/terminal/stream/ptyClient.ts` | Repassar `integration?: ShellIntegrationRequest` opcional em `ptyStreamHub.spawn` / `ptyClient.spawn` |
| Criar | `src/ipc/tabState.ts` | Wrappers tipados `tabStateList`, `tabStateGet`, `tabMarkSeen`, `tabSetFocus`, `tabSetContinuous` e listeners `onTabStateChanged`, `onTabStateRemoved` |
| Criar | `src/ipc/shell.ts` | Wrappers tipados `shellIntegrationInfo`, `commandBlockList`, `commandBlockGet`, `commandBlockErrorContext`, `commandBlockSearch` e listeners `onCommandStarted`, `onCommandFinished`, `onCwdChanged`, `onIntegrationModeChanged` |

---

## 3. Assinaturas Exatas dos Comandos, Eventos e Wrappers

### 3.1. Extensão de `pty_spawn` (`src-tauri/src/commands/pty.rs`)

```rust
#[tauri::command]
#[specta::specta]
pub async fn pty_spawn(
    pty_manager: tauri::State<'_, Arc<PtyManager>>,
    shell_service: tauri::State<'_, Arc<ShellService>>,
    spec: SpawnSpec,
    integration: Option<ShellIntegrationRequest>,
) -> Result<PtySummary, MtError> {
    let resolved_req = match integration {
        Some(req) => Some(req),
        None => match (spec.owner.tab_id, spec.owner.pane_id) {
            (Some(tab_id), Some(pane_id)) => Some(ShellIntegrationRequest {
                tab_id,
                pane_id,
                session_id: None,
                enabled: true,
                continuous: ContinuousPolicy::Auto,
            }),
            _ => None,
        },
    };

    if let Some(req) = resolved_req {
        let plan = shell_service.prepare_spawn(&spec, req).await?;
        let manager = pty_manager.inner().clone();
        tokio::task::spawn_blocking(move || {
            manager.spawn_with_lifecycle(spec, None, plan.lifecycle())
        })
        .await
        .map_err(|_| MtError::internal("Falha ao aguardar tarefa de inicialização do terminal."))?
        .map_err(MtError::from)
    } else {
        let manager = pty_manager.inner().clone();
        tokio::task::spawn_blocking(move || manager.spawn(spec, None))
            .await
            .map_err(|_| MtError::internal("Falha ao aguardar tarefa de inicialização do terminal."))?
            .map_err(MtError::from)
    }
}
```

### 3.2. Comandos de Estados de Aba (`src-tauri/src/commands/tab_state.rs`)

```rust
use std::sync::Arc;
use tauri::{State, WebviewWindow};
use mt_core::{MtError, PaneId, TabId};
use mt_core::shell::{
    ContinuousPolicy, FocusSnapshot, SeenRequest, TabStateListResponse, TabStateSnapshot,
};
use mt_shell::ShellService;

#[tauri::command]
#[specta::specta]
pub async fn tab_state_list(
    shell: State<'_, Arc<ShellService>>,
) -> Result<TabStateListResponse, MtError> {
    Ok(shell.registry().list_tabs())
}

#[tauri::command]
#[specta::specta]
pub async fn tab_state_get(
    shell: State<'_, Arc<ShellService>>,
    tab_id: TabId,
) -> Result<TabStateSnapshot, MtError> {
    shell.registry().get_tab(tab_id)
}

#[tauri::command]
#[specta::specta]
pub async fn tab_mark_seen(
    window: WebviewWindow,
    shell: State<'_, Arc<ShellService>>,
    request: SeenRequest,
) -> Result<TabStateSnapshot, MtError> {
    shell.mark_seen(window.label(), request)
}

#[tauri::command]
#[specta::specta]
pub async fn tab_set_focus(
    window: WebviewWindow,
    shell: State<'_, Arc<ShellService>>,
    focus: FocusSnapshot,
) -> Result<(), MtError> {
    shell.registry().set_window_focus(window.label(), focus);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn tab_set_continuous(
    shell: State<'_, Arc<ShellService>>,
    tab_id: TabId,
    pane_id: PaneId,
    policy: ContinuousPolicy,
) -> Result<TabStateSnapshot, MtError> {
    shell.set_continuous_policy(tab_id, pane_id, policy)
}
```

### 3.3. Comandos de Histórico e Contexto de Erro (`src-tauri/src/commands/shell.rs`)

```rust
use std::sync::Arc;
use tauri::State;
use mt_core::{CommandBlockId, MtError, PtyId, SessionId};
use mt_core::num::U64String;
use mt_core::shell::{
    CommandBlockDto, CommandBlockHit, ErrorContext, IntegrationInfo, Page,
};
use mt_shell::ShellService;

#[tauri::command]
#[specta::specta]
pub async fn shell_integration_info(
    shell: State<'_, Arc<ShellService>>,
    pty_id: PtyId,
) -> Result<IntegrationInfo, MtError> {
    shell.integration_info(pty_id)
}

#[tauri::command]
#[specta::specta]
pub async fn command_block_list(
    shell: State<'_, Arc<ShellService>>,
    session_id: SessionId,
    before_seq: Option<U64String>,
    limit: Option<u32>,
) -> Result<Page<CommandBlockDto>, MtError> {
    let limit = limit.unwrap_or(100);
    if !(1..=500).contains(&limit) {
        return Err(MtError::shell_invalid_argument(
            "O limite de listagem de blocos deve estar entre 1 e 500.",
        ));
    }
    shell.list_blocks(session_id, before_seq.map( Into::into ), limit).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_block_get(
    shell: State<'_, Arc<ShellService>>,
    block_id: CommandBlockId,
    include_output: Option<bool>,
) -> Result<CommandBlockDto, MtError> {
    shell.get_block(block_id, include_output.unwrap_or(false)).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_block_error_context(
    shell: State<'_, Arc<ShellService>>,
    block_id: CommandBlockId,
    max_lines: Option<u32>,
) -> Result<ErrorContext, MtError> {
    let max_lines = max_lines.unwrap_or(50);
    if !(1..=200).contains(&max_lines) {
        return Err(MtError::shell_invalid_argument(
            "O limite de linhas do contexto de erro deve estar entre 1 e 200.",
        ));
    }
    // Aceita blocos `Failed`, inclusive os finalizados em modo `Proc` por PTY encerrado com `exit_code != 0` (T-07);
    // blocos `Unknown` continuam recusados com `shell.block_not_failed`.
    shell.error_context(block_id, max_lines).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_block_search(
    shell: State<'_, Arc<ShellService>>,
    query: String,
    session_id: Option<SessionId>,
    limit: Option<u32>,
) -> Result<Vec<CommandBlockHit>, MtError> {
    let limit = limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(MtError::shell_invalid_argument(
            "O limite de resultados da busca deve estar entre 1 e 100.",
        ));
    }
    shell.search_blocks(&query, session_id, limit).await
}
```

### 3.4. Eventos Globais Revisionados (`src-tauri/src/events/shell.rs`)

Todos os eventos de shell usam o envelope revisionado `{ eventSeq: U64String, binding: RuntimeBinding, payload: T }` (ou payload direto para `tab://state-changed` e `tab://state-removed`):

```rust
use serde::{Deserialize, Serialize};
use mt_core::TabId;
use mt_core::num::U64String;
use mt_core::shell::{
    CommandFinished, CommandStarted, IntegrationMode, ObservationConfidence, ObservationSource,
    RuntimeBinding, TabStateSnapshot,
};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "tab://state-changed")]
#[serde(rename_all = "camelCase")]
pub struct TabStateChangedEvent(pub TabStateSnapshot);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "tab://state-removed")]
#[serde(rename_all = "camelCase")]
pub struct TabStateRemovedEvent {
    pub tab_id: TabId,
    pub revision: U64String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShellEventEnvelope<T> {
    pub event_seq: U64String,
    pub binding: RuntimeBinding,
    pub revision: U64String,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "shell://command-started")]
#[serde(rename_all = "camelCase")]
pub struct ShellCommandStartedEvent(pub ShellEventEnvelope<CommandStarted>);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "shell://command-finished")]
#[serde(rename_all = "camelCase")]
pub struct ShellCommandFinishedEvent(pub ShellEventEnvelope<CommandFinished>);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CwdChangedPayload {
    pub cwd: String,
    pub source: ObservationSource,
    pub confidence: ObservationConfidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "shell://cwd-changed")]
#[serde(rename_all = "camelCase")]
pub struct ShellCwdChangedEvent(pub ShellEventEnvelope<CwdChangedPayload>);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationModePayload {
    pub mode: IntegrationMode,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "shell://integration-mode")]
#[serde(rename_all = "camelCase")]
pub struct ShellIntegrationModeEvent(pub ShellEventEnvelope<IntegrationModePayload>);
```

### 3.5. Wrappers TypeScript (`src/ipc/tabState.ts` e `src/ipc/shell.ts`)

#### `src/ipc/tabState.ts`
```ts
import { commands, events } from '@/types/bindings';
import type {
    ContinuousPolicy,
    FocusSnapshot,
    PaneId,
    SeenRequest,
    TabId,
    TabStateListResponse,
    TabStateRemovedEvent,
    TabStateSnapshot
} from '@/types/bindings';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';

export const tabStateList = (): Promise<TabStateListResponse> =>
    unwrap(commands.tabStateList(), 'tab_state_list');

export const tabStateGet = (tabId: TabId): Promise<TabStateSnapshot> =>
    unwrap(commands.tabStateGet(tabId), 'tab_state_get');

export const tabMarkSeen = (request: SeenRequest): Promise<TabStateSnapshot> =>
    unwrap(commands.tabMarkSeen(request), 'tab_mark_seen');

export const tabSetFocus = (focus: FocusSnapshot): Promise<void> =>
    unwrap(commands.tabSetFocus(focus), 'tab_set_focus');

export const tabSetContinuous = (
    tabId: TabId,
    paneId: PaneId,
    policy: ContinuousPolicy
): Promise<TabStateSnapshot> =>
    unwrap(commands.tabSetContinuous(tabId, paneId, policy), 'tab_set_continuous');

export const onTabStateChanged = (
    handler: (snapshot: TabStateSnapshot) => void
): Promise<UnlistenFn> =>
    events.tabStateChangedEvent.listen((ev) => handler(ev.payload));

export const onTabStateRemoved = (
    handler: (payload: TabStateRemovedEvent) => void
): Promise<UnlistenFn> =>
    events.tabStateRemovedEvent.listen((ev) => handler(ev.payload));
```

#### `src/ipc/shell.ts`
```ts
import { commands, events } from '@/types/bindings';
import type {
    CommandBlockDto,
    CommandBlockHit,
    CommandBlockId,
    CommandFinished,
    CommandStarted,
    CwdChangedPayload,
    ErrorContext,
    IntegrationInfo,
    IntegrationModePayload,
    Page,
    PtyId,
    SessionId,
    ShellEventEnvelope
} from '@/types/bindings';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';

export const shellIntegrationInfo = (ptyId: PtyId): Promise<IntegrationInfo> =>
    unwrap(commands.shellIntegrationInfo(ptyId), 'shell_integration_info');

export const commandBlockList = (
    sessionId: SessionId,
    beforeSeq: string | null = null,
    limit = 100
): Promise<Page<CommandBlockDto>> =>
    unwrap(commands.commandBlockList(sessionId, beforeSeq, limit), 'command_block_list');

export const commandBlockGet = (
    blockId: CommandBlockId,
    includeOutput = false
): Promise<CommandBlockDto> =>
    unwrap(commands.commandBlockGet(blockId, includeOutput), 'command_block_get');

export const commandBlockErrorContext = (
    blockId: CommandBlockId,
    maxLines = 50
): Promise<ErrorContext> =>
    unwrap(commands.commandBlockErrorContext(blockId, maxLines), 'command_block_error_context');

export const commandBlockSearch = (
    query: string,
    sessionId: SessionId | null = null,
    limit = 50
): Promise<CommandBlockHit[]> =>
    unwrap(commands.commandBlockSearch(query, sessionId, limit), 'command_block_search');

export const onCommandStarted = (
    handler: (envelope: ShellEventEnvelope<CommandStarted>) => void
): Promise<UnlistenFn> =>
    events.shellCommandStartedEvent.listen((ev) => handler(ev.payload));

export const onCommandFinished = (
    handler: (envelope: ShellEventEnvelope<CommandFinished>) => void
): Promise<UnlistenFn> =>
    events.shellCommandFinishedEvent.listen((ev) => handler(ev.payload));

export const onCwdChanged = (
    handler: (envelope: ShellEventEnvelope<CwdChangedPayload>) => void
): Promise<UnlistenFn> =>
    events.shellCwdChangedEvent.listen((ev) => handler(ev.payload));

export const onIntegrationModeChanged = (
    handler: (envelope: ShellEventEnvelope<IntegrationModePayload>) => void
): Promise<UnlistenFn> =>
    events.shellIntegrationModeEvent.listen((ev) => handler(ev.payload));
```

---

## 4. Regras de Negócio e Casos de Borda no IPC

1. **Ordem de Inicialização e Shutdown no Binário (`src-tauri/src/lib.rs`):**
   - No startup do banco (`start_db_service`), assim que `Db::open` concluir, executar `CommandBlocksRepo::new(db.clone()).close_dangling(now_ms).await` **antes** de sinalizar que o serviço de banco está pronto para spawns.
   - Reter o `TapHandle` retornado por `pty_manager.add_output_tap(shell_service.output_tap())` dentro do estado do `ShellService` para que nunca seja dropado prematuramente.
   - No `RunEvent::ExitRequested`: parar novos spawns → encerrar PTYs (`PtyManager::shutdown`) → drenar o `ShellService::shutdown().await` (que finaliza blocos abertos como `unknown/Shutdown` e aguarda flush da fila de persistência até o deadline) → encerrar `Db::shutdown().await`.
2. **Debounce de `shell://cwd-changed`:**
   - Mudanças de `cwd` em rajada (ex.: script fazendo dezenas de `cd` seguidos) sofrem debounce de `100 ms` por `PaneId` antes de emitir o evento global `shell://cwd-changed`, mas o `cwd` capturado no início de cada `CommandBlock` permanece exato e síncrono no `CommandTracker`.
