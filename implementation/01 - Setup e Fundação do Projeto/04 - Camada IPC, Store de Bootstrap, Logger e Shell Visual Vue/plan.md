# Implementação 04 — Camada IPC, Store de Bootstrap, Logger e Shell Visual Vue (`plan.md`)

> Especificação técnica desta etapa: [`./spec.md`](./spec.md)
> Fase: [`../instructions.md`](../instructions.md) | [`../spec.md`](../spec.md)

---

## 1. Pré-requisitos e Regras Operacionais

- As Implementações 01, 02 e 03 devem estar concluídas (`npm run sync:local-libs` executado e `src/types/bindings.ts` gerado por `npm run bindings`).
- Nenhum tipo Rust (`AppInfo`, `AppPathsDto`, `BuildProfile`, `MtError`, `ErrorCode`, `FrontendLogEvent` ou os 9 IDs, incluindo `TabSessionId`) pode ser redeclarado manualmente em `src/types/` — todos devem vir de `src/types/bindings.ts`.
- Componentes e stores nunca importam `commands` de `src/types/bindings.ts` diretamente; todo acesso passa por `src/ipc/app.ts` e `src/ipc/core.ts`.
- Todos os SFCs `.vue` devem obedecer à ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, com tags de abertura em linha única e zero cores hexadecimais hardcoded.

---

## 2. Checklist Passo a Passo de Execução

### Passo 1 — Tipos de Erro, Camada IPC e Composable `useIpcError`
- [ ] **1.1.** Confirmar que `src/types/bindings.ts` foi gerado por `npm run bindings` e criar `src/types/errors.ts` e `src/types/index.ts`.
- [ ] **1.2.** Implementar `src/ipc/core.ts` (`MtIpcError`, `isMtIpcError`, `call`, `unwrap`, `IpcResult<T>`), `src/ipc/app.ts` (`getAppInfo`, `logFrontend`) e `src/ipc/index.ts`.
- [ ] **1.3.** Implementar `src/composables/useIpcError.ts` traduzindo `MtIpcError` para mensagens em `pt-BR`.

### Passo 2 — Logger do Frontend e Store `useAppStore`
- [ ] **2.1.** Implementar `src/app/logger.ts` (`createFrontendLogger`, `logger`, `installGlobalErrorHandlers`) com fila limitada a 64 itens, deduplicação por microtask, 1 envio em voo e desmontagem idempotente.
- [ ] **2.2.** Implementar `src/stores/app.ts` (`useAppStore` e `BootstrapStep`) com `info`, `status`, `error`, `isReady`, `versionLabel`, `isDebug`, `registerBootstrapStep`, `bootstrap` (idempotente/concorrente) e `retry`.

### Passo 3 — Estilos Navy/Teal, Componentes do Shell e Bootstrap Vue
- [ ] **3.1.** Criar `src/styles/params.scss`, `src/styles/theme.scss` (`color-scheme: dark`, tokens `--background-*`, `--max-primary-*`, `--mt-*`) e `src/styles/main.scss`.
- [ ] **3.2.** Implementar `src/components/shell/AppTitleBar.vue`, `src/components/shell/AppEmptyState.vue` e `src/components/shell/AppShell.vue`.
- [ ] **3.3.** Implementar `src/app/App.vue`, `src/app/plugins.ts` (integrando `@maxvue/max-pinia` e `@maxvue/max-components-ui` conforme exports reais) e `src/app/main.ts`, além de `index.html` na raiz da worktree.

### Passo 4 — Testes Unitários e Cobertura (`tests/`)
- [ ] **4.1.** Criar `tests/ipc/core.spec.ts` cobrindo sucesso, erro de domínio (`MtError`), falha de transporte, exceção síncrona, payloads malformados e `useIpcError`.
- [ ] **4.2.** Criar `tests/stores/app.spec.ts` usando `mockIPC` de `@tauri-apps/api/mocks` cobrindo `bootstrap()`, deduplicação de chamadas concorrentes, `registerBootstrapStep()`, falha de IPC e `retry()`.
- [ ] **4.3.** Criar `tests/app/logger.spec.ts` cobrindo deduplicação por microtask, limite de fila (64), serialização de 1 request em voo, silenciamento de erro de transporte, não-vazamento de sentinelas nos handlers globais e desmontagem idempotente.
- [ ] **4.4.** Criar `tests/components/shell/AppShell.spec.ts` testando slots de `AppShell.vue`, renderização de `App.vue` quando `ready` (`"MaxTerminal"`, `"v0.1.0"`, `"Nenhum projeto selecionado"`) e estado de erro em `pt-BR` com `code` e botão `"Tentar novamente"`.

### Passo 5 — Validação End-to-End da Fase 01
- [ ] **5.0.** Com os primeiros `.vue`/`.scss` de `src/` já criados (passos 2 a 4), executar `npm run lint:style` e `npm run typecheck:vue` isoladamente: é aqui que essas verificações passam a validar arquivos reais (em `01/03` elas passam sobre árvore sem SFCs, via `--allow-empty-input`).
- [ ] **5.1.** Executar `npm run check` e confirmar que todos os 10 estágios (links locais, ESLint, Stylelint, `tsgo`, `vue-tsc`, Vitest com cobertura ≥ 80%, Vite build, `cargo fmt --check`, `cargo clippy`, `cargo test`) passam com exit code `0`.
- [ ] **5.2.** Executar `npm run tauri build -- --no-bundle` (e smoke de `npm run tauri dev` / binário nativo quando houver sessão gráfica disponível) confirmando a criação dos diretórios XDG sem `.db` e o log `MaxTerminal iniciado`.

---

## 3. Comandos de Validação e Mapeamento Completo para Critérios de Aceite (`CA-01` a `CA-30`)

| Critério | Comando de Verificação | Resultado Esperado |
|----------|------------------------|--------------------|
| **CA-10** (Testes e Cobertura ≥ 80%) | `npm run test && npm run test:coverage` | Todos os testes passam e cobertura de linhas ≥ 80% em `src/ipc/**`, `src/stores/**`, `src/app/logger.ts` e `src/composables/**`. |
| **CA-18** (Sem DTOs Rust manuais em `src/types/`) | `rg -n "interface (AppInfo|AppPathsDto|MtError)|type (BuildProfile|ErrorCode|FrontendLogEvent|ProjectId)" src/types/errors.ts src/types/index.ts` | Nenhuma ocorrência fora de `src/types/bindings.ts`. |
| **CA-19** (Janela `MaxTerminal` 1440×900 / min 960×600) | `cd src-tauri && cargo test --locked --test capabilities` + smoke `npm run tauri dev` | Janela configurada e aberta com título `"MaxTerminal"`, `1440×900` e mínimo `960×600`. |
| **CA-20** (`AppTitleBar` + `AppEmptyState` Navy/Teal) | `npx --no-install vitest run tests/components/shell/AppShell.spec.ts` | Exibe título + versão vinda de `app_info` e `"Nenhum projeto selecionado"` com tokens Navy/Teal. |
| **CA-23** (Handler Vue sem vazar segredos) | `npx --no-install vitest run tests/app/logger.spec.ts` | Erro Vue envia apenas `'vue_error'`; mensagem, stack e sentinelas são descartados. |
| **CA-24** (Estado de erro de boot em `pt-BR` + `code`) | `npx --no-install vitest run tests/components/shell/AppShell.spec.ts -t "erro"` | Falha simulada com `mockIPC` renderiza mensagem em `pt-BR`, `code` do `MtError` e botão `"Tentar novamente"` funcional. |
| **CA-25** (Build nativo sem bundle) | `npm run tauri build -- --no-bundle` | Conclui com exit code `0` e gera o binário executável em `src-tauri/target/release/maxterminal`. |
| **CA-27** (Idioma `pt-BR` nos textos/comentários e `en-US` no código) | `npm run lint && npm run test` | Comentários, textos de UI e `MtError.message` em `pt-BR`; identificadores e caminhos em `en-US`. |
| **CA-28** (Pipeline agregador `npm run check`) | `npm run check` | Executa `sync:local-libs --check`, `lint`, `lint:style`, `typecheck`, `typecheck:vue`, `test:coverage`, `build`, `cargo fmt --check`, `cargo clippy` e `cargo test` retornando `0`. |
