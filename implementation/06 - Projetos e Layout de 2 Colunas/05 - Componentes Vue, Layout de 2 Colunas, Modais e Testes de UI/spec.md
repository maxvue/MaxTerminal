# Implementação 05 — Componentes Vue, Layout de 2 Colunas, Modais e Testes de UI (`spec.md`)

> Parte 05 de 05 da **Fase 06: Projetos e Layout de 2 Colunas**.
> Referências canônicas: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) §6.8, §7, §8, §10, §11.2 e §11.3.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação entrega toda a interface gráfica da Fase 06: o layout principal de **2 colunas redimensionáveis** (`AppLayout.vue` com `splitpanes`), a coluna esquerda completa de projetos (`ProjectList.vue`, `ProjectItem.vue`, `ProjectStatusBadges.vue`, `ProjectDiscoveryPanel.vue`, `ProjectEmptyState.vue`), os três modais de gerenciamento (`ProjectAddModal.vue`, `ProjectRootModal.vue`, `ProjectEditModal.vue`), o placeholder da coluna direita (`ProjectWorkspace.vue`) preparado para receber a Fase 07 via `<slot />`, a integração em `src/app/App.vue` e os testes de componentes Vitest.

### 1.1. Escopo Incluído
- `src/components/layout/AppLayout.vue` com `splitpanes` (`:horizontal="false"`, `:dbl-click-splitter="false"`, coluna esquerda `min-size="15"`, `max-size="45"`), divisor acessível por teclado e montagem de `provideProjectBadgeRegistry`, `useProjectEvents`, `useProjectTabFocus` e `useLayoutPersistence`.
- `src/components/projects/ProjectStatusBadges.vue` renderizando os badges semânticos 1–5 + providers do `useProjectBadgeRegistry`.
- `src/components/projects/ProjectItem.vue` com `MaxIcon`, cor do projeto (`--project-color`), indicador de pasta ausente (`missing`), badges e menu contextual com confirmação de remoção (`MaxPopoverConfirm`).
- `src/components/projects/ProjectDiscoveryPanel.vue` para aceitar ou dispensar subpastas descobertas individualmente ou em lote.
- `src/components/projects/ProjectEmptyState.vue` para estado vazio sem projetos cadastrados.
- `src/components/projects/ProjectList.vue` com cabeçalho de ações, status de raízes monitoradas, navegação por setas (`↑/↓/Enter`) e reordenação por Drag-and-Drop nativo + teclado (`Alt+↑/↓`).
- `src/components/projects/ProjectAddModal.vue`, `src/components/projects/ProjectRootModal.vue` e `src/components/projects/ProjectEditModal.vue`.
- `src/components/projects/ProjectWorkspace.vue` (placeholder da coluna direita com `<slot />`).
- Integração de `AppLayout.vue` na tela principal e testes de componentes Vitest (`ProjectStatusBadges.spec.ts`, `ProjectItem.spec.ts`, `ProjectAddModal.spec.ts`, `ProjectList.spec.ts`).

### 1.2. Padrões Obrigatórios de SFC Vue (`AGENTS.md` §3)
- Ordem estrita dos blocos: `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`.
- Tags de abertura em linha única no `<template>` (proibido quebrar atributos em múltiplas linhas).
- Recuo de 4 espaços (`baseIndent: 1` em `<script setup>`), aspas simples (`'`), ponto e vírgula obrigatório (`;`) e sem vírgula final (`comma-dangle: 'never'`).
- Todos os textos visíveis, `aria-label` e tooltips estritamente em Português do Brasil (`pt-BR`).

---

## 2. Arquivos a Criar ou Modificar

| Ação | Caminho Exato | Descrição |
|------|---------------|-----------|
| Criar | `src/components/projects/ProjectStatusBadges.vue` | Badges 1–5 (`running`, `continuous`, `finished`, `error`, `idle`) + extensões do registry |
| Criar | `src/components/projects/ProjectItem.vue` | Item da lista de projetos com ícone, título, caminho, estado `missing`, badges e ações |
| Criar | `src/components/projects/ProjectDiscoveryPanel.vue` | Painel colapsável "Novas pastas encontradas (N)" |
| Criar | `src/components/projects/ProjectEmptyState.vue` | Estado vazio "Nenhum projeto cadastrado" |
| Criar | `src/components/projects/ProjectAddModal.vue` | Modal de adição de projeto individual com preview reativo de stack/ícone/cor |
| Criar | `src/components/projects/ProjectRootModal.vue` | Modal de adição/edição de Pasta de Projetos (`suggest` / `auto_import`) |
| Criar | `src/components/projects/ProjectEditModal.vue` | Modal de edição de projeto (`title`, `icon`, `color`, `resetIdentity`) |
| Criar | `src/components/projects/ProjectList.vue` | Coluna esquerda completa de projetos, cabeçalho, raízes, sugestões, lista e modais |
| Criar | `src/components/projects/ProjectWorkspace.vue` | Coluna direita (placeholder da Fase 06 com `<slot />` para a Fase 07) |
| Criar | `src/components/layout/AppLayout.vue` | Layout principal de 2 colunas com `splitpanes` |
| Modificar | `src/app/App.vue` | Renderizar `<AppLayout />` como container principal da janela |
| Criar | `tests/frontend/projects/ProjectStatusBadges.spec.ts` | Testes de renderização, ocultação de zeros, ordem, `aria-label` e registry de Issues |
| Criar | `tests/frontend/projects/ProjectItem.spec.ts` | Testes de `ProjectItem.vue` (`MaxIcon`, `missing`, seleção e emits de ações) |
| Criar | `tests/frontend/projects/ProjectAddModal.spec.ts` | Testes de `ProjectAddModal.vue` (preview debounced, anti-corrida, auto vs manual, erro inline) |
| Criar | `tests/frontend/projects/ProjectList.spec.ts` | Testes de `ProjectList.vue` e `AppLayout.vue` (teclado, reordenação, estado vazio, sugestões) |

---

## 3. Especificação Técnica dos Componentes Vue

### 3.1. `src/components/projects/ProjectStatusBadges.vue`

- **Props e Emits:**
  ```ts
  interface Props {
      projectId: ProjectId;
      counters: ProjectStatusCounters;
      compact?: boolean;
  }
  const props = withDefaults(defineProps<Props>(), {
      compact: false
  });
  ```
- **Ordem e Semântica dos 5 Badges Nativos (`order: 10..50`):**
  1. `running` (`order: 10`): cor `--max-success-500`, rótulo `"Executando"`, `aria-label`: `"{n} aba(s) executando"`.
  2. `continuous` (`order: 20`): cor `--max-info-500`, rótulo `"Execução contínua"`, `aria-label`: `"{n} aba(s) em execução contínua"`.
  3. `finished` (`order: 30`): cor `--max-warning-500`, rótulo `"Finalizado"`, `aria-label`: `"{n} aba(s) finalizada(s) não visualizada(s)"`.
  4. `error` (`order: 40`): cor `--max-danger-500`, rótulo `"Com erro"`, `aria-label`: `"{n} aba(s) com erro não visualizada(s)"`.
  5. `idle` (`order: 50`): cor `--background-500` (cinza neutro), rótulo `"Parado"`, `aria-label`: `"{n} aba(s) parada(s)"`.
- **Regras de Renderização:**
  - Qualquer badge nativo com valor `<= 0` não é renderizado no DOM.
  - Após os 5 nativos, itera sobre `badgeRegistry.providers.value` (ordenados por `order >= 60`): chama `provider.count(props.projectId)`; renderiza somente se o retorno for inteiro finito positivo (`Number.isInteger(val) && val > 0`), exibindo `<MaxIcon :icon="provider.icon" />`, contagem e `provider.tooltip(val)`.
  - Estilização: `border-radius: 9999px; font-variant-numeric: tabular-nums;`.

### 3.2. `src/components/projects/ProjectItem.vue`

- **Props e Emits:**
  ```ts
  interface Props {
      project: Project;
      counters: ProjectStatusCounters;
      selected: boolean;
      reorderDisabled?: boolean;
  }
  const props = withDefaults(defineProps<Props>(), {
      reorderDisabled: false
  });
  const emit = defineEmits<{
      select: [id: ProjectId];
      edit: [id: ProjectId];
      remove: [id: ProjectId];
      redetect: [id: ProjectId];
      moveUp: [id: ProjectId];
      moveDown: [id: ProjectId];
  }>();
  ```
- **Estrutura e Acessibilidade:**
  - Container `<li class="project-item" :style="{ '--project-color': project.color }">` sem aninhar `<button>` dentro de `<button>`: o botão principal de seleção (`role="option"` ou `<button type="button" :aria-current="selected ? 'true' : undefined">`) fica ao lado do botão de menu de ações (`MaxPopoverMenu` / `MaxPopoverConfirm`).
  - Ícone: `<MaxIcon :icon="safeIcon" />` dentro de um avatar com fundo `color-mix(in srgb, var(--project-color) 15%, transparent)` e cor `var(--project-color)`.
  - Título truncado com `v-tooltip="project.path"` (ou atributo `title`).
  - Quando `project.missing === true`, exibe tag/subtítulo de alerta `"Pasta não encontrada no disco"`.
  - Menu de contexto / ações:
    1. `"Editar projeto"` → emite `edit(project.id)`.
    2. `"Re-detectar identidade"` → emite `redetect(project.id)`.
    3. `"Remover do MaxTerminal"` → abre confirmação (`MaxPopoverConfirm`) informando `"Remover este projeto apenas do MaxTerminal? Nenhum arquivo no disco será apagado."` → ao confirmar, emite `remove(project.id)`.
  - Proibido usar `v-memo` que ignore alterações reativas dos providers do `useProjectBadgeRegistry`.

### 3.3. `src/components/projects/ProjectDiscoveryPanel.vue`

- **Props e Emits:**
  ```ts
  interface Props {
      items: DiscoveredFolder[];
      busy?: boolean;
  }
  const emit = defineEmits<{
      accept: [paths: string[]];
      dismiss: [paths: string[]];
  }>();
  ```
- **Comportamento:**
  - Renderizado somente quando `items.length > 0`.
  - Cabeçalho colapsável `"Novas pastas encontradas (${items.length})"` com botão `"Adicionar todas"` (`emit('accept', items.map((i) => i.path))`).
  - Cada item exibe preview de `MaxIcon` com `item.identity.icon` e `item.identity.color`, `item.suggestedTitle`, caminho abreviado e botões `"Adicionar"` (`emit('accept', [item.path])`) e `"Dispensar"` (`emit('dismiss', [item.path])`).

### 3.4. `src/components/projects/ProjectAddModal.vue`

- **Props e Emits:**
  ```ts
  interface Props {
      visible: boolean;
  }
  const emit = defineEmits<{
      'update:visible': [value: boolean];
      created: [project: Project];
  }>();
  ```
- **Regras de Estado (Auto vs Manual e Anti-Corrida):**
  - Campos do formulário: `path`, `title`, `icon`, `color`.
  - Flags internas: `titleTouched: boolean`, `iconTouched: boolean`, `colorTouched: boolean`, `previewRequestToken: number`, `submitting: boolean`.
  - Botão `"Escolher pasta"` chama `pickFolder('Selecionar pasta do projeto')`: se retornar `null` (cancelado), não altera `path` nem dispara preview.
  - Ao alterar `path` (com debounce de `250 ms`):
    - Incrementa `const token = ++previewRequestToken`.
    - Chama `projectPreviewIdentity(path)`. Se quando a Promise resolver `token !== previewRequestToken` ou `!props.visible`, descarta o resultado!
    - Se válido: atualiza `detectedIdentity`; preenche `title` (com `defaultTitle`) se `!titleTouched`, `icon` se `!iconTouched` e `color` se `!colorTouched`.
  - Ao submeter (`onSubmit`):
    - Se `submitting.value` for `true`, ignora cliques adicionais (proteção contra double-click).
    - Constrói `NewProject`:
      - Envia `title: titleTouched ? title.trim() : undefined`.
      - **Crucial (CA-2)**: envia `icon: iconTouched ? icon : undefined` e `color: colorTouched ? color : undefined`. Nunca envia os valores vindos do preview caso o usuário não os tenha editado manualmente, preservando `identity_source = 'auto'` no backend!
    - Em caso de erro (ex.: `project.duplicate_path`), exibe `err.message` inline no modal em pt-BR sem fechar o modal.

### 3.5. `src/components/projects/ProjectRootModal.vue`

- **Props e Emits:**
  ```ts
  interface Props {
      visible: boolean;
      root?: ProjectRoot | null;
  }
  const emit = defineEmits<{
      'update:visible': [value: boolean];
      saved: [result: RootImportReport | ProjectRoot];
      removed: [id: ProjectRootId];
  }>();
  ```
- **Comportamento:**
  - Modo criação (`!props.root`):
    - Campo `path` + botão `"Escolher pasta"` (`pickFolder('Selecionar Pasta de Projetos')`).
    - Rádio `discoveryMode`: `"Sugerir novas pastas"` (`'suggest'`, padrão) vs `"Importar automaticamente"` (`'auto_import'`).
    - Checkbox `"Importar subpastas existentes agora"` (`importExisting`, padrão `true`): exibido apenas quando `discoveryMode === 'suggest'` (no modo `'auto_import'`, exibe nota explicativa de que subpastas existentes e futuras são importadas automaticamente).
    - Checkbox `"Monitorar alterações automaticamente"` (`watchEnabled`, padrão `true`).
  - Modo edição (`props.root` presente):
    - Caminho exibido como somente leitura, status atual do monitoramento (`Watching`, `Disabled`, `Missing`, `Error`), edição de `discoveryMode` e `watchEnabled`.
    - Botão `"Varrer agora"` (`rescanRoot(root.id)`).
    - Botão `"Remover pasta de projetos"` com confirmação informando que os projetos já importados permanecerão cadastrados no MaxTerminal (`removeRoot(root.id)`).

### 3.6. `src/components/projects/ProjectEditModal.vue`

- **Props e Emits:**
  ```ts
  interface Props {
      visible: boolean;
      project: Project | null;
  }
  const emit = defineEmits<{
      'update:visible': [value: boolean];
      saved: [project: Project];
  }>();
  ```
- **Comportamento:**
  - Edição de `title`, `icon` (input texto Iconify + preview `<MaxIcon>` + chips de sugestão rápida de `STACK_IDENTITY_SUGGESTIONS` e `FALLBACK_PROJECT_ICONS`) e `color` (`<input type="color">` + chips de `FALLBACK_PROJECT_COLORS`).
  - Botão `"Restaurar identidade automática"`: chama `updateProject(project.id, { title: titleChanged ? title : undefined, resetIdentity: true })` (sem enviar `icon` nem `color`, evitando `project.invalid_patch`).

### 3.7. `src/components/projects/ProjectList.vue`, `ProjectEmptyState.vue`, `ProjectWorkspace.vue` e `AppLayout.vue`

1. **`ProjectList.vue`**:
   - Cabeçalho com título `"Projetos"` e botões `"Adicionar projeto"` e `"Adicionar pasta de projetos"`, além de lista compacta/popover das Pastas de Projetos cadastradas (exibindo alerta quando alguma raiz estiver com `watchStatus === 'error'` ou `'missing'` com botão `"Tentar novamente"` → `rescanRoot`).
   - Renderiza `<ProjectDiscoveryPanel>` quando houver sugestões pendentes.
   - Renderiza `<ProjectEmptyState>` quando `projects.length === 0`.
   - Navegação por teclado na lista: `ArrowUp` / `ArrowDown` movem foco/seleção; `Enter` confirma seleção; `Alt+ArrowUp` e `Alt+ArrowDown` reordenam o projeto focado chamando `projectsStore.reorder(newIds)` (desabilitado enquanto uma reordenação estiver em voo). Ignora atalhos quando o elemento ativo for `input`, `textarea` ou `isContentEditable`.
   - Suporte a HTML5 Drag-and-Drop (`draggable="true"`, `@dragstart`, `@dragover.prevent`, `@drop`) chamando `projectsStore.reorder(newIds)`.
2. **`ProjectWorkspace.vue`**:
   - Recebe `project: Project | null`.
   - Se `project === null`: exibe estado neutro centralizado `"Selecione um projeto na coluna à esquerda para visualizar seu workspace."`.
   - Se `project !== null`: exibe cabeçalho com `<MaxIcon :icon="safeIcon" />`, `project.title`, `project.path`, badge da stack detectada e, se `project.missing === true`, banner de aviso `"A pasta deste projeto não foi encontrada no disco."`. Abaixo do cabeçalho, renderiza `<slot />` com fallback informativo da Fase 06 (sem botão de criar aba antes da Fase 07).
3. **`AppLayout.vue`**:
   - Importa `'splitpanes/dist/splitpanes.css'`.
   - Inicializa `provideProjectBadgeRegistry()`, `useProjectEvents()`, `useProjectTabFocus()` e `const { leftSize, onResized, ready } = useLayoutPersistence()`.
   - Renderiza `<Splitpanes class="mt-columns" :horizontal="false" :dbl-click-splitter="false" @resized="onResized">` com `<Pane :size="leftSize" :min-size="15" :max-size="45">` contendo `<ProjectList />` e `<Pane :size="100 - leftSize">` contendo `<ProjectWorkspace :project="projectsStore.selectedProject"><slot /></ProjectWorkspace>`.
   - Oferece controle acessível por teclado para o divisor (`role="separator"`, `aria-orientation="vertical"`, `tabindex="0"`, `aria-valuenow`, `aria-valuemin="15"`, `aria-valuemax="45"`, ajustando `leftSize` em `±1%` com `ArrowLeft`/`ArrowRight`).
