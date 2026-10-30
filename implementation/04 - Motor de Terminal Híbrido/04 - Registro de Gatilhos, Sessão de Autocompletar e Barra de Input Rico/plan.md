# Plano de Execução — Implementação 04: Registro de Gatilhos, Sessão de Autocompletar e Barra de Input Rico

> Especificação de referência: [`./spec.md`](./spec.md)
> Fase 04: Motor de Terminal Híbrido

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Implementar `fuzzyFilter` (`src/terminal/triggers/fuzzy.ts`)
1. Criar `src/terminal/triggers/fuzzy.ts` com `fuzzyScore` e `fuzzyFilter`:
   - Implementar busca por subsequência case-insensitive.
   - Atribuir bônus de pontuação para correspondência no início do texto, após separadores (`-`, `:`, `/`, `_`, espaço) e caracteres consecutivos.
   - Retornar lista inalterada quando `query` for vazia.

### Passo 2: Implementar `TriggerRegistry` (`src/terminal/triggers/triggerRegistry.ts`)
1. Criar `src/terminal/triggers/triggerRegistry.ts` com `TriggerRegistry`, `createTriggerRegistry()` e singleton `triggerRegistry`:
   - Validar prefixo (não vazio, sem espaços/controles, `<= 32` chars).
   - Lançar `TriggerConflictError` se `prefix` ou `id` já estiverem registrados.
   - Ordenar internamente por `prefix.length desc` para que `>>` sempre preceda `>`.
   - Implementar `match(input)` apenas na posição `0` da linha e retornar `null` para escapes literais (`\>` / `\?`) sem remover a barra invertida do texto.
   - Retornar `Disposable` em `register(def)` e `onChange(cb)`.

### Passo 3: Implementar `CompletionSession` (`src/terminal/triggers/completionSession.ts`)
1. Criar `src/terminal/triggers/completionSession.ts` implementando a máquina de estados completa (`idle`, `querying`, `showing`, `error`):
   - Controle de `generation` + `AbortController` + debounce (`def.debounceMs ?? 80`) + timeout de `5000 ms`.
   - Verificação de `def.isAvailable(ctx)` sem recair em prefixo menor caso o maior prefixo registrado retorne `false`.
   - Ordenação por `section.order` e fixação de itens `pinned: 'bottom'` sempre ao final.
   - Tratamento de `passthrough: true` (retorno imediato a `idle`) e `refreshAfter` (nova consulta automática se a geração não mudou).
   - Transições multi-etapa (`push-stage` e `handleBackspaceOnEmptyQuery` para `pop-stage`).
   - Supressão pós-`Escape` (`suppressCurrentTrigger`) até a linha esvaziar ou o prefixo mudar.
   - Proteção single-flight em `confirmSelection()`.

### Passo 4: Implementar Composables `useCompletion` e `useRichInput` (`src/composables/terminal/`)
1. Criar `src/composables/terminal/useCompletion.ts` envolvendo `CompletionSession` em refs reativas do Vue e limpando recursos em `onScopeDispose`.
2. Criar `src/composables/terminal/useRichInput.ts`:
   - Gerenciar `text`, `cursor`, histórico local (máx. 200 itens, sem duplicatas consecutivas) e integração com `useCompletion`.
   - Tratar teclas no popup aberto (`ArrowUp`, `ArrowDown`, `PageUp`, `PageDown`, `Home`, `End`, `Enter`, `Tab`, `Escape`, `Backspace` em etapa > 0) e no popup fechado (`ArrowUp`/`ArrowDown` para histórico, `Enter` para submissão, `Shift+Enter` para quebra de linha no rascunho, `Ctrl+C` com campo vazio para enviar `\x03`).
   - Garantir que `event.isComposing` nunca dispare submissão.

### Passo 5: Implementar Componentes `CompletionItem.vue`, `CompletionPopup.vue` e `RichInput.vue`
1. Criar `src/components/terminal/CompletionItem.vue` com atributos ARIA (`role="option"`, `aria-selected`, `aria-disabled`).
2. Criar `src/components/terminal/CompletionPopup.vue` com `role="listbox"`, cabeçalho com `title` e `stageLabel`, agrupamento por seções, itens `pinned: 'bottom'` e virtualização para listas com mais de 200 itens.
3. Criar `src/components/terminal/RichInput.vue` com `role="combobox"`, `aria-expanded`, `aria-controls`, `aria-activedescendant`, textarea auto-expansível (máx. 8 linhas) e integração com `CompletionPopup` e `InputModeToggle`.
4. Validar formatação SFC (tags de abertura em linha única, 4 espaços, `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`).

### Passo 6: Criar e Executar Testes Unitários e de Componentes (`tests/terminal/`)
1. Criar `tests/terminal/fuzzy.spec.ts`.
2. Criar `tests/terminal/triggerRegistry.spec.ts`.
3. Criar `tests/terminal/completionSession.spec.ts`.
4. Criar `tests/terminal/CompletionPopup.spec.ts`.
5. Criar `tests/terminal/RichInput.spec.ts`.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificação de tipos TypeScript
npm run typecheck

# 2. Execução dos testes da Implementação 04
npx --no-install vitest run tests/terminal/fuzzy.spec.ts tests/terminal/triggerRegistry.spec.ts tests/terminal/completionSession.spec.ts tests/terminal/CompletionPopup.spec.ts tests/terminal/RichInput.spec.ts

# 3. Verificação de lint (ESLint + Stylelint)
npm run lint
npm run lint:style
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 04)

- **[CA-5.1.3f — `triggerRegistry.spec.ts`]** `>>` vence `>`; prefixo fora da posição 0 não ativa; registro duplicado de prefixo ou id lança `TriggerConflictError`; `dispose()` remove o gatilho; escape `\>` e `\?` desativam o gatilho preservando a barra.
- **[CA-5.1.3g — `completionSession.spec.ts`]** Digitação rápida aborta a consulta anterior via `AbortSignal`; `push-stage` e `pop-stage` (`Backspace` em query vazia com `stage > 0`) funcionam; itens `pinned: 'bottom'` ficam sempre ao final; `passthrough: true` volta a `idle` sem popup; `refreshAfter` reconsulta o provider ao resolver; `Escape` suprime o gatilho no painel até a linha esvaziar ou o prefixo ser apagado; `>>` indisponível não recai em `>`.
- **[CA-5.1.3i — `CompletionPopup.spec.ts` / `RichInput.spec.ts`]** Navegação por setas (`↑`/`↓`), `Enter`, `Tab`, `Escape`, `Home`/`End`, wrap-around pulando itens `disabled`, `aria-activedescendant` sincronizado, envio de `texto + '\r'` ao PTY uma única vez, histórico local `↑`/`↓` e colagem alterando apenas o rascunho sem transmitir bytes imediatos.
