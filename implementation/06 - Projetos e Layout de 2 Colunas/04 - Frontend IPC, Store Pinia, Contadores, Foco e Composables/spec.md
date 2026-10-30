# Implementação 04 — Frontend IPC, Store Pinia, Contadores, Foco e Composables (`spec.md`)

> Parte 04 de 05 da **Fase 06: Projetos e Layout de 2 Colunas**.
> Referências canônicas: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) §6.1–§6.7, §8, §9 e §11.2.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação constrói toda a camada de estado, comunicação IPC, agregação reativa de contadores de abas, coordenação de foco/visualização com a Fase 05, persistência de layout com a Fase 03 e o slot de extensão de badges para a Fase 16 no frontend Vue 3.6 + TypeScript estrito (`tsgo`).

### 1.1. Escopo Incluído
- Extensão de `SettingsSchema` via declaration merging em `src/types/projectSettings.ts`.
- Catálogo somente-leitura de ícones/cores sugeridos por stack e registro offline das coleções Iconify (`@iconify-json/logos` e `@iconify-json/mdi`) em `src/constants/projectIdentity.ts`.
- Wrappers tipados de IPC e listeners de eventos Tauri + `pickFolder` em `src/ipc/projects.ts`.
- Registry por instância da aplicação (provide/inject) para extensão de badges (`useProjectBadgeRegistry.ts`).
- Store Pinia `useProjectsStore` (`src/stores/projects.ts`) com hidratação consistente por `ProjectSnapshot` (`epoch`/`revision`), single-flight `resync`, vínculo em memória `tabOwner` / `activeTabByProject` e agregação `O(abas)` em `countersByProject` com estabilidade referencial.
- Composables `useProjectEvents.ts`, `useProjectTabFocus.ts` e `useLayoutPersistence.ts`.
- Suíte de testes unitários Vitest cobrindo store, contadores, foco/ACK da Fase 05, eventos concorrentes e persistência de proporção das colunas.

### 1.2. Fora de Escopo desta Implementação
- Componentes `.vue` (`AppLayout.vue`, `ProjectList.vue`, `ProjectItem.vue`, modais, etc.) — cobertos na **Implementação 05**.

---

## 2. Arquivos a Criar ou Modificar

| Ação | Caminho Exato | Descrição |
|------|---------------|-----------|
| Modificar | `package.json` | Garantir dependências `@tauri-apps/plugin-dialog`, `@iconify-json/logos` e `@iconify-json/mdi` |
| Criar | `src/types/projectSettings.ts` | Extensão de `SettingsSchema` com `'projects.selected_id'` e `'layout.columns.left_size'` |
| Criar | `src/constants/projectIdentity.ts` | Constantes de ícones/cores por stack, paleta fallback e registro offline de ícones Iconify |
| Criar | `src/ipc/projects.ts` | Wrappers IPC sobre `commands.*`, `onProject*` listeners e `pickFolder` |
| Criar | `src/composables/useProjectBadgeRegistry.ts` | `ProjectBadgeProvider`, `ProjectBadgeRegistry`, `provideProjectBadgeRegistry`, `useProjectBadgeRegistry` |
| Criar | `src/stores/projects.ts` | Store `useProjectsStore` e interface `ProjectStatusCounters` |
| Criar | `src/composables/useProjectEvents.ts` | Assinatura de `project://*`, coalescência de 50 ms, polling de 30 s e refresh no foco |
| Criar | `src/composables/useProjectTabFocus.ts` | Integração de seleção/aba ativa e `ProjectPresentationSource` ao `useTabSeenTracker` da Fase 05 |
| Criar | `src/composables/useLayoutPersistence.ts` | Persistência de `layout.columns.left_size` (15..45, default 22) via `useSetting` da Fase 03 |
| Criar | `tests/frontend/projects/projects.store.spec.ts` | Testes unitários de `useProjectsStore` e `countersByProject` |
| Criar | `tests/frontend/projects/useProjectTabFocus.spec.ts` | Testes de integração com `useTabSeenTracker` da Fase 05 |
| Criar | `tests/frontend/projects/useProjectEvents.spec.ts` | Testes de `useProjectEvents` (corridas de snapshot, unmount tardio, epoch/revision) |
| Criar | `tests/frontend/projects/useLayoutPersistence.spec.ts` | Testes de `useLayoutPersistence` (debounce, limites 15..45, adapter `splitpanes`) |

---

## 3. Especificação Técnica Detalhada (TypeScript)

### 3.1. Extensão de Settings (`src/types/projectSettings.ts`)

```ts
import type { ProjectId } from '@/types/bindings';

declare module '@/types/settings' {
    interface SettingsSchema {
        'projects.selected_id': ProjectId | null;
        'layout.columns.left_size': number;
    }
}

export const PROJECT_SETTINGS_KEYS = {
    selectedId: 'projects.selected_id',
    leftColumnSize: 'layout.columns.left_size'
} as const;
```

### 3.2. Constantes de Identidade e Registro Offline Iconify (`src/constants/projectIdentity.ts`)

```ts
import type { StackKind } from '@/types/bindings';

export interface StackIdentitySuggestion {
    stack: StackKind;
    label: string;
    icon: string;
    color: string;
}

export const STACK_IDENTITY_SUGGESTIONS: readonly StackIdentitySuggestion[] = [
    { stack: 'tauri', label: 'Tauri', icon: 'logos:tauri', color: '#FFC131' },
    { stack: 'vscode_extension', label: 'Extensão VS Code', icon: 'logos:visual-studio-code', color: '#007ACC' },
    { stack: 'laravel', label: 'Laravel', icon: 'logos:laravel', color: '#FF2D20' },
    { stack: 'vue', label: 'Vue.js', icon: 'logos:vue', color: '#42B883' },
    { stack: 'rust', label: 'Rust', icon: 'logos:rust', color: '#CE422B' },
    { stack: 'python', label: 'Python', icon: 'logos:python', color: '#3776AB' }
] as const;

export const FALLBACK_PROJECT_ICONS: readonly string[] = [
    'mdi:folder',
    'mdi:folder-star',
    'mdi:folder-code',
    'mdi:folder-heart',
    'mdi:folder-cog',
    'mdi:folder-wrench',
    'mdi:folder-table',
    'mdi:folder-text',
    'mdi:folder-multiple',
    'mdi:folder-network',
    'mdi:folder-zip',
    'mdi:folder-file'
] as const;

export const FALLBACK_PROJECT_COLORS: readonly string[] = [
    '#14B8A6',
    '#0EA5E9',
    '#6366F1',
    '#A855F7',
    '#EC4899',
    '#F43F5E',
    '#F97316',
    '#EAB308',
    '#84CC16',
    '#22C55E',
    '#06B6D4',
    '#64748B'
] as const;

export const DEFAULT_PROJECT_ICON = 'mdi:folder';

export const ICONIFY_NAME_REGEX = /^[a-z0-9]+(-[a-z0-9]+)*:[a-z0-9]+(-[a-z0-9]+)*$/;
export const HEX_COLOR_REGEX = /^#[0-9A-Fa-f]{6}$/;

/**
 * Registra os ícones necessários de `@iconify-json/logos` e `@iconify-json/mdi`
 * localmente (sem fetch de rede) e resolve fallback seguro para `mdi:folder`.
 */
export function ensureProjectIconsRegistered(): void;
export function resolveSafeProjectIcon(icon: string | null | undefined): string;
```

### 3.3. Wrappers IPC (`src/ipc/projects.ts`)

```ts
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type DiscoveredFolder,
    type NewProject,
    type NewProjectRoot,
    type Project,
    type ProjectChangedPayload,
    type ProjectDiscoveredPayload,
    type ProjectEvent,
    type ProjectId,
    type ProjectIdentity,
    type ProjectPatch,
    type ProjectRemovedPayload,
    type ProjectRoot,
    type ProjectRootChangedPayload,
    type ProjectRootId,
    type ProjectRootPatch,
    type ProjectSnapshot,
    type RootImportReport
} from '@/types/bindings';

export const PROJECT_EVENTS = {
    discovered: 'project://discovered',
    removed: 'project://removed',
    changed: 'project://changed',
    rootChanged: 'project://root-changed'
} as const;

export const projectSnapshot = (): Promise<ProjectSnapshot> =>
    unwrap(commands.projectSnapshot(), 'project_snapshot');

export const projectList = (): Promise<Project[]> =>
    unwrap(commands.projectList(), 'project_list');

export const projectAdd = (input: NewProject): Promise<Project> =>
    unwrap(commands.projectAdd(input), 'project_add');

export const projectUpdate = (id: ProjectId, patch: ProjectPatch): Promise<Project> =>
    unwrap(commands.projectUpdate(id, patch), 'project_update');

export const projectRemove = (id: ProjectId): Promise<void> =>
    unwrap(commands.projectRemove(id), 'project_remove');

export const projectReorder = (ids: ProjectId[]): Promise<void> =>
    unwrap(commands.projectReorder(ids), 'project_reorder');

export const projectRedetect = (id: ProjectId): Promise<Project> =>
    unwrap(commands.projectRedetect(id), 'project_redetect');

export const projectPreviewIdentity = (path: string): Promise<ProjectIdentity> =>
    unwrap(commands.projectPreviewIdentity(path), 'project_preview_identity');

export const projectRefreshMissing = (): Promise<Project[]> =>
    unwrap(commands.projectRefreshMissing(), 'project_refresh_missing');

export const projectRootList = (): Promise<ProjectRoot[]> =>
    unwrap(commands.projectRootList(), 'project_root_list');

export const projectRootAdd = (input: NewProjectRoot): Promise<RootImportReport> =>
    unwrap(commands.projectRootAdd(input), 'project_root_add');

export const projectRootUpdate = (id: ProjectRootId, patch: ProjectRootPatch): Promise<ProjectRoot> =>
    unwrap(commands.projectRootUpdate(id, patch), 'project_root_update');

export const projectRootRemove = (id: ProjectRootId): Promise<void> =>
    unwrap(commands.projectRootRemove(id), 'project_root_remove');

export const projectRootRescan = (id: ProjectRootId): Promise<RootImportReport> =>
    unwrap(commands.projectRootRescan(id), 'project_root_rescan');

export const projectDiscoveryList = (): Promise<DiscoveredFolder[]> =>
    unwrap(commands.projectDiscoveryList(), 'project_discovery_list');

export const projectDiscoveryAccept = (paths: string[]): Promise<Project[]> =>
    unwrap(commands.projectDiscoveryAccept(paths), 'project_discovery_accept');

export const projectDiscoveryDismiss = (paths: string[]): Promise<void> =>
    unwrap(commands.projectDiscoveryDismiss(paths), 'project_discovery_dismiss');

export function onProjectDiscovered(
    cb: (p: ProjectEvent<ProjectDiscoveredPayload>) => void
): Promise<UnlistenFn> {
    return listen<ProjectEvent<ProjectDiscoveredPayload>>(PROJECT_EVENTS.discovered, (e) => cb(e.payload));
}

export function onProjectRemoved(
    cb: (p: ProjectEvent<ProjectRemovedPayload>) => void
): Promise<UnlistenFn> {
    return listen<ProjectEvent<ProjectRemovedPayload>>(PROJECT_EVENTS.removed, (e) => cb(e.payload));
}

export function onProjectChanged(
    cb: (p: ProjectEvent<ProjectChangedPayload>) => void
): Promise<UnlistenFn> {
    return listen<ProjectEvent<ProjectChangedPayload>>(PROJECT_EVENTS.changed, (e) => cb(e.payload));
}

export function onProjectRootChanged(
    cb: (p: ProjectEvent<ProjectRootChangedPayload>) => void
): Promise<UnlistenFn> {
    return listen<ProjectEvent<ProjectRootChangedPayload>>(PROJECT_EVENTS.rootChanged, (e) => cb(e.payload));
}

export async function pickFolder(title: string): Promise<string | null> {
    const selected = await open({
        title,
        directory: true,
        multiple: false
    });
    return typeof selected === 'string' ? selected : null;
}
```

### 3.4. Registry de Extensão de Badges (`src/composables/useProjectBadgeRegistry.ts`)

```ts
import {
    inject,
    onScopeDispose,
    provide,
    readonly,
    shallowRef,
    type InjectionKey,
    type Ref
} from 'vue';
import type { ProjectId } from '@/types/bindings';

export interface ProjectBadgeProvider {
    id: string;
    order: number;                                   // Nativos ocupam 10..50; extensões exigem order >= 60
    icon: string;
    colorVar: string;
    count: (projectId: ProjectId) => number | null;
    tooltip: (count: number) => string;
}

export interface ProjectBadgeRegistry {
    register(provider: ProjectBadgeProvider): () => void;
    providers: Readonly<Ref<readonly ProjectBadgeProvider[]>>;
}

export const PROJECT_BADGE_REGISTRY_KEY: InjectionKey<ProjectBadgeRegistry> =
    Symbol('MaxTerminal.ProjectBadgeRegistry');

export function createProjectBadgeRegistry(): ProjectBadgeRegistry;
export function provideProjectBadgeRegistry(): ProjectBadgeRegistry;
export function useProjectBadgeRegistry(): ProjectBadgeRegistry;
```

**Regras do Registry (`useProjectBadgeRegistry.ts`):**
- Isolado por instância da aplicação via `provide`/`inject` (se `inject` for chamado fora de uma árvore com `provideProjectBadgeRegistry`, cria instância local isolada de fallback sem compartilhar estado global mutável entre testes).
- `register(provider)`:
  - Se `!provider.id.trim()`, lança `Error('ID de provider de badge inválido.')`.
  - Se `provider.order < 60` ou `!Number.isFinite(provider.order)`, lança `Error('Providers de extensão devem usar order >= 60.')`.
  - Se já existir um provider registrado com o mesmo `id`, lança `Error(\`Provider de badge "\${provider.id}" já está registrado.\`)`.
  - Insere e ordena `providers` por `(a.order - b.order) || a.id.localeCompare(b.id)`.
  - Registra limpeza automática via `onScopeDispose` (se houver scope ativo) e retorna função `dispose()` idempotente.

### 3.5. Store `useProjectsStore` (`src/stores/projects.ts`)

```ts
import type {
    DiscoveredFolder,
    NewProject,
    NewProjectRoot,
    Project,
    ProjectId,
    ProjectPatch,
    ProjectRoot,
    ProjectRootId,
    ProjectRootPatch,
    ProjectSnapshot,
    RootImportReport,
    TabId
} from '@/types/bindings';

export interface ProjectStatusCounters {
    running: number;
    continuous: number;
    finished: number;
    error: number;
    idle: number;
}

export const ZERO_PROJECT_COUNTERS: Readonly<ProjectStatusCounters> = Object.freeze({
    running: 0,
    continuous: 0,
    finished: 0,
    error: 0,
    idle: 0
});
```

**Regras Críticas de `useProjectsStore`:**
1. **Hidratação e Consistência (`applySnapshot`, `invalidate`, `resync`)**:
   - `applySnapshot(snapshot)`:
     - Se `epoch.value === snapshot.epoch && snapshot.revision < revision.value`, descarta o snapshot obsoleto.
     - Atualiza `epoch.value = snapshot.epoch`, `revision.value = snapshot.revision`, `projects.value = snapshot.projects`, `roots.value = snapshot.roots`, `discovered.value = snapshot.discovered`.
     - Remove de `tabOwner` e `activeTabByProject` quaisquer projetos que não existam mais em `snapshot.projects`.
     - Reconcilia `selectedId`: se o projeto anteriormente selecionado foi removido (ou se na hidratação inicial o ID salvo nas settings não existir no catálogo), seleciona o vizinho na ordem anterior (próximo índice, senão anterior, senão `projects[0]?.id ?? null`) e persiste a nova seleção nas settings.
   - `resync()`:
     - Single-flight: se uma chamada `resync()` já estiver em voo, marca `resyncDirty = true` e reutiliza a promise existente.
     - Se `invalidate({ epoch: evEpoch, revision: evRev })` chegar enquanto `resync()` está em andamento (ou com revisão maior que a retornada pelo snapshot), repete a busca de `projectSnapshot()` até que `epoch === targetEpoch && revision >= targetRevision` (ou que `epoch` mude).
   - Token de ciclo de vida (`lifecycleToken`): incrementado em `dispose()` para garantir que respostas assíncronas tardias após desmontagem/reset da store sejam descartadas.
2. **Agregação de Contadores (`countersByProject` e `countersFor`)**:
   - Único `computed` percorrendo `Object.entries(tabOwner.value)` e consultando `tabStatusStore.byTab[tabId]`:
     - Ignora `tabId` cujo snapshot não exista em `tabStatusStore.byTab` ou cujo `projectId` não exista em `projects`.
     - Incrementa `c[entry.badgeBucket]++` (`running`, `continuous`, `finished`, `error` ou `idle`).
     - **Estabilidade referencial**: mantém um cache interno `previousCountersByProject: Map<ProjectId, ProjectStatusCounters>`. Se para um `projectId` os 5 números (`running`, `continuous`, `finished`, `error`, `idle`) forem idênticos aos do cálculo anterior, reutiliza a mesma referência de objeto congelado (`Object.freeze(c)`), evitando re-render de `ProjectItem` inalterados!
   - `countersFor(id)` retorna `countersByProject.value.get(id) ?? ZERO_PROJECT_COUNTERS`.
3. **Pontos de Extensão de Abas (`attachTab`, `detachTab`, `setActiveTab`)**:
   - `attachTab(tabId, projectId)`: exige que `projectById(projectId)` exista; idempotente para o mesmo `(tabId, projectId)`; lança erro se `tabId` já estiver vinculado a outro `projectId` sem `detachTab` prévio.
   - `detachTab(tabId)`: remove de `tabOwner` e, se era a `activeTabByProject[ownerId]`, limpa para `null`.
   - `setActiveTab(projectId, tabId)`: aceita `null` ou `tabId` cujo `tabOwner[tabId] === projectId`.

### 3.6. Composables (`useProjectEvents.ts`, `useProjectTabFocus.ts`, `useLayoutPersistence.ts`)

1. **`useProjectEvents()` (`src/composables/useProjectEvents.ts`)**:
   - Registra `onProjectDiscovered`, `onProjectRemoved`, `onProjectChanged` e `onProjectRootChanged` **antes** de chamar `projectsStore.init()`.
   - Se o componente desmontar enquanto o `Promise.all` dos listeners ainda está resolvendo, invoca `unlisten()` imediatamente assim que cada promise resolver.
   - Ao receber qualquer um dos 4 eventos, chama `projectsStore.invalidate({ epoch: ev.epoch, revision: ev.revision })` com coalescência de `50 ms`.
   - Mantém intervalo de `30_000 ms` (ativo quando `document.visibilityState === 'visible'`) e listeners de `window.focus` / `document.visibilitychange` (com throttle de `2_000 ms`) que disparam `projectRefreshMissing()`, `projectsStore.resync()` e rescan coalescido das raízes habilitadas.

2. **`useProjectTabFocus(source?: ProjectPresentationSource)` (`src/composables/useProjectTabFocus.ts`)**:
   ```ts
   import type { Ref } from 'vue';
   import type { PaneId, SeenRequest } from '@/types/bindings';

   export interface ProjectPresentationSource {
       visiblePaneIds: Readonly<Ref<readonly PaneId[]>>;
       presentedConclusions: Readonly<Ref<readonly SeenRequest[]>>;
   }

   export function useProjectTabFocus(source?: ProjectPresentationSource): void;
   ```
   - Computa `activeTabId` a partir de `projectsStore.selectedId`, `projectsStore.activeTabByProject` e `projectsStore.tabOwner`.
   - Conecta `activeTabId`, `visiblePaneIds` e `presentedConclusions` ao **único** `useTabSeenTracker` da Fase 05.
   - Filtra `presentedConclusions` para aceitar apenas `SeenRequest` cujo `tabId === activeTabId.value` e cujo `paneId` esteja contido em `visiblePaneIds.value`.
   - Nunca marca `seen` diretamente na store nem envia ACK apenas porque o usuário clicou no projeto na sidebar.

3. **`useLayoutPersistence(key = 'layout.columns.left_size')` (`src/composables/useLayoutPersistence.ts`)**:
   - Consome `useSetting(key, 22, { debounceMs: 300 })` da Fase 03.
   - Normaliza valores lidos: se não for `number` finito, usa `22`; se finito fora de `15..45`, aplica `Math.min(45, Math.max(15, val))`.
   - `onResized(event: unknown)`: suporta tanto o formato de objeto `{ panes: [{ size: number }, { size: number }] }` quanto o formato de array `[{ size: number }, { size: number }]` do `splitpanes`. Só atualiza `leftSize` se houver exatamente 2 panes com `size` numérico finito, aplicando clamp `15..45`. Ignora payloads inválidos sem sobrescrever com `22`.
