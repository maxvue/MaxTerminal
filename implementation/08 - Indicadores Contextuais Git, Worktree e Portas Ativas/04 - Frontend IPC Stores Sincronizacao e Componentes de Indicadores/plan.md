# Plano de Execução — Implementação 04: Frontend IPC, Stores, Sincronização e Componentes de Indicadores

> **Fase:** 08 — Indicadores Contextuais: Git, Worktree e Portas Ativas
> **Unidade:** `04 - Frontend IPC Stores Sincronizacao e Componentes de Indicadores`
> **Especificação:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1 — Wrappers IPC (`src/ipc/git.ts`, `src/ipc/ports.ts`)
1. Criar `src/ipc/git.ts` exportando `gitTrackPane`, `gitUntrackPane`, `gitSetVisibleTab`, `gitRefresh`, `gitStatus`, `gitWorktreeList`, `gitWorktreeCreate`, `gitWorktreeRemove`, `gitWorktreePrune` e `onGitStatusChanged`, consumindo os tipos e funções de `@/types/bindings` e `unwrap` de `@/ipc/core`.
2. Criar `src/ipc/ports.ts` exportando `portTrackPane`, `portUntrackPane`, `portSetTabStatus`, `portList`, `portRefresh`, `portOpenBrowser`, `portKill`, `onPortOpened`, `onPortUpdated`, `onPortClosed` e `onPortSnapshot`.

### Passo 2 — Stores Pinia Voláteis (`src/stores/git.ts`, `src/stores/ports.ts`)
1. Implementar `useGitStore` em `src/stores/git.ts`:
   - Controle de `envelopeByPane` (`epoch`, `revision`, `generation`), `intentTokenByPane` e buffer de eventos por painel durante `trackPane`.
   - Limpeza imediata do `contextByPane[paneId]` quando o `cwd` do painel muda.
   - Getters `contextForPane`, `contextForTab`, `isDirty` e actions `init`, `trackPane`, `untrackPane`, `setVisibleTab`, `refresh`, `worktreeList`, `worktreeCreate`, `worktreeRemove`, `dispose`.
2. Implementar `usePortsStore` em `src/stores/ports.ts`:
   - Instalação de listeners antes dos snapshots, ordenação determinística por `(port, protocol, paneId, pid)` e deduplicação por `PortId`.
   - Detecção de lacunas de `revision` e troca de `epoch` disparando `refresh(tabId)` automaticamente.
   - Controle de `killing[portId]` com limpeza garantida em `finally` e notificações `Toast.show` em `pt-BR` para cada `KillOutcome` e `remaining`.
   - Getters `portsForTab`, `portsForProject` e actions `init`, `trackPane`, `untrackPane`, `setTabStatus`, `openInBrowser`, `kill`, `refresh`, `dispose`.

### Passo 3 — Composable de Sincronização (`src/composables/useTabIndicatorsSync.ts`)
1. Implementar `useTabIndicatorsSync.ts` observando:
   - `useTabsStore().tabsById` (criação/remoção de painéis, troca de `ptyId`, mudança de `cwd` com debounce de 100 ms por painel).
   - `useTabStatusStore()` da Fase 05 (mudança de `TabStatus` e evento de fim de comando OSC 133 `D` por painel).
   - Aba ativa e `useWindowFocus()` (`@maxvue/max-use`) para acionar `setVisibleTab` e `refresh(activePaneId, 'windowFocus')`.
   - Limpeza de timers e listeners em `onScopeDispose`.

### Passo 4 — Componentes SFC de Indicadores (`src/components/indicators/`) e Integração na `TabBar`
1. Implementar `src/components/indicators/GitIndicator.vue` (seguindo `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, 4 espaços, tags de abertura em linha única e tokens `--max-*`).
2. Implementar `src/components/indicators/WorktreeBadge.vue` com cores para `maxCode` (`--max-primary-500`), `project` (`--max-info-500`) e `other`.
3. Implementar `src/components/indicators/PortBadge.vue` com botões HTML separados, `@click.stop`, `MaxPopoverConfirm` para encerramento e cancelamento automático se `port.id` mudar.
4. Implementar `src/components/indicators/TabIndicators.vue` exibindo `GitIndicator`, `WorktreeBadge`, até 3 `PortBadge` e o popover `+N` para portas excedentes.
5. Integrar `<TabIndicators :tab="tab" />` no slot `#indicators="{ tab }"` da `TabBar` e montar `useTabIndicatorsSync()` no componente de workspace da Fase 07.

### Passo 5 — Testes Unitários e de Componentes com Vitest
1. Criar `src/stores/__tests__/git.spec.ts` testando `trackPane`, descarte de eventos fora de ordem/geração antiga, dois `cd` rápidos com respostas invertidas, tombstone `context: null`, `untrackPane` e `worktreeCreate`/`worktreeList`.
2. Criar `src/stores/__tests__/ports.spec.ts` testando `opened`/`updated`/`closed`, ordenação, deduplicação, lacuna de `revision` acionando `refresh`, troca de `epoch`, `kill` (estado `killing` e toasts para cada `KillOutcome`), fechamento de aba durante invoke e `portsForProject`.
3. Criar `src/composables/__tests__/useTabIndicatorsSync.spec.ts` testando criação/remoção de painel, debounce de 100 ms no `cwd`, troca de `ptyId`, fim de comando em painel inativo e foco da janela.
4. Criar `src/components/indicators/__tests__/GitIndicator.spec.ts`, `WorktreeBadge.spec.ts`, `PortBadge.spec.ts` e `TabIndicators.spec.ts` validando renderização, textos `pt-BR`, overflow `+N`, clique para abrir navegador e confirmação no `MaxPopoverConfirm`.

---

## 2. Comandos de Verificação e Testes

Execute os comandos abaixo na raiz da worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)"

# 1. Executar verificação de tipos TypeScript (tsgo)
npm run typecheck
npm run typecheck:vue

# 2. Executar ESLint e Stylelint (verificando regras Vue SFC e SCSS scoped)
npm run lint
npm run lint:style

# 3. Executar suíte de testes Vitest das stores, composable e componentes da Fase 08
npx --no-install vitest run src/stores/__tests__/git.spec.ts src/stores/__tests__/ports.spec.ts src/composables/__tests__/useTabIndicatorsSync.spec.ts src/components/indicators/__tests__

# 4. Verificar que nenhum estilo usa cores hardcoded fora dos tokens semânticos --max-* / --background-*
rg -n "#[0-9a-fA-F]{3,8}|rgb\(|hsl\(" src/components/indicators/
```

---

## 3. Critérios de Aceite da Implementação 04 (Mapeados para a Fase 08)

- [ ] **CA-08-18 (`typecheck` & `lint`):** `npm run typecheck` (`tsgo`), `npm run typecheck:vue`, `npm run lint` e `npm run lint:style` passam sem erros e sem warnings; todos os `.vue` respeitam a ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, recuo de 4 espaços, tags de abertura em linha única e tokens `--max-*`.
- [ ] **CA-08-19 (`useGitStore`):** `trackPane` popula `contextByPane`; dois `cd` consecutivos com respostas fora de ordem nunca exibem o repositório antigo; `worktreeCreate()` e `worktreeList()` estão disponíveis e cobertos por Vitest (contrato para Fases 13 e 16).
- [ ] **CA-08-20 (`usePortsStore`):** `opened`/`updated`/`closed` mantêm lista ordenada e sem duplicatas; lacunas de `revision` ou mudança de `epoch` disparam recarga; `portsForProject(projectId)` agrega portas de todas as abas do projeto.
- [ ] **CA-08-21 (`useTabIndicatorsSync`):** Sincroniza criação/remoção de painéis, aplica debounce de 100 ms em mudanças de `cwd`, ignora `ptyId: null` para portas, atualiza repo em fim de comando (OSC 133 `D`) mesmo em painel inativo e não duplica listeners ao remontar.
- [ ] **CA-08-22 (Componentes de UI):** `GitIndicator` renderiza `named`/`detached`/`unborn`, ponto `dirty`/`conflicted`, `↑A ↓B`, `gone` e tooltips de modo degradado em `pt-BR`; `WorktreeBadge` diferencia `maxCode`, `project` e `other`; `PortBadge` abre navegador em TCP compatível, exibe `UDP` sem ação de navegador para sockets UDP, exige confirmação em `MaxPopoverConfirm` antes de encerrar e cancela confirmação se `PortId` mudar; `TabIndicators` agrupa excedentes acima de 3 portas no chip `+N` interativo.
