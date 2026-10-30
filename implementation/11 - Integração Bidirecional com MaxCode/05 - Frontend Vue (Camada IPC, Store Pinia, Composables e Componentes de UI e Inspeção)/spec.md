# Implementação 05 — Frontend Vue (Camada IPC, Store Pinia, Composables e Componentes de UI e Inspeção) (`spec.md`)

> Escopo da Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§13, §14, §18.3).
> Diretrizes de código Vue 3.6 + TypeScript + UnoCSS + SCSS: [`../../../AGENTS.md`](../../../AGENTS.md) §3 e §6.
> Depende de: **Implementação 01 a 04**.

---

## 1. Contexto, Objetivo e Escopo Exato

Esta é a **quinta e última unidade de implementação da Fase 11**. Com o crate `mt-bridge`, o host Tauri, os 9 comandos IPC e os bindings TypeScript gerados na Implementação 04, esta etapa constrói toda a camada de frontend Vue 3.6 + TypeScript + Pinia + `@maxvue/max-components-ui`:
1. Cliente IPC tipado `src/ipc/bridge.ts` encapsulando os 9 comandos Tauri e os 6 listeners de eventos `bridge://*`.
2. Store Pinia `useMaxCodeBridgeStore` (`src/stores/maxCodeBridge.ts`) com inicialização idempotente single-flight, controle de geração no `dispose()`, reconciliação de `instance_id` e `revision`, deduplicação de cliques concorrentes em `inspect(blockId)`, toasts em `pt-BR` via `Toast.show` e API pública `openPlan` para consumo das Fases 14 e 16.
3. Composable `useActiveProjectSync` (`src/composables/useActiveProjectSync.ts`) montado no `AppShell.vue`, integrando a seleção de projetos da Fase 06 (`useProjectsStore`) com confirmação causal `bridge_notify_active_project(projectId, context)` sem loops.
4. Composable `useMaxCodeInspect` (`src/composables/useMaxCodeInspect.ts`) e componente `InspectButton.vue` (`src/components/bridge/InspectButton.vue`) montado no slot `#actions` do bloco de comando (Fase 04), visível **exclusivamente** quando `exitCode !== null && exitCode !== undefined && exitCode !== 0`.
5. Componentes `BridgeStatusIndicator.vue` (inserido na `AppTitleBar.vue` com cores semânticas `--max-success-500`, `--max-info-500`, `--max-danger-500` e tooltip de porta/conexões) e `BridgeSettingsSection.vue` (seção de configurações da ponte com `MaxPopoverConfirm` ao regenerar token).
6. Suíte completa de testes unitários e de componentes com Vitest (`tests/stores/maxCodeBridge.spec.ts`, `tests/composables/useActiveProjectSync.spec.ts`, `tests/components/bridge/InspectButton.spec.ts`, `tests/components/bridge/BridgeStatusIndicator.spec.ts`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho Exato | Descrição |
|------|---------------|-----------|
| Criar | `src/ipc/bridge.ts` | Wrapper `bridgeIpc` para os 9 comandos e 6 funções `on*` de eventos `bridge://*`. |
| Criar | `src/stores/maxCodeBridge.ts` | Store `useMaxCodeBridgeStore` com estado, getters (`state`, `isConnected`, `peers`, `port`, `syncEnabled`, `indicator`, `isInspecting`) e actions (`init`, `dispose`, `updateConfig`, `restart`, `regenerateToken`, `openPlan`, `openProject`, `inspect`). |
| Criar | `src/composables/useActiveProjectSync.ts` | Sincronização bidirecional entre `useProjectsStore` (Fase 06) e `bridgeIpc` com propagação causal de `SyncContext`. |
| Criar | `src/composables/useMaxCodeInspect.ts` | Composable reativo (`visible`, `loading`, `inspect`) para blocos de comando com erro. |
| Criar | `src/components/bridge/InspectButton.vue` | Botão `"Inspecionar"` para o slot `#actions` de `CommandBlock.vue`. |
| Criar | `src/components/bridge/BridgeStatusIndicator.vue` | Indicador semântico de status da ponte para a `AppTitleBar.vue`. |
| Criar | `src/components/bridge/BridgeSettingsSection.vue` | Formulário de configuração da ponte (ativar, porta preferida, sincronizar projeto ativo, lista de peers, reiniciar e gerar novo token). |
| Modificar | `src/components/layout/AppShell.vue` | Invocar `useMaxCodeBridgeStore().init()` e `useActiveProjectSync()` no ciclo de vida do shell principal. |
| Modificar | `src/components/layout/AppTitleBar.vue` | Inserir `<BridgeStatusIndicator />` na barra de título. |
| Modificar | `src/components/terminal/CommandBlock.vue` | Renderizar `<InspectButton :block="block" />` no slot `#actions` (`order: 10`, ao lado do slot futuro da Fase 12). |
| Criar | `tests/stores/maxCodeBridge.spec.ts` | Testes unitários da store `useMaxCodeBridgeStore`. |
| Criar | `tests/composables/useActiveProjectSync.spec.ts` | Testes unitários do composable `useActiveProjectSync`. |
| Criar | `tests/components/bridge/InspectButton.spec.ts` | Testes de componente do `InspectButton.vue` (`exitCode` `null`, `undefined`, `0`, `2`, `-1`, loading e emit). |
| Criar | `tests/components/bridge/BridgeStatusIndicator.spec.ts` | Testes de componente do `BridgeStatusIndicator.vue` (4 estados visuais, tooltip e clique). |

---

## 3. Assinaturas Exatas e Completas (TypeScript e Vue SFC)

> **Padrões obrigatórios (`AGENTS.md` §3):**
> - Ordem dos blocos em `.vue`: `<template>`, `<script setup lang="ts">`, `<style lang="scss" scoped>`.
> - Tags de abertura no `<template>` em **linha única** (nome da tag, atributos, props, diretivas, eventos e `>` / `/>` na mesma linha).
> - Recuo de 4 espaços (`baseIndent: 1` dentro de `<script setup>`), aspas simples (`'`), ponto e vírgula obrigatório e sem vírgula final (`comma-dangle: 'never'`).

### 3.1. `src/ipc/bridge.ts`

```ts
import {
    commands,
    type BridgeConfig,
    type BridgeStatus,
    type CommandBlockId,
    type ConversationSource,
    type DeliveryFailedEvent,
    type DeliveryReport,
    type PeerDisconnectedEvent,
    type PeerInfo,
    type ProjectFocusRequested,
    type ProjectId,
    type SyncContext,
    type TabOpenedEvent
} from '@/types/bindings';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from './core';

export const bridgeIpc = {
    status: (): Promise<BridgeStatus> => unwrap(commands.bridgeStatus()),
    getConfig: (): Promise<BridgeConfig> => unwrap(commands.bridgeGetConfig()),
    setConfig: (config: BridgeConfig): Promise<BridgeStatus> => unwrap(commands.bridgeSetConfig(config)),
    restart: (): Promise<BridgeStatus> => unwrap(commands.bridgeRestart()),
    regenerateToken: (): Promise<BridgeStatus> => unwrap(commands.bridgeRegenerateToken()),
    openPlan: (
        prompt: string,
        projectPath: string | null,
        source: ConversationSource = 'generic'
    ): Promise<DeliveryReport> => unwrap(commands.maxcodeOpenPlan(prompt, projectPath, source)),
    openProject: (projectPath: string): Promise<DeliveryReport> => unwrap(commands.maxcodeOpenProject(projectPath)),
    inspectBlock: (blockId: CommandBlockId): Promise<DeliveryReport> => unwrap(commands.maxcodeInspectBlock(blockId)),
    notifyActiveProject: (projectId: ProjectId, context: SyncContext): Promise<void> =>
        unwrap(commands.bridgeNotifyActiveProject(projectId, context))
};

export const onBridgeStatus = (cb: (status: BridgeStatus) => void): Promise<UnlistenFn> =>
    listen<BridgeStatus>('bridge://status-changed', (event) => cb(event.payload));

export const onPeerConnected = (cb: (peer: PeerInfo) => void): Promise<UnlistenFn> =>
    listen<PeerInfo>('bridge://peer-connected', (event) => cb(event.payload));

export const onPeerDisconnected = (cb: (payload: PeerDisconnectedEvent) => void): Promise<UnlistenFn> =>
    listen<PeerDisconnectedEvent>('bridge://peer-disconnected', (event) => cb(event.payload));

export const onProjectFocusRequested = (cb: (payload: ProjectFocusRequested) => void): Promise<UnlistenFn> =>
    listen<ProjectFocusRequested>('bridge://project-focus-requested', (event) => cb(event.payload));

export const onTabOpened = (cb: (payload: TabOpenedEvent) => void): Promise<UnlistenFn> =>
    listen<TabOpenedEvent>('bridge://tab-opened', (event) => cb(event.payload));

export const onDeliveryFailed = (cb: (payload: DeliveryFailedEvent) => void): Promise<UnlistenFn> =>
    listen<DeliveryFailedEvent>('bridge://delivery-failed', (event) => cb(event.payload));
```

### 3.2. `src/stores/maxCodeBridge.ts`

```ts
import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import { Toast } from '@maxvue/max-components-ui';
import {
    bridgeIpc,
    onBridgeStatus,
    onDeliveryFailed,
    onPeerConnected,
    onPeerDisconnected
} from '@/ipc/bridge';
import type {
    BridgeConfig,
    BridgeState,
    BridgeStatus,
    CommandBlockId,
    ConversationSource,
    DeliveryReport,
    MtError,
    PeerInfo
} from '@/types/bindings';

export interface BridgeIndicator {
    color: 'success' | 'info' | 'danger' | 'neutral';
    label: string;
}

export const useMaxCodeBridgeStore = defineStore('maxCodeBridge', () => {
    const status = ref<BridgeStatus | null>(null);
    const config = ref<BridgeConfig | null>(null);
    const pendingBlockIds = ref<Set<CommandBlockId>>(new Set());
    const lastDelivery = ref<DeliveryReport | null>(null);
    const lastError = ref<MtError | null>(null);
    const initialized = ref<boolean>(false);

    // Getters
    const state = computed<BridgeState>(() => status.value?.state ?? 'disabled');
    const isConnected = computed<boolean>(() => state.value === 'connected');
    const peers = computed<PeerInfo[]>(() => status.value?.peers ?? []);
    const port = computed<number | null>(() => status.value?.port ?? null);
    const syncEnabled = computed<boolean>(() => config.value?.sync_active_project ?? false);
    const indicator = computed<BridgeIndicator>(() => {
        switch (state.value) {
            case 'connected':
                return { color: 'success', label: 'MaxCode conectado' };
            case 'listening':
            case 'starting':
                return { color: 'info', label: 'Aguardando MaxCode' };
            case 'failed':
                return { color: 'danger', label: 'Ponte indisponível' };
            case 'disabled':
            default:
                return { color: 'neutral', label: 'Ponte desativada' };
        }
    });
    const isInspecting = (id: CommandBlockId): boolean => pendingBlockIds.value.has(id);

    // Actions: init, dispose, updateConfig, restart, regenerateToken, openPlan, openProject, inspect
    // ...
});
```

Regras estritas de `useMaxCodeBridgeStore`:
- **`applyStatus(incoming: BridgeStatus)`**: se `status.value` já existir e `status.value.instance_id === incoming.instance_id` e `incoming.revision < status.value.revision`, descarta o evento obsoleto. Se `instance_id` mudou ou `revision >= status.value.revision`, atualiza `status.value`.
- **`init()`**: single-flight (se já houver Promise de `init` em andamento, retorna a mesma Promise). Instala os listeners (`onBridgeStatus`, `onPeerConnected`, `onPeerDisconnected`, `onDeliveryFailed`) **antes** de buscar `Promise.all([bridgeIpc.status(), bridgeIpc.getConfig()])`. Se `dispose()` for chamado enquanto `init()` aguarda, os listeners recém-resolvidos são imediatamente desregistrados.
- **`inspect(blockId: CommandBlockId)`**: mantém um `Map<CommandBlockId, Promise<DeliveryReport>>` para coalescer cliques simultâneos no mesmo bloco. Adiciona `blockId` em `pendingBlockIds` antes da chamada IPC e remove no bloco `finally`.
- **Toasts (`CA-32`)**:
  - Quando `DeliveryReport.confirmed === true`: `Toast.show({ title: 'Conversa aberta no MaxCode', severity: 'success' })`.
  - Quando `err.code === 'bridge.delivery_unknown'`: `Toast.show({ title: 'Não foi possível confirmar a abertura no MaxCode', description: err.code, severity: 'warning' })`.
  - Para qualquer outra falha de abertura/inspeção: `Toast.show({ title: 'Não foi possível abrir o MaxCode', description: err.code ?? err.message, severity: 'danger' })`.

### 3.3. `src/composables/useActiveProjectSync.ts`

```ts
import { onScopeDispose, watch } from 'vue';
import { bridgeIpc, onProjectFocusRequested } from '@/ipc/bridge';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
import { useProjectsStore } from '@/stores/projects';
import type { ProjectFocusRequested, ProjectId, SyncContext } from '@/types/bindings';

export function useActiveProjectSync(): void;
```

Comportamento exato:
1. Mantém uma fila/mapa de contextos remotos em aplicação (`remoteContextByProject = new Map<ProjectId, { context: SyncContext; revision: number; instanceId: string }>()`).
2. Ao receber `onProjectFocusRequested(payload)` (ou ao reconciliar `bridgeStore.status?.selection` no startup/reload):
   - Ignora se `bridgeStore.syncEnabled` for `false`.
   - Ignora se `payload.instance_id === lastInstanceId` e `payload.revision <= lastAppliedRevision`.
   - Registra `remoteContextByProject.set(payload.project_id, { context: payload.context, revision: payload.revision, instanceId: payload.instance_id })`.
   - Seleciona o projeto na store da Fase 06 (`projectsStore.selectProject(payload.project_id)`).
   - Confirma imediatamente a aplicação para o Rust chamando `await bridgeIpc.notifyActiveProject(payload.project_id, payload.context)` (com `origin: 'maxcode'`, permitindo ao Rust resolver o ACK pendente do MaxCode sem reenviar `project.focus`).
3. Observa `() => projectsStore.selectedProjectId`:
   - Quando `selectedProjectId` muda para um `id` não nulo:
     - Se `remoteContextByProject.has(id)`, consome/remove a marcação remota e **não** gera notificação local manual (pois já foi confirmada com contexto remoto no passo 2).
     - Caso contrário, se `bridgeStore.syncEnabled` for `true`, gera um contexto local novo `{ origin: 'maxterminal', correlationId: crypto.randomUUID() }` e chama `bridgeIpc.notifyActiveProject(id, localContext)`.
4. Registra `onScopeDispose` para remover o listener `onProjectFocusRequested`.

### 3.4. `src/composables/useMaxCodeInspect.ts`

```ts
import { computed, toValue, type ComputedRef, type MaybeRefOrGetter } from 'vue';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
import type { CommandBlockDto, DeliveryReport } from '@/types/bindings';

export function useMaxCodeInspect(block: MaybeRefOrGetter<CommandBlockDto>): {
    visible: ComputedRef<boolean>;
    loading: ComputedRef<boolean>;
    inspect: () => Promise<DeliveryReport>;
} {
    const store = useMaxCodeBridgeStore();
    const visible = computed<boolean>(() => {
        const current = toValue(block);
        return current.exitCode !== null && current.exitCode !== undefined && current.exitCode !== 0;
    });
    const loading = computed<boolean>(() => {
        const current = toValue(block);
        return store.isInspecting(current.id);
    });
    const inspect = async (): Promise<DeliveryReport> => {
        const current = toValue(block);
        return await store.inspect(current.id);
    };
    return { visible, loading, inspect };
}
```

### 3.5. `src/components/bridge/InspectButton.vue`

```vue
<template>
    <MaxButton v-if="visible" :loading="loading" :disabled="loading" severity="danger" size="small" text icon="mdi:magnify-scan" label="Inspecionar" v-tooltip="'Abrir conversa em Plan Mode no MaxCode'" class="inspect-button" @click="onClick" />
</template>

<script setup lang="ts">
    import { useMaxCodeInspect } from '@/composables/useMaxCodeInspect';
    import type { CommandBlockDto, DeliveryReport } from '@/types/bindings';

    interface Props {
        block: CommandBlockDto;
    }

    const props = defineProps<Props>();

    const emit = defineEmits<{
        inspected: [report: DeliveryReport];
    }>();

    const { visible, loading, inspect } = useMaxCodeInspect(() => props.block);

    async function onClick(): Promise<void> {
        try {
            const report = await inspect();
            emit('inspected', report);
        } catch {
            // O toast em pt-BR já é apresentado pela useMaxCodeBridgeStore
        }
    }
</script>

<style lang="scss" scoped>
    .inspect-button {
        order: 10;
    }
</style>
```

### 3.6. `src/components/bridge/BridgeStatusIndicator.vue`

- Props: `interface Props { compact?: boolean }` e `defineEmits<{ openSettings: [] }>()`.
- Exibe ponto colorido (`.status-dot`) estilizado com os tokens semânticos do `@maxvue/max-components-ui`:
  - `success` → `var(--max-success-500)` (`"MaxCode conectado"`)
  - `info` → `var(--max-info-500)` (`"Aguardando MaxCode"`)
  - `danger` → `var(--max-danger-500)` (`"Ponte indisponível"`)
  - `neutral` → `var(--text-color-secondary, #8b949e)` (`"Ponte desativada"`)
- Tooltip via `v-tooltip` exibindo `"Porta ${port} · ${peers.length} ${peers.length === 1 ? 'conexão' : 'conexões'}"` quando `port` está presente, ou o rótulo de estado quando desativada/indisponível.

### 3.7. `src/components/bridge/BridgeSettingsSection.vue`

- Props: `interface Props { embedded?: boolean }` e `defineEmits<{ saved: [config: BridgeConfig] }>()`.
- Controles em `pt-BR`:
  - Switch `"Ativar ponte com MaxCode"` (`enabled`)
  - Input numérico `"Porta preferida"` (`preferred_port`, `1024..65535`)
  - Switch `"Sincronizar projeto ativo"` (`sync_active_project`)
  - Lista de peers conectados (`peers` com versão, pastas de workspace e capabilities)
  - Botão `"Reiniciar ponte"` (`store.restart()`)
  - Botão `"Gerar novo token"` envolvido em `<MaxPopoverConfirm>` com a mensagem `"As conexões atuais serão encerradas."`, chamando `store.regenerateToken()`.

---

## 4. Regras de Negócio e Casos de Borda no Frontend

1. **Visibilidade Estrita do Botão "Inspecionar" (`CA-28`)**:
   - Enquanto um comando está em execução (`exitCode === null` ou `undefined`) ou quando finaliza com sucesso (`exitCode === 0`), `visible` é `false` e o botão não é renderizado no DOM.
   - Somente para `exitCode !== 0` (ex.: `1`, `2`, `127`, `-1`) o botão é renderizado.
2. **Prevenção de Duplo Clique e Vazamento de Subscriptions (`CA-32`, `CA-35`)**:
   - Clicar repetidamente em "Inspecionar" desabilita o botão (`:disabled="loading"`) e reutiliza a mesma Promise na store.
   - Recarregar a webview recupera `BridgeStatus.selection` no `init()` aplicando o foco pendente de forma idempotente se a revisão for mais nova, sem abrir abas duplicadas.
3. **Desativação Imediata de Sync (`CA-27`)**:
   - Ao desmarcar `"Sincronizar projeto ativo"` em `BridgeSettingsSection.vue`, `store.updateConfig({ sync_active_project: false })` atualiza `bridge_set_config` a quente no backend e `store.syncEnabled` passa a `false`, interrompendo imediatamente tanto o envio de `notifyActiveProject` quanto a reação a eventos `onProjectFocusRequested`.
