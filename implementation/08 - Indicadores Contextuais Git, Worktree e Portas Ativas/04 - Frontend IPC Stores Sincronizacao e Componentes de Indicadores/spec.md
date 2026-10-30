# Implementação 04 — Frontend: IPC, Stores Pinia, Sincronização e Componentes de Indicadores

> **Fase:** 08 — Indicadores Contextuais: Git, Worktree e Portas Ativas
> **Unidade:** `04 - Frontend IPC Stores Sincronizacao e Componentes de Indicadores`
> **Referências Canônicas:** [`../instructions.md`](../instructions.md) §2.1, §4.4, §5.1, §5.3, §5.4; [`../spec.md`](../spec.md) §6.1–§6.6, §7.1–§7.3, §8, §11.3.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação entrega toda a camada Frontend (Vue 3.6 + TypeScript estrito + Pinia + UnoCSS + SCSS Scoped + `@maxvue/max-components-ui`) da Fase 08, conectando os comandos e eventos expostos na Implementação 03 ao slot `#indicators="{ tab }"` da `TabBar` (Fase 07).

### 1.1. Objetivos Específicos
1. **Wrappers IPC Tipados (`src/ipc/git.ts`, `src/ipc/ports.ts`):** Encapsular todos os comandos e eventos gerados em `src/types/bindings.ts` usando o adaptador `unwrap` de `src/ipc/core.ts` (contrato da Fase 01).
2. **Store Volátil `useGitStore` (`src/stores/git.ts`):** Gerenciar o contexto Git por painel (`contextByPane`), controlar concorrência e ordenação por envelope (`epoch`, `revision`, `generation`) e token local de intenção, limpar imediatamente o indicador antigo ao trocar `cwd`, bufferizar eventos recebidos durante `trackPane` e expor a API de worktrees (`worktreeList`, `worktreeCreate`, `worktreeRemove`) para as Fases 13 e 16.
3. **Store Volátil `usePortsStore` (`src/stores/ports.ts`):** Gerenciar as portas ativas por aba (`portsByTab`) e snapshots de saúde (`snapshots`), instalar listeners (`opened`, `updated`, `closed`, `snapshot`) antes de carregar snapshots, reconciliar lacunas de `revision` ou mudança de `epoch`, controlar o estado `killing` por `PortId`, disparar `Toast.show` em `pt-BR` para cada desfecho de `PortKillResult` (`Terminated`, `Killed`, `SocketReleased`, `AlreadyGone`, `remaining`) e expor o getter agregado `portsForProject(projectId)`.
4. **Composable `useTabIndicatorsSync` (`src/composables/useTabIndicatorsSync.ts`):** Observar reativamente as abas e painéis de `useTabsStore` (Fase 07), os status e eventos de fim de comando de `useTabStatusStore` (Fase 05) e o foco da janela via `useWindowFocus()` (`@maxvue/max-use`), aplicando debounce de 100 ms em mudanças de `cwd` e sincronizando aberturas, fechamentos e trocas de `ptyId`.
5. **Componentes Visuais de Indicadores (`src/components/indicators/`):**
   - `GitIndicator.vue`: branch (`Named`, `Detached`, `Unborn`), ponto de dirty/conflito (`--max-warning-500` / `--max-danger-500`), contadores ahead/behind (`↑A ↓B`), ícone de upstream `gone`, ícone/tooltip de modo degradado e tooltip detalhado em `pt-BR`.
   - `WorktreeBadge.vue`: ícone `mdi:file-tree`, cores semânticas por `WorktreeKind` (`--max-primary-500` para `maxCode`, `--max-info-500` para `project`, neutro para `other`), nome da worktree e tooltip indicando a worktree principal.
   - `PortBadge.vue`: exibição `:<port>` + protocolo (`TCP`/`UDP`), botão de abrir no navegador (somente quando `canOpenBrowser`), botão de encerrar com `MaxPopoverConfirm` (capturando `PortId` e cancelando confirmação se o token mudar/fechar) e `stopPropagation` para não interferir na seleção/fechamento da aba.
   - `TabIndicators.vue`: contêiner inserido no slot `#indicators="{ tab }"` da `TabBar`, exibindo `GitIndicator`, `WorktreeBadge` e até 3 `PortBadge`, com chip `+N` (`MaxPopover`) acessível para portas excedentes.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Novos Arquivos de Código Frontend (`src/`)
- `src/ipc/git.ts`
- `src/ipc/ports.ts`
- `src/stores/git.ts`
- `src/stores/ports.ts`
- `src/composables/useTabIndicatorsSync.ts`
- `src/components/indicators/GitIndicator.vue`
- `src/components/indicators/WorktreeBadge.vue`
- `src/components/indicators/PortBadge.vue`
- `src/components/indicators/TabIndicators.vue`

### 2.2. Modificações em Arquivos Existentes (`src/`)
- `src/components/workspace/TabBar.vue` (ou `ProjectWorkspace.vue` da Fase 07) — Preencher o slot `<template #indicators="{ tab }">` com `<TabIndicators :tab="tab" />` e montar `useTabIndicatorsSync()` uma única vez no layout do workspace.

### 2.3. Novos Arquivos de Testes Unitários e de Componentes (`src/**/__tests__/`)
- `src/stores/__tests__/git.spec.ts`
- `src/stores/__tests__/ports.spec.ts`
- `src/composables/__tests__/useTabIndicatorsSync.spec.ts`
- `src/components/indicators/__tests__/GitIndicator.spec.ts`
- `src/components/indicators/__tests__/WorktreeBadge.spec.ts`
- `src/components/indicators/__tests__/PortBadge.spec.ts`
- `src/components/indicators/__tests__/TabIndicators.spec.ts`

---

## 3. Especificação Técnica Detalhada e Assinaturas TypeScript / Vue

### 3.1. Wrappers IPC (`src/ipc/git.ts` e `src/ipc/ports.ts`)

Todos os tipos são importados exclusivamente de `@/types/bindings` (proibido redeclarar interfaces manualmente).

#### `src/ipc/git.ts`
```ts
import type { UnlistenFn } from '@tauri-apps/api/event';
import { commands, events } from '@/types/bindings';
import type {
    GitContext,
    GitStatusChanged,
    PaneId,
    RefreshReason,
    TabId,
    WorktreeCreateRequest,
    WorktreeEntry
} from '@/types/bindings';
import { unwrap } from '@/ipc/core';

export const gitTrackPane = (
    tabId: TabId,
    paneId: PaneId,
    cwd: string | null,
    visible: boolean
): Promise<GitStatusChanged> => unwrap(commands.gitTrackPane(tabId, paneId, cwd, visible), 'git_track_pane');

export const gitUntrackPane = (paneId: PaneId): Promise<null> =>
    unwrap(commands.gitUntrackPane(paneId), 'git_untrack_pane');

export const gitSetVisibleTab = (tabId: TabId | null): Promise<null> =>
    unwrap(commands.gitSetVisibleTab(tabId), 'git_set_visible_tab');

export const gitRefresh = (paneId: PaneId, reason: RefreshReason): Promise<null> =>
    unwrap(commands.gitRefresh(paneId, reason), 'git_refresh');

export const gitStatus = (cwd: string): Promise<GitContext | null> =>
    unwrap(commands.gitStatus(cwd), 'git_status');

export const gitWorktreeList = (repoPath: string): Promise<WorktreeEntry[]> =>
    unwrap(commands.gitWorktreeList(repoPath), 'git_worktree_list');

export const gitWorktreeCreate = (req: WorktreeCreateRequest): Promise<WorktreeEntry> =>
    unwrap(commands.gitWorktreeCreate(req), 'git_worktree_create');

export const gitWorktreeRemove = (path: string, force = false): Promise<null> =>
    unwrap(commands.gitWorktreeRemove(path, force), 'git_worktree_remove');

export const gitWorktreePrune = (repoPath: string): Promise<null> =>
    unwrap(commands.gitWorktreePrune(repoPath), 'git_worktree_prune');

export const onGitStatusChanged = (
    cb: (payload: GitStatusChanged) => void
): Promise<UnlistenFn> => events.gitStatusChangedEvent.listen((e) => cb(e.payload));
```

#### `src/ipc/ports.ts`
```ts
import type { UnlistenFn } from '@tauri-apps/api/event';
import { commands, events } from '@/types/bindings';
import type {
    PaneId,
    PortClosed,
    PortId,
    PortKillResult,
    PortOpened,
    PortSnapshot,
    PortUpdated,
    PtyId,
    TabId,
    TabStatus
} from '@/types/bindings';
import { unwrap } from '@/ipc/core';

export const portTrackPane = (
    tabId: TabId,
    paneId: PaneId,
    ptyId: PtyId
): Promise<PortSnapshot> => unwrap(commands.portTrackPane(tabId, paneId, ptyId), 'port_track_pane');

export const portUntrackPane = (tabId: TabId, paneId: PaneId): Promise<null> =>
    unwrap(commands.portUntrackPane(tabId, paneId), 'port_untrack_pane');

export const portSetTabStatus = (tabId: TabId, status: TabStatus): Promise<null> =>
    unwrap(commands.portSetTabStatus(tabId, status), 'port_set_tab_status');

export const portList = (tabId: TabId): Promise<PortSnapshot> =>
    unwrap(commands.portList(tabId), 'port_list');

export const portRefresh = (tabId: TabId): Promise<PortSnapshot> =>
    unwrap(commands.portRefresh(tabId), 'port_refresh');

export const portOpenBrowser = (tabId: TabId, portId: PortId): Promise<null> =>
    unwrap(commands.portOpenBrowser(tabId, portId), 'port_open_browser');

export const portKill = (tabId: TabId, portId: PortId): Promise<PortKillResult> =>
    unwrap(commands.portKill(tabId, portId), 'port_kill');

export const onPortOpened = (cb: (payload: PortOpened) => void): Promise<UnlistenFn> =>
    events.portOpenedEvent.listen((e) => cb(e.payload));

export const onPortUpdated = (cb: (payload: PortUpdated) => void): Promise<UnlistenFn> =>
    events.portUpdatedEvent.listen((e) => cb(e.payload));

export const onPortClosed = (cb: (payload: PortClosed) => void): Promise<UnlistenFn> =>
    events.portClosedEvent.listen((e) => cb(e.payload));

export const onPortSnapshot = (cb: (payload: PortSnapshot) => void): Promise<UnlistenFn> =>
    events.portSnapshotEvent.listen((e) => cb(e.payload));
```

### 3.2. Store `useGitStore` (`src/stores/git.ts`)

```ts
export interface PaneGitEnvelope {
    epoch: string;
    revision: number;
    generation: number;
    cwd: string;
}
```
- **Estado:**
  - `contextByPane: Record<PaneId, GitContext | null>`
  - `envelopeByPane: Record<PaneId, PaneGitEnvelope>`
  - `loadingPanes: Record<PaneId, boolean>`
  - `listening: boolean`
- **Getters:**
  - `contextForPane: (paneId: PaneId) => GitContext | null`
  - `contextForTab: (tab: TabView) => GitContext | null` (usa `tab.activePaneId`)
  - `isDirty: (paneId: PaneId) => boolean` (`Boolean(c && (c.changes.staged + c.changes.unstaged + c.changes.untracked + c.changes.conflicted > 0))`)
- **Regras de Sincronização e Ordenação (`useGitStore`):**
  1. `init()` registra `onGitStatusChanged` de forma idempotente (compartilhando uma única `initPromise` entre chamadas concorrentes).
  2. Em `trackPane(tabId, paneId, cwd, visible)`:
     - Incrementa `intentTokenByPane[paneId]`.
     - Se o `cwd` mudou em relação ao `envelopeByPane[paneId]?.cwd`, define imediatamente `contextByPane[paneId] = null` e `loadingPanes[paneId] = true`, ativando um buffer temporário de eventos para `paneId`.
     - Ao receber a resposta de `gitTrackPane`: se `intentTokenByPane[paneId] !== myToken` (ou se o painel foi removido por `untrackPane`), descarta a resposta. Caso contrário, aplica o envelope base e reproduz apenas os eventos bufferizados que tenham a mesma `epoch`, `generation >= res.generation` e `revision > res.revision`.
  3. Ao receber evento em `onGitStatusChanged(ev)`:
     - Se o painel não está rastreado, ignora.
     - Se o painel está com `trackPane` em voo, adiciona ao buffer do painel.
     - Se `ev.epoch !== current.epoch`: solicita novo `trackPane` para ressincronizar.
     - Se `ev.generation < current.generation`: descarta evento obsoleto.
     - Se `ev.generation === current.generation && ev.revision <= current.revision`: descarta evento duplicado/fora de ordem.
     - Caso contrário, atualiza `envelopeByPane[paneId]` e `contextByPane[paneId] = ev.context`.
  4. Ações de worktree (`worktreeList`, `worktreeCreate`, `worktreeRemove`) delegam para `src/ipc/git.ts`; em caso de erro em chamadas iniciadas pelo usuário, propagam `MtIpcError` (e exibem toast quando aplicável).

### 3.3. Store `usePortsStore` (`src/stores/ports.ts`)

- **Estado:**
  - `portsByTab: Record<TabId, ListeningPort[]>`
  - `killing: Record<string, boolean>` (indexado por `PortId`)
  - `snapshots: Record<TabId, PortSnapshot>`
- **Getters:**
  - `portsForTab: (tabId: TabId) => ListeningPort[]`
  - `portsForProject: (projectId: ProjectId) => ListeningPort[]` (agrega as abas abertas do projeto consultando `useTabsStore`)
- **Regras de Sincronização, Ordenação e Toasts (`usePortsStore`):**
  1. Função pura `sortPorts(ports: ListeningPort[]): ListeningPort[]`: ordena por `a.port - b.port`, depois `a.protocol.localeCompare(b.protocol)`, depois `a.paneId.localeCompare(b.paneId)`, depois `a.pid - b.pid`, deduplicando por `id`.
  2. Protocolo de Revisão e Lacunas:
     - Cada snapshot define `(epoch, revision)` base da aba.
     - Eventos delta (`opened`, `updated`, `closed`) para `tabId`:
       - Se chegarem enquanto `trackPane`/`refresh` inicial da aba está em voo, ficam no buffer da aba e são aplicados após o snapshot se `revision > snapshot.revision`.
       - Se `ev.epoch !== snap.epoch` ou `ev.revision > snap.revision + 1` (lacuna detectada!), dispara automaticamente `refresh(tabId)` sem aplicar o delta isolado.
       - Se `ev.revision <= snap.revision`, ignora como duplicata.
       - Se `ev.revision === snap.revision + 1`, aplica o delta em `portsByTab[tabId]`, reordena com `sortPorts` e avança `snap.revision = ev.revision`.
  3. Ação `kill(tabId, portId)`:
     - Marca `killing[portId] = true`.
     - Em bloco `try / finally`, garante `delete killing[portId]` mesmo se a aba fechar ou ocorrer exceção.
     - Trata `PortKillResult`:
       - `outcome === 'terminated'`: `Toast.show({ title: 'Processo encerrado com sucesso', severity: 'success' })`.
       - `outcome === 'killed'`: `Toast.show({ title: 'Processo encerrado à força', severity: 'warning' })`.
       - `outcome === 'socketReleased'`: `Toast.show({ title: 'Socket liberado; processo continua vivo', severity: 'info' })`.
       - `outcome === 'alreadyGone'`: `Toast.show({ title: 'O processo já havia encerrado', severity: 'info' })`.
       - Se `res.remaining.length > 0` na mesma porta/socket compartilhado: informa em complemento que outros processos ainda escutam na porta.
     - Em caso de erro (`MtIpcError`), exibe `Toast.show({ title: err.message, severity: 'error' })` e relança o erro.
  4. Ação `openInBrowser(tabId, portId)`:
     - Chama `portOpenBrowser(tabId, portId)`. Em caso de erro (`ports.opener_failed`, `ports.not_browser_compatible`, etc.), exibe `Toast.show({ title: err.message, severity: 'error' })`.

### 3.4. Composable `useTabIndicatorsSync` (`src/composables/useTabIndicatorsSync.ts`)

- Inicializa `gitStore.init()` e `portsStore.init()`.
- Mantém mapa local `trackedPanes = new Map<PaneId, { tabId: TabId; cwd: string; ptyId: PtyId | null }>()`.
- Observa a projeção de painéis de `useTabsStore().tabsById`:
  - Painel novo: chama `gitStore.trackPane(tab.id, pane.id, pane.cwd || null, isTabVisible)` e, se `pane.ptyId !== null`, `portsStore.trackPane(tab.id, pane.id, pane.ptyId)`.
  - Mudança de `pane.cwd`: agenda chamada debounced de 100 ms por `pane.id` para `gitStore.trackPane(tab.id, pane.id, pane.cwd, isTabVisible)`.
  - Mudança de `pane.ptyId` (substituição/restart/nova sessão): chama `portsStore.untrackPane(tab.id, pane.id)` para o PTY antigo e, se o novo `pane.ptyId` não for `null`, chama `portsStore.trackPane(tab.id, pane.id, pane.ptyId)` e `gitStore.trackPane(...)`.
  - Painel removido / aba fechada: cancela timer de debounce do painel e chama `gitStore.untrackPane(paneId)` e `portsStore.untrackPane(tabId, paneId)`.
- Observa status da aba na store da Fase 05 ⇒ `portsStore.setTabStatus(tabId, status)`.
- Observa conclusão de comando por painel/PTY (OSC 133 `D` da Fase 05) ⇒ `gitStore.refresh(paneId, 'commandFinished')` (mesmo quando outro painel da mesma aba continua em execução).
- Observa aba ativa e `useWindowFocus()` (`@maxvue/max-use`):
  - Mudança de aba ativa ou perda/ganho de visibilidade ⇒ `gitStore.setVisibleTab(activeTabId)`.
  - Ao ganhar foco na janela com uma aba ativa ⇒ `gitStore.refresh(activePaneId, 'windowFocus')`.
- Registra limpeza completa em `onScopeDispose` (limpa timers de debounce e remove listeners).

### 3.5. Especificação dos Componentes Vue (`src/components/indicators/`)

Todos os componentes seguem **estritamente** as regras do `AGENTS.md` §3:
- Ordem obrigatória dos blocos: `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`.
- Recuo de 4 espaços (`baseIndent: 1` dentro de `<script setup>`), aspas simples (`'single'`), ponto e vírgula obrigatório (`;`), sem vírgula final (`comma-dangle: 'never'`).
- **Tags de abertura em linha única** no `<template>`.
- Tokens semânticos `--max-*` e `--background-*`.

#### 1. `GitIndicator.vue`
- **Props:** `defineProps<{ context: GitContext; compact?: boolean }>()`
- **Exibição:**
  - Ícone principal via `<MaxIcon :name="branchIcon" />`: `'mdi:source-commit'` quando `context.branch.kind === 'detached'`, senão `'mdi:source-branch'`.
  - Texto da branch: `shortOid` (7 chars) em `detached` (com classe estilizada monoespaçada/destacada), ou `name` truncado em 24 caracteres com reticências (`…`).
  - Indicador visual de `dirty`: bolinha (`.git-indicator__dot`) exibida quando `isDirty` for `true`; cor `var(--max-danger-500)` se `changes.conflicted > 0`, senão `var(--max-warning-500)`.
  - Sincronismo Upstream: se `context.upstream?.gone`, exibe `<MaxIcon name="mdi:cloud-off-outline" />`; senão, exibe `↑{{ context.upstream.ahead }}` (se `> 0`) e `↓{{ context.upstream.behind }}` (se `> 0`).
  - Modo Degradado (`context.degraded !== null`): exibe ícone discreto (`'mdi:clock-alert-outline'` para `'timeout'`, `'mdi:alert-outline'` para os demais) com mensagem em `pt-BR`:
    - `'timeout'`: `'Status do Git excedeu o tempo limite'`
    - `'gitNotFound'`: `'Git não encontrado — status reduzido'`
    - `'unsafeRepository'`: `'Repositório bloqueado por safe.directory'`
    - `'watchUnavailable'`: `'Monitoramento indisponível — atualização periódica'`
    - `'commandFailed'`: `'Falha ao consultar status do Git'`
  - Tooltip (`v-tooltip.bottom="tooltipText"`):
    - Quando `changes !== null`: `'Branch: <nome> · Preparados: <staged> · Modificados: <unstaged> · Não rastreados: <untracked> · Conflitos: <conflicted>'` (acrescentando o texto de `degraded` se presente).

#### 2. `WorktreeBadge.vue`
- **Props:** `defineProps<{ worktree: GitWorktree }>()`
- **Exibição:**
  - `<MaxIcon name="mdi:file-tree" />` + texto `{{ worktree.name }}`.
  - Variante de cor CSS por `worktree.kind`:
    - `'maxCode'` ⇒ cor/borda baseada em `var(--max-primary-500)`
    - `'project'` ⇒ cor/borda baseada em `var(--max-info-500)`
    - `'other'` ⇒ neutro (`var(--max-text-secondary)` / `var(--background-tertiary)`)
  - Tooltip (`v-tooltip.bottom="tooltipText"`): se `worktree.mainWorkTree` existir, `'Worktree de ' + worktree.mainWorkTree`; caso contrário, `'Worktree vinculada'`.

#### 3. `PortBadge.vue`
- **Props e Emits:**
  ```ts
  const props = defineProps<{ port: ListeningPort; tabId: TabId; stale?: boolean; killing?: boolean }>();
  const emit = defineEmits<{ open: [portId: PortId]; kill: [portId: PortId] }>();
  ```
- **Acessibilidade e Isolamento de Eventos:**
  - O elemento raiz usa `@click.stop` e `@mousedown.stop` para impedir que clicar no badge selecione ou arraste a aba.
  - Botões HTML `<button type="button">` separados (jamais aninhados!).
  - Se `port.canOpenBrowser && !stale && !killing`: o botão principal `:<port>` (`aria-label="Abrir porta :<port> no navegador"`) dispara `emit('open', port.id)`. Quando `port.protocol === 'udp'`, exibe rótulo `UDP :<port>` sem ação de navegador.
  - Botão de encerramento (`<MaxIcon name="mdi:close" />` ou spinner quando `killing`) envolto em `MaxPopoverConfirm`:
    - Mensagem de confirmação: `"Encerrar ${port.processName} (PID ${port.pid})? Todas as portas desse processo serão afetadas."`
    - Rótulo do botão de confirmação: `'Encerrar'`.
    - Captura `confirmedPortId = port.id` ao abrir o popover; um `watch(() => props.port.id)` fecha/cancela o popover imediatamente se o `PortId` mudar enquanto o popover estiver aberto.
    - Se `!port.canKill || stale`, o botão de encerrar fica `disabled` com tooltip explicando o motivo (`'Atualização indisponível'` quando `stale`, `'Recusado: o processo é o shell da aba'` ou `'Sinalização segura indisponível'`).

#### 4. `TabIndicators.vue`
- **Props:** `defineProps<{ tab: TabView; compact?: boolean }>()`
- **Comportamento:**
  - Obtém `context = computed(() => gitStore.contextForTab(props.tab))` e `ports = computed(() => portsStore.portsForTab(props.tab.id))`.
  - Renderiza `<GitIndicator v-if="context" :context="context" :compact="compact" />`.
  - Renderiza `<WorktreeBadge v-if="context?.worktree" :worktree="context.worktree" />`.
  - Renderiza até 3 `<PortBadge>` (`visiblePorts = ports.slice(0, 3)`).
  - Se `ports.length > 3`, renderiza um botão chip `+{{ ports.length - 3 }}` com `MaxPopover` listando todos os `<PortBadge>` excedentes (`overflowPorts = ports.slice(3)`) com suporte completo às ações de abrir no navegador e encerrar processo.
