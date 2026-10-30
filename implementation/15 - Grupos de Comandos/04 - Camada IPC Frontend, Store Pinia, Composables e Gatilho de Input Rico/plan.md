# Implementação 04 — Plano de Execução Passo a Passo (`plan.md`)

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1: Criar Tipos Derivados de UI e Wrappers IPC (`src/types/commandGroups.ts` e `src/ipc/commandGroups.ts`)**
  1. Criar `src/types/commandGroups.ts` reexportando todos os contratos da Fase 15 gerados em `src/types/bindings.ts` e definindo `GroupRunItemAction`, `GroupTriggerSecondaryAction`, `EditableGroupItem` e `EditableCommandGroupDraft`.
  2. Criar `src/ipc/commandGroups.ts` implementando `commandGroupsIpc` sobre `commands` (`@/types/bindings`), `Channel` (`@tauri-apps/api/core`) e `listen` (`@tauri-apps/api/event`), utilizando o helper `unwrap` de `src/ipc/core.ts`.

- [ ] **Passo 2: Implementar a Store Pinia (`src/stores/commandGroups.ts`)**
  1. Criar `useCommandGroupsStore` em `src/stores/commandGroups.ts`.
  2. Implementar o cache separado para `globalGroups` e `localGroupsByProject[projectId]`, controle single-flight de `load(projectId)` e tombstones `deletedRevisionByGroupId`.
  3. Implementar `initListeners()` assinando `command-group://changed`, `command-group://run-started`, `command-group://run-item-changed` e `command-group://run-finished` com limpeza segura em `disposeListeners()` e HMR.
  4. Implementar a lógica de reconciliação de eventos de execução:
     - Bufferizar eventos em `pendingEventsByRunId[runId]` enquanto `startRun` ou `loadRun` estiver em voo.
     - Descartar eventos com `seq <= run.version`.
     - Aplicar diretamente quando `seq === run.version + 1`.
     - Invocar `loadRun(runId)` quando `seq > run.version + 1` ou quando a execução ainda não estiver em `runsById`.

- [ ] **Passo 3: Implementar os Composables (`useCommandGroupEditor.ts`, `useCommandGroupRun.ts`, `useGroupRunOutput.ts`)**
  1. Criar `src/composables/commandGroups/useCommandGroupEditor.ts`:
     - Inicialização de rascunho para criação ou edição (clonando `CommandGroup` e guardando `baseRevision`).
     - Validação reativa por campo (nome 1–80, 1–50 itens, título 1–80, comando sem quebra de linha, `cwd` relativo sem `..` escapando da raiz, chaves de `env` POSIX sem `MAXTERMINAL_`).
     - Funções `addItem`, `removeItem`, `duplicateItem` (gerando novo `localKey` e `id: null`), `moveItemUp`, `moveItemDown`, `reorderItems`.
     - Fluxo de sugestão IA (`requestSuggestion`, `applySuggestion`, `undoSuggestion`, `discardSuggestion`) garantindo que resposta de `draftRevision` antigo seja descartada se o usuário editou o rascunho, e que aplicar sugestão permita desfazer (`undo`) para o estado anterior do rascunho.
  2. Criar `src/composables/commandGroups/useCommandGroupRun.ts`:
     - Captura de `projectId`, `originPaneId` e `expectedRevision` no instante do acionamento.
     - Geração de `requestId` único por tentativa e proteção contra duplo clique/Enter repetido.
     - Resolução de templates compatíveis para o modo Multi-Abas (abrindo seletor/aviso caso o template padrão não coincida com `continuous`).
  3. Criar `src/composables/commandGroups/useGroupRunOutput.ts`:
     - Gerenciamento de `attachItemOutput` e `detachItemOutput`, controle de `isTruncated`, `gapDroppedBytes`, `isEnded` e limpeza automática no `onBeforeUnmount` sem cancelar a execução.

- [ ] **Passo 4: Implementar e Registrar o Provedor do Gatilho `>>` (`commandGroupsTrigger.ts` e `providers/index.ts`)**
  1. Criar `src/terminal/triggers/providers/commandGroupsTrigger.ts` implementando o contrato do `TriggerRegistry` da Fase 04:
     - Regra de ativação: somente modo `rich`, posição 0, sem espaço inicial (`" >>"`), sem escape (`"\\>>"`), prefixo `">>"`.
     - Extração de query: `input.slice(2)` (portanto `">>>dev"` produz query `">dev"`).
     - Pontuação fuzzy sobre `name`, `description` e `labels` (com bônus para prefixo do `name`), mantendo sempre a seção `"Deste projeto"` antes da seção `"Globais"`.
     - Adição incondicional das duas opções `pinned: 'bottom'`: `"Criar grupo de comandos local"` (desabilitada com dica quando `!projectId`) e `"Criar grupo de comandos global"`.
     - Suporte à Etapa 2 (`Tab` / `→`) com as 5 ações do grupo (`Executar em modo Sequencial`, `Executar em modo Paralelo`, `Executar em modo Multi-Abas`, `Editar grupo`, `Excluir grupo`).
  2. Registrar `registerCommandGroupsTrigger` em `src/terminal/triggers/providers/index.ts`.

- [ ] **Passo 5: Criar Testes Unitários Vitest (`commandGroupsStore.spec.ts` e `commandGroupsTrigger.spec.ts`)**
  1. Criar `tests/commandGroups/commandGroupsStore.spec.ts` verificando `CA-11`, `CA-14` e `CA-26` (cobertura ≥ 90% de linhas em `src/stores/commandGroups.ts`).
  2. Criar `tests/commandGroups/commandGroupsTrigger.spec.ts` verificando `CA-11`, `CA-12`, `CA-13`, `CA-15` e `CA-25` (cobertura ≥ 90% de linhas em `commandGroupsTrigger.ts`).

---

## 2. Comandos Exatos de Verificação

Execute os comandos abaixo a partir de `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0`:

```bash
npm run typecheck
npm run lint
npx --no-install vitest run tests/commandGroups/commandGroupsStore.spec.ts tests/commandGroups/commandGroupsTrigger.spec.ts --coverage
```

---

## 3. Mapeamento dos Critérios de Aceite

| Critério da Fase 15 | Como é atendido e verificado nesta Implementação |
|---------------------|--------------------------------------------------|
| **CA-10** | `npm run typecheck` e `npm run lint` sem erros nos arquivos TS criados. |
| **CA-11 (store e trigger)** | Cobertura ≥ 90% de linhas em `src/stores/commandGroups.ts` e `src/terminal/triggers/providers/commandGroupsTrigger.ts`. |
| **CA-12** | Testado em `commandGroupsTrigger.spec.ts`: `>>` ganha de `>`; `>` sozinho permanece no Artisan; `>>` → Backspace → `>` aborta o `AbortSignal` de grupos e volta ao Artisan; `\>>`, ` >>` e modo `passthrough` não abrem popup; `>>>dev` usa query `>dev`. |
| **CA-13** | Testado em `commandGroupsTrigger.spec.ts`: seção "Deste projeto" sempre antes de "Globais"; fuzzy `dvup` encontra "Dev Up"; opções "Criar grupo de comandos local" e "Criar grupo de comandos global" sempre presentes no fim (mesmo com 0 resultados ou erro); sem projeto ativo, a opção local fica desabilitada com dica. |
| **CA-14** | Testado em `commandGroupsStore.spec.ts`: `command-group://changed` invalida apenas o projeto afetado (ou globais); `run-item-changed` com `seq` menor é descartado; salto de `seq` dispara `getRun`. |
| **CA-15** | Navegação ↑/↓ com wrap-around ignorando cabeçalhos/desabilitados, `Enter` executando no modo do grupo, `Tab`/`→` abrindo a etapa 2 de ações e `Escape` fechando mantendo o literal. |
| **CA-23 (frontend)** | `useCommandGroupRun` e o gatilho `>>` mantêm `requestId` estável por tentativa e bloqueiam submissão duplicada em `Enter` repetido. |
| **CA-25** | Busca por nome, descrição e labels preserva locais antes de globais e nunca substitui as duas ações finais. |
| **CA-26** | Reconciliação de eventos perdidos/fora de ordem na store e detecção de conflito de revisão em `useCommandGroupEditor` sem perder o rascunho. |
