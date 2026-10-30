# Implementação 05 — Frontend: Bindings, Stores Pinia, Composables, Componentes Vue e Testes (`spec.md`)

> Fase: **13 — Agentes CLI, Telemetria e MaxSkills**
> Referências: [`../instructions.md`](../instructions.md) (I-05, I-06, I-09, I-15; CA-01, CA-03, CA-09, CA-10, CA-11, CA-13, CA-14, CA-19, CA-24, CA-25, CA-34) e [`../spec.md`](../spec.md) (§1.2, §3, §6.1–§6.4, §11).

---

## 1. Contexto, Objetivo e Escopo

Esta quinta e última unidade da Fase 13 entrega toda a camada de Frontend (Vue 3.6 + TypeScript estrito `tsgo` + Pinia + UnoCSS + SCSS Scoped) integrada aos contratos Rust das Implementações 01 a 04:

1. **Bindings Gerados e Wrappers IPC (`src/types/bindings.ts` e `src/ipc/{agents,skills,memory}.ts`)**:
   - Regeneração automática de `src/types/bindings.ts` via `cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings` (`tauri-specta`), sem tipos duplicados à mão.
   - Wrappers tipados usando `call` / `unwrap` de `src/ipc/core.ts` e assinaturas tipadas de eventos (`listen`).
2. **Stores Pinia (`useAgentsStore` e `useSkillsStore`)**:
   - Hidratação livre de condição de corrida: registra listeners IPC **antes** de buscar snapshots iniciais, bufferiza eventos recebidos durante o carregamento e aplica somente `revision` superior do `runId` ativo (`spec.md` §6.2).
   - Tombstones para abas fechadas (`tab://closed`), limpeza de timers e reconciliação em reconexão ou troca de execução.
3. **Composables (`useAgentShortcuts`, `useAgentTelemetry`, `useAgentAttention`, `useCommandMemorySuggestions`)**:
   - `useAgentShortcuts`: registra `Ctrl+Alt+C/X/A/O` (e `+Shift` para abrir `AgentTabOpenDialog` com isolamento marcado) tanto na janela quanto na extensão de teclado do xterm (`registerTerminalExtension` da Fase 04), prevenindo propagação ao PTY quando o atalho é tratado, ignorando IME/`event.repeat`/modais e exibindo toast pt-BR `"Agente não encontrado no PATH"` quando não instalado (CA-10).
   - `useAgentTelemetry`: relógio compartilhado de 1 s para calcular idade individual por campo, marcando dados > `staleAfterS` (60 s) como `"desatualizado"`, exibindo `"—"` com tooltip `"Indisponível para este agente"` quando `null` (CA-19), e distinguindo nos rótulos/tooltips `Reported` ("Fonte estruturada"), `Derived` ("Derivado da sessão"), `Estimated` ("Estimado") e `Configured` ("Modelo configurado") (CA-34).
   - `useAgentAttention`: integra `xterm.onData` via `registerTerminalExtension` para disparar `ackAttention(tabId, runId, episode, 'userInput')` (reconhecendo visualização sem resolver a espera no backend — CA-24) e distingue visualmente `Suspected` ("Possivelmente aguardando intervenção") de `Awaiting` ("Aguardando aprovação" / "Aguardando resposta").
   - `useCommandMemorySuggestions`: consulta com debounce de 150 ms, número de sequência monotônico para descartar respostas antigas, limite de 20 itens e inserção explícita no draft de `useRichInput(paneId)` sem executar no PTY nem adulterar `.history`.
4. **Componentes Vue 3.6 (`src/components/agents/*` e `src/components/skills/*`) e Suíte Vitest (`tests/**`)**:
   - Seguindo estritamente as diretrizes do `AGENTS.md`: ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, recuo de 4 espaços (`baseIndent: 1`), aspas simples, ponto e vírgula obrigatório, sem vírgula final, tags de abertura em linha única e tokens semânticos `--max-*`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Criados
- `src/ipc/agents.ts`
- `src/ipc/skills.ts`
- `src/ipc/memory.ts`
- `src/composables/agents/adapters.ts`
- `src/stores/agents.ts`
- `src/stores/skills.ts`
- `src/composables/useAgentShortcuts.ts`
- `src/composables/useAgentTelemetry.ts`
- `src/composables/useAgentAttention.ts`
- `src/composables/useCommandMemorySuggestions.ts`
- `src/components/agents/AgentStatusLine.vue`
- `src/components/agents/AgentAttentionIndicator.vue`
- `src/components/agents/AgentLauncherMenu.vue`
- `src/components/agents/AgentTabOpenDialog.vue`
- `src/components/agents/AgentWorktreeCloseDialog.vue`
- `src/components/agents/AgentSettingsPanel.vue`
- `src/components/skills/SkillsSyncPanel.vue`
- `src/components/skills/SkillConflictList.vue`
- `src/components/skills/CommandMemoryList.vue`
- `tests/stores/agents.spec.ts`
- `tests/stores/skills.spec.ts`
- `tests/components/agents/AgentStatusLine.spec.ts`
- `tests/components/agents/AgentWorktreeCloseDialog.spec.ts`
- `tests/composables/useAgentShortcuts.spec.ts`

### 2.2. Arquivos Modificados
- `src/types/bindings.ts` — regenerado via `cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings`.
- Componentes de barra de abas/workspace da Fase 07 (`ProjectWorkspaceHeader.vue` / `TabBarItem.vue`) — montagem de `AgentLauncherMenu.vue`, `AgentStatusLine.vue`, `AgentAttentionIndicator.vue`, `AgentTabOpenDialog.vue` e `AgentWorktreeCloseDialog.vue`.

---

## 3. Especificação Técnica Detalhada

### 3.1. Wrappers IPC (`src/ipc/agents.ts`, `src/ipc/skills.ts`, `src/ipc/memory.ts`)

```ts
// src/ipc/agents.ts
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { call } from '@/ipc/core';
import {
    commands,
    type AckReason,
    type AgentCloseInspection,
    type AgentInfo,
    type AgentsSettings,
    type AgentTabLink,
    type AgentTabOpenRequest,
    type AgentTabOpened,
    type AgentTelemetry,
    type AgentTemplateOptions,
    type AttentionSnapshot,
    type AgentKind,
    type ProjectId,
    type TabId,
    type TemplateId,
    type WorktreeProposal,
    type WorktreeResolution
} from '@/types/bindings';

export const agentsIpc = {
    list: () => call('agent_list', () => commands.agentList()),
    detectRefresh: () => call('agent_detect_refresh', () => commands.agentDetectRefresh()),
    settingsGet: () => call('agent_settings_get', () => commands.agentSettingsGet()),
    settingsUpdate: (settings: AgentsSettings) =>
        call('agent_settings_update', () => commands.agentSettingsUpdate(settings)),
    descriptorsReload: () => call('agent_descriptors_reload', () => commands.agentDescriptorsReload()),
    templatesEnsure: (projectId: ProjectId) =>
        call('agent_templates_ensure', () => commands.agentTemplatesEnsure(projectId)),
    templateOptionsGet: (templateId: TemplateId) =>
        call('agent_template_options_get', () => commands.agentTemplateOptionsGet(templateId)),
    templateOptionsSet: (templateId: TemplateId, options: AgentTemplateOptions) =>
        call('agent_template_options_set', () => commands.agentTemplateOptionsSet(templateId, options)),
    worktreePropose: (projectId: ProjectId, agent: AgentKind, hint: string | null) =>
        call('agent_worktree_propose', () => commands.agentWorktreePropose(projectId, agent, hint)),
    tabOpen: (request: AgentTabOpenRequest) =>
        call('agent_tab_open', () => commands.agentTabOpen(request)),
    tabLinkGet: (tabId: TabId) =>
        call('agent_tab_link_get', () => commands.agentTabLinkGet(tabId)),
    tabCloseCheck: (tabId: TabId) =>
        call('agent_tab_close_check', () => commands.agentTabCloseCheck(tabId)),
    worktreeResolve: (linkId: number, action: WorktreeResolution, inspectionToken: string) =>
        call('agent_worktree_resolve', () => commands.agentWorktreeResolve(linkId, action, inspectionToken)),
    worktreePendingList: (projectId: ProjectId | null) =>
        call('agent_worktree_pending_list', () => commands.agentWorktreePendingList(projectId)),
    telemetryGet: (tabId: TabId) =>
        call('agent_telemetry_get', () => commands.agentTelemetryGet(tabId)),
    telemetryList: () =>
        call('agent_telemetry_list', () => commands.agentTelemetryList()),
    attentionGet: (tabId: TabId) =>
        call('agent_attention_get', () => commands.agentAttentionGet(tabId)),
    attentionAck: (tabId: TabId, runId: string, episode: number, reason: AckReason) =>
        call('agent_attention_ack', () => commands.agentAttentionAck(tabId, runId, episode, reason)),

    onInstallationsChanged: (cb: (agents: AgentInfo[]) => void): Promise<UnlistenFn> =>
        listen<AgentInfo[]>('agent://installations-changed', (ev) => cb(ev.payload)),
    onSettingsChanged: (cb: (settings: AgentsSettings) => void): Promise<UnlistenFn> =>
        listen<AgentsSettings>('agent://settings-changed', (ev) => cb(ev.payload)),
    onTelemetry: (cb: (telemetry: AgentTelemetry) => void): Promise<UnlistenFn> =>
        listen<AgentTelemetry>('agent://telemetry', (ev) => cb(ev.payload)),
    onAttention: (cb: (snapshot: AttentionSnapshot) => void): Promise<UnlistenFn> =>
        listen<AttentionSnapshot>('agent://attention', (ev) => cb(ev.payload)),
    onLinkChanged: (cb: (link: AgentTabLink) => void): Promise<UnlistenFn> =>
        listen<AgentTabLink>('agent://link-changed', (ev) => cb(ev.payload)),
    onWorktreeClosePending: (cb: (inspection: AgentCloseInspection) => void): Promise<UnlistenFn> =>
        listen<AgentCloseInspection>('agent://worktree-close-pending', (ev) => cb(ev.payload))
};
```

```ts
// src/ipc/skills.ts
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { call } from '@/ipc/core';
import {
    commands,
    type ConflictResolution,
    type ProjectId,
    type SkillCatalog,
    type SkillSyncTarget,
    type SyncPlan,
    type SyncResult
} from '@/types/bindings';

export interface SkillsChangedPayload {
    sourceDir: string;
    changedSkills: string[];
}

export const skillsIpc = {
    scan: (projectId: ProjectId | null) =>
        call('skills_scan', () => commands.skillsScan(projectId)),
    targetsList: (projectId: ProjectId | null) =>
        call('skills_targets_list', () => commands.skillsTargetsList(projectId)),
    targetUpsert: (target: SkillSyncTarget) =>
        call('skills_target_upsert', () => commands.skillsTargetUpsert(target)),
    targetRemove: (targetId: number, removeManaged: boolean) =>
        call('skills_target_remove', () => commands.skillsTargetRemove(targetId, removeManaged)),
    plan: (targetId: number) =>
        call('skills_plan', () => commands.skillsPlan(targetId)),
    apply: (planId: string, resolutions: Partial<Record<string, ConflictResolution>>) =>
        call('skills_apply', () => commands.skillsApply(planId, resolutions)),

    onChanged: (cb: (payload: SkillsChangedPayload) => void): Promise<UnlistenFn> =>
        listen<SkillsChangedPayload>('skills://changed', (ev) => cb(ev.payload)),
    onSyncCompleted: (cb: (result: SyncResult) => void): Promise<UnlistenFn> =>
        listen<SyncResult>('skills://sync-completed', (ev) => cb(ev.payload))
};
```

```ts
// src/ipc/memory.ts
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { call } from '@/ipc/core';
import {
    commands,
    type MemoryEntry,
    type MemoryQuery,
    type ProjectId
} from '@/types/bindings';

export const memoryIpc = {
    suggestions: (query: MemoryQuery) =>
        call('memory_suggestions', () => commands.memorySuggestions(query)),
    list: (projectId: ProjectId, includeHidden: boolean, limit: number) =>
        call('memory_list', () => commands.memoryList(projectId, includeHidden, limit)),
    pin: (projectId: ProjectId, command: string, pinned: boolean) =>
        call('memory_pin', () => commands.memoryPin(projectId, command, pinned)),
    hide: (projectId: ProjectId, command: string, hidden: boolean) =>
        call('memory_hide', () => commands.memoryHide(projectId, command, hidden)),
    forget: (projectId: ProjectId, command: string) =>
        call('memory_forget', () => commands.memoryForget(projectId, command)),
    rebuild: (projectId: ProjectId | null) =>
        call('memory_rebuild', () => commands.memoryRebuild(projectId)),

    onUpdated: (cb: (payload: { projectId: ProjectId }) => void): Promise<UnlistenFn> =>
        listen<{ projectId: ProjectId }>('memory://updated', (ev) => cb(ev.payload))
};
```

---

### 3.2. Stores Pinia (`src/stores/agents.ts` e `src/stores/skills.ts`)

#### Regras de `useAgentsStore` (`src/stores/agents.ts`):
- **Estado**:
  - `agents: Ref<AgentInfo[]>`
  - `settings: Ref<AgentsSettings | null>`
  - `capabilitiesByAgent: ComputedRef<Record<AgentKind, CapabilityReport | undefined>>`
  - `telemetryByTab: Ref<Record<TabId, AgentTelemetry>>`
  - `attentionByTab: Ref<Record<TabId, AttentionSnapshot>>`
  - `linksByTab: Ref<Record<TabId, AgentTabLink>>`
  - `pendingWorktrees: Ref<AgentCloseInspection[]>`
  - `activeCloseDialogInspection: Ref<AgentCloseInspection | null>`
  - `openDialogState: Ref<{ open: boolean; projectId: ProjectId | null; agent: AgentKind; forceIsolation: boolean }>`
  - `closedTabTombstones: Set<TabId>`
  - `loading: Ref<boolean>`
  - `error: Ref<MtError | null>`
- **Protocolo anti-race em `init()` (`spec.md` §6.2)**:
  1. Se já inicializado, retorna a promise em curso.
  2. Registra **primeiro** todos os listeners (`onInstallationsChanged`, `onSettingsChanged`, `onTelemetry`, `onAttention`, `onLinkChanged`, `onWorktreeClosePending` e `tab://closed`). Enquanto a carga inicial de snapshots (`agent_list`, `agent_settings_get`, `agent_telemetry_list`, `agent_worktree_pending_list`) está em voo, eventos `agent://telemetry` e `agent://attention` são acumulados num buffer temporário.
  3. Ao receber os snapshots do backend, aplica cada item e em seguida drena o buffer aplicando `applyTelemetry(incoming)` / `applyAttention(incoming)` somente se `!closedTabTombstones.has(tabId)` e:
     - `current == null` **ou**
     - `incoming.runId === current.runId && incoming.revision > current.revision`.
     - Se `incoming.runId !== current.runId`, dispara `telemetryGet(tabId)` / `attentionGet(tabId)` no backend para confirmar qual `runId` é o corrente antes de substituir (evitando que um evento atrasado de uma execução encerrada sobrescreva o novo run após restart — §6.2).
  4. Ao receber `tab://closed`, adiciona `tabId` em `closedTabTombstones` e remove entradas de `telemetryByTab` e `attentionByTab`.

---

### 3.3. Composables (`src/composables/useAgent*.ts` e `useCommandMemorySuggestions.ts`)

1. **`useAgentShortcuts()` (`src/composables/useAgentShortcuts.ts`)**:
   - Calcula o mapa de atalhos efetivos combinando `descriptor.shortcut` e `settings.shortcuts[kind]`.
   - Registra listener em `window` (`keydown`, fase de captura) e registra extensão no terminal (`registerTerminalExtension` da Fase 04 usando `xterm.attachCustomKeyEventHandler`).
   - Ignora eventos quando `event.isComposing`, `event.repeat`, ou quando o elemento ativo for um editor de atalhos (`[data-shortcut-recorder]`) ou modal com foco capturado.
   - Para cada combinação `<Modificadores>+<Tecla>`:
     - Se coincidir exatamente com o atalho do agente `K`:
       - Se o agente não estiver instalado (`!info.installation.installed`), exibe `Toast.show({ title: 'Agente não encontrado no PATH', severity: 'warn' })` e **não** abre aba (CA-10).
       - Se `settings.isolation.openIsolatedByDefault` for `true`, abre o `AgentTabOpenDialog` com isolamento marcado; caso contrário chama `agentsStore.openAgent({ projectId, agent: K, isolation: { mode: 'none' }, focus: true })`.
     - Se coincidir com `<Atalho de K>+Shift`:
       - Abre o `AgentTabOpenDialog` para o agente `K` já com isolamento em worktree selecionado (CA-10).
     - Quando o atalho é tratado dentro do xterm, retorna `false` no `attachCustomKeyEventHandler` e chama `event.preventDefault(); event.stopPropagation()` para garantir que nenhuma sequência de bytes seja enviada ao PTY e que apenas 1 única ação ocorra por `keydown`.

2. **`useAgentTelemetry(tabId: MaybeRefOrGetter<TabId>)` (`src/composables/useAgentTelemetry.ts`)**:
   - Usa um relógio global compartilhado (`useIntervalFn` de 1 000 ms ativo apenas enquanto houver componentes montados) para atualizar `nowMs`.
   - Expõe projeções formatadas em `pt-BR` para `model`, `context`, `quota5h`, `quotaWeekly` e `cost`:
     - Quando o campo é `null`: `display = '—'`, `isAvailable = false`, `tooltip = reason ?? 'Indisponível para este agente'` (CA-19).
     - Quando `nowMs - field.observedAtMs > staleAfterS * 1000` (padrão 60 s): `isStale = true`, adiciona sufixo/badge `"desatualizado"` e mostra no tooltip a idade exata (`"Observado há Xs"`) sem renovar `observedAtMs` por render ou por atualização de outro campo (CA-19, CA-34).
     - Distingue explicitamente a qualidade da medição (`qualityLabel`):
       - `'reported'` → `'Fonte estruturada'`
       - `'derived'` → `'Derivado da sessão'`
       - `'estimated'` → `'Estimado'` (exibe prefixo `~` no valor)
       - `'configured'` → `'Modelo configurado'` (exibe rótulo `"Modelo configurado"`, nunca `"Modelo ativo"` — §6.3, CA-34).

3. **`useAgentAttention(tabId: MaybeRefOrGetter<TabId>)` (`src/composables/useAgentAttention.ts`)**:
   - Registra uma única vez via `registerTerminalExtension` um listener em `xterm.onData` que, se a aba do painel possuir `attention.state.state === 'awaiting' || 'suspected'` e `!attention.acknowledged`, chama `agentsStore.ackAttention(tabId, runId, episode, 'userInput')` (CA-24).
   - Expõe `isAwaiting` (confirmado), `isSuspected` (indício heurístico), `badgeLabel`, `sourceLabel`, `focusAgentTab()` e `dismissNotice()` (`ackAttention` com `'dismissed'`, que apenas marca o aviso como visto na UI sem enviar `y` ou `Enter` ao PTY — §6.3).

4. **`useCommandMemorySuggestions(paneId: MaybeRefOrGetter<PaneId>)` (`src/composables/useCommandMemorySuggestions.ts`)**:
   - Observa o texto atual do draft de `useRichInput(paneId)` com debounce de 150 ms.
   - Mantém `requestSeq` monotônico: respostas de chamadas IPC com `seq < latestSeq` ou após troca de aba são descartadas.
   - `applySuggestion(entry: MemoryEntry)` escreve `entry.command` apenas no draft do input rico (`richInput.setDraft(entry.command)`), nunca executando no PTY nem modificando `.history` (§6.3).

---

### 3.4. Componentes Vue 3.6 (`src/components/agents/*` e `src/components/skills/*`)

Todos os SFCs devem obedecer:
1. Ordem de blocos: `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`.
2. Tags de abertura no `<template>` em **linha única** (incluindo todas as props, diretivas e eventos).
3. Recuo de 4 espaços (`baseIndent: 1`), aspas simples `'`, ponto e vírgula `;`, sem vírgula final.
4. Uso de tokens semânticos (`--max-primary-*`, `--max-success-*`, `--max-warning-*`, `--max-danger-*`, `--max-info-*`, `--background-*`).

| Componente | Props e Emits | Responsabilidades Principais |
|------------|---------------|------------------------------|
| `AgentStatusLine.vue` | `props: { tabId: TabId; compact?: boolean }` | Renderiza ícone/nome do modelo (ou `"Modelo configurado"` / `"—"`), barra estática de contexto `%` (sem animação contínua), cotas `5h` e `Semanal` com tempo de reset e indicador de `"desatualizado"` (> 60 s), mais tooltips detalhando fonte, qualidade e idade (CA-19, CA-34). |
| `AgentAttentionIndicator.vue` | `props: { tabId: TabId }` | Exibe alerta distinto para `Awaiting` (`--max-danger-500`, `"Aguardando aprovação"` / `"Aguardando resposta"`, `aria-live="polite"`) vs `Suspected` (`--max-warning-500`, `"Possivelmente aguardando intervenção"`), com botões `"Visualizar"` e `"Dispensar aviso"` (CA-22, CA-23, CA-24). |
| `AgentLauncherMenu.vue` | `props: { projectId: ProjectId }` | Menu de 1 clique na barra de abas listando os 4 agentes (`Claude Code`, `Codex`, `Antigravity (agy)`, `OpenCode`) com `MaxIcon`, atalho configurado, badge de instalado/indisponível e botão secundário para abrir com opções/isolamento (CA-09, CA-10). |
| `AgentTabOpenDialog.vue` | `props: { modelValue: boolean; projectId: ProjectId; initialAgent?: AgentKind; initialIsolate?: boolean }`, `emits: ['update:modelValue', 'opened']` | Modal para escolher agente, template dedicado, modelo local suportado e isolamento (`None`, `Create`, `Existing`), exibindo preview ao vivo de `agent_worktree_propose` (branch, pasta e alerta amarelo quando `rootIgnored === false` — CA-12, CA-13). |
| `AgentWorktreeCloseDialog.vue` | `props: { modelValue: boolean; inspection: AgentCloseInspection | null }`, `emits: ['update:modelValue', 'resolved']` | Modal pós-fechamento da aba exibindo arquivos modificados, commits à frente da base, motivos de bloqueio em `pt-BR` e botões para as resoluções em `inspection.allowed` (`"Manter worktree"` padrão, `"Remover worktree"` e `"Remover worktree e apagar branch"`), revalidando a inspeção se ocorrer `agents.inspection_stale` (CA-14, CA-15). |
| `AgentSettingsPanel.vue` | — | Painel de configurações de agentes: edição de atalhos com validação de colisão (`agents.shortcut_conflict`), toggles de injeção e telemetria por agente e tabela de `CapabilityReport` por versão instalada. |
| `SkillsSyncPanel.vue` | `props: { projectId: ProjectId | null }` | Painel de catálogo e sincronização do `MaxSkills`: botão de scan, configuração de alvos por agente/escopo (`project`/`global`) e modo (`symlink`/`copy`), execução de `skills_plan` (*dry-run* padrão), exibição de avisos e aplicação com `skills_apply` (CA-26..CA-30, CA-35). |
| `SkillConflictList.vue` | `props: { conflicts: Extract<SyncAction, { action: 'conflict' }>[]; modelValue: Record<string, ConflictResolution> }`, `emits: ['update:modelValue']` | Lista interativa de conflitos (`ForeignSymlink`, `Unmanaged`, `LocallyModified`) permitindo selecionar `Keep` ("Manter atual"), `Overwrite` ("Sobrescrever com backup") ou `Adopt` ("Adotar") por skill (CA-29). |
| `CommandMemoryList.vue` | `props: { projectId: ProjectId; paneId: PaneId }` | Lista de sugestões e gestão de memória contextual de comandos: filtro por prefixo, exibição de `useCount`, taxa de sucesso e último `cwd`, clique para inserir no draft do input rico (sem executar) e ações de fixar (`pin`), ocultar (`hide`), esquecer (`forget`) e reconstruir (`rebuild`) (CA-31..CA-33). |
