# Implementação 04 — Gatilhos `@` e `?`, Seleção em Cascata de Harness, Sugestão de Comandos, Confirmação de Risco e RAG (`spec.md`)

> **Fase:** 12 — Integração MaxAiManager (`@` / `?`)
> **Unidade:** `04 - Gatilhos @ e ?, Seleção em Cascata de Harness, Sugestão de Comandos, Confirmação de Risco e RAG`
> **Pré-requisitos:** Implementações 01, 02 e 03 da Fase 12 + Input Rico e `TriggerRegistry` da Fase 04
> **Cobertura de Escopo da Fase 12:** `I-06`, `I-07`, `I-08`, `I-09` (UI), `I-11` (UI), `I-14` (componentes de sugestão, confirmação e RAG)
> **Cobertura de Critérios de Aceite:** `CA-13`, `CA-14`, `CA-15`, `CA-16`, `CA-18`, `CA-25`, `CA-26` (UI)

---

## 1. Contexto, Objetivo e Escopo

Esta quarta unidade entrega a experiência completa de **Comando por Linguagem Natural (`@` e `?`)** e **Consulta RAG** no frontend Vue 3.6:
1. **Registro dos Gatilhos `@` e `?` (`useAiTriggers.ts`):** Integra-se ao `triggerRegistry` da Fase 04, garantindo que digitar `@` abra o seletor em cascata de 3 etapas (**Harness → Modelo → Nível de Raciocínio**) seguido do prompt em pt-BR (`CA-13`), e digitar `?` utilize imediatamente a seleção padrão configurada em Preferências (ou exiba aviso acionável com link quando ausente — `CA-14`), além de permitir alternar para **"Consultar documentação (RAG)"** (`CA-25`). Todo submit retorna `{ kind: 'handled' }`, impedindo que `@pedido` ou `?pedido` sejam enviados literalmente ao shell.
2. **Orquestração de Sugestão e Segurança de Execução (`useAiSuggestion.ts`):** Controla o início do stream, edição do comando sugerido, inserção no input rico sem enviar Enter (`insertIntoInput`) e execução explícita (`writeToPty`) apenas mediante clique do usuário e após validação de risco e de integridade do contexto/sessão/PTY (`CA-15`, `CA-16`, `CA-33`).
3. **Orquestração de Consulta RAG (`useRagSearch.ts`):** Resolve o slug padrão (`last_slug` → stack do projeto da Fase 06), dispara `ai_rag_search` e adiciona o card rico `RagResults.vue` em `AiPaneResults.vue`.
4. **Componentes Visuais (`src/components/ai/`):** Implementa `HarnessPicker.vue`, `AiPromptInput.vue`, `RiskBadge.vue`, `DestructiveConfirm.vue`, `AiContextPreview.vue`, `AiSuggestionBlock.vue`, `RagResults.vue` e `AiPaneResults.vue` seguindo rigorosamente o padrão SFC do MaxTerminal (`<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, tags de abertura em linha única, 4 espaços de indentação, tokens `--max-*`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

### 2.1. Arquivos a Criar
- `src/composables/ai/useAiTriggers.ts`
- `src/terminal/triggers/providers/ai.ts` — definições dos providers `ai.at` e `ai.question` (D-09).
- `src/composables/ai/useAiSuggestion.ts`
- `src/composables/ai/useRagSearch.ts`
- `src/components/ai/HarnessPicker.vue`
- `src/components/ai/AiPromptInput.vue`
- `src/components/ai/RiskBadge.vue`
- `src/components/ai/DestructiveConfirm.vue`
- `src/components/ai/AiContextPreview.vue`
- `src/components/ai/AiSuggestionBlock.vue`
- `src/components/ai/RagResults.vue`
- `src/components/ai/AiPaneResults.vue`
- `tests/composables/ai/useAiTriggers.spec.ts`
- `tests/composables/ai/useAiSuggestion.spec.ts`
- `tests/composables/ai/useRagSearch.spec.ts`
- `tests/components/ai/HarnessPicker.spec.ts`
- `tests/components/ai/AiSuggestionBlock.spec.ts`
- `tests/components/ai/DestructiveConfirm.spec.ts`
- `tests/components/ai/RagResults.spec.ts`
- `tests/components/ai/RiskBadge.spec.ts`

---

## 3. Especificação Técnica Exaustiva

### 3.1. Composables de Gatilhos, Sugestão e RAG (`src/composables/ai/`)

Os objetos `TriggerDefinition` de `@`/`?` e seus handlers ficam em
`src/terminal/triggers/providers/ai.ts`, exportados como `aiAtTrigger` e `aiQuestionTrigger`.
`useAiTriggers` importa diretamente esse módulo para registrar/desregistrar os providers, mantendo estado
reativo da cascata nos composables. O barrel `providers/index.ts` nasce na Fase 14 e é estendido na 15;
esta fase não depende desse arquivo futuro nem cria outro diretório de providers.

#### `src/composables/ai/useAiTriggers.ts`
```ts
import type { Disposable } from '@/types/triggers';
import type { AiSelection, PaneId } from '@/types/bindings';

export type AtCascadeStep = 'harness' | 'model' | 'reasoning' | 'prompt';

export interface AtCascadeState {
    step: AtCascadeStep;
    harnessId: string | null;
    modelId: string | null;
    reasoningId: string | null;
    reasoningNotConfigurable: boolean;
}

/**
 * Registra os gatilhos `@` (id: 'ai.at') e `?` (id: 'ai.question') no TriggerRegistry da Fase 04.
 * - `@`: abre a cascata de 3 etapas (Harness -> Modelo -> Nível de Raciocínio) e depois o campo de prompt pt-BR.
 * - `?`: abre diretamente o campo de prompt usando `settings.defaultSelection` (ou exibe aviso com link para Preferências > IA),
 *        e oferece botão/item explícito "Consultar documentação (RAG)" que funciona mesmo sem `defaultSelection`.
 * - Retorna sempre `{ kind: 'handled' }` no submit para nunca vazar o texto do prompt para o PTY.
 */
export function useAiTriggers(): Disposable;
```

#### `src/composables/ai/useAiSuggestion.ts`
```ts
import type { ComputedRef, Ref } from 'vue';
import type { AiRequestId, AiSelection, AiTrigger, PaneId, RiskAssessment } from '@/types/bindings';
import type { SuggestionState } from '@/stores/ai';

export interface UseAiSuggestionReturn {
    state: ComputedRef<SuggestionState | null>;
    effectiveCommand: ComputedRef<string>;
    effectiveRisk: ComputedRef<RiskAssessment | null>;
    canExecuteOrInsert: ComputedRef<boolean>;
    isStaleContext: ComputedRef<boolean>;
    showDestructiveModal: Ref<boolean>;
    start: (args: { trigger: AiTrigger; prompt: string; selection?: AiSelection }) => Promise<AiRequestId>;
    updateCommand: (nextCommand: string) => void;
    cancel: () => Promise<void>;
    regenerate: () => Promise<AiRequestId | null>;
    insert: () => boolean;
    requestExecute: () => void;
    confirmExecute: () => boolean;
    copyCommand: () => Promise<void>;
    discard: () => void;
}

export function useAiSuggestion(paneId: PaneId, requestId?: AiRequestId): UseAiSuggestionReturn;
```
- **Regras de negócio de `useAiSuggestion` (`CA-15`, `CA-16`, `CA-33`):**
  1. Enquanto `state.value?.status === 'pending' || state.value?.status === 'streaming'` ou enquanto uma edição estiver aguardando reclassificação (`editedRisk === null` após `editCommand`), `canExecuteOrInsert` é `false`.
  2. `insert()` chama `insertIntoInput(paneId, effectiveCommand.value)` que atualiza o texto da barra de entrada rica **sem** adicionar `\r` ou `\n` e **sem** chamar `writeToPty` (`CA-15`).
  3. `requestExecute()` verifica `isStaleContext`: se a sessão ou o PTY do painel mudou desde que a sugestão foi gerada, bloqueia a execução e exibe o aviso `"O painel ou a sessão mudou. Gere uma nova sugestão."` (`ai.context_stale`). Caso o contexto seja válido:
     - Se `effectiveRisk.value.level === 'safe'`: executa imediatamente chamando `confirmExecute()` (1 clique — `CA-16`).
     - Se `effectiveRisk.value.level === 'caution'`: o botão "Executar" no template usa `MaxPopoverConfirm` para confirmação rápida; ao confirmar, chama `confirmExecute()`.
     - Se `effectiveRisk.value.level === 'destructive'`: abre `DestructiveConfirm.vue` (`showDestructiveModal.value = true`), exigindo a digitação exata de `confirmToken` (ex.: `rm`) antes de chamar `confirmExecute()` (`CA-16`).
  4. `confirmExecute()` chama `writeToPty({ paneId, sessionId: state.sessionId, ptyId: state.ptyId }, `${effectiveCommand.value}\r`)`.

#### `src/composables/ai/useRagSearch.ts`
```ts
import type { ComputedRef, Ref } from 'vue';
import type { AiRequestId, PaneId, RagSearchResponse, RagSlug } from '@/types/bindings';

export interface UseRagSearchReturn {
    slugs: ComputedRef<RagSlug[]>;
    selectedSlug: Ref<string>;
    loading: Ref<boolean>;
    errorMessage: Ref<string | null>;
    search: (query: string, customSlug?: string) => Promise<RagSearchResponse | null>;
    cancel: () => Promise<void>;
}

export function useRagSearch(paneId: PaneId): UseRagSearchReturn;
```

---

### 3.2. Componentes Vue (`src/components/ai/`)

1. **`HarnessPicker.vue` (`CA-13`):**
   - **Props:** `paneId: PaneId`, `query: string`, `initialSelection?: AiSelection | null`.
   - **Emits:** `submit: [payload: { selection: AiSelection; prompt: string }]`, `close: []`, `openPreferences: []`.
   - **Comportamento:**
     - Etapa 1 (`harness`): lista os harnesses do `useAiStore().harnesses` (os 4 canônicos `Claude Code`, `Codex`, `Antigravity (agy)`, `OpenCode` sempre visíveis; itens com `models.length === 0` exibem badge `"Indisponível no gateway"` e ficam desabilitados). Pré-seleciona o último harness usado na sessão.
     - Etapa 2 (`model`): lista os modelos do harness escolhido (`modelsOf(harnessId)`), mostrando `label` e `contextWindow` formatada (ex.: `200k ctx`).
     - Etapa 3 (`reasoning`): lista os níveis de raciocínio do modelo (`reasoningOf(harnessId, modelId)`). Se o modelo tiver `reasoningLevels: []`, exibe a opção única `"Não configurável"` e define `reasoningId: null`.
     - Etapa 4 (`prompt`): renderiza o chip resumo `"{Harness} · {Modelo} · {Raciocínio}"` (clicável para voltar etapas) e foca automaticamente o `AiPromptInput.vue`.
     - Teclado nas etapas 1–3: `ArrowUp`/`ArrowDown` movem o índice ativo (`role="listbox"`, `aria-activedescendant`), `Enter` avança para a próxima etapa, `Backspace` (com filtro vazio) ou `ArrowLeft` volta para a etapa anterior limpando escolhas descendentes, `Escape` emite `close`.

2. **`AiPromptInput.vue` (`CA-14`, `CA-25`):**
   - **Props:** `paneId: PaneId`, `mode: 'command' | 'rag'`, `selection: AiSelection | null`, `selectionChip?: string`, `initialSlug?: string`, `requireDefaultSelection?: boolean`.
   - **Emits:** `submit: [payload: { mode: 'command' | 'rag'; prompt: string; slug?: string }]`, `close: []`, `update:mode: [mode: 'command' | 'rag']`, `openPreferences: []`.
   - **Comportamento:**
     - Quando `mode === 'command'` e `requireDefaultSelection && !selection`: exibe alerta `"Configure o padrão em Preferências > IA"` com botão/link `"Abrir Preferências > IA"` (`CA-14`), mantendo o botão de alternar para `"Consultar documentação (RAG)"` habilitado (`CA-25`).
     - Quando `mode === 'rag'`: exibe seletor/input de `slug` (pré-preenchido com `last_slug` ou stack do projeto, permitindo selecionar da lista `ragSlugs` ou digitar um slug como `laravel`) e o campo de consulta em pt-BR.

3. **`RiskBadge.vue` (`CA-16`):**
   - **Props:** `risk: RiskAssessment`.
   - **Estilos e Rótulos:**
     - `safe`: cor `--max-success-500`, ícone `mdi:shield-check-outline`, texto `"Sem alerta detectado"`.
     - `caution`: cor `--max-warning-500`, ícone `mdi:alert-outline`, texto `"Atenção"`.
     - `destructive`: cor `--max-danger-500`, ícone `mdi:skull-outline`, texto `"Destrutivo"`.
     - Tooltip (`v-tooltip`) listando todos os `risk.reasons` (`message` em pt-BR) e a nota `"Classificação heurística — revise o comando antes de executar"`.

4. **`DestructiveConfirm.vue` (`CA-16`):**
   - **Props:** `visible: boolean`, `command: string`, `risk: RiskAssessment`.
   - **Emits:** `confirm: []`, `cancel: []`, `update:visible: [value: boolean]`.
   - **Comportamento:** Modal (`MaxModal`) com destaque `--max-danger-500`, exibindo o comando completo em bloco monoespaçado, a lista de motivos (`risk.reasons`), e um input solicitando `"Digite {confirmToken} para confirmar a execução"`. O botão `"Executar comando destrutivo"` permanece `disabled` até que `typedToken.trim() === (risk.confirm_token ?? firstToken)`.

5. **`AiContextPreview.vue` (`CA-18`):**
   - **Props:** `context: AiContext`.
   - **Comportamento:** Painel colapsável (`<details>` estilizado) com o título `"Dados enviados ao MaxAiManager"`, exibindo `shell`, `os.distro` (`os.kernel`), `cwd`, `projectStack` e a lista de `recentCommands` (com código de saída e comando já mascarado pelo sanitizer).

6. **`AiSuggestionBlock.vue` (`CA-15`, `CA-16`, `CA-18`):**
   - **Props:** `requestId: AiRequestId`.
   - **Emits:** `inserted: []`, `executed: []`, `discarded: []`, `openPreferences: []`.
   - **Comportamento:**
     - Cabeçalho com chip `selectionLabel`, badge de latência (`latencyMs ms`) e botão `"Cancelar"` (`Esc`) enquanto `status === 'pending' || status === 'streaming'`.
     - Durante o streaming, mostra o texto parcial em `shallowRef` e mantém os botões `"Inserir no input"` e `"Executar"` desabilitados.
     - Quando `status === 'ready'`, exibe o comando em um `MaxInput` monoespaçado editável (`v-model="editedCommand"`), a explicação em pt-BR, o `RiskBadge`, o `AiContextPreview` colapsado e a barra de ações: `"Inserir no input"`, `"Executar"` (com `MaxPopoverConfirm` para `caution` ou `DestructiveConfirm` para `destructive`), `"Copiar"`, `"Regenerar"` e `"Descartar"`.
     - Quando `status === 'failed'`, exibe a mensagem em pt-BR correspondente ao código `error.code` (§8 da spec da Fase 12) com botões `"Tentar novamente"` e `"Abrir Preferências > IA"`.

7. **`RagResults.vue` (`CA-25`, `CA-26`):**
   - **Props:** `requestId: AiRequestId`.
   - **Emits:** `copy: [text: string]`, `discard: []`.
   - **Comportamento:** Renderiza cabeçalho `"{slug} · {query}"` (`latencyMs ms`), estado vazio `"Nenhum resultado encontrado para esta consulta"` preservando a query (`CA-25`), ou lista de resultados com `title`, trecho `content` interpolado com `{{ }}` (proibido `v-html`) com botão `"Expandir / Recolher"`, badge de `score` percentual, `source`, botão `"Copiar trecho"` e abertura de `url` (`http:`/`https:`) via `@tauri-apps/plugin-opener`.

8. **`AiPaneResults.vue`:**
   - **Props:** `paneId: PaneId`, `sessionId: SessionId`.
   - **Comportamento:** Hospedeiro por painel posicionado junto ao `HybridTerminal`, renderizando a lista `usePaneRichBlocks(paneId, sessionId)` com `AiSuggestionBlock` e `RagResults` identificados por `requestId`.
