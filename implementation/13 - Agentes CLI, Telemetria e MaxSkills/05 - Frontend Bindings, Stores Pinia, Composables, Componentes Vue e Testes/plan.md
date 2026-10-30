# Implementação 05 — Frontend: Bindings, Stores Pinia, Composables, Componentes Vue e Testes (`plan.md`)

> Especificação técnica: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Execução

- [ ] **Passo 1 — Regenerar Bindings TypeScript e Criar Wrappers IPC (`src/types/bindings.ts` e `src/ipc/`)**
  1. Executar `npm run bindings` para gerar `src/types/bindings.ts` a partir das structs/comandos Rust das Implementações 01–04.
  2. Criar `src/ipc/agents.ts`, `src/ipc/skills.ts`, `src/ipc/memory.ts` e `src/composables/agents/adapters.ts`.

- [ ] **Passo 2 — Implementar `useAgentsStore` e `useSkillsStore` (`src/stores/agents.ts` e `src/stores/skills.ts`)**
  1. Implementar `src/stores/agents.ts` com protocolo de hidratação sem condição de corrida (registro de listeners antes dos snapshots + bufferização), controle por `(runId, revision)`, tombstones em `tab://closed` e ações `init`, `refreshInstallations`, `updateSettings`, `reloadDescriptors`, `ensureTemplates`, `proposeWorktree`, `openAgent`, `ackAttention`, `inspectClose`, `resolveWorktree` e `dispose`.
  2. Implementar `src/stores/skills.ts` gerenciando catálogo de skills, alvos, planos *dry-run*, resultados de sync e memória contextual de comandos (`init`, `scan`, `upsertTarget`, `removeTarget`, `planSync`, `applySync`, `loadMemory`, `suggest`, `pin`, `hide`, `forget`, `rebuild`, `dispose`).

- [ ] **Passo 3 — Implementar os 4 Composables (`src/composables/`)**
  1. Implementar `src/composables/useAgentShortcuts.ts` integrando atalhos `Ctrl+Alt+C/X/A/O` (e `+Shift`) na janela e na extensão do xterm (`registerTerminalExtension`), bloqueando envio de bytes ao PTY quando o atalho é tratado e exibindo toast `"Agente não encontrado no PATH"` quando ausente.
  2. Implementar `src/composables/useAgentTelemetry.ts` com ticker compartilhado de 1 s, formatação `"—"` + tooltip `"Indisponível para este agente"` para campos `null`, marcação `"desatualizado"` após 60 s e rótulos distintos para `Reported`, `Derived`, `Estimated` e `Configured`.
  3. Implementar `src/composables/useAgentAttention.ts` integrando `xterm.onData` para chamar `ackAttention(..., 'userInput')` e diferenciando `Suspected` de `Awaiting`.
  4. Implementar `src/composables/useCommandMemorySuggestions.ts` com debounce de 150 ms, descarte de respostas fora de ordem e inserção explícita no draft de `useRichInput(paneId)` sem executar no PTY.

- [ ] **Passo 4 — Implementar os Componentes Vue de Agentes (`src/components/agents/`)**
  1. Criar `AgentStatusLine.vue` e `AgentAttentionIndicator.vue` (respeitando ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, tags de abertura em linha única, 4 espaços, aspas simples, `;` e sem vírgula final).
  2. Criar `AgentLauncherMenu.vue`, `AgentTabOpenDialog.vue` (com aviso pt-BR quando `rootIgnored === false`), `AgentWorktreeCloseDialog.vue` (com blockers pt-BR e ações permitidas por `inspection.allowed`) e `AgentSettingsPanel.vue`.
  3. Integrar `AgentLauncherMenu.vue`, `AgentStatusLine.vue`, `AgentAttentionIndicator.vue`, `AgentTabOpenDialog.vue` e `AgentWorktreeCloseDialog.vue` na barra de abas/workspace da Fase 07.

- [ ] **Passo 5 — Implementar os Componentes Vue de MaxSkills e Memória (`src/components/skills/`)**
  1. Criar `SkillConflictList.vue` e `SkillsSyncPanel.vue` (com fluxo *dry-run* obrigatório antes de aplicar e seleção de resolução por conflito).
  2. Criar `CommandMemoryList.vue` permitindo filtrar, inserir no draft do input rico, fixar, ocultar, esquecer e reconstruir a memória contextual do projeto.

- [ ] **Passo 6 — Escrever as Suítes de Testes Vitest (`src/tests/`) e Validar Qualidade Completa**
  1. Criar `src/tests/stores/agents.spec.ts` (testando corrida entre hidratação e eventos, rejeição de revisão antiga/run encerrado e limpeza em `tab://closed`).
  2. Criar `src/tests/stores/skills.spec.ts` (testando fluxo `scan` → `planSync` → `applySync` com resoluções e operações de memória `suggest`/`pin`/`hide`/`forget`).
  3. Criar `src/tests/components/agents/AgentStatusLine.spec.ts` (testando renderização de `"—"` com tooltip `"Indisponível para este agente"`, marcação `"desatualizado"` após > 60 s e rótulos `"Modelo configurado"` vs `"Estimado"`).
  4. Criar `src/tests/components/agents/AgentWorktreeCloseDialog.spec.ts` (testando botões habilitados conforme `allowed`, exibição de blockers quando `dirty = true` e bloqueio de exclusão de branch não mesclada).
  5. Criar `src/tests/composables/useAgentShortcuts.spec.ts` (testando `Ctrl+Alt+C/X/A/O`, variante `+Shift`, intercepção no xterm sem vazar teclas ao PTY, toast para agente não instalado e ignorar IME/`event.repeat`).

---

## 2. Comandos de Verificação e Testes

Executar na raiz da worktree (`/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0`):

```bash
# 1. Regenerar bindings e garantir que src/types/bindings.ts está sincronizado (CA-03)
npm run bindings
git diff --exit-code src/types/bindings.ts

# 2. Executar ESLint, Stylelint e checagem estrita de tipos (tsgo + vue-tsc) (CA-03)
npm run lint
npm run typecheck
npm run typecheck:vue

# 3. Executar as suítes Vitest da Fase 13 e a suíte completa de frontend (CA-03)
npx --no-install vitest run \
    src/tests/stores/agents.spec.ts \
    src/tests/stores/skills.spec.ts \
    src/tests/components/agents/AgentStatusLine.spec.ts \
    src/tests/components/agents/AgentWorktreeCloseDialog.spec.ts \
    src/tests/composables/useAgentShortcuts.spec.ts
npm run test

# 4. Validação final integrada Rust + Frontend da Fase 13 (CA-01, CA-02, CA-03, CA-04)
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml -p mt-agents
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

- **CA-01**: `cargo clippy --workspace --all-targets -- -D warnings` e `cargo fmt --all --check` passam em todo o projeto.
- **CA-03**: `npm run lint`, `npm run typecheck`, `npm run typecheck:vue` e `npm run test` passam; `git diff --exit-code src/types/bindings.ts` após `npm run bindings` retorna `0`.
- **CA-09**: Clicar em um agente no `AgentLauncherMenu.vue` abre uma aba cujo processo em foreground é o binário do agente no `cwd` do projeto/template.
- **CA-10**: `Ctrl+Alt+C/X/A/O` abrem a aba do agente correspondente mesmo com foco dentro do xterm; com `Shift` abrem o `AgentTabOpenDialog.vue` com isolamento marcado; agente não instalado exibe toast pt-BR `"Agente não encontrado no PATH"` e não cria aba.
- **CA-11**: Edição de atalho no `AgentSettingsPanel.vue` igual a outro atalho existente é rejeitada com `agents.shortcut_conflict`.
- **CA-13**: Quando `rootIgnored === false`, o `AgentTabOpenDialog.vue` exibe o aviso em pt-BR antes de criar a worktree.
- **CA-14 (UI)**: `AgentWorktreeCloseDialog.vue` apresenta apenas as ações permitidas por `inspection.allowed`, destaca os motivos de bloqueio em pt-BR e usa `"Manter"` como opção segura padrão.
- **CA-19**: `AgentStatusLine.vue` renderiza `"—"` com tooltip `"Indisponível para este agente"` para campos `null` e marca dados com mais de 60 s como `"desatualizado"`.
- **CA-24 (UI)**: Digitar no xterm ou clicar em `"Dispensar aviso"` no `AgentAttentionIndicator.vue` envia `agent_attention_ack` sem enviar caracteres ao PTY e sem limpar falsamente a espera antes da resolução nativa.
- **CA-25**: Clique na ação da notificação nativa (`os://focus-tab`) seleciona o projeto e ativa a aba correspondente.
- **CA-34 (UI)**: Tooltips e rótulos de `AgentStatusLine.vue` distinguem claramente fonte estruturada (`Reported`), derivada (`Derived`), estimada (`Estimated`) e configuração (`Configured`).
