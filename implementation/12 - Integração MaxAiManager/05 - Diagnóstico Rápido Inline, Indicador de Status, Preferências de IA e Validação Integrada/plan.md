# Implementação 05 — Plano de Execução Passo a Passo (`plan.md`)

> **Fase:** 12 — Integração MaxAiManager (`@` / `?`)
> **Unidade:** `05 - Diagnóstico Rápido Inline, Indicador de Status, Preferências de IA e Validação Integrada`
> **Especificação técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1 — Implementar o Composable `useInlineDiagnosis` (`src/composables/ai/useInlineDiagnosis.ts`):**
  1. Criar `src/composables/ai/useInlineDiagnosis.ts` escutando `shell://command-finished` via `adapters.onBlockFinished`.
  2. Implementar todos os filtros de elegibilidade (`exitCode !== null && exitCode !== 0`, `diagnosis.enabled`, `ignoredExitCodes` incluindo `130`, `!isInteractiveOrAgent`, opt-out por projeto) e deduplicação por `blockId`.
  3. Implementar o debounce de `800 ms` por `paneId` cancelando qualquer diagnóstico anterior do mesmo painel quando um novo erro ocorre (`CA-20`), preservando a independência entre painéis divididos (Split Panes) na mesma aba (`CA-34`).

- [ ] **Passo 2 — Implementar `InlineDiagnosis.vue` e `AiStatusIndicator.vue`:**
  1. Criar `src/components/ai/InlineDiagnosis.vue` com os estados `pending` (`"Analisando erro…"`), `ready` (resumo `<= 160` chars, `fixCommand`, `RiskBadge`, `"Inserir no input"` e `"Ignorar"`) e `failed` (ícone silencioso com tooltip), sem qualquer dependência do botão `"Inspecionar"` da Fase 11 (`CA-23`).
  2. Criar `src/components/ai/AiStatusIndicator.vue` exibindo o estado do gateway (`online`, `degraded`, `offline`, `unconfigured`, `disabled`), tooltip de capacidades e evento `openPreferences`.

- [ ] **Passo 3 — Implementar `AiPreferences.vue` (`src/components/preferences/AiPreferences.vue`):**
  1. Criar `src/components/preferences/AiPreferences.vue` com as 6 seções de configuração (Conexão, Padrão do `?`, Diagnóstico Rápido Inline, RAG, Contexto/Privacidade e Limpeza de Histórico/Caches).
  2. Garantir que desativar `"Diagnóstico Rápido Inline"` cancele imediatamente os diagnósticos pendentes e esconda os existentes na UI (`CA-24`).

- [ ] **Passo 4 — Integrar os Componentes e Composables no Workspace do Aplicativo:**
  1. Inicializar `useAiStore().init()`, `useAiTriggers()` e `useInlineDiagnosis()` no ciclo de vida do workspace (`App.vue` / `WorkspaceView.vue`).
  2. Montar `<AiPaneResults :pane-id="pane.id" :session-id="pane.sessionId" />` junto ao `HybridTerminal` e injetar `<InlineDiagnosis :block-id="block.id" :tab-id="tabId" :pane-id="pane.id" />` no slot `#block-actions` do `HybridTerminal`.
  3. Montar `<AiStatusIndicator />` na barra de status e `<AiPreferences />` no modal/rota de Preferências.

- [ ] **Passo 5 — Escrever Testes Finais e Executar Validação Completa da Fase 12 (`CA-01`..`CA-36`):**
  1. Criar `tests/composables/ai/useInlineDiagnosis.spec.ts` (usando `vi.useFakeTimers()` para provar que 3 erros em `< 800 ms` no mesmo painel disparam apenas 1 chamada IPC, que `exit 130` / interativo / opt-out são ignorados e que dois painéis na mesma aba não cancelam um ao outro).
  2. Criar `tests/components/ai/InlineDiagnosis.spec.ts`, `tests/components/ai/AiStatusIndicator.spec.ts` e `tests/components/ai/AiPreferences.spec.ts`.
  3. Executar toda a bateria de validação Rust + TypeScript + Vue + Vitest com verificação de cobertura `>= 80%` em `src/stores/ai.ts` e `src/composables/ai/`.

---

## 2. Comandos de Verificação e Validação Final da Fase 12

Execute todos os comandos abaixo a partir da raiz da worktree (todos devem terminar com `exit code 0`):

```bash
# 1. Validação completa do Backend Rust (CA-01, CA-02)
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-ai --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai
cargo test --manifest-path src-tauri/Cargo.toml --workspace

# 2. Validação estática do Frontend (Lint + Typecheck TS e Vue) (CA-03, CA-04)
npm run lint
npm run typecheck
npm run typecheck:vue

# 3. Testes unitários desta unidade no Vitest (CA-20, CA-22, CA-23, CA-24, CA-34)
npx --no-install vitest run \
  tests/composables/ai/useInlineDiagnosis.spec.ts \
  tests/components/ai/InlineDiagnosis.spec.ts \
  tests/components/ai/AiStatusIndicator.spec.ts \
  tests/components/ai/AiPreferences.spec.ts

# 4. Suíte completa de testes Frontend + Cobertura >= 80% em src/stores/ai.ts e src/composables/ai/ (CA-03)
npx --no-install vitest run --coverage \
  tests/stores/ai.spec.ts \
  tests/composables/ai/ \
  tests/components/ai/
```

---

## 3. Mapeamento dos Critérios de Aceite da Fase 12

| Critério | Cobertura nesta Implementação | Teste / Verificação |
|----------|-------------------------------|---------------------|
| `CA-03` | `npm run lint`, `npm run typecheck`, `npm run typecheck:vue` e `npm run test` passam com cobertura `>= 80%` de linhas em `src/stores/ai.ts` e `src/composables/ai/`. | Comandos 2 e 4 de verificação |
| `CA-20` | Bloco com `exit 1` dispara 1 requisição após 800 ms; 3 erros em `< 800 ms` no mesmo painel geram apenas 1 requisição (o último); `exitCode: null` não dispara diagnóstico automático. | `tests/composables/ai/useInlineDiagnosis.spec.ts` |
| `CA-22` | `exit 130`, blocos de TUI/agentes e projetos com opt-out não disparam diagnóstico. | `tests/composables/ai/useInlineDiagnosis.spec.ts` |
| `CA-23` | `InlineDiagnosis.vue` renderiza em `HybridTerminal #block-actions` → `CommandBlock #actions` mesmo sem o botão `"Inspecionar"` da Fase 11, exibindo resumo `<= 160` chars, comando corretivo com `RiskBadge`, `"Inserir no input"` e `"Ignorar"`. | `tests/components/ai/InlineDiagnosis.spec.ts` |
| `CA-24` | Desligar `"Diagnóstico Rápido Inline"` em `AiPreferences.vue` cancela diagnósticos pendentes e oculta os existentes. | `tests/components/ai/AiPreferences.spec.ts` + `tests/composables/ai/useInlineDiagnosis.spec.ts` |
| `CA-34` | Dois painéis na mesma aba diagnosticam independentemente; deduplicação por `blockId`; clicar em `"Ignorar"` em um bloco nunca oculta outro bloco com o mesmo `errorHash`. | `tests/composables/ai/useInlineDiagnosis.spec.ts` + `tests/components/ai/InlineDiagnosis.spec.ts` |
