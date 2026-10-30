# Plano de Execução — Implementação 05: Frontend — Stores Pinia, Composables, Extensão do Terminal e Componentes Visuais

> **Fase:** 05 — Shell Integration OSC 133 e Máquina de Estados das Abas
> **Implementação:** `05 - Stores Pinia, Composables, Extensão do Terminal e Componentes Vue`
> **Especificação de referência:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1: Adicionar Callback Pós-Flush no Motor da Fase 04 e Implementar `useShellMarkers`**
  1. Atualizar `src/types/terminal.ts` e `src/terminal/engine/terminalInstance.ts` adicionando `onDidFlushWrite(cb: (cursor: OutputCursor) => void): Disposable`, invocado após cada callback de parse do `WriteBatcher`.
  2. Criar `src/composables/useShellMarkers.ts` gerenciando posições visuais aproximadas (`positionQuality: 'approximate' | 'unavailable'`) ancoradas após o flush do renderer, descartando marcadores quando o terminal estiver em `altScreen`, após `Gap` ou após trim do buffer.

- [ ] **Passo 2: Implementar as Stores `useTabStatusStore` (`src/stores/tabStatus.ts`) e `useShellHistoryStore` (`src/stores/shellHistory.ts`)**
  1. Criar `src/stores/tabStatus.ts` com hidratação *listeners-before-snapshot*, comparação monotônica de `revision` via `BigInt`, mapa de `tombstones`, getters `countsFor(tabIds)`, `unseenTabIds`, `hasActiveProcesses` e actions `markSeen`, `setFocus`, `setContinuous` e `dispose`.
  2. Criar `src/stores/shellHistory.ts` com paginação de blocos por `SessionId` (retendo no máximo 500 blocos por painel), consulta de detalhes (`commandBlockGet`), consulta e cache local de `ErrorContext` (`commandBlockErrorContext`) e busca FTS5 (`commandBlockSearch`).

- [ ] **Passo 3: Implementar `useShellCommandBlocks`, `useTabSeenTracker` e a Extensão `shellIntegration.ts`**
  1. Criar `src/composables/useShellCommandBlocks.ts` assinando `onCommandStarted` e `onCommandFinished` e alimentando `useCommandBlocksStore().feed` (`begin`, `markOutputStart`, `end`, `patchMeta`) com deduplicação por `ExecutionRef`, rótulo `'Comando indisponível'` quando `command === null`, e mapeamento `succeeded → success`, `failed → error`, `unknown → aborted`.
  2. Criar `src/composables/useTabSeenTracker.ts` monitorando foco da janela (`focus`/`blur`), `document.visibilityState`, aba/painel visível e `requestAnimationFrame` pós-renderização da conclusão para enviar `SeenRequest` exato ao backend.
  3. Criar `src/terminal/extensions/shellIntegration.ts` registrando a extensão no `registerTerminalExtension`, provedor de `cwd` em `registerContextProvider` e dica de modo (`rich` em prompt correlacionado A/B ocioso; `passthrough` em execução/proc/unknown) em `registerInputModeHint`.

- [ ] **Passo 4: Criar `TabStatusIndicator.vue`, `CommandBlockStatus.vue`, Integrar em `CommandBlock.vue` e Atualizar o Playground**
  1. Criar `src/components/shell/TabStatusIndicator.vue` obedecendo à ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, recuo de 4 espaços, tags de abertura em linha única e tokens semânticos `--max-success-500`, `--max-info-500`, `--max-warning-500`, `--max-danger-500`.
  2. Criar `src/components/shell/CommandBlockStatus.vue` exibindo código de saída, duração segura, badges `"Execução contínua"`, `"Resultado desconhecido"`, `"Histórico não salvo"` e `"Saída parcial"`, sem usar `v-html`.
  3. Atualizar `src/components/terminal/CommandBlock.vue` para renderizar `CommandBlockStatus.vue` com os metadados preenchidos em `block.meta` pela Fase 05.
  4. Atualizar o playground da Fase 04 (`src/components/terminal/__dev__/TerminalPlayground.vue`) para demonstrar duas abas lógicas de um painel usando `ShellIntegrationRequest`, exibindo `TabStatusIndicator`, contadores agregados (`countsFor`), seletor de `ContinuousPolicy` (`Auto`/`On`/`Off`) e visualização de `ErrorContext` para blocos com falha.

- [ ] **Passo 5: Criar Suíte de Testes Unitários e de Componentes no Vitest**
  1. Criar `tests/stores/tabStatusStore.spec.ts` e `tests/stores/shellHistoryStore.spec.ts` testando *listeners-before-snapshot*, descarte de revisões antigas, tombstones, `countsFor` sem duplicar IDs, `hasActiveProcesses` multi-pane e ausência de update otimista sobre outra geração.
  2. Criar `tests/composables/useShellCommandBlocks.spec.ts` e `tests/composables/useTabSeenTracker.spec.ts` testando mapeamento para o `feed` da Fase 04, ACK após `rAF`, cancelamento imediato no `blur` e não reexecução de transições durante replay.
  3. Criar `tests/components/shell/TabStatusIndicator.spec.ts` e `tests/components/shell/CommandBlockStatus.spec.ts` validando todos os estados visuais, textos em `pt-BR` e atributos ARIA (`role="status"`).

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Executar os testes unitários e de componentes da Fase 05 no Vitest
npx --no-install vitest run tests/stores/tabStatusStore.spec.ts tests/stores/shellHistoryStore.spec.ts tests/composables/useShellCommandBlocks.spec.ts tests/composables/useTabSeenTracker.spec.ts tests/components/shell/TabStatusIndicator.spec.ts tests/components/shell/CommandBlockStatus.spec.ts

# 2. Validar tipagem TypeScript/Vue estrita (tsgo / vue-tsc)
npm run typecheck

# 3. Validar ESLint e Stylelint (ordem de blocos SFC, 4 espaços, tags de abertura em linha única)
npm run lint

# 4. Validar build completo do frontend e workspace Rust
npm run build
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 05)

- [x] **CA-FE-01 (instructions.md §5):** Hidratação assina eventos antes de consultar snapshots, descarta revisões antigas (`BigInt`), reconcilia perdas/reload e limpa listeners no `dispose`; replay visual não reexecuta transições nem `INSERT`s.
- [x] **CA-FE-02 (instructions.md §5):** Adaptador `useShellCommandBlocks` usa exclusivamente `begin/markOutputStart/end/patchMeta` do `useCommandBlocksStore().feed` da Fase 04, mapeando `succeeded → success`, `failed → error`, `unknown → aborted`; sem segunda store `commandBlocks` e sem `v-html`.
- [x] **CA-EST-02 e CA-EST-03 (instructions.md §5):** `useTabSeenTracker` confirma somente a geração concluída efetivamente apresentada após renderização com janela focada e documento visível, cancelando no blur; manter foco durante execução não confirma `D` antecipadamente.
- [x] **CA-QLD-01 (instructions.md §5 e AGENTS.md §3):** SFCs Vue seguem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, 4 espaços, tags de abertura em linha única, textos em `pt-BR` e passam em `typecheck`, `lint`, `vitest` e `cargo test`.
