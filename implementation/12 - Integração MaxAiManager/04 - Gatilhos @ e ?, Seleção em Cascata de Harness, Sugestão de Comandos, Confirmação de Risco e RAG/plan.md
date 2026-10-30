# Implementação 04 — Plano de Execução Passo a Passo (`plan.md`)

> **Fase:** 12 — Integração MaxAiManager (`@` / `?`)
> **Unidade:** `04 - Gatilhos @ e ?, Seleção em Cascata de Harness, Sugestão de Comandos, Confirmação de Risco e RAG`
> **Especificação técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1 — Implementar os Composables `useAiSuggestion`, `useRagSearch` e `useAiTriggers`:**
  1. Criar `src/composables/ai/useAiSuggestion.ts` implementando `start`, `updateCommand`, `cancel`, `regenerate`, `insert` (via `insertIntoInput` sem `\r`), `requestExecute` / `confirmExecute` (validando risco e `isStaleContext` antes de `writeToPty`), `copyCommand` e `discard`.
  2. Criar `src/composables/ai/useRagSearch.ts` resolvendo o slug padrão (`last_slug` → stack do projeto da Fase 06), executando `aiStore.ragSearch` e inserindo o card `'ai-rag'` via `pushRichBlock`.
  3. Criar `src/composables/ai/useAiTriggers.ts` registrando `@` (`ai.at`) e `?` (`ai.question`) no `triggerRegistry` da Fase 04 e garantindo retorno `{ kind: 'handled' }` em todos os caminhos de submissão.

- [ ] **Passo 2 — Implementar os Componentes de Risco, Confirmação e Preview de Contexto:**
  1. Criar `src/components/ai/RiskBadge.vue` com cores semânticas `--max-success-500`, `--max-warning-500` e `--max-danger-500` e tooltip detalhando os motivos da classificação.
  2. Criar `src/components/ai/DestructiveConfirm.vue` exigindo digitação exata de `risk.confirmToken` (ex.: `rm`) para liberar o botão de confirmação (`CA-16`).
  3. Criar `src/components/ai/AiContextPreview.vue` exibindo `shell`, `os`, `cwd`, `projectStack` e os comandos recentes sanitizados com seus códigos de saída (`CA-18`).

- [ ] **Passo 3 — Implementar `HarnessPicker.vue` e `AiPromptInput.vue`:**
  1. Criar `src/components/ai/AiPromptInput.vue` com suporte aos modos `'command'` e `'rag'`, aviso `"Configure o padrão em Preferências > IA"` quando `?` não possuir `defaultSelection` (`CA-14`), e campo de slug selecionável/digitável para RAG (`CA-25`).
  2. Criar `src/components/ai/HarnessPicker.vue` com navegação por teclado (`ArrowUp`, `ArrowDown`, `Enter`, `Backspace`, `ArrowLeft`, `Escape`) entre as 3 etapas (`Harness → Modelo → Raciocínio`), chip resumo `Harness · Modelo · Raciocínio` e transição automática de foco para o campo de texto pt-BR (`CA-13`).

- [ ] **Passo 4 — Implementar `AiSuggestionBlock.vue`, `RagResults.vue` e `AiPaneResults.vue`:**
  1. Criar `src/components/ai/AiSuggestionBlock.vue` com streaming em tempo real, comando editável em `MaxInput` monoespaçado, reclassificação de risco ao editar, `RiskBadge`, `AiContextPreview` e ações `"Inserir no input"`, `"Executar"`, `"Copiar"`, `"Regenerar"` e `"Descartar"` (`CA-15`, `CA-16`).
  2. Criar `src/components/ai/RagResults.vue` exibindo resultados da busca em `/v1/rags/{slug}/search`, estado vazio preservando a consulta e erro `AI_RAG_NOT_FOUND` / `AI_INVALID_SLUG` em pt-BR (`CA-25`, `CA-26`).
  3. Criar `src/components/ai/AiPaneResults.vue` renderizando os blocos de sugestão e RAG ativos do painel/sessão.

- [ ] **Passo 5 — Escrever e Executar os Testes Unitários e de Componentes (Vitest):**
  1. Criar `tests/composables/ai/useAiTriggers.spec.ts`, `tests/composables/ai/useAiSuggestion.spec.ts` e `tests/composables/ai/useRagSearch.spec.ts`.
  2. Criar `tests/components/ai/HarnessPicker.spec.ts`, `tests/components/ai/AiSuggestionBlock.spec.ts`, `tests/components/ai/DestructiveConfirm.spec.ts`, `tests/components/ai/RagResults.spec.ts` e `tests/components/ai/RiskBadge.spec.ts`.

---

## 2. Comandos de Verificação e Validação

Execute os comandos abaixo a partir da raiz da worktree:

```bash
# 1. Lint de código Vue/TypeScript/SCSS (ordem de blocos SFC, tags em linha única, 4 espaços)
npm run lint

# 2. Typecheck estrito
npm run typecheck
npm run typecheck:vue

# 3. Executar todos os testes unitários dos composables e componentes desta unidade no Vitest
npx --no-install vitest run \
  tests/composables/ai/useAiTriggers.spec.ts \
  tests/composables/ai/useAiSuggestion.spec.ts \
  tests/composables/ai/useRagSearch.spec.ts \
  tests/components/ai/HarnessPicker.spec.ts \
  tests/components/ai/AiSuggestionBlock.spec.ts \
  tests/components/ai/DestructiveConfirm.spec.ts \
  tests/components/ai/RagResults.spec.ts \
  tests/components/ai/RiskBadge.spec.ts
```

---

## 3. Mapeamento dos Critérios de Aceite da Fase 12

| Critério | Cobertura nesta Implementação | Teste / Verificação |
|----------|-------------------------------|---------------------|
| `CA-13` | `@` abre `HarnessPicker` na etapa 1; `↑`/`↓` navegam, `Enter` avança, `Backspace`/`←` voltam etapa, `Esc` fecha; após a etapa 3 o foco vai ao prompt pt-BR com chip `Harness · Modelo · Raciocínio`. | `tests/components/ai/HarnessPicker.spec.ts` |
| `CA-14` | `?` usa o padrão das Preferências sem exibir as 3 etapas; sem padrão configurado, exibe `"Configure o padrão em Preferências > IA"` com link. | `tests/composables/ai/useAiTriggers.spec.ts` |
| `CA-15` | Enviar prompt exibe `AiSuggestionBlock` com stream e comando editável; **nenhum byte é escrito no PTY** até clicar em `"Executar"`, e `"Inserir no input"` não envia `\r`. | `tests/composables/ai/useAiSuggestion.spec.ts` + `tests/components/ai/AiSuggestionBlock.spec.ts` |
| `CA-16` | `rm -rf ./build` (`destructive`) exige digitar `rm` em `DestructiveConfirm`; `ls -la` (`safe`) executa com 1 clique. | `tests/components/ai/DestructiveConfirm.spec.ts` + `tests/components/ai/AiSuggestionBlock.spec.ts` |
| `CA-18` | `AiContextPreview` exibe shell, SO, cwd, stack e comandos recentes com `Bearer abc123` mascarado. | `tests/components/ai/AiSuggestionBlock.spec.ts` |
| `CA-25` | Modo RAG no popup `?` funciona mesmo sem `defaultSelection`; exibe resultados ou `"Nenhum resultado"` preservando a consulta. | `tests/composables/ai/useRagSearch.spec.ts` + `tests/components/ai/RagResults.spec.ts` |
| `CA-26` (UI) | Mensagens pt-BR para base inexistente (`AI_RAG_NOT_FOUND`) e slug inválido (`AI_INVALID_SLUG`) renderizadas em `RagResults.vue` / `AiPromptInput.vue`. | `tests/components/ai/RagResults.spec.ts` |
