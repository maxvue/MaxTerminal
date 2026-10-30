# Implementação 05 — Componentes Visuais, Modal "Criar Comando Artisan" e Host de Confirmação (`plan.md`)

> **Fase:** 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`)
> **Unidade:** `05 - Componentes Visuais Modal Criar Comando Artisan e Host de Confirmacao`
> **Especificação associada:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Execução

### Passo 1 — Composable `useArtisanForm` e Item do Popup (`useArtisanForm.ts`, `QuickCommandItem.vue`)
- [ ] **1.1.** Implementar `src/composables/quick-commands/useArtisanForm.ts` conectando o formulário reativo a `useQuickCommandsStore`, implementando sugestão automática de `className` a partir de `signatureName` até edição manual, indexação de `FieldIssue` por `path` e `focusFirstInvalidField`.
- [ ] **1.2.** Implementar `src/components/quick-commands/QuickCommandItem.vue` seguindo a ordem obrigatória `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>` e tags de abertura em linha única, renderizando destaque de caracteres fuzzy e badges.

### Passo 2 — Construtor de Parâmetros e Seletor de Schedule (`ArtisanParamRow.vue`, `ArtisanParamsBuilder.vue`, `ArtisanSchedulePicker.vue`)
- [ ] **2.1.** Implementar `src/components/quick-commands/ArtisanParamRow.vue` com campos condicionais para `Argument` e `Option`, exibição de erros por campo (`params[i].*`) e botões de mover (`↑`/`↓`) e remover.
- [ ] **2.2.** Implementar `src/components/quick-commands/ArtisanParamsBuilder.vue` com botões `"Adicionar argumento"` e `"Adicionar opção"`, lista de `ArtisanParamRow` e reordenação imutável.
- [ ] **2.3.** Implementar `src/components/quick-commands/ArtisanSchedulePicker.vue` com checkbox `"Agendamento (Schedule)"`, seletor das 13 frequências, inputs dinâmicos (`minute`, `time`, `first`/`second`, `day`, `expression` cron), preenchimento de argumentos obrigatórios/opções do comando agendado e checkboxes `withoutOverlapping`, `onOneServer` e `runInBackground`.

### Passo 3 — Preview PHP, Modal Principal e Host (`ArtisanPhpPreview.vue`, `ArtisanCommandModal.vue`, `QuickCommandsHost.vue`)
- [ ] **3.1.** Implementar `src/components/quick-commands/ArtisanPhpPreview.vue` com `<pre><code>` somente leitura (sem `v-html`) e alternância entre as abas `"Comando PHP"`, `"routes/console.php"` e `"Passos manuais"`.
- [ ] **3.2.** Implementar `src/components/quick-commands/ArtisanCommandModal.vue` em layout de 2 colunas com todos os campos na ordem da spec, exibição de erros por campo, estado pós-criação com `manualSteps` e botões `"Preencher com IA"`, `"Desfazer sugestão"`, `"Criar com MaxCode"` e `"Criar"`.
- [ ] **3.3.** Implementar `src/components/quick-commands/QuickCommandsHost.vue` renderizando `ArtisanCommandModal` e a confirmação (`MaxPopoverConfirm`) para `/install-force`, `/update-force` e `/deploy`, e montá-lo em `src/App.vue`.

### Passo 4 — Testes de Componentes Vitest e Qualidade Frontend Completa
- [ ] **4.1.** Criar `src/components/quick-commands/__tests__/ArtisanParamsBuilder.spec.ts` testando adição de argumento e opção, edição de campos, reordenação (`↑`/`↓`) e remoção.
- [ ] **4.2.** Criar `src/components/quick-commands/__tests__/ArtisanSchedulePicker.spec.ts` testando ativação do schedule, troca de frequências (`dailyAt`, `weeklyOn`, `twiceDaily`, `cron`), entradas de argumentos obrigatórios e modificadores.
- [ ] **4.3.** Criar `src/components/quick-commands/__tests__/ArtisanCommandModal.spec.ts` e `QuickCommandsHost.spec.ts` testando:
  - Sugestão automática de `className` a partir de `signatureName` e parada após edição manual.
  - Botão `"Criar"` desabilitado quando existem issues de erro ou enquanto o preview está desatualizado (CA-18).
  - Atualização do preview com debounce de 250 ms (`vi.useFakeTimers`) em ≤ 300 ms (CA-19).
  - Fluxo de `"Preencher com IA"` e `"Desfazer sugestão"` pela UI (CA-25).
  - Fluxo de `"Criar com MaxCode"` enviando o prompt ao Plan Mode (CA-24).
  - Exibição de `manualSteps` após criar comando em layout Legacy ou com `routes/console.php` existente (CA-20, CA-22).
  - Confirmação em `QuickCommandsHost.vue` aprovando ou recusando `/install-force`, `/update-force` e `/deploy` (CA-31).

---

## 2. Comandos de Verificação e Validação Final da Fase 14

Execute a partir da raiz da worktree `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0`:

```bash
# Testes unitários focados dos componentes e stores da Fase 14
npx --no-install vitest run src/composables/quick-commands/__tests__/ src/stores/__tests__/quickCommands.spec.ts src/components/quick-commands/__tests__/

# Verificações de tipos, lint e estilo do Frontend (CA-03)
npm run typecheck
npm run typecheck:vue
npm run lint
npm run lint:style

# Verificação de sincronia dos bindings Specta (CA-04)
npm run bindings && git diff --exit-code src/types/bindings.ts

# Verificações completas de Backend Rust e Snapshots (CA-01, CA-02)
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands
cargo insta test --manifest-path src-tauri/Cargo.toml -p mt-commands --check
```

---

## 3. Mapeamento de Critérios de Aceite

| Critério | Cobertura nesta Implementação |
|----------|-------------------------------|
| **CA-03** | `npm run typecheck`, `npm run typecheck:vue`, `npm run lint`, `npm run lint:style` e `npm run test` retornando 0. |
| **CA-16** | Modal `ArtisanCommandModal.vue` abre com `Signature` pré-preenchida e sugere `className` automaticamente. |
| **CA-18** | Mensagens de erro em `pt-BR` exibidas abaixo de cada campo correspondente (`path`) e botão `"Criar"` bloqueado enquanto inválido. |
| **CA-19** | Preview PHP atualiza com debounce de 250 ms (≤ 300 ms após a última edição) e reflete exatamente o retorno do backend. |
| **CA-20 / CA-21 / CA-22** | Exibição de `routes/console.php`, `manualSteps` (inclusive `Kernel::schedule()` em Laravel ≤ 10) e tratamento de `artisan.file_exists`, `artisan.preview_stale` e `artisan.partial_write` no modal. |
| **CA-24 / CA-25** | Botões `"Criar com MaxCode"`, `"Preencher com IA"` e `"Desfazer sugestão"` integrados e testados no `ArtisanCommandModal.vue`. |
| **CA-31** | `QuickCommandsHost.vue` apresentando confirmação (`MaxPopoverConfirm`) para `/install-force`, `/update-force` e `/deploy`. |
| **CA-33** | Todos os SFCs na ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, tags de abertura em linha única, textos e comentários em `pt-BR`. |
| **CA-36** | Fechamento do modal com `Escape` preservando rascunho e restaurando o foco ao input do painel. |
