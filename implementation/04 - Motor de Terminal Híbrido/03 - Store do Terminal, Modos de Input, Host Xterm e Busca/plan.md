# Plano de Execução — Implementação 03: Store do Terminal, Modos de Input, Host Xterm e Busca

> Especificação de referência: [`./spec.md`](./spec.md)
> Fase 04: Motor de Terminal Híbrido

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Implementar `InputModeResolver` (`src/terminal/input/inputModeResolver.ts`)
1. Criar `src/terminal/input/inputModeResolver.ts` implementando:
   - Registro `registerInputModeHint(provider): Disposable` e `clearInputModeHintsForTests()`.
   - Função `resolveInputMode({ context, modes, override, richInputEnabled })` respeitando a hierarquia estrita: `altScreen || mouseTracking !== 'none'` ⇒ `'passthrough'`; senão `override`; senão dica de maior `priority` (empate: última registrada); senão `'rich'` se `richInputEnabled` ou `'passthrough'`.
   - Helper `shouldClearOverrideOnModesChange(prevModes, nextModes)` retornando `true` quando `prevModes.altScreen !== nextModes.altScreen`.

### Passo 2: Implementar `useTerminalStore` (`src/stores/terminal.ts`)
1. Criar `src/stores/terminal.ts` com `useTerminalStore`:
   - Estado puramente serializável (`instances`, `preferences`, `flowConfig`, `activePaneId`).
   - Getters `instance`, `isPassthrough`, `activeInstance`, `webglCount`.
   - Actions `register`, `unregister`, `patch` (com recálculo automático de `inputMode` e limpeza de override em transição de `altScreen`), `setInputModeOverride`, `toggleInputMode`, `setPreferences` (propagando às instâncias do `terminalInstanceRegistry`), `openSearch`, `closeSearch` e `setActive`.

### Passo 3: Implementar Composables `useTerminal` e `useTerminalSearch` (`src/composables/terminal/`)
1. Criar `src/composables/terminal/useTerminal.ts`:
   - Conectar o elemento `hostRef` ao `terminalInstanceRegistry.getOrCreate(...)`.
   - Usar `markRaw` para armazenar o `TerminalInstanceHandle` em `ShallowRef`.
   - Chamar `attach(host)` em `onMounted` e `detach()` em `onBeforeUnmount` (sem chamar `dispose`).
   - Sincronizar callbacks do `TerminalInstance` (`onDimensionsChange`, `onRendererChange`, `onModesChange`, `onTitleChange`, `onFlowStateChange`, `onExit`) com `terminalStore.patch(paneId, ...)`.
2. Criar `src/composables/terminal/useTerminalSearch.ts`:
   - Conectar `searchAddon` da instância no registry ao estado `search` do `useTerminalStore`.
   - Resolver tokens CSS `--max-warning-*` e `--max-primary-*` em cores `#RRGGBB` para as decorações do `SearchAddon`.
   - Tratar `RegExp` inválida localmente em `regexError` sem propagar exceção.

### Passo 4: Criar Componentes Vue (`TerminalView`, `TerminalSearchBar`, `TerminalExitBanner`, `InputModeToggle`)
1. Criar `src/components/terminal/TerminalView.vue` (importando `@xterm/xterm/css/xterm.css`, com `contain: strict`, `role="application"`, `aria-label="Terminal"`).
2. Criar `src/components/terminal/TerminalSearchBar.vue` com input de busca, botões toggle (`Aa`, `Palavra`, `.*`), navegação `Enter`/`Shift+Enter`, fechamento com `Escape` e exibição de `"n de m"` / `"Sem resultados"` / `"Limite de destaques atingido"`.
3. Criar `src/components/terminal/TerminalExitBanner.vue` exibindo status de encerramento por código ou sinal (`signalName`) e botão `"Reiniciar"`.
4. Criar `src/components/terminal/InputModeToggle.vue` alternando o modo de entrada do painel e exibindo tooltip em pt-BR.
5. Verificar em todos os 4 SFCs: ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, indentação de 4 espaços, tags de abertura em linha única, aspas simples e ponto e vírgula.

### Passo 5: Criar e Executar Testes Unitários e de Componentes (`tests/terminal/`)
1. Criar `tests/terminal/inputModeResolver.spec.ts`.
2. Criar `tests/terminal/terminalStore.spec.ts`.
3. Criar `tests/terminal/TerminalSearchBar.spec.ts`.
4. Criar `tests/terminal/TerminalExitBanner.spec.ts`.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificação de tipos TypeScript (tsgo)
npm run typecheck

# 2. Execução dos testes da Implementação 03
npx --no-install vitest run tests/terminal/inputModeResolver.spec.ts tests/terminal/terminalStore.spec.ts tests/terminal/TerminalSearchBar.spec.ts tests/terminal/TerminalExitBanner.spec.ts

# 3. Verificação de lint (ESLint + Stylelint nos novos SFCs)
npm run lint
npm run lint:style
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 04)

- **[CA-5.1.1]** `npm run typecheck`, `npm run lint` e `npm run lint:style` passam sem erros nos arquivos `.ts` e `.vue`.
- **[CA-5.1.3h — `inputModeResolver.spec.ts`]** `altScreen = true` ou `mouseTracking !== 'none'` forçam `'passthrough'` inclusive quando há `inputModeOverride = 'rich'`; fora dessas condições, `override` vence dicas; `InputModeHintProvider` de prioridade maior vence a menor (e em empate, o último registrado vence); transição de `altScreen` limpa o override manual.
- **[CA-5.2.6 — `TerminalSearchBar.spec.ts`]** Digitar na barra de busca aciona `findNext`; `Enter` avança e `Shift+Enter` retrocede; toggles `Aa`, `Palavra` e `.*` repassam opções ao `SearchAddon`; contador exibe `"n de m"` (1-based) ou `"Sem resultados"`; regex malformada exibe mensagem de erro local sem quebrar o componente; `Escape` fecha a busca e restaura foco no terminal.
- **[CA-5.2.13 / §10 — `TerminalExitBanner.spec.ts`]** Exibe `"Processo encerrado (código X)"` quando finalizado por código e `"Processo encerrado pelo sinal SIGKILL"` quando finalizado por sinal, emitindo `restart-requested` ao clicar em `"Reiniciar"`.
