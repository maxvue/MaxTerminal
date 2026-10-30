# Implementação 05 — Frontend Vue (Camada IPC, Store Pinia, Composables e Componentes de UI e Inspeção) (`plan.md`)

> Especificação técnica desta unidade: [`./spec.md`](./spec.md).
> Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md).

---

## 1. Checklist Sequencial de Execução

- [ ] **Passo 1: Criar Camada IPC (`src/ipc/bridge.ts`) e Store (`src/stores/maxCodeBridge.ts`)**
  1. Criar `src/ipc/bridge.ts` com `bridgeIpc` e os 6 helpers de eventos (`onBridgeStatus`, `onPeerConnected`, `onPeerDisconnected`, `onProjectFocusRequested`, `onTabOpened`, `onDeliveryFailed`).
  2. Criar `src/stores/maxCodeBridge.ts` implementando `useMaxCodeBridgeStore` com controle de geração em `init()`/`dispose()`, comparação monotônica de `(instance_id, revision)`, coalescência por `blockId` em `inspect(blockId)`, rollback de configuração em caso de erro no `updateConfig` e disparo de toasts via `Toast.show` em `pt-BR`.

- [ ] **Passo 2: Implementar Composables (`useActiveProjectSync.ts` e `useMaxCodeInspect.ts`)**
  1. Criar `src/composables/useActiveProjectSync.ts` escutando `onProjectFocusRequested`, reconciliando `status.selection`, confirmando seleções remotas via `bridgeIpc.notifyActiveProject(projectId, remoteContext)` sem reenviar `project.focus`, e enviando seleções manuais com `{ origin: 'maxterminal', correlationId: crypto.randomUUID() }`.
  2. Criar `src/composables/useMaxCodeInspect.ts` expondo `visible`, `loading` e `inspect`.

- [ ] **Passo 3: Implementar Componentes Vue (`InspectButton.vue`, `BridgeStatusIndicator.vue`, `BridgeSettingsSection.vue`) e Integrar no Layout**
  1. Criar `src/components/bridge/InspectButton.vue` seguindo a ordem `<template>`, `<script setup lang="ts">`, `<style lang="scss" scoped>`, tags de abertura em linha única e recuo de 4 espaços.
  2. Criar `src/components/bridge/BridgeStatusIndicator.vue` com cores `--max-success-500`, `--max-info-500`, `--max-danger-500`, cinza neutro e tooltip detalhando porta e número de conexões.
  3. Criar `src/components/bridge/BridgeSettingsSection.vue` com controles para `enabled`, `preferred_port`, `sync_active_project`, lista de conexões ativas, botão `"Reiniciar ponte"` e botão `"Gerar novo token"` com `<MaxPopoverConfirm>`.
  4. Integrar `useMaxCodeBridgeStore().init()` e `useActiveProjectSync()` em `src/components/shell/AppShell.vue`, `<BridgeStatusIndicator />` em `src/components/shell/AppTitleBar.vue` e `<InspectButton :block="block" />` no slot `#actions` de `src/components/terminal/CommandBlock.vue`. DTOs de estado/eventos/entrega e fixtures usam camelCase gerado (ex.: `instanceId`, `projectId`, `wakeUsed`); não converter o wire format manualmente no frontend.

- [ ] **Passo 4: Criar Testes Unitários e de Componentes (Vitest)**
  1. Criar `tests/stores/maxCodeBridge.spec.ts` testando:
     - `init()` idempotente (single-flight) e descarte de revisões obsoletas.
     - Getters `indicator` nos 4 estados (`connected`, `listening`, `failed`, `disabled`).
     - `inspect(blockId)` alternando `pendingBlockIds`, coalescendo duplo clique e disparando toasts de sucesso (`"Conversa aberta no MaxCode"`) e erro (`"Não foi possível abrir o MaxCode"` com `code`).
     - `openPlan` repassando `source` (`'generic'` ou `'inspect'`).
  2. Criar `tests/composables/useActiveProjectSync.spec.ts` testando:
     - Recebimento de `bridge://project-focus-requested` selecionando o projeto e enviando apenas a confirmação com `origin: 'maxcode'` (sem disparar nova seleção manual com `origin: 'maxterminal'`).
     - Seleção manual do usuário enviando uma única chamada com `origin: 'maxterminal'`.
     - Desativação de `sync_active_project` interrompendo envio e recebimento imediatamente (**CA-27**).
  3. Criar `tests/components/bridge/InspectButton.spec.ts` testando:
     - Visibilidade para `exitCode: null`, `exitCode: undefined`, `exitCode: 0` (oculto nos 3 casos) e `exitCode: 2`, `exitCode: -1` (visível) (**CA-28**).
     - Estado `loading`/`disabled` durante o envio e emissão do evento `inspected` (**CA-32**).
  4. Criar `tests/components/bridge/BridgeStatusIndicator.spec.ts` testando:
     - Cores e rótulos para cada estado (`--max-success-500` `"MaxCode conectado"`, `--max-info-500` `"Aguardando MaxCode"`, `--max-danger-500` `"Ponte indisponível"`) e conteúdo do tooltip com porta e número de peers (**CA-33**).

---

## 2. Comandos Exatos de Verificação

```bash
# 1. Executar os testes unitários e de componentes da Fase 11 no Vitest
npx --no-install vitest run tests/stores/maxCodeBridge.spec.ts tests/composables/useActiveProjectSync.spec.ts tests/components/bridge/InspectButton.spec.ts tests/components/bridge/BridgeStatusIndicator.spec.ts

# 2. Executar typecheck estrito (tsgo / vue-tsc) e linters (ESLint + Stylelint) (CA-02)
npm run typecheck
npm run lint

# 3. Executar suíte completa Rust + Frontend da Fase 11 (CA-01, CA-02, CA-03)
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --workspace
npm test
```

---

## 3. Critérios de Aceite Mapeados (`CA-*`)

| Critério | Descrição | Como é verificado nesta Implementação |
|----------|-----------|---------------------------------------|
| **CA-02** | Scripts de lint, typecheck (`tsgo`/`vue-tsc`) e testes Vitest passam. | Validado por `npm run typecheck`, `npm run lint` e `npx --no-install vitest run`. |
| **CA-27** | Desligar "Sincronizar projeto ativo" em `BridgeSettingsSection` interrompe envio e recebimento imediatamente. | Validado em `BridgeSettingsSection.vue` e `tests/composables/useActiveProjectSync.spec.ts`. |
| **CA-28** | Botão "Inspecionar" aparece somente em blocos com `exit_code != 0` e oculta em `null`/`0`. | Validado em `tests/components/bridge/InspectButton.spec.ts`. |
| **CA-32** | Botão mostra loading/disabled durante envio e exibe toast `pt-BR` `"Não foi possível abrir o MaxCode"` com `code` em falha. | Validado em `tests/stores/maxCodeBridge.spec.ts` e `tests/components/bridge/InspectButton.spec.ts`. |
| **CA-33** | `BridgeStatusIndicator` na `AppTitleBar` exibe verde `--max-success-500`, teal `--max-info-500`, vermelho `--max-danger-500` e tooltip com porta e peers. | Validado em `tests/components/bridge/BridgeStatusIndicator.spec.ts`. |
| **CA-34** | Matriz de capabilities registrada na store/indicador; APIs não suportadas pelo peer exibem erro explícito `bridge.unsupported_capability`. | Validado em `useMaxCodeBridgeStore` e `BridgeSettingsSection.vue`. |
| **CA-35** | Reconexão/reload da webview recupera snapshot revisionado sem duplicar abas ou vazar listeners. | Validado em `tests/stores/maxCodeBridge.spec.ts` e `tests/composables/useActiveProjectSync.spec.ts`. |
