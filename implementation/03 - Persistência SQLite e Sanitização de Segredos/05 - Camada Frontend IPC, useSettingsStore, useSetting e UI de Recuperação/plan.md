# Implementação 05 — Camada Frontend IPC, `useSettingsStore`, `useSetting` e UI de Recuperação (`plan.md`)

> Guia de execução sequencial para a Implementação 05 da Fase 03.
> Referência técnica: [`./spec.md`](./spec.md).

---

## Passo 1 — Regenerar Bindings e Definir Contratos TypeScript (`src/types/`)

1. Executar `npm run bindings` para garantir que `src/types/bindings.ts` contenha os 5 comandos (`dbSettingsGet`, `dbSettingsSet`, `dbSettingSnapshot`, `dbStatus`, `dbRecover`), os 4 eventos (`settingsChangedEvent`, `dbRecoveredEvent`, `dbMaintenanceFinishedEvent`, `dbWriterLagEvent`) e todos os DTOs da Fase 03.
2. Criar `src/types/settings.ts` com `SettingsSchema`, `UiSettingKey`, `SettingKey` e `SettingValue<K>`.
3. Atualizar `src/types/errors.ts` adicionando as mensagens em `pt-BR` para todos os códigos `db.*` no mapa `errorMessages`.
4. Atualizar `src/types/index.ts` re-exportando `./settings`.

---

## Passo 2 — Implementar Wrappers IPC de Banco (`src/ipc/db.ts`)

1. Criar `src/ipc/db.ts` implementando `getSetting`, `setSetting`, `getSettingSnapshot`, `setSettingSnapshot`, `getDbStatus`, `recoverDb` e os listeners `onSettingsChanged`, `onDbRecovered`, `onDbMaintenanceFinished` e `onDbWriterLag`.
2. Atualizar `src/ipc/index.ts` adicionando `export * as dbIpc from './db';`.

---

## Passo 3 — Implementar `useSettingsStore` (`src/stores/settings.ts`)

1. Criar `src/stores/settings.ts`:
   - Implementar estado reativo (`values`, `loading`, `pending`, `revisions`, `requestGenerations`, `listening`, `dbStatus`).
   - Implementar getters `get`, `isLoaded` e `isRecoveryRequired`.
   - Implementar actions `ensure`, `set`, `setPreview`, `startListening`, `refreshStatus`, `recover` e `$reset`.
   - Garantir serialização de escritas por chave, controle monotônico de `revision`, proteção de geração (`requestGenerations`) em rollbacks de escrita otimista e limpeza segura de listeners no `$reset`.

---

## Passo 4 — Implementar `useSetting` (`src/composables/useSetting.ts`)

1. Criar `src/composables/useSetting.ts`:
   - Implementar `useSetting<K extends SettingKey>(key, fallback, options)` com `WritableComputedRef`, `ready`, `error` e `flush`.
   - Implementar debounce (padrão `300 ms`) que atualiza `store.setPreview(key, val)` imediatamente e envia uma única chamada `store.set(key, val)` ao expirar o timer.
   - Registrar `onScopeDispose` para cancelar o timer e disparar `flush()` pendente.

---

## Passo 5 — Integrar Bootstrap e UI de Recuperação no Shell (`DbRecoveryPanel.vue`, `App.vue`)

1. Criar `src/components/shell/DbRecoveryPanel.vue` seguindo todas as regras de SFC Vue (`<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, tags de abertura em linha única, 4 espaços com `baseIndent: 1`, tokens `--max-*` e `--background-*`):
   - Exibir lista de backups disponíveis com botão **"Restaurar backup"** e ação alternativa **"Criar banco vazio"** quando `state === 'recovery_required'`.
   - Exibir banner informativo quando o banco tiver acabado de se recuperar (`lastRecovered`).
2. Atualizar `src/app/App.vue`:
   - Registrar o passo de bootstrap `'db-settings'` via `appStore.registerBootstrapStep(...)` que chama `await settingsStore.startListening()` e `await settingsStore.refreshStatus()`.
   - Renderizar `DbRecoveryPanel` quando `settingsStore.isRecoveryRequired` (ou quando houver `lastRecovered` ativo).

---

## Passo 6 — Criar Suíte de Testes Vitest e Validar Cobertura ≥ 85%

1. Criar `tests/ipc/db.spec.ts` testando todos os wrappers e listeners de `src/ipc/db.ts` com `mockIPC`.
2. Criar `tests/stores/settings.spec.ts` testando `ensure`, `set` otimista, rollback em falha, preservação de edição mais recente quando requisição anterior falha, eventos `db://settings-changed` em ordem e fora de ordem, `recover` e `$reset` durante `startListening`.
3. Criar `tests/composables/useSetting.spec.ts` testando leitura de `fallback` vs `null`, escrita com debounce de 300 ms gerando 1 único IPC, reversão e exposição de `MtIpcError` via `useIpcError` e `flush` automático no dispose.
4. Atualizar `vitest.config.ts` garantindo verificação de cobertura ≥ 85% em `src/ipc/db.ts`, `src/stores/settings.ts` e `src/composables/useSetting.ts`.

---

## Passo 7 — Comandos de Verificação Final de Toda a Fase 03

Executar na raiz da worktree:

```bash
npm run bindings
npm run lint
npm run lint:style
npm run typecheck
npm run typecheck:vue
npm run test:coverage
npm run check
```

---

## Critérios de Aceite Cobertos nesta Implementação

- [x] **CA-04**: `src/types/bindings.ts` regenerado sem drift e exportando todos os comandos, DTOs e eventos da Fase 03.
- [x] **CA-05**: `npm run typecheck`, `npm run lint` e `npm run test` passam com cobertura ≥ 85% em `src/stores/settings.ts`, `src/ipc/db.ts` e `src/composables/useSetting.ts`.
- [x] **CA-34 / CA-35 / CA-36 / CA-37**: Wrappers IPC e store refletem defaults (`7`), erros de validação (`db.setting_invalid`), chaves desconhecidas (`db.setting_unknown`), chaves `ui.*` livres e limite de 64 KiB (`db.setting_too_large`).
- [x] **CA-38**: Evento pós-commit `{ key, value, revision }` sincroniza a store e a revisão monotônica impede que respostas/eventos atrasados sobrescrevam valores mais novos.
- [x] **CA-39**: `useSetting('ui.example.flag', false)` consolida alterações rápidas em uma única chamada IPC (debounce 300 ms), reverte o valor em caso de erro e expõe a falha compatível com `useIpcError`.
- [x] **CA-43**: Estado `recovery_required` exposto na UI do shell permitindo restaurar backup validado ou criar banco vazio mediante ação explícita do usuário.
