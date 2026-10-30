# Implementação 05 — Camada Frontend IPC, `useSettingsStore`, `useSetting` e UI de Recuperação (`spec.md`)

> Escopo da Fase 03: **I-16** (Frontend `src/ipc/db.ts`, `src/types/settings.ts`, `src/stores/settings.ts`, `src/composables/useSetting.ts` e integração mínima de loading/erro/recuperação no shell da Fase 01) e **I-17** (Testes Vitest com cobertura ≥ 85%).
> Critérios de aceite cobertos: **CA-04**, **CA-05**, **CA-34**, **CA-35**, **CA-36**, **CA-37**, **CA-38**, **CA-39**, **CA-43**.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação entrega toda a superfície Frontend (Vue 3.6 + TypeScript estrito + Pinia + UnoCSS/SCSS) da Fase 03:
1. **Esquema Tipado de Settings (`src/types/settings.ts`)**: interface `SettingsSchema` extensível via *declaration merging* pelas fases seguintes, `UiSettingKey` (`ui.${string}`), `SettingKey` e `SettingValue<K>`.
2. **Catálogo de Mensagens de Erro (`src/types/errors.ts`)**: mapeamento em `pt-BR` para todos os códigos `db.*`.
3. **Wrappers IPC (`src/ipc/db.ts`)**: funções tipadas `getSetting`, `setSetting`, `getSettingSnapshot`, `setSettingSnapshot`, `getDbStatus`, `recoverDb` e listeners `onSettingsChanged`, `onDbRecovered`, `onDbMaintenanceFinished`, `onDbWriterLag` construídas exclusivamente sobre `call()` de `src/ipc/core.ts` e `src/types/bindings.ts`.
4. **Store `useSettingsStore` (`src/stores/settings.ts`)**: gerenciamento reativo de settings sincronizado com o SQLite (sem cache local paralelo), escrita otimista serializada por chave com controle de `requestGenerations` e `revisions`, reversão cirúrgica em caso de falha (sem desfazer edições mais novas) e reconciliação via evento `db://settings-changed`.
5. **Composable `useSetting` (`src/composables/useSetting.ts`)**: `WritableComputedRef<SettingValue<K>>` com fallback tipado, debounce configurável (padrão 300 ms), `flush()` explícito e no `onScopeDispose`, e exposição de erros via `useIpcError`.
6. **Bootstrap e UI Mínima de Recuperação no Shell (`src/app/App.vue`, `src/components/shell/DbRecoveryPanel.vue`)**: integração no bootstrap via `registerBootstrapStep`, exibição do estado `recovery_required` com as ações explícitas **"Restaurar backup"** (listando ID e data) ou **"Criar banco vazio"** (informando perda potencial de dados) e banner/aviso pós-recuperação (`db://recovered`).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Novos Arquivos
- `src/types/settings.ts`
- `src/ipc/db.ts`
- `src/stores/settings.ts`
- `src/composables/useSetting.ts`
- `src/components/shell/DbRecoveryPanel.vue`
- `tests/ipc/db.spec.ts`
- `tests/stores/settings.spec.ts`
- `tests/composables/useSetting.spec.ts`

### 2.2. Arquivos a Modificar
- `src/types/bindings.ts` (regenerado via `npm run bindings`)
- `src/types/errors.ts` (adição das mensagens `db.*` em `errorMessages`)
- `src/types/index.ts` (re-export de `settings.ts`)
- `src/ipc/index.ts` (re-export `export * as dbIpc from './db';`)
- `src/app/App.vue` (registro do bootstrap step do banco e renderização de `DbRecoveryPanel.vue` quando o banco exigir recuperação)
- `vitest.config.ts` (inclusão de `src/composables/**` nos limiares de cobertura ≥ 85%)

---

## 3. Contratos TypeScript e IPC

### 3.1. `src/types/settings.ts`

```ts
import type { JsonValue } from './bindings';

/**
 * Mapa chave → tipo das configurações registradas no backend.
 * Fases futuras estendem esta interface via declaration merging:
 * `declare module '@/types/settings' { interface SettingsSchema { ... } }`
 */
export interface SettingsSchema {
    'db.retention.backup_daily_keep': number;
    'db.retention.max_db_size_mb': number;
    'db.maintenance.interval_hours': number;
    'db.maintenance.last_run_at': number | null;
}

export type UiSettingKey = `ui.${string}`;
export type SettingKey = keyof SettingsSchema | UiSettingKey;
export type SettingValue<K extends SettingKey> = K extends keyof SettingsSchema
    ? SettingsSchema[K]
    : JsonValue;
```

### 3.2. Adições em `src/types/errors.ts`

```ts
// Adições ao mapa errorMessages em src/types/errors.ts:
'db.open_failed': 'Não foi possível abrir o banco de dados local.',
'db.pragma_failed': 'Falha ao configurar parâmetros obrigatórios do banco de dados.',
'db.busy': 'O banco de dados está ocupado no momento. Tente novamente em instantes.',
'db.constraint': 'Violação de integridade ao salvar os dados no banco.',
'db.not_found': 'O registro solicitado não foi encontrado.',
'db.migration_failed': 'Falha ao atualizar a estrutura do banco de dados.',
'db.migration_checksum_mismatch': 'Inconsistência detectada nas migrações do banco de dados.',
'db.schema_too_new': 'Este banco de dados foi criado por uma versão mais recente do MaxTerminal.',
'db.corrupt': 'O arquivo do banco de dados apresenta sinais de corrupção.',
'db.recovery_required': 'O banco de dados precisa de recuperação antes de continuar.',
'db.in_use': 'O banco de dados já está aberto em outra instância do MaxTerminal.',
'db.conflict': 'Conflito de atualização concorrente no banco de dados.',
'db.cursor_invalid': 'Cursor de paginação inválido.',
'db.cursor_expired': 'A sessão de busca expirou. Realize a pesquisa novamente.',
'db.query_invalid': 'A consulta de busca informada é inválida.',
'db.storage_full': 'Espaço em disco insuficiente para gravar no banco de dados.',
'db.shutdown_timeout': 'Tempo esgotado ao aguardar gravações pendentes no encerramento.',
'db.not_ready': 'O banco de dados ainda está inicializando.',
'db.internal': 'Erro interno no banco de dados.',
'db.setting_unknown': 'Configuração desconhecida.',
'db.setting_invalid': 'O valor informado para a configuração é inválido.',
'db.setting_too_large': 'O valor da configuração excede o limite de 64 KiB.',
'db.setting_sensitive': 'Dados sensíveis ou credenciais não podem ser salvos nas configurações gerais.',
'db.backup_failed': 'Não foi possível criar ou restaurar o backup do banco de dados.'
```

### 3.3. `src/ipc/db.ts`

```ts
import {
    commands,
    events,
    type DbMaintenanceFinished,
    type DbRecovered,
    type DbRecoveryAction,
    type DbStatusDto,
    type DbWriterLag,
    type JsonValue,
    type SettingSnapshot,
    type SettingsChanged
} from '@/types/bindings';
import type { SettingKey, SettingValue } from '@/types/settings';
import { call } from './core';

export async function getSetting<K extends SettingKey>(key: K): Promise<SettingValue<K> | null> {
    const result = await call('db_settings_get', () => commands.dbSettingsGet(key));
    return result as SettingValue<K> | null;
}

export async function getSettingSnapshot<K extends SettingKey>(
    key: K
): Promise<{ value: SettingValue<K> | null; revision: number }> {
    const snap: SettingSnapshot = await call('db_setting_snapshot', () => commands.dbSettingSnapshot(key));
    return {
        value: snap.value as SettingValue<K> | null,
        revision: Number(snap.revision)
    };
}

export async function setSettingSnapshot<K extends SettingKey>(
    key: K,
    value: SettingValue<K>
): Promise<{ value: SettingValue<K> | null; revision: number }> {
    const snap: SettingSnapshot = await call('db_settings_set', () =>
        commands.dbSettingsSet(key, value as JsonValue)
    );
    return {
        value: snap.value as SettingValue<K> | null,
        revision: Number(snap.revision)
    };
}

export async function setSetting<K extends SettingKey>(key: K, value: SettingValue<K>): Promise<void> {
    await setSettingSnapshot(key, value);
}

export async function getDbStatus(): Promise<DbStatusDto> {
    return await call('db_status', () => commands.dbStatus());
}

export async function recoverDb(action: DbRecoveryAction): Promise<DbStatusDto> {
    return await call('db_recover', () => commands.dbRecover(action));
}

export const onSettingsChanged = (
    handler: (payload: SettingsChanged) => void
): Promise<() => void> => events.settingsChangedEvent.listen((ev) => handler(ev.payload));

export const onDbRecovered = (
    handler: (payload: DbRecovered) => void
): Promise<() => void> => events.dbRecoveredEvent.listen((ev) => handler(ev.payload));

export const onDbMaintenanceFinished = (
    handler: (payload: DbMaintenanceFinished) => void
): Promise<() => void> => events.dbMaintenanceFinishedEvent.listen((ev) => handler(ev.payload));

export const onDbWriterLag = (
    handler: (payload: DbWriterLag) => void
): Promise<() => void> => events.dbWriterLagEvent.listen((ev) => handler(ev.payload));
```

---

## 4. Store `useSettingsStore` (`src/stores/settings.ts`)

```ts
import { defineStore } from 'pinia';
import type { DbStatusDto, JsonValue } from '@/types/bindings';
import type { SettingKey, SettingValue } from '@/types/settings';
```

### 4.1. Estado, Getters e Actions Obrigatórios

- **State:**
  - `values: Partial<Record<SettingKey, JsonValue | null>>`
  - `loading: Partial<Record<SettingKey, boolean>>`
  - `pending: Partial<Record<SettingKey, JsonValue | null>>`
  - `revisions: Partial<Record<SettingKey, number>>`
  - `requestGenerations: Partial<Record<SettingKey, number>>`
  - `listening: boolean`
  - `dbStatus: DbStatusDto | null`
- **Getters:**
  - `get<K extends SettingKey>(key: K): SettingValue<K> | undefined` — retorna `pending[key]` se a chave existir em `pending` (`Object.prototype.hasOwnProperty.call(this.pending, key)`), senão `values[key]`.
  - `isLoaded(key: SettingKey): boolean` — `Object.prototype.hasOwnProperty.call(this.values, key)`.
  - `isRecoveryRequired: boolean` — `this.dbStatus?.state === 'recovery_required'`.
- **Actions:**
  - `ensure<K extends SettingKey>(key: K): Promise<SettingValue<K> | null>`:
    - Se já estiver em `values` e não houver requisição em voo, retorna `get(key) ?? null`.
    - Deduplica chamadas concorrentes para a mesma `key` compartilhando a `Promise` em voo.
    - Ao receber `{ value, revision }` de `getSettingSnapshot(key)`, aplica em `values[key]` e `revisions[key]` **somente se** `revision >= (revisions[key] ?? 0)`.
  - `set<K extends SettingKey>(key: K, value: SettingValue<K>): Promise<void>`:
    - Incrementa `gen = (requestGenerations[key] ?? 0) + 1` e define `requestGenerations[key] = gen`.
    - Define `pending[key] = value as JsonValue | null` imediatamente (atualização otimista).
    - Encadeia a chamada IPC na fila serial daquela `key` para que escritas consecutivas na mesma chave nunca cheguem fora de ordem no backend.
    - No sucesso `{ value: savedValue, revision }`:
      - Se `revision >= (revisions[key] ?? 0)`, atualiza `values[key] = savedValue` e `revisions[key] = revision`.
      - Se `requestGenerations[key] === gen`, remove `delete pending[key]`.
    - Na falha (`err`):
      - Se `requestGenerations[key] === gen`, remove `delete pending[key]` (revertendo para `values[key]`). Se outra edição mais recente (`gen2 > gen`) já tiver ocorrido, **preserva** o `pending[key]` da edição nova!
      - Relança o `MtIpcError`.
  - `setPreview<K extends SettingKey>(key: K, value: SettingValue<K>): void`:
    - Atualiza `pending[key]` localmente durante a janela de debounce do `useSetting` antes do disparo efetivo de `set()`.
  - `startListening(): Promise<void>`:
    - Idempotente. Incrementa token de ciclo de escuta para que, se `$reset()` for chamado antes de `onSettingsChanged` resolver, o `unlisten` seja executado imediatamente após resolver (evitando vazamento de listener).
    - Ao receber `{ key, value, revision }` de `db://settings-changed`:
      - Se `revision > (revisions[key] ?? 0)`, atualiza `values[key] = value` e `revisions[key] = revision`, **sem apagar** `pending[key]` caso haja uma escrita local mais recente ainda em voo.
  - `refreshStatus(): Promise<DbStatusDto>` e `recover(action: DbRecoveryAction): Promise<DbStatusDto>`.
  - `$reset(): void`:
    - Cancela listeners ativos, limpa mapas de promises em voo, incrementa época de geração e zera o estado.

---

## 5. Composable `useSetting` (`src/composables/useSetting.ts`)

```ts
import { computed, onScopeDispose, readonly, ref, type DeepReadonly, type Ref, type WritableComputedRef } from 'vue';
import type { MtIpcError } from '@/ipc/core';
import { isMtIpcError } from '@/ipc/core';
import { useSettingsStore } from '@/stores/settings';
import type { SettingKey, SettingValue } from '@/types/settings';

export interface UseSettingOptions {
    debounceMs?: number;
}

export interface UseSettingReturn<K extends SettingKey> {
    value: WritableComputedRef<SettingValue<K>>;
    ready: DeepReadonly<Ref<boolean>>;
    error: DeepReadonly<Ref<MtIpcError | null>>;
    flush: () => Promise<void>;
}

export function useSetting<K extends SettingKey>(
    key: K,
    fallback: SettingValue<K>,
    options?: UseSettingOptions
): UseSettingReturn<K>;
```

**Regras de comportamento de `useSetting` (CA-39):**
1. Ao inicializar, dispara `void store.ensure(key)` (tratando erro em `error.value` se falhar).
2. `ready` é `computed(() => store.isLoaded(key))`.
3. `value.get()`:
   - Se `key` existir em `store.pending` ou `store.isLoaded(key)`, retorna `store.get(key) as SettingValue<K>` (preservando `null` quando `null` é um valor válido do schema, como em `db.maintenance.last_run_at`).
   - Caso contrário, retorna `fallback`.
4. `value.set(newValue)`:
   - Limpa `error.value = null`.
   - Chama `store.setPreview(key, newValue)` para refletir a escrita otimista imediatamente em `value.value`.
   - Agenda timer de debounce (`options?.debounceMs ?? 300` ms). Escritas rápidas dentro da janela substituem o timer anterior, gerando **uma única chamada IPC** ao final do debounce.
5. `flush()`:
   - Se houver timer de debounce pendente, cancela o timer e executa imediatamente `await store.set(key, latestQueuedValue)`.
   - Se `store.set` lançar `MtIpcError`, captura em `error.value` (e a store reverte `pending[key]` automaticamente para o valor anterior).
6. `onScopeDispose`:
   - Se houver escrita pendente no debounce, cancela o timer e dispara `void flush()` com captura interna de erro.

---

## 6. Integração no Bootstrap e UI Mínima de Recuperação (`DbRecoveryPanel.vue`, `App.vue`)

### 6.1. `src/components/shell/DbRecoveryPanel.vue`

Seguindo estritamente as regras de SFC Vue do `AGENTS.md` (§3: `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, recuo de 4 espaços com `baseIndent: 1`, tags de abertura em linha única, aspas simples, ponto e vírgula, sem vírgula final e tokens `--max-*` / `--background-*`):

- Exibe alerta semântico quando `status.state === 'recovery_required'`:
  - Título em `pt-BR`: `"Recuperação do Banco de Dados Necessária"`
  - Explicação clara sobre a preservação dos arquivos originais em quarentena e a perda potencial de dados não salvos desde o último backup.
  - Lista de backups disponíveis (`status.recovery?.availableBackups`) exibindo identificador, tipo (`Diário` / `Pré-migração`) e data formatada em `pt-BR`, com botão **"Restaurar backup"** para cada item.
  - Botão secundário de confirmação explícita **"Criar banco vazio"** caso não existam backups ou o usuário opte por iniciar um banco limpo.
- Exibe aviso dispensável quando `lastRecovered` estiver presente (`db://recovered`), informando se o banco foi restaurado de um backup ou recriado vazio.

---

## 7. Testes Obrigatórios da Implementação 05

1. **`tests/ipc/db.spec.ts` (Vitest + `mockIPC`):**
   - `getSetting`, `setSetting`, `getSettingSnapshot`, `setSettingSnapshot`, `getDbStatus` e `recoverDb` invocam os comandos Tauri corretos com os argumentos esperados.
   - Rejeições de domínio (`db.setting_invalid`, `db.setting_unknown`, `db.setting_too_large`, `db.setting_sensitive`) são convertidas em `MtIpcError` preservando `code`, `message` e `details`.
2. **`tests/stores/settings.spec.ts` (CA-34, CA-38):**
   - `ensure` busca snapshot uma única vez e deduplica chamadas simultâneas.
   - `set` aplica valor otimista em `pending`, confirma `revision` no retorno e reverte `pending` em caso de erro.
   - **Concorrência e ordenação:**
     - Eventos/respostas com `revision` menor que a conhecida são ignorados (revisão monotônica).
     - Se a escrita `G1` falhar após o usuário já ter disparado uma escrita mais nova `G2`, a falha de `G1` **não** apaga o `pending` de `G2`.
     - Chamar `$reset()` enquanto `startListening()` ainda está aguardando registro executa `unlisten()` assim que a promise resolve.
3. **`tests/composables/useSetting.spec.ts` (CA-39):**
   - `useSetting('ui.example.flag', false)` retorna `Ref` gravável iniciando com `false` e atualizando após `ensure`.
   - 5 alterações rápidas em sequência com `vi.useFakeTimers()` geram **exatamente 1 chamada IPC** `db_settings_set` após 300 ms.
   - Em caso de erro IPC (`db.setting_invalid`), o valor de `value.value` reverte para o anterior e `error.value` expõe o `MtIpcError` traduzível por `useIpcError()`.
   - Desmontar o escopo (`effectScope().stop()`) com edição pendente no debounce executa o `flush()` imediatamente.
