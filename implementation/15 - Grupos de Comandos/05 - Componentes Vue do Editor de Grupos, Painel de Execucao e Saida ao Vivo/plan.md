# Implementação 05 — Plano de Execução Passo a Passo (`plan.md`)

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1: Implementar `CommandGroupItemRow.vue` e `CommandGroupSuggestPanel.vue`**
  1. Criar `src/components/commandGroups/CommandGroupItemRow.vue` seguindo a ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, recuo de 4 espaços e todas as tags de abertura em linha única.
  2. Implementar campos de título, comando, `cwd` opcional, editor de pares chave/valor de `env` (com mascaramento visual de chaves sensíveis), checkbox `Execução Contínua`, botões de mover para cima/baixo, duplicar e remover, além dos atalhos `Alt+ArrowUp` e `Alt+ArrowDown` com região `aria-live="polite"`.
  3. Criar `src/components/commandGroups/CommandGroupSuggestPanel.vue` integrando a chamada `suggest` da Fase 12, cancelamento via `ai_cancel`, preview isolado da sugestão, botões "Aplicar ao rascunho" e "Desfazer aplicação", e listagem condicional de chips de comandos recorrentes da Fase 13 (`available === true`).

- [ ] **Passo 2: Implementar `CommandGroupModal.vue`**
  1. Criar `src/components/commandGroups/CommandGroupModal.vue` utilizando `useCommandGroupEditor` e `useCommandGroupsStore`.
  2. Implementar formulário completo: Nome, Descrição, Labels (chips adicionáveis/removíveis), seletor de Escopo (na criação) ou botão "Copiar para…" (na edição), seletor de Modo (`Sequencial`, `Paralelo em Background`, `Multi-Abas`), seletor de Política Sequencial (`&&` / `;`), `cwd` e `env` do grupo, lista reordenável de `CommandGroupItemRow.vue` e painel `CommandGroupSuggestPanel.vue`.
  3. Implementar exibição de erros por campo (`details.fields` de `commands.group.secret_detected` e `commands.group.validation`), alerta preventivo quando um item contínuo não é o último no modo Sequencial e banner de resolução de conflito de revisão (`commands.group.revision_conflict`).

- [ ] **Passo 3: Implementar `GroupRunItemRow.vue`, `GroupRunOutputView.vue`, `GroupRunPanel.vue` e `GroupRunIndicator.vue`**
  1. Criar `src/components/commandGroups/GroupRunItemRow.vue` renderizando os badges semânticos (`--max-success-500`, `--max-info-500`, `--max-warning-500`, `--max-danger-500`, neutros), duração formatada, código de saída/sinal, motivo de `skipped`/`interrupted` e botões das ações `show_output`, `focus_tab` e `cancel`.
  2. Criar `src/components/commandGroups/GroupRunOutputView.vue` acoplando `useGroupRunOutput` ao componente `TerminalView.vue` em modo somente leitura (`readonly`), exibindo avisos de truncamento (256 KiB) e lacunas de stream (`gap`).
  3. Criar `src/components/commandGroups/GroupRunPanel.vue` exibindo o cabeçalho da execução (nome do snapshot, modo efetivo, shell dialeto, contadores agregados, botão "Cancelar execução"), ações de fallback para painel ocupado ("Executar em nova aba") ou processo que ignora `Ctrl+C` ("Encerrar e reiniciar painel"), lista de `GroupRunItemRow.vue` e abertura lateral/inferior do `GroupRunOutputView.vue`.
  4. Criar `src/components/commandGroups/GroupRunIndicator.vue` e integrá-lo junto ao `CommandGroupModal.vue` e `GroupRunPanel.vue` no componente de workspace da coluna direita.

- [ ] **Passo 4: Criar Testes de Componentes Vue (`CommandGroupModal.spec.ts`, `GroupRunPanel.spec.ts`, `GroupRunOutputView.spec.ts`)**
  1. Criar `tests/commandGroups/CommandGroupModal.spec.ts` testando criação local/global, adição de 3 itens, reordenação por botões e `Alt+↑/↓`, escolha de modo e separador, salvamento imediato refletido na store (`CA-16`), fluxo de sugestão IA com Aplicar/Desfazer e descarte de resposta tardia (`CA-20`), e conflito de revisão (`CA-26`).
  2. Criar `tests/commandGroups/GroupRunPanel.spec.ts` testando cores semânticas de cada estado, botões de cancelar item/tudo, link para focar aba em Multi-Abas e fallback "Executar em nova aba" quando o painel está ocupado (`CA-17`, `CA-18`, `CA-19`).
  3. Criar `tests/commandGroups/GroupRunOutputView.spec.ts` testando replay + live via `Channel`, aviso de truncamento/gap, bloqueio de input e `detach` no fechamento sem cancelar a tarefa (`CA-18`).

- [ ] **Passo 5: Executar Validação Completa da Fase 15 (Frontend + Backend)**
  1. Rodar `npm run lint`, `npm run lint:style`, `npm run typecheck` e `npm run typecheck:vue` garantindo zero avisos ou violações de formatação SFC.
  2. Rodar toda a suíte `npx --no-install vitest run tests/commandGroups --coverage` verificando cobertura ≥ 90% na store e trigger e ≥ 80% nos componentes Vue.

---

## 2. Comandos Exatos de Verificação

Execute os comandos abaixo a partir de a raiz da worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)"
npm run typecheck
npm run typecheck:vue
npm run lint
npm run lint:style
npx --no-install vitest run tests/commandGroups --coverage
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands
cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings
git diff --exit-code src/types/bindings.ts
```

---

## 3. Mapeamento dos Critérios de Aceite

| Critério da Fase 15 | Como é atendido e verificado nesta Implementação |
|---------------------|--------------------------------------------------|
| **CA-10** | `npm run typecheck`, `npm run typecheck:vue`, `npm run lint`, `npm run lint:style` e `bindings.ts` sem erros. |
| **CA-11** | `npx --no-install vitest run tests/commandGroups` verde com cobertura ≥ 90% em `commandGroups.ts` e `commandGroupsTrigger.ts` e ≥ 80% em `src/components/commandGroups/**`. |
| **CA-16** | Testado em `CommandGroupModal.spec.ts`: abrir com escopo local pré-selecionado, adicionar 3 itens, reordenar por drag e `Alt+↑/↓`, escolher modo e separador, salvar e refletir imediatamente no `>>`. |
| **CA-17 (UI)** | Testado em `GroupRunPanel.spec.ts`: quando o painel está ocupado, em alt-screen ou sem prompt confirmado, exibe o motivo e oferece o botão "Executar em nova aba". |
| **CA-18 (UI)** | Testado em `GroupRunPanel.spec.ts` e `GroupRunOutputView.spec.ts`: cores semânticas `--max-success-500`, `--max-info-500`, `--max-warning-500`, `--max-danger-500` e neutro; "Ver saída" em terminal somente leitura com replay + live; botão "Cancelar" por item ou execução. |
| **CA-19 (UI)** | Seletor de template compatível quando necessário ou abertura do editor de templates da Fase 07 caso não exista template compatível com `continuous`; link para focar abas abertas no painel. |
| **CA-20 (UI)** | Testado em `CommandGroupModal.spec.ts`: "Sugerir com IA" gera preview revisável sem salvar nem executar; resposta tardia não sobrescreve edição; erro/cancelamento preservam o formulário; "Comandos recorrentes do projeto" visível apenas quando a fonte opcional da Fase 13 está disponível. |
| **CA-22** | Todos os textos visíveis em `pt-BR`, SFCs na ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>` e tags de abertura em linha única. |
| **CA-26 (UI)** | Banner de conflito de revisão no `CommandGroupModal.vue` preserva o rascunho editado pelo usuário. |
