# Implementação 05 — Comandos IPC, Eventos, Stores Pinia e Interface Vue do Workspace

## 1. Contexto, Objetivo e Escopo

Esta implementação expõe os 22 comandos Tauri e 10 eventos tipados do workspace (`src-tauri/src/commands/`, `src-tauri/src/events/workspace.rs`, geração `tauri-specta` em `src/types/bindings.ts`), os clientes IPC e composables reativos do frontend (`src/ipc/`, `src/composables/workspace/`), as 3 stores Pinia (`useTemplatesStore`, `useTabsStore`, `useSessionsStore`) e todos os componentes Vue 3.6 da coluna direita do workspace (`ProjectWorkspace.vue` e subcomponentes em `src/components/workspace/`).

### Escopo Exato
1. **Comandos Tauri, Eventos e Bindings (`src-tauri/src/commands/`, `src-tauri/src/events/workspace.rs`):**
   - Implementação dos 22 comandos IPC com validação de limites e retorno `Result<T, MtError>`:
     - Templates: `template_list`, `template_get`, `template_create`, `template_update`, `template_delete`, `template_set_default`.
     - Abas: `tab_list`, `tab_open`, `tab_close`, `tab_reorder`, `tab_focus`.
     - Painéis: `pane_split`, `pane_close`, `pane_restart`, `pane_layout_update`.
     - Sessões: `session_new`, `session_list`, `session_get`, `session_scrollback_get`, `session_search`.
     - Workspace: `workspace_snapshot`, `workspace_initial_command_run`.
   - Emissão de eventos globais com envelope `WorkspaceEvent<T> { boot_id, event_seq, payload }` para `tab://opened`, `tab://layout-changed`, `tab://closed`, `tab://reordered`, `pane://pty-replaced`, `pane://exited`, `session://started`, `session://archived`, `template://changed` e `workspace://restored`.
   - Regeneração determinística de `src/types/bindings.ts` via `tauri-specta`.
2. **Camada IPC Frontend (`src/ipc/{templates,tabs,sessions,workspace}.ts`):**
   - Wrappers tipados usando `call` / `unwrap` de `src/ipc/core.ts` e funções de assinatura de eventos com `UnlistenFn`.
3. **Stores Pinia (`src/stores/{templates,tabs,sessions}.ts`):**
   - `useTemplatesStore`: cache por projeto, proteção por `revision` contra respostas IPC fora de ordem e invalidação via `template://changed`.
   - `useTabsStore`: `tabsById`, `orderByProject`, `tombstones` por `TabId` (`revision`), sincronização `bootId`/`eventSeq`, integração com `useProjectsStore` (`attachTab`, `detachTab`, `setActiveTab`) da Fase 06 e `useTabStatusStore` da Fase 05, e reutilização de `requestId` em retries.
   - `useSessionsStore`: paginação de agrupamentos por aba, LRU de até 3 sessões detalhadas em memória com teto de bytes de scrollback, e busca com descarte de respostas obsoletas por token de requisição.
4. **Composables do Workspace (`src/composables/workspace/`):**
   - `layoutTree.ts`: funções puras (`leaves`, `findPane`, `findNode`).
   - `useWorkspaceEvents.ts`: montado uma única vez no `AppLayout`, inscreve listeners **antes** de chamar `workspace_snapshot`, aplica buffer ordenado por `eventSeq`, solicita ressync em lacunas ou mudança de `bootId`, e garante `unlisten()` seguro mesmo após unmount.
   - `useActiveTabSeen.ts`: sincroniza aba/painel ativo e visibilidade com o tracker integrado das Fases 05 e 06 sem duplicar chamadas diretas a `markSeen`.
   - `usePaneLayout.ts`: adaptador testado para o payload `resized` do `splitpanes` (suportando formato array v3 e objeto `{ panes }` v4), debounce de 300 ms, coalescência por `nodeId` e envio serializado com `expectedLayoutRevision`.
5. **Componentes Vue 3.6 (`ProjectWorkspace.vue` e `src/components/workspace/`):**
   - `ProjectWorkspace.vue` (preservando a prop `project: Project | null` da Fase 06), `TabBar.vue`, `TabBarItem.vue`, `TemplateLaunchPopover.vue`, `TemplateEditorModal.vue`, `TabWorkspaceView.vue`, `PaneLayout.vue`, `PaneView.vue`, `SessionHistoryDrawer.vue` e `SessionReadonlyViewer.vue`.

---

## 2. Arquivos a Criar ou Modificar

### Criar
- `src-tauri/src/commands/templates.rs`
- `src-tauri/src/commands/tabs.rs`
- `src-tauri/src/commands/panes.rs`
- `src-tauri/src/commands/sessions.rs`
- `src-tauri/src/commands/workspace.rs`
- `src-tauri/src/events/workspace.rs`
- `src/ipc/templates.ts`
- `src/ipc/tabs.ts`
- `src/ipc/sessions.ts`
- `src/ipc/workspace.ts`
- `src/stores/templates.ts`
- `src/stores/tabs.ts`
- `src/stores/sessions.ts`
- `src/composables/workspace/layoutTree.ts`
- `src/composables/workspace/useWorkspaceEvents.ts`
- `src/composables/workspace/useActiveTabSeen.ts`
- `src/composables/workspace/usePaneLayout.ts`
- `src/components/workspace/TabBar.vue`
- `src/components/workspace/TabBarItem.vue`
- `src/components/workspace/TemplateLaunchPopover.vue`
- `src/components/workspace/TemplateEditorModal.vue`
- `src/components/workspace/TabWorkspaceView.vue`
- `src/components/workspace/PaneLayout.vue`
- `src/components/workspace/PaneView.vue`
- `src/components/workspace/SessionHistoryDrawer.vue`
- `src/components/workspace/SessionReadonlyViewer.vue`

### Modificar
- `src-tauri/src/commands/mod.rs` (exportar `templates`, `tabs`, `panes`, `sessions`, `workspace`)
- `src-tauri/src/events/mod.rs` (exportar `workspace`)
- `src-tauri/src/lib.rs` (registrar todos os comandos e eventos no builder `tauri_specta`)
- `src/types/bindings.ts` (atualizado via gerador `tauri-specta`)
- `src/components/projects/ProjectWorkspace.vue` (substituir placeholder da Fase 06 pela composição completa mantendo a prop `project: Project | null`)
- `src/components/layout/AppLayout.vue` (inicializar `useWorkspaceEvents()` e `useActiveTabSeen()`)

---

## 3. Especificação Técnica Detalhada

### 3.1. Envelope de Eventos e Comandos Tauri (`src-tauri/src/events/workspace.rs` e `commands/*.rs`)

```rust
use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TemplateId};
use mt_pty::ExitInfo;
use mt_core::workspace::session::{SessionSummary, WorkspaceRestoreReport};
use mt_core::workspace::tab::TabView;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEvent<T> {
    pub boot_id: String,
    pub event_seq: i64,
    pub payload: T,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TabClosedPayload {
    pub tab_id: TabId,
    pub project_id: ProjectId,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TabReorderedPayload {
    pub project_id: ProjectId,
    pub ordered_tab_ids: Vec<TabId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PanePtyReplacedPayload {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub session_id: SessionId,
    pub generation: i64,
    pub old_pty_id: Option<PtyId>,
    pub new_pty_id: Option<PtyId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PaneExitedPayload {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub session_id: SessionId,
    pub generation: i64,
    pub pty_id: PtyId,
    pub exit: ExitInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateChangedPayload {
    pub project_id: ProjectId,
    pub template_ids: Vec<TemplateId>,
}
```

#### Assinaturas Exatas dos 22 Comandos Tauri
- `src-tauri/src/commands/templates.rs`:
  - `pub async fn template_list(state: State<'_, Arc<WorkspaceManager>>, project_id: ProjectId) -> Result<Vec<TabTemplate>, MtError>`
  - `pub async fn template_get(state: State<'_, Arc<WorkspaceManager>>, template_id: TemplateId) -> Result<TabTemplate, MtError>`
  - `pub async fn template_create(state: State<'_, Arc<WorkspaceManager>>, project_id: ProjectId, input: TabTemplateInput) -> Result<TabTemplate, MtError>`
  - `pub async fn template_update(state: State<'_, Arc<WorkspaceManager>>, template_id: TemplateId, expected_revision: i64, input: TabTemplateInput) -> Result<TabTemplate, MtError>`
  - `pub async fn template_delete(state: State<'_, Arc<WorkspaceManager>>, template_id: TemplateId, expected_revision: i64) -> Result<(), MtError>`
  - `pub async fn template_set_default(state: State<'_, Arc<WorkspaceManager>>, template_id: TemplateId) -> Result<Vec<TabTemplate>, MtError>`
- `src-tauri/src/commands/tabs.rs`:
  - `pub async fn tab_list(state: State<'_, Arc<WorkspaceManager>>, project_id: ProjectId) -> Result<Vec<TabView>, MtError>`
  - `pub async fn tab_open(state: State<'_, Arc<WorkspaceManager>>, request: TabOpenRequest) -> Result<TabView, MtError>`
  - `pub async fn tab_close(state: State<'_, Arc<WorkspaceManager>>, tab_id: TabId, request_id: String) -> Result<(), MtError>`
  - `pub async fn tab_reorder(state: State<'_, Arc<WorkspaceManager>>, project_id: ProjectId, ordered_tab_ids: Vec<TabId>) -> Result<Vec<TabId>, MtError>`
  - `pub async fn tab_focus(state: State<'_, Arc<WorkspaceManager>>, tab_id: TabId, pane_id: Option<PaneId>) -> Result<TabView, MtError>`
- `src-tauri/src/commands/panes.rs`:
  - `pub async fn pane_split(state: State<'_, Arc<WorkspaceManager>>, pane_id: PaneId, direction: SplitDirection, expected_layout_revision: i64, request_id: String) -> Result<TabView, MtError>`
  - `pub async fn pane_close(state: State<'_, Arc<WorkspaceManager>>, pane_id: PaneId, request_id: String) -> Result<PaneCloseOutcome, MtError>`
  - `pub async fn pane_restart(state: State<'_, Arc<WorkspaceManager>>, pane_id: PaneId, cwd_fallback_to_project: bool, request_id: String) -> Result<TabView, MtError>`
  - `pub async fn pane_layout_update(state: State<'_, Arc<WorkspaceManager>>, tab_id: TabId, expected_layout_revision: i64, patches: Vec<LayoutSizePatch>) -> Result<TabView, MtError>`
- `src-tauri/src/commands/sessions.rs`:
  - `pub async fn session_new(state: State<'_, Arc<WorkspaceManager>>, tab_id: TabId, request_id: String) -> Result<TabView, MtError>`
  - `pub async fn session_list(state: State<'_, Arc<WorkspaceManager>>, tab_id: TabId, cursor: Option<String>, limit: u32) -> Result<CursorPage<SessionSummary>, MtError>` (valida `1 <= limit <= 100`)
  - `pub async fn session_get(state: State<'_, Arc<WorkspaceManager>>, tab_session_id: TabSessionId) -> Result<SessionDetail, MtError>`
  - `pub async fn session_scrollback_get(state: State<'_, Arc<WorkspaceManager>>, tab_session_id: TabSessionId, session_id: SessionId, cursor: Option<String>, limit: u32) -> Result<CursorPage<ScrollbackChunk>, MtError>` (valida `1 <= limit <= 200`)
  - `pub async fn session_search(state: State<'_, Arc<WorkspaceManager>>, tab_id: TabId, query: String, limit: u32) -> Result<Vec<SessionSearchHit>, MtError>` (valida `1 <= query.trim().chars().count() <= 256` e `1 <= limit <= 50`)
- `src-tauri/src/commands/workspace.rs`:
  - `pub async fn workspace_snapshot(state: State<'_, Arc<WorkspaceManager>>) -> Result<WorkspaceSnapshot, MtError>`
  - `pub async fn workspace_initial_command_run(state: State<'_, Arc<WorkspaceManager>>, tab_id: TabId, pane_id: PaneId, generation: i64, request_id: String) -> Result<TabView, MtError>`

---

### 3.2. Stores Pinia e Composables (`src/stores/` e `src/composables/workspace/`)

#### Interfaces TypeScript Exportadas
```ts
import type {
    CursorPage,
    LayoutSizePatch,
    PaneCloseOutcome,
    PaneId,
    PaneLayoutNode,
    PaneView,
    ProjectId,
    ScrollbackChunk,
    SessionDetail,
    SessionId,
    SessionSearchHit,
    SessionSummary,
    SplitDirection,
    TabId,
    TabOpenOverrides,
    TabOpenRequest,
    TabSessionId,
    TabTemplate,
    TabTemplateInput,
    TabView,
    TemplateId,
    WorkspaceRestoreReport,
    WorkspaceSnapshot
} from '@/types/bindings';

export interface TemplateEnvRow {
    key: string;
    kind: 'literal' | 'inherited';
    value: string;
    source: string;
}

export interface TemplateFormState {
    input: TabTemplateInput;
    envRows: TemplateEnvRow[];
    errors: Record<string, string>;
}

export interface WorkspaceSelection {
    projectId: ProjectId | null;
    tabId: TabId | null;
    paneId: PaneId | null;
}

export type TabBarIndicatorSlotProps = { tab: TabView };
```

#### Garantias de `useTabsStore` (`src/stores/tabs.ts`)
- Mantém `tabsById: Record<TabId, TabView>`, `orderByProject: Record<ProjectId, TabId[]>`, `tombstones: Record<TabId, number>` (guardando a `revision` de fechamento), `pendingByTab: Record<TabId, boolean>`, `bootId: string | null`, `lastEventSeq: number`.
- **Upsert Protegido por Revisão e Tombstone:**
  - `upsertTab(tab: TabView)` verifica primeiro se `tombstones[tab.id] !== undefined && tab.revision <= tombstones[tab.id]`. Se verdadeiro, ignora silenciosamente (impedindo que uma resposta IPC atrasada ressuscite uma aba já fechada).
  - Se `tabsById[tab.id]` já existir e `tab.revision < tabsById[tab.id].revision`, ignora.
  - Ao inserir uma nova aba ou hidratar do snapshot, invoca `useProjectsStore().attachTab(tab.id, tab.projectId)` e, se o projeto ainda não tiver aba ativa ou `focus === true`, chama `useProjectsStore().setActiveTab(tab.projectId, tab.id)`.
- **Remoção e Limpeza (`applyTabClosed`):**
  - Registra `tombstones[tabId] = revision`.
  - Para cada painel da aba fechada, chama `TerminalInstanceRegistry.dispose(pane.id)` da Fase 04.
  - Chama `useTabStatusStore().removeTab(tabId)` da Fase 05 e `useProjectsStore().detachTab(tabId)` da Fase 06 (contrato de 1 argumento; o projeto dono é resolvido internamente por `tabOwner`).
  - Se a aba fechada era a ativa do projeto, seleciona a aba vizinha imediata na ordem `orderByProject[projectId]` (ou `null` se não restarem abas).

#### Garantias de `usePaneLayout` (`src/composables/workspace/usePaneLayout.ts`)
- **Normalização de `resized` do `splitpanes`:**
  - Suporta tanto o payload da v3 (`Array<{ size: number; min?: number; max?: number }>`) quanto o payload da v4 (`{ panes: Array<{ size: number }> }`).
  - Extrai `sizes = rawPanes.map((p) => Number(p.size))` sem `as any` cego, validando que todos os números são finitos e somam ~100.
- **Coalescência e Envio Otimista com Revisão:**
  - Acumula alterações em `pendingPatches: Map<string, number[]>` indexado por `nodeId` e aplica debounce de 300 ms.
  - Ao disparar, envia `pane_layout_update(tabId, expectedLayoutRevision, patches)`.
  - Se o backend retornar erro com `code === 'layout.conflict'`, descarta `pendingPatches` obsoletos e recarrega o estado da aba via `tab_list`. Qualquer mudança estrutural (`pane_split` / `pane_close`) cancela imediatamente timers de resize pendentes.

---

### 3.3. Componentes Vue 3.6 (`ProjectWorkspace.vue` e `src/components/workspace/`)

Todos os arquivos `.vue` obedecem estritamente às diretrizes do `AGENTS.md`:
- Ordem obrigatória dos blocos: `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`.
- Recuo de 4 espaços, aspas simples, ponto e vírgula obrigatório, sem vírgula final, tags de abertura em linha única.
- Tokens semânticos `--max-*` e `--background-*`, ícones via `MaxIcon`, textos em `pt-BR`.

#### Contratos de Props, Emits e Slots dos Componentes
1. **`ProjectWorkspace.vue` (`src/components/projects/ProjectWorkspace.vue`):**
   - Props: `defineProps<{ project: Project | null }>()`.
   - Mantém compatibilidade 100% com a Fase 06. Quando `project === null`, renderiza estado vazio orientando a seleção ou criação de um projeto. Quando `project !== null`, carrega os templates do projeto, exibe `TabBar` no topo, `TabWorkspaceView` para a aba ativa (com preservação das instâncias xterm das abas inativas via `visible: boolean` e `TerminalInstanceRegistry` da Fase 04), `TemplateEditorModal` e `SessionHistoryDrawer`.
   - Expõe slots de repasse `#pinned`, `#indicators="{ tab }"` (para a Fase 08) e `#actions`.
2. **`TabBar.vue` e `TabBarItem.vue`:**
   - Acessibilidade e navegação de teclado completas: container com `role="tablist"`, itens com `role="tab"`, `aria-selected`, roving `tabindex` (`0` na ativa, `-1` nas demais), navegação com `ArrowLeft`, `ArrowRight`, `Home`, `End`, ativação com `Enter`/`Space`, reordenação também por atalho (`Alt+Shift+ArrowLeft` / `Alt+Shift+ArrowRight`) e restauração de foco após fechar aba.
   - Botões de ação com rótulos exatos:
     - **"Adicionar nova aba"** (abre imediatamente usando o Template Padrão do projeto com novo `requestId` UUID v7).
     - **"Iniciar Aba Template"** (abre `TemplateLaunchPopover`).
     - **"Iniciar nova Sessão"** (aciona `session_new` para a aba ativa).
     - **"Histórico de Sessões"** (abre `SessionHistoryDrawer`).
3. **`TemplateLaunchPopover.vue` e `TemplateEditorModal.vue`:**
   - `TemplateLaunchPopover`: lista o Template Padrão em destaque no topo (badge `"Padrão"`), seguido dos demais templates ordenados por `position` e `name`; oferece botões para iniciar aba a partir do template, definir como padrão, editar template e **"Novo template"**.
   - `TemplateEditorModal`: formulário completo para todos os parâmetros de `AGENTS.md` §4.2 (`Nome`, `Ícone`, `Pasta personalizada`, `Comando Inicial`, checkboxes `Adicionar ao Workspace`, `Execução Contínua`, `Definir como Padrão`, seletor `Tipo de Ambiente / Agente` (`Shell` ou `Agente CLI`), e tabela editável de `Variáveis de Ambiente` com chave, modo `Literal` ou `Herdada do sistema` e valor/variável de origem). Exibe erros por campo vindos de `MtError` e aviso informativo quando `environment.kind === 'agent'` ainda não tiver resolvedor ativo, sem bloquear o salvamento.
4. **`TabWorkspaceView.vue`, `PaneLayout.vue` e `PaneView.vue`:**
   - `PaneLayout.vue`: componente recursivo sobre `Splitpanes` e `Pane`. Usa `:horizontal="node.direction === 'horizontal'"` (divisor horizontal = painéis empilhados acima/abaixo; `false` = divisor vertical, lado a lado) e `:min-size="5"`. Usa keys estáveis (`node.nodeId` para `Split` e `node.paneId` para `Leaf`) para que operações de split/close nunca remontem os painéis irmãos.
   - `PaneView.vue`:
     - Cabeçalho compacto / barra de ferramentas do painel com indicador de `cwd`, botões **"Dividir horizontalmente"**, **"Dividir verticalmente"**, **"Fechar painel"** e, quando `initial_dispatch === 'waiting'`, banner com botão **"Executar comando inicial"**.
     - Quando `pane.ptyId !== null`, renderiza `<HybridTerminal :pane-id="pane.id" :pty-id="pane.ptyId" :visible="visible" />` da Fase 04.
     - Ao detectar substituição de execução (`pane://pty-replaced` ou mudança de `(ptyId, generation)` em `session_new` / `pane_restart`), descarta a instância xterm anterior via `TerminalInstanceRegistry.dispose(pane.id)` e limpa o `CommandBlockFeed` da Fase 04 antes de montar a nova execução, garantindo um único consumidor de ACK por `PtyId`.
     - Quando `pane.state === 'exited'` ou `'failed'`, exibe overlay semântico com código/sinal de saída ou mensagem `pane.error.message`, botão **"Reiniciar painel"** (`pane_restart`) e, se o erro for `template.cwd_not_found`, botão **"Reiniciar na pasta do projeto"** (`cwd_fallback_to_project: true`).
5. **`SessionHistoryDrawer.vue` e `SessionReadonlyViewer.vue`:**
   - `SessionHistoryDrawer`: lista paginada de `SessionSummary` da aba (`seq`, badge `"Ativa"` ou motivo de término em pt-BR, timestamps formatados, contagem de comandos/erros, bytes de scrollback e aviso de truncamento), campo de busca FTS5 (`session_search`) com navegação direta para a sessão/painel encontrado e seletor de painel (`SessionPaneSummary`).
   - `SessionReadonlyViewer`: exibe em modo estritamente somente leitura os blocos de comando da sessão base (via `command_block_list` da Fase 05) e os chunks paginados de scrollback (`session_scrollback_get`) renderizados como texto puro virtualizado (nunca usa `v-html`, nunca interpreta sequências ANSI ativas e não possui campo de input ou conexão com PTY).

---

## 4. Casos de Borda e Tratamento de Erros

1. **Troca de Aba sem Perder Estado do Terminal:** Alternar entre abas de um mesmo projeto ou entre projetos apenas alterna a prop `visible` (`setVisible(false)` / detach visual no `HybridTerminal` da Fase 04); as instâncias xterm no `TerminalInstanceRegistry` e os PTYs no backend continuam vivos e processando output.
2. **Eventos Recebidos Durante o Carregamento do Snapshot Inicial:** `useWorkspaceEvents` registra todos os listeners Tauri antes de invocar `workspace_snapshot()`. Eventos que chegam enquanto o snapshot está em trânsito ficam em um buffer em memória; após aplicar o snapshot (com `snapshot.eventSeq`), o composable descarta eventos do buffer com `eventSeq <= snapshot.eventSeq` e aplica em ordem estrita os eventos com `eventSeq > snapshot.eventSeq`. Se detectar salto (`event.eventSeq > lastEventSeq + 1`) ou mudança de `bootId`, recarrega `workspace_snapshot()` automaticamente.
3. **Desabilitação de Ações Conflitantes:** Enquanto `pendingByTab[tabId]` for `true` ou `tab.lifecycle` estiver em `'opening' | 'restarting' | 'closing'`, os botões de split, fechar painel, reiniciar painel e iniciar nova sessão daquela aba ficam desabilitados para evitar disparos duplicados.
