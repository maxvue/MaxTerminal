# Especificação Técnica — Implementação 05: Frontend — Stores Pinia, Composables, Extensão do Terminal e Componentes Visuais

> **Fase:** 05 — Shell Integration OSC 133 e Máquina de Estados das Abas
> **Implementação:** `05 - Stores Pinia, Composables, Extensão do Terminal e Componentes Vue`
> **Fontes da verdade:** [`../../AGENTS.md`](../../../AGENTS.md), [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§§2.2, 6.2, 9 e 10).

---

## 1. Contexto, Objetivo e Escopo

Esta implementação entrega toda a camada frontend da Fase 05 em Vue 3.6 + TypeScript + Pinia + UnoCSS/SCSS: as stores `useTabStatusStore` e `useShellHistoryStore`, os composables `useShellMarkers`, `useShellCommandBlocks` e `useTabSeenTracker`, a extensão `shellIntegration` para o motor da Fase 04, os componentes `TabStatusIndicator.vue` e `CommandBlockStatus.vue`, e a evolução do playground da Fase 04 para demonstrar duas abas lógicas com máquina de estados completa e inspeção de contexto de erro.

### Escopo Incluído
1. **Callback Pós-Flush no Motor da Fase 04 (`src/types/terminal.ts`, `src/terminal/engine/terminalInstance.ts`):**
   - Adição do método `onDidFlushWrite(cb: (cursor: OutputCursor) => void): Disposable` em `TerminalInstanceHandle` para permitir que `useShellMarkers` posicione âncoras visuais aproximadas somente após o flush efetivo do batcher no xterm.
2. **Stores Pinia (`src/stores/tabStatus.ts`, `src/stores/shellHistory.ts`):**
   - `useTabStatusStore`: hidratação *listeners-before-snapshot*, reconciliação monotônica por `BigInt(revision)`, tombstones, `countsFor(tabIds)`, `unseenTabIds`, `hasActiveProcesses`, e actions `markSeen`, `setFocus`, `setContinuous` sem mutação otimista destrutiva sobre outra geração.
   - `useShellHistoryStore`: cache paginado de `CommandBlockDto` por sessão/painel (máximo 500 blocos por painel), consulta de detalhes (`includeOutput`), busca FTS5 e carregamento de `ErrorContext`.
3. **Composables e Extensão do Terminal (`src/composables/`, `src/terminal/extensions/shellIntegration.ts`):**
   - `useShellMarkers`: ancoragem pós-flush com `positionQuality: 'approximate' | 'unavailable'`, descartando marcadores em `altScreen`, `Gap` ou trim do scrollback.
   - `useShellCommandBlocks`: ponte entre `shell://command-started` / `shell://command-finished` / `useShellHistoryStore` e o `useCommandBlocksStore().feed` já existente na Fase 04 (`begin`, `markOutputStart`, `end`, `patchMeta`), sem criar uma segunda store de blocos de comando.
   - `useTabSeenTracker`: observação de foco da janela, `document.visibilityState`, aba/painel ativo e `requestAnimationFrame` pós-renderização da conclusão para enviar `SeenRequest` com a identidade exata (`ptyId`, `blockId`, `generation`, `observedRevision`), cancelando imediatamente em `blur` ou ocultação.
   - `shellIntegration.ts`: registro via `registerTerminalExtension`, `registerContextProvider` (`cwd` confiável/inferido) e `registerInputModeHint` (`rich` apenas quando em prompt correlacionado A/B ocioso; `passthrough` durante execução `C`, modo `proc` ou fase `unknown`).
4. **Componentes Vue e Playground (`src/components/shell/`, `src/components/terminal/CommandBlock.vue`, Playground):**
   - `TabStatusIndicator.vue` e `CommandBlockStatus.vue` seguindo estritamente as convenções SFC (`<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, 4 espaços, tags de abertura em linha única, textos em `pt-BR`, tokens `--max-*`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho do Arquivo | Responsabilidade |
|------|--------------------|------------------|
| Modificar | `src/types/terminal.ts` | Adicionar `onDidFlushWrite(cb: (cursor: OutputCursor) => void): Disposable` em `TerminalInstanceHandle` |
| Modificar | `src/terminal/engine/terminalInstance.ts` | Disparar listeners de `onDidFlushWrite` ao final de cada callback de parse do `WriteBatcher` |
| Criar | `src/stores/tabStatus.ts` | Store Pinia `useTabStatusStore` (snapshots, revisões BigInt, tombstones, `countsFor`, `hasActiveProcesses`, foco e ACK) |
| Criar | `src/stores/shellHistory.ts` | Store Pinia `useShellHistoryStore` (paginação de `CommandBlockDto`, cache de `ErrorContext` e busca FTS5) |
| Criar | `src/composables/useShellMarkers.ts` | Gerenciamento de posições visuais aproximadas pós-flush e descarte em `altScreen`/`Gap`/trim |
| Criar | `src/composables/useShellCommandBlocks.ts` | Adaptador alimentando `useCommandBlocksStore().feed` a partir dos eventos e snapshots da Fase 05 |
| Criar | `src/composables/useTabSeenTracker.ts` | Rastreamento de visibilidade/foco real e envio de `SeenRequest` por geração após render |
| Criar | `src/terminal/extensions/shellIntegration.ts` | Extensão do terminal registrando contexto `cwd` e dicas de modo (`rich` vs `passthrough`) |
| Criar | `src/components/shell/TabStatusIndicator.vue` | Badge semântico de estado da aba (`running`, `continuous`, `finished`, `error`, `idle`) |
| Criar | `src/components/shell/CommandBlockStatus.vue` | Indicador rico de status de bloco (código de saída, duração, contínuo, truncado, falha de persistência) |
| Modificar | `src/components/terminal/CommandBlock.vue` | Integrar `CommandBlockStatus.vue` exibindo metadados da Fase 05 sem quebrar o contrato da Fase 04 |
| Modificar | `src/components/terminal/__dev__/TerminalPlayground.vue` | Demonstrar 2 abas lógicas com `TabStatusIndicator`, foco/ACK por geração, override contínuo e modal/painel de `ErrorContext` |
| Criar | `tests/stores/tabStatusStore.spec.ts` | Testes unitários de hidratação, ordenação por revisão, tombstones, `countsFor` e `markSeen` |
| Criar | `tests/stores/shellHistoryStore.spec.ts` | Testes unitários de paginação, limite de 500 blocos, busca e tratamento de `persistence: 'failed'` |
| Criar | `tests/composables/useShellCommandBlocks.spec.ts` | Testes do adaptador para `useCommandBlocksStore().feed` e mapeamento de status/labels |
| Criar | `tests/composables/useTabSeenTracker.spec.ts` | Testes de ACK pós-render, cancelamento no blur e não confirmação de geração futura |
| Criar | `tests/components/shell/TabStatusIndicator.spec.ts` | Testes de renderização, classes semânticas, `role="status"` e tooltips em `pt-BR` |
| Criar | `tests/components/shell/CommandBlockStatus.spec.ts` | Testes de exibição de código de saída, resultado desconhecido, saída parcial e histórico não salvo |

---

## 3. Contratos e Assinaturas TypeScript / Vue

### 3.1. Store `useTabStatusStore` (`src/stores/tabStatus.ts`)

```ts
import { defineStore } from 'pinia';
import type {
    ContinuousPolicy,
    ExecutionRef,
    PaneId,
    SeenRequest,
    TabId,
    TabStateSnapshot,
    TabStatus
} from '@/types/bindings';

export interface TabStatusCounts {
    running: number;
    continuous: number;
    finished: number;
    error: number;
    idle: number;
}

export function executionKey(exec: ExecutionRef): string {
    return `${exec.ptyId}:${exec.blockId}:${exec.generation}`;
}
```
Regras obrigatórias da `useTabStatusStore`:
1. **Estado:**
   - `byTab: Record<TabId, TabStateSnapshot>`
   - `tombstones: Record<TabId, string>` (guarda `revision` decimal da remoção para impedir ressurreição por `tab://state-changed` atrasado)
   - `pendingSeenByPane: Record<PaneId, string>` (`executionKey` em voo; nunca muta otimisticamente `snapshot.seen = true`)
   - `ready: boolean`
   - `focusSeq: bigint` (sequência monotônica local incrementada a cada `syncFocus`)
2. **Inicialização (`init()`):**
   - Idempotente com token de geração (`initToken`).
   - Instala `onTabStateChanged` e `onTabStateRemoved` **antes** de chamar `tabStateList()`.
   - Enquanto `tabStateList()` está em voo, eventos recebidos entram em um buffer temporário; após resolver `tabStateList()`, aplica cada item respeitando `BigInt(incoming.revision) > BigInt(existing.revision)` e `BigInt(incoming.revision) > BigInt(tombstoneRevision)`, e em seguida drena o buffer com a mesma regra.
3. **Getters e Helpers:**
   - `countsFor(tabIds: readonly TabId[]): TabStatusCounts`: deduplica `tabIds` com `new Set(tabIds)`, ignora abas ausentes em `byTab` ou presentes em `tombstones`, e incrementa o contador correspondente a `snapshot.badgeBucket`.
   - `hasActiveProcesses`: retorna `true` se qualquer `snapshot` em `byTab` tiver `snapshot.hasActiveProcesses === true`.
   - `unseenTabIds`: lista `TabId`s cujo `badgeBucket` é `'finished'` ou `'error'`.

### 3.2. Adaptador `useShellCommandBlocks` (`src/composables/useShellCommandBlocks.ts`)

Conecta os eventos `shell://command-started` e `shell://command-finished` ao `useCommandBlocksStore().feed` da Fase 04:
- **Mapeamento de comando ausente:**
  - `const displayCommand = started.command ?? 'Comando indisponível';`
- **Mapeamento de status (`BlockStatus` → `CommandBlockStatus` da Fase 04):**
  - `'running'` → `'running'`
  - `'succeeded'` → `'success'`
  - `'failed'` → `'error'`
  - `'unknown'` → `'aborted'`
- **Chamadas ao `feed`:**
  1. Ao receber `CommandStarted` (deduplicado por `executionKey(started.execution)`):
     - `feed.begin(started.binding.paneId, { id: started.execution.blockId, command: displayCommand, cwd: started.cwd, startedAt: started.startedAtMs, line: markerLine })`
     - `feed.patchMeta(started.binding.paneId, started.execution.blockId, { execution: started.execution, seq: started.seq, commandQuality: started.commandQuality, continuous: started.continuous, source: started.source, confidence: started.confidence, persistence: started.persistence, positionQuality })`
  2. Ao receber `CommandFinished`:
     - Se o bloco ainda não existia no store (ex.: evento `started` perdido antes de reload), reconstrói o início via `commandBlockGet(finished.execution.blockId, false)` antes de fechar.
     - Para mapear `'unknown'` para `'aborted'` no `feed.end` da Fase 04, passa `exitCode: finished.status === 'unknown' ? null : finished.exitCode`.
     - Chama `feed.patchMeta(finished.binding.paneId, finished.execution.blockId, { blockStatus: finished.status, endReason: finished.endReason, durationMs: finished.durationMs, outputBytes: finished.outputBytes, outputTruncated: finished.outputTruncated, droppedBytes: finished.droppedBytes, persistence: finished.persistence, hasErrorContext: finished.hasErrorContext })`.
- **Proibição de replay duplicado:**
  - Eventos de replay binário do `PtyStreamHub` nunca disparam `feed.begin` ou `feed.end`.

### 3.3. Composable `useTabSeenTracker` (`src/composables/useTabSeenTracker.ts`)

```ts
import type { Ref } from 'vue';
import type { PaneId, TabId } from '@/types/bindings';

export interface UseTabSeenTrackerOptions {
    activeTabId: Ref<TabId | null>;
    visiblePaneIds: Ref<readonly PaneId[]>;
}
```
Algoritmo normativo:
1. Escuta `window` (`focus`, `blur`) e `document` (`visibilitychange`).
2. Sempre que `(activeTabId, visiblePaneIds, windowFocused, documentVisible)` mudar:
   - Incrementa `focusRevision` monotônico e chama `tabStatusStore.setFocus({ tabId: activeTabId.value, visiblePaneIds: [...visiblePaneIds.value], windowFocused, documentVisible, focusRevision: focusRevision.toString() })`.
   - Se `!windowFocused || !documentVisible`, cancela imediatamente qualquer `requestAnimationFrame` de confirmação pendente (sem debounce no blur!).
3. Quando a aba ativa está visível, focada (`windowFocused && documentVisible`) e um painel visível em `visiblePaneIds` possui `completion !== null` e `seen === false` (ou seja, `status === 'finished' || status === 'error'` ainda não visto):
   - Agenda um `requestAnimationFrame` vinculado ao par `(executionKey, snapshot.revision)`.
   - Quando o `rAF` dispara (garantindo que o frame de conclusão foi efetivamente apresentado na tela), revalida que `windowFocused && documentVisible`, que a aba/painel continuam visíveis e que a execução ainda é a mesma, e então chama `tabStatusStore.markSeen({ tabId, paneId, execution: completion.execution, observedRevision: snapshot.revision })`.

### 3.4. Componentes Vue (`src/components/shell/`)

#### A. `src/components/shell/TabStatusIndicator.vue`
- **Ordem dos blocos:** `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`.
- **Props:**
  ```ts
  export interface TabStatusIndicatorProps {
      status: TabStatus;
      seen?: boolean;
      confidence?: ObservationConfidence;
      hasUnhandledUnknown?: boolean;
      size?: 'sm' | 'md';
  }
  ```
- **Semântica Visual e Acessibilidade:**
  - Atributo `role="status"` e `aria-label` em `pt-BR`.
  - Mapeamento visual (usando o bucket efetivo: se `status` é `'finished'` ou `'error'` e `seen === true`, projeta como `'idle'` com indicador sutil do último resultado):
    - `running` (🟢): `--max-success-500`, label `"Executando"`
    - `continuous` (🔵): `--max-info-500`, label `"Execução contínua"`
    - `finished` com `seen === false` (🟠): `--max-warning-500`, label `"Finalizado (não visualizado)"`
    - `error` com `seen === false` (🔴): `--max-danger-500`, label `"Com erro (não visualizado)"`
    - `idle` (⚪): neutro (`--background-400` / `--max-text-secondary`), label `"Parado"` (ou `"Resultado desconhecido"` quando `hasUnhandledUnknown === true`).

#### B. `src/components/shell/CommandBlockStatus.vue`
- **Props:**
  ```ts
  export interface CommandBlockStatusProps {
      status: BlockStatus;
      exitCode: number | null;
      durationMs: string | null;
      continuous?: boolean;
      persistence?: PersistenceState;
      outputTruncated?: boolean;
  }
  ```
- **Rótulos em `pt-BR`:**
  - `status === 'running'` e `continuous === true` → `"Execução contínua"` (`--max-info-500`)
  - `status === 'running'` e `!continuous` → `"Executando…"` (`--max-success-500`)
  - `status === 'succeeded'` → `"Concluído (0)"` (`--max-success-500`)
  - `status === 'failed'` → `"Código de saída ${exitCode ?? '?'}"` (`--max-danger-500`)
  - `status === 'unknown'` → `"Resultado desconhecido"` (`--max-warning-500`)
  - Se `persistence === 'failed'` → exibe badge secundário `"Histórico não salvo"` com ícone `MaxIcon`.
  - Se `outputTruncated === true` → exibe badge secundário `"Saída parcial"` com ícone `MaxIcon`.
  - Conversão segura de `durationMs`: converte a string decimal via `BigInt`, valida se `<= BigInt(Number.MAX_SAFE_INTEGER)` antes de formatar em `ms` / `s` / `min`.
  - Nunca usar `v-html`: toda interpolação é textual (`{{ }}`).
- **Tags de abertura em linha única:** Todas as tags no `<template>` devem manter nome, atributos, diretivas e `>` na mesma linha.

---

## 4. Casos de Borda e Garantias de Qualidade no Frontend

1. **Prevenção de Confirmação Otimista Incorreta:**
   - Chamar `markSeen` nunca seta `snapshot.seen = true` localmente antes da resposta do backend; se uma nova execução começar enquanto o IPC `tab_mark_seen` da execução anterior estava em voo, o snapshot retornado/evento com `revision` maior prevalece.
2. **Descarte de Recursos (`dispose`):**
   - `useTabStatusStore.dispose()`, `useShellCommandBlocks` e `useTabSeenTracker` removem todos os `UnlistenFn`, cancelam `requestAnimationFrame` pendentes e invalidam chamadas em voo via token de inicialização.
3. **Fidelidade a TUIs (`altScreen`):**
   - Em `shellIntegration.ts`, o `InputModeHintProvider` retorna `{ mode: 'rich', priority: 50, reason: 'shell-prompt-ready' }` **somente** quando `mode === 'hooks'` e a fase atual é `Prompt`/`Input` sem execução ativa; quando `phase === 'executing'`, `mode === 'proc'` ou `phase === 'unknown'`, retorna `{ mode: 'passthrough', priority: 50, reason: 'shell-command-active' }`. O `InputModeResolver` da Fase 04 continua dando precedência máxima para `altScreen` e `mouseTracking`.
