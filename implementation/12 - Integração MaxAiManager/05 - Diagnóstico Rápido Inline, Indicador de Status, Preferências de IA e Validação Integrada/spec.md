# Implementação 05 — Diagnóstico Rápido Inline, Indicador de Status, Preferências de IA e Validação Integrada (`spec.md`)

> **Fase:** 12 — Integração MaxAiManager (`@` / `?`)
> **Unidade:** `05 - Diagnóstico Rápido Inline, Indicador de Status, Preferências de IA e Validação Integrada`
> **Pré-requisitos:** Implementações 01, 02, 03 e 04 da Fase 12 + Slot `#block-actions` do `HybridTerminal` (Fase 04) e evento `shell://command-finished` (Fase 05)
> **Cobertura de Escopo da Fase 12:** `I-10` (UI e composable), `I-14` (`InlineDiagnosis.vue`, `AiStatusIndicator.vue`, `AiPreferences.vue`), `I-15` (UI completa)
> **Cobertura de Critérios de Aceite:** `CA-03`, `CA-20`, `CA-21` (UI), `CA-22`, `CA-23`, `CA-24`, `CA-34` (UI), `CA-35` (UI)

---

## 1. Contexto, Objetivo e Escopo

Esta quinta e última unidade conclui a Fase 12 entregando:
1. **Diagnóstico Rápido Inline (`useInlineDiagnosis.ts` e `InlineDiagnosis.vue`):** Escuta o evento `shell://command-finished` (Fase 05), aplicando todos os filtros locais (habilitado globalmente, projeto sem opt-out, `exitCode !== null && exitCode !== 0`, código fora de `ignoredExitCodes` como `130`, painel não interativo/TUI/Agente CLI), deduplicação por `blockId` e **debounce de 800 ms por painel** (`paneId`), de modo que 3 erros em `< 800 ms` no mesmo painel gerem apenas 1 requisição (a última) (`CA-20`, `CA-22`, `CA-34`). Renderiza `InlineDiagnosis.vue` no slot `HybridTerminal #block-actions` → `CommandBlock #actions` ao lado do botão `"Inspecionar"` (sem depender da Fase 11 estar instalada — `CA-23`), exibindo resumo curto (`<= 160` caracteres), comando corretivo com `RiskBadge` e ações `"Inserir no input"` e `"Ignorar"` (sem jamais exibir botão `"Executar"` direto).
2. **Indicador de Status do Gateway (`AiStatusIndicator.vue`):** Exibe na barra de status do aplicativo um ponto discreto refletindo `GatewayStatus` (`online`, `degraded`, `offline`, `disabled`, `unconfigured`), tooltip detalhado com URL base, versão, latência e capacidades validadas, e clique que abre diretamente **Preferências > IA** (`I-15`).
3. **Tela de Preferências de IA (`src/components/preferences/AiPreferences.vue`):** Permite configurar e testar a conexão com o MaxAiManager, definir o padrão do gatilho `?` (cascata de 3 selects Harness → Modelo → Raciocínio), parametrizar o Diagnóstico Rápido Inline (toggle, debounce, códigos ignorados, opt-out por projeto, seleção própria), RAG (`topK`, mapeamento `stack → slug`), Contexto (`recentCommands`, `includeOutputTail`, `outputTailBytes`), Histórico (`saveHistory`, limpar histórico) e Caches (limpar cache de diagnóstico/catálogo) (`CA-06`, `CA-24`, `CA-30`).
4. **Montagem no Workspace e Validação Integrada (`CA-01`..`CA-36`):** Integra `useAiStore().init()`, `useAiTriggers()`, `useInlineDiagnosis()`, `AiPaneResults.vue`, `InlineDiagnosis.vue`, `AiStatusIndicator.vue` e `AiPreferences.vue` na árvore principal do aplicativo e valida a cobertura `>= 80%` em `src/stores/ai.ts` e `src/composables/ai/` (`CA-03`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

### 2.1. Arquivos a Modificar
- `src/app/App.vue` (ou inicializador do workspace da Fase 07) — chamada idempotente a `useAiStore().init()`, montagem de `useAiTriggers()` e `useInlineDiagnosis()`.
- `src/components/terminal/PaneContainer.vue` (ou componente hospedeiro de painel da Fase 04/07) — montagem de `AiPaneResults.vue` junto ao `HybridTerminal` e passagem de `<InlineDiagnosis :block-id="block.id" :tab-id="tabId" :pane-id="paneId" />` no slot `#block-actions` do `HybridTerminal`.
- `src/components/layout/StatusBar.vue` (ou barra de rodapé do workspace) — inclusão de `<AiStatusIndicator @open-preferences="openAiPreferences" />`.
- `src/components/preferences/PreferencesModal.vue` (ou `src/app/router.ts` na rota `/preferences/ai`) — registro da aba/seção `"IA (MaxAiManager)"` renderizando `<AiPreferences />`.

### 2.2. Arquivos a Criar
- `src/composables/ai/useInlineDiagnosis.ts`
- `src/components/ai/InlineDiagnosis.vue`
- `src/components/ai/AiStatusIndicator.vue`
- `src/components/preferences/AiPreferences.vue`
- `tests/composables/ai/useInlineDiagnosis.spec.ts`
- `tests/components/ai/InlineDiagnosis.spec.ts`
- `tests/components/ai/AiStatusIndicator.spec.ts`
- `tests/components/ai/AiPreferences.spec.ts`

---

## 3. Especificação Técnica Exaustiva

### 3.1. Composable `useInlineDiagnosis` (`src/composables/ai/useInlineDiagnosis.ts`)

```ts
import type { CommandBlockId, PaneId } from '@/types/bindings';
import type { Disposable } from '@/types/triggers';

export interface UseInlineDiagnosisReturn extends Disposable {
    /**
     * Dispara manualmente o diagnóstico de um bloco (usado quando metadados automáticos não estavam disponíveis
     * ou quando o usuário clica para tentar novamente).
     */
    triggerManualDiagnosis: (blockId: CommandBlockId, paneId: PaneId) => Promise<void>;
    /**
     * Cancela todos os timers de debounce e requisições pendentes (chamado ao desligar diagnóstico ou desmontar).
     */
    cancelAll: () => void;
}

/**
 * Escuta eventos `shell://command-finished` (Fase 05) via `adapters.onBlockFinished`:
 * 1. Ignora eventos duplicados para o mesmo `blockId` (`seenBlocks: Set<CommandBlockId>`).
 * 2. Verifica se `exitCode !== null && exitCode !== 0` (exit desconhecido `null` nunca dispara automático — CA-20).
 * 3. Verifica `settings.enabled && settings.diagnosis.enabled`.
 * 4. Verifica se `exitCode` não pertence a `settings.diagnosis.ignoredExitCodes` (ex.: 130 / SIGINT — CA-22).
 * 5. Verifica se `isInteractiveOrAgent === false` e `hasReliableMetadata === true` (CA-22).
 * 6. Verifica `aiStore.diagnosisEnabledFor(projectId)` (opt-out por projeto — CA-22).
 * 7. Aplica debounce por `paneId` (`settings.diagnosis.debounceMs`, padrão 800 ms):
 *    - Se outro erro ocorrer no mesmo `paneId` dentro dos 800 ms, limpa o timer anterior e cancela eventual requisição em voo daquele `paneId` (CA-20).
 *    - Dois `paneId` distintos na mesma aba possuem timers e requisições independentes (CA-34).
 */
export function useInlineDiagnosis(): UseInlineDiagnosisReturn;
```

---

### 3.2. Componente `InlineDiagnosis.vue` (`src/components/ai/InlineDiagnosis.vue`)

- **Props:**
  ```ts
  interface Props {
      blockId: CommandBlockId;
      tabId: TabId;
      paneId: PaneId;
  }
  ```
- **Slot onde é montado:** `HybridTerminal #block-actions` → repassado ao `CommandBlock #actions`, convivendo lado a lado com o botão `"Inspecionar"` da Fase 11 sem importar nenhum módulo da Fase 11 (`CA-23`).
- **Estados Visuais (`CA-23`, `CA-24`):**
  1. **Oculto (`v-if`):** Quando `settings?.diagnosis.enabled === false` (`CA-24`), ou quando o bloco não possui entrada em `aiStore.diagnoses[blockId]`, ou quando `status === 'skipped' || status === 'dismissed'`.
  2. **Carregando (`status === 'pending'`):** Exibe spinner compacto e discreto com rótulo `"Analisando erro…"` e botão `"Cancelar"` (`×`).
  3. **Pronto (`status === 'ready'`):**
     - Exibe ícone `mdi:lightbulb-on-outline`, o resumo `diagnosis.summary` (`<= 160` caracteres) e badge opcional `"Cache"` quando `diagnosis.fromCache === true`.
     - Se `diagnosis.fixCommand` estiver presente: exibe o comando corretivo em `<code>` monoespaçado, o `<RiskBadge :risk="diagnosis.fixRisk" />` e o botão **`"Inserir no input"`** (que chama `insertIntoInput(paneId, diagnosis.fixCommand)` — **jamais possui botão `"Executar"` direto**).
     - Botão **`"Ignorar"`** que chama `aiStore.dismissDiagnosis(blockId)` (ocultando apenas este `blockId`, sem afetar outros blocos com o mesmo `errorHash` — `CA-34`).
  4. **Falha (`status === 'failed'`):** Exibe apenas um ícone discreto `mdi:alert-circle-outline` com `v-tooltip` contendo a mensagem amigável do erro e clique para tentar novamente (nunca dispara Toast para não interromper o fluxo de terminal — §8 da spec da Fase 12).

---

### 3.3. Componente `AiStatusIndicator.vue` (`src/components/ai/AiStatusIndicator.vue`)

- **Emits:** `openPreferences: []`.
- **Mapeamento de Status e Cores Semânticas:**
  - `online`: ponto `--max-success-500`, texto `"IA Online"`.
  - `degraded`: ponto `--max-warning-500`, texto `"IA Degradada"`.
  - `offline`: ponto `--max-danger-500`, texto `"MaxAiManager Offline"`.
  - `unconfigured`: ponto neutro `--text-secondary`, texto `"IA Não Configurada"`.
  - `disabled`: ponto neutro opaco, texto `"IA Desativada"`.
- **Tooltip:** Exibe `baseUrl`, `version`, `latencyMs` e o resumo das capacidades (`generation`, `catalog`, `ragSearch`, `sse`). Clicar no indicador emite `openPreferences`.

---

### 3.4. Componente `AiPreferences.vue` (`src/components/preferences/AiPreferences.vue`)

- Divide o formulário de configurações em 6 seções claras em português do Brasil (`pt-BR`):
  1. **Conexão com o Gateway (`MaxAiManager`):**
     - Switch `"Habilitar integração com MaxAiManager"` (`enabled`).
     - Input `"URL Base do Gateway"` (`baseUrl`, placeholder `"http://127.0.0.1:..."`).
     - Checkbox `"Permitir host remoto (fora de loopback)"` (`allowRemote`) — quando marcado, exibe alerta `--max-warning-500`: `"Atenção: dados sanitizados do terminal sairão desta máquina."` (`§10.3` da spec).
     - Botão `"Testar conexão e atualizar catálogo"` que chama `aiStore.updateSettings` + `aiStore.refreshCatalog` + `ipc.getHealth` e exibe os chips de `GatewayCapabilities` (`validated` vs `unavailable` com o respectivo motivo).
  2. **Seleção Padrão (Gatilho `?`):**
     - 3 selects encadeados (**Harness** → **Modelo** → **Nível de Raciocínio**) alimentados pelo `useAiStore().catalog`.
     - Quando o modelo selecionado não possuir níveis de raciocínio configuráveis, o 3º select fica desabilitado exibindo `"Não configurável"`.
  3. **Diagnóstico Rápido Inline:**
     - Switch `"Analisar automaticamente comandos que terminarem com erro"` (`diagnosis.enabled`) — desligar cancela imediatamente os diagnósticos pendentes e esconde os existentes (`CA-24`).
     - Input numérico `"Atraso antes de diagnosticar (ms)"` (`diagnosis.debounceMs`, padrão `800`).
     - Input de tags/números `"Códigos de saída ignorados"` (`diagnosis.ignoredExitCodes`, padrão `130`).
     - Multi-select `"Projetos com diagnóstico desativado (Opt-out)"` (`diagnosis.disabledProjects`).
     - Seleção opcional de Harness/Modelo própria para diagnóstico (vazio = usa o padrão com menor raciocínio suportado).
  4. **Consulta de Documentação (RAG):**
     - Input numérico `"Quantidade de trechos retornados (top_k)"` (`rag.topK`, `1` a `20`).
     - Editor de pares `"Mapeamento Stack → Slug RAG"` (`rag.stackSlugs`, ex.: `laravel → laravel`).
  5. **Contexto e Privacidade:**
     - Input numérico `"Comandos recentes enviados no contexto"` (`context.recentCommands`, `0` a `20`).
     - Switch `"Incluir cauda de saída do último comando em @ e ?"` (`context.includeOutputTail`).
     - Switch `"Salvar histórico sanitizado de prompts no SQLite"` (`saveHistory`) (`CA-30`).
  6. **Manutenção de Histórico e Caches:**
     - Botão com `MaxPopoverConfirm`: `"Limpar histórico de prompts"` (`ipc.clearHistory(null)`) exibindo a contagem removida (`CA-30`).
     - Botões com `MaxPopoverConfirm`: `"Limpar cache de diagnósticos"` (`ipc.clearCache('diagnosis')`) e `"Limpar todos os caches de IA"` (`ipc.clearCache('all')`) (`CA-30`).
- Trata conflito de revisão (`AI_SETTINGS_CONFLICT`): se outra janela salvar preferências simultaneamente, exibe banner `"As preferências foram alteradas em outra janela"` com botão `"Recarregar preferências"`.
