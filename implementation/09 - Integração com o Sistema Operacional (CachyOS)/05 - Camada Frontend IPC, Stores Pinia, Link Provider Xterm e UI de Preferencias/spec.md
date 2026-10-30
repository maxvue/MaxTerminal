# Implementação 05 — Camada Frontend IPC, Stores Pinia, Link Provider Xterm e UI de Preferências (`spec.md`)

> Escopo desta sub-implementação: implementar na camada frontend (Vue 3.6 + TypeScript estrito + UnoCSS + SCSS Scoped) o cliente IPC (`src/ipc/os.ts`), as stores de estado de janela e configurações de SO (`src/stores/window.ts` / `src/stores/osWindow.ts` e `src/stores/osSettings.ts`), o parser linear de candidatos a link, cache LRU e link provider do xterm (`parseLinkCandidates.ts`, `linkCache.ts`, `useEditorLinks.ts` / `useTerminalEditorLinks.ts`), os composables de foco e atalho global (`useOsFocusTab.ts`, `useShortcutRecorder.ts`, `useGlobalQuakeShortcut.ts`), os componentes visuais de Preferências > Sistema e modais/badges (`PreferencesDialog.vue`, `PreferencesSystemSection.vue` / `OsPreferencesSection.vue`, `WaylandShortcutHelp.vue` / `WaylandShortcutGuideModal.vue`, `CloseConfirmModal.vue`, `TrayStatusPill.vue`, `ShortcutStatusBadge.vue`) e a suíte completa de testes Vitest.

---

## 1. Padrões Obrigatórios de Frontend (`AGENTS.md` §3)

1. **Ordem de Blocos em `.vue`**: `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`.
2. **Tags de Abertura em Linha Única**: toda tag no `<template>` mantém nome, atributos, props, diretivas, eventos e fechamento `>` ou `/>` na mesma linha.
3. **Formatação**: recuo de 4 espaços (`baseIndent: 1` em `<script setup>`), aspas simples (`'`), ponto e vírgula obrigatório (`;`), sem vírgula final (`comma-dangle: 'never'`).
4. **Estilização e Idioma**: tokens semânticos do `@maxvue/max-components-ui` (`--max-primary-*`, `--max-success-*`, `--max-warning-*`, `--max-danger-*`, `--max-info-*`, `--background-*`); textos visíveis e comentários em `pt-BR`, identificadores em `en-US`.

---

## 2. Camada IPC (`src/ipc/os.ts`)

```ts
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
    commands,
    type AttentionReason,
    type AutostartStatus,
    type FocusTabPayload,
    type LinkCandidate,
    type LinkContext,
    type MtError,
    type NotifyOutcome,
    type NotifyRequest,
    type OpenPreferencesPayload,
    type OsEnvironment,
    type OsEvent,
    type OsRuntimeSnapshot,
    type OsSettings,
    type OsSettingsPatch,
    type QuitOutcome,
    type ResolvedLink,
    type ShortcutStatus,
    type ShutdownFailedPayload,
    type ShutdownStartedPayload,
    type StatusCounters,
    type TabId,
    type ToggleDecisionDto,
    type ToggleSource,
    type WindowVisibilityPayload
} from '@/types/bindings';

async function unwrap<T>(promise: Promise<{ status: 'ok'; data: T } | { status: 'error'; error: MtError }>): Promise<T> {
    const res = await promise;
    if (res.status === 'error') {
        throw res.error;
    }
    return res.data;
}

export const osIpc = {
    getSettings: (): Promise<OsSettings> => unwrap(commands.osSettingsGet()),
    updateSettings: (patch: OsSettingsPatch): Promise<OsSettings> => unwrap(commands.osSettingsUpdate(patch)),
    environment: (refresh = false): Promise<OsEnvironment> => unwrap(commands.osEnvironmentInfo(refresh)),
    shortcutStatus: (): Promise<ShortcutStatus> => unwrap(commands.osShortcutStatus()),
    configurePortal: (): Promise<ShortcutStatus> => unwrap(commands.osShortcutConfigurePortal()),
    toggleWindow: (source: ToggleSource = 'ipc'): Promise<ToggleDecisionDto> => unwrap(commands.osWindowToggle(source)),
    showWindow: (): Promise<void> => unwrap(commands.osWindowShow()),
    hideWindow: (): Promise<void> => unwrap(commands.osWindowHide()),
    requestQuit: (): Promise<QuitOutcome> => unwrap(commands.osQuitRequest()),
    resolveLinks: (context: LinkContext, candidates: LinkCandidate[]): Promise<ResolvedLink[]> =>
        unwrap(commands.osLinkResolve(context, candidates)),
    openLink: (context: LinkContext, candidate: LinkCandidate): Promise<void> =>
        unwrap(commands.osLinkOpen(context, candidate)),
    notify: (request: NotifyRequest): Promise<NotifyOutcome> => unwrap(commands.osNotify(request)),
    setAttention: (tabId: TabId, reason: AttentionReason | null): Promise<StatusCounters> =>
        unwrap(commands.osAttentionSet(tabId, reason)),
    autostartStatus: (): Promise<AutostartStatus> => unwrap(commands.osAutostartStatus()),
    runtimeSnapshot: (): Promise<OsRuntimeSnapshot> => unwrap(commands.osRuntimeSnapshot()),
    uiReady: (): Promise<OsRuntimeSnapshot> => unwrap(commands.osUiReady())
};

export function onFocusTab(cb: (e: OsEvent<FocusTabPayload>) => void): Promise<UnlistenFn> {
    return listen<OsEvent<FocusTabPayload>>('os://focus-tab', (ev) => cb(ev.payload));
}
export function onWindowVisibility(cb: (e: OsEvent<WindowVisibilityPayload>) => void): Promise<UnlistenFn> {
    return listen<OsEvent<WindowVisibilityPayload>>('os://window-visibility', (ev) => cb(ev.payload));
}
export function onSettingsChanged(cb: (e: OsEvent<OsSettings>) => void): Promise<UnlistenFn> {
    return listen<OsEvent<OsSettings>>('os://settings-changed', (ev) => cb(ev.payload));
}
export function onShortcutStatus(cb: (e: OsEvent<ShortcutStatus>) => void): Promise<UnlistenFn> {
    return listen<OsEvent<ShortcutStatus>>('os://shortcut-status', (ev) => cb(ev.payload));
}
export function onCounters(cb: (e: OsEvent<StatusCounters>) => void): Promise<UnlistenFn> {
    return listen<OsEvent<StatusCounters>>('os://counters', (ev) => cb(ev.payload));
}
export function onOpenPreferences(cb: (e: OsEvent<OpenPreferencesPayload>) => void): Promise<UnlistenFn> {
    return listen<OsEvent<OpenPreferencesPayload>>('os://open-preferences', (ev) => cb(ev.payload));
}
export function onShutdownStarted(cb: (e: OsEvent<ShutdownStartedPayload>) => void): Promise<UnlistenFn> {
    return listen<OsEvent<ShutdownStartedPayload>>('os://shutdown-started', (ev) => cb(ev.payload));
}
export function onShutdownFailed(cb: (e: OsEvent<ShutdownFailedPayload>) => void): Promise<UnlistenFn> {
    return listen<OsEvent<ShutdownFailedPayload>>('os://shutdown-failed', (ev) => cb(ev.payload));
}
export function onEnvironmentChanged(cb: (e: OsEvent<OsEnvironment>) => void): Promise<UnlistenFn> {
    return listen<OsEvent<OsEnvironment>>('os://environment-changed', (ev) => cb(ev.payload));
}
export function onLinkContextChanged(cb: (e: OsEvent<LinkContext>) => void): Promise<UnlistenFn> {
    return listen<OsEvent<LinkContext>>('os://link-context-changed', (ev) => cb(ev.payload));
}
```

---

## 3. Stores Pinia (`window.ts` / `osWindow.ts` e `osSettings.ts`)

### 3.1. `src/stores/window.ts` e Re-export `src/stores/osWindow.ts`

- **Protocolo de Inicialização Livre de Corridas (`init()`)**:
  1. Registra todos os listeners `on*` primeiro, enfileirando eventos recebidos em um buffer temporário enquanto `osIpc.uiReady()` (no boot) ou `osIpc.runtimeSnapshot()` está em voo.
  2. Recebe `snapshot: OsRuntimeSnapshot`, aplica `revision = snapshot.revision`, hidrata `visible`, `focused`, `minimized`, `environment`, `shortcutStatus`, `counters`, `shutdownPhase`, `linkContexts` e `pendingFocus`.
  3. Reproduz apenas os eventos de estado do buffer cuja `event.revision > snapshot.revision` (eventos de intenção como `os://focus-tab` e `os://open-preferences` são sempre processados).
  4. `dispose()` cancela todos os `UnlistenFn`, inclusive se `dispose()` for chamado enquanto a Promise de registro ainda estiver resolvendo (usando flag `disposed`).
- **`src/stores/osWindow.ts`**: re-exporta `useWindowStore` também como `useOsWindowStore` para compatibilidade entre módulos consumidores.

### 3.2. `src/stores/osSettings.ts` (`useOsSettingsStore`)

- Estado: `settings: OsSettings | null`, `saving: boolean`, `error: MtError | null`, `autostart: AutostartStatus | null`.
- `load()`: busca `osIpc.getSettings()` e `osIpc.autostartStatus()` em paralelo e assina `onSettingsChanged`.
- `update(patch: OsSettingsPatch): Promise<boolean>`:
  - Serializa atualizações em fila; aplica atualização otimista local guardando o snapshot anterior da última revisão confirmada.
  - Em caso de falha (`MtError`), restaura o snapshot confirmado (ou recarrega via `osIpc.getSettings()`), grava `this.error = err`, exibe `Toast.show({ title: err.message, severity: 'error' })` e retorna `false` (CA-19, CA-40).
- `setAccelerator(acc: string): Promise<boolean>`: delega para `update({ quake: { accelerator: acc } })`.

---

## 4. Parser de Links, Cache LRU e Xterm Link Provider (`composables/editorLinks/`)

### 4.1. `src/composables/editorLinks/parseLinkCandidates.ts`

```ts
export interface ParsedLink {
    key: string;
    path: string;
    line: number | null;
    column: number | null;
    /** Índice 0-based em code units UTF-16 no texto concatenado da linha lógica. */
    startIndex: number;
    /** Índice 0-based exclusivo em code units UTF-16 no texto concatenado da linha lógica. */
    endIndex: number;
}

export const MAX_LINE_SCAN_CHARS = 16_384;
export const MAX_CANDIDATES_PER_LINE = 200;
export const MAX_LINE_COL_VALUE = 10_000_000;

export function parseLinkCandidates(lineText: string): ParsedLink[];
```

- **Regras Estritas do Scanner Linear (`parseLinkCandidates`)**:
  1. Trunca a varredura em `16_384` caracteres (`MAX_LINE_SCAN_CHARS`) e retorna no máximo `200` candidatos (`MAX_CANDIDATES_PER_LINE`).
  2. Suporta tokens entre aspas simples (`'...'`) ou duplas (`"..."`) contendo espaços, além de tokens delimitados por espaços/parênteses/colchetes.
  3. **Exclusão Total de URLs**: qualquer token que contenha `://` (ex.: `http://127.0.0.1:8000/app/User.php:10`, `https://...`, `vscode://...`) ou comece com `mailto:` é descartado por inteiro (não extrai o sufixo após `://`).
  4. **Exclusão de Falsos Positivos**:
     - Horários (`10:30`, `23:59:59`), versões puramente numéricas (`1.2.3:4`, `v1.0.0`) e endereços IP/host:porta (`127.0.0.1:5173`, `localhost:8080`) são rejeitados.
     - O caminho candidato precisa começar com `/`, `./`, `../`, `~/` **ou** conter `/` **ou** possuir uma extensão alfanumérica válida (ex.: `.php`, `.ts`, `.vue`, `.rs`, `.json`, `.scss`, `.py`, `.sh`, `.md`, `.toml`, `.yaml`, `.yml`, `.sql`, `.js`, `.css`, `.html`).
     - Rejeita caminhos que terminam com `/` (diretórios explícitos como `src/` — CA-22).
  5. **Extração de `:linha[:coluna]` da Direita**:
     - Se houver `:N` ou `:N:M` no final, ambos devem ser inteiros estritamente entre `1` e `10_000_000`. Se `N == 0`, `M == 0` ou houver overflow numérico (`> 10_000_000` ou mais de 8 dígitos), o candidato inteiro é rejeitado (CA-02).
     - Remove pontuação final de frase fora de aspas (`.`, `,`, `;`, `)`, `]`, `}`) antes de extrair `:linha[:coluna]`, ajustando `endIndex`.
  6. `key` único no formato `${startIndex}:${endIndex}:${path}:${line ?? 0}:${column ?? 0}`.

### 4.2. `src/composables/editorLinks/linkCache.ts`

- Cache LRU com capacidade máxima de `512` entradas e TTL de `5_000 ms` (5 s — CA-25).
- Chave composta: `${ctx.tabId}|${ctx.paneId}|${ctx.ptyId}|${ctx.cwdEpoch}|${path}|${line ?? 0}|${column ?? 0}`.
- Armazena tanto hits (`ResolvedLink`) quanto misses (`null`), evitando novas chamadas IPC em hovers repetidos na mesma linha dentro de 5 s.
- Método `invalidatePane(tabId, paneId)` limpa entradas obsoletas quando `cwdEpoch` ou `ptyId` muda.

### 4.3. `src/composables/useEditorLinks.ts` e Re-export `src/composables/useTerminalEditorLinks.ts`

- Registra `terminal.registerLinkProvider({ provideLinks(bufferLineNumber, callback) })`:
  - Reconstrói a linha lógica unindo linhas adjacentes com `isWrapped === true` (limitado a no máximo 8 linhas físicas para evitar varredura excessiva).
  - Converte offsets UTF-16 (`startIndex`, `endIndex`) para coordenadas de células 1-based (`ILink.range.start.{x, y}`, `ILink.range.end.{x, y}`) levando em conta largura de célula (`cell.getWidth()`: `0`, `1` ou `2` para caracteres CJK/emojis) e quebras de linha por wrap (CA-38).
  - Consulta `linkCache`; agrupa candidatos não cacheados em lotes de no máximo **64 itens** (`MAX_BATCH = 64`) e chama `osIpc.resolveLinks(capturedContext, chunk)` (uma linha com 200 candidatos dispara no máximo `ceil(200 / 64) = 4` chamadas — CA-25).
  - Debounce de `60 ms` por linha e geração lógica (`requestEpoch`) que descarta respostas se o usuário já moveu o cursor para outra linha ou se `context.value.cwdEpoch` mudou.
  - Em `activate(event, text)`: verifica modificador (`event.ctrlKey || event.metaKey`, ou clique direto conforme política configurada na Fase 04) e chama `osIpc.openLink(capturedContext, candidate)` usando o `capturedContext` da resolução (CA-38).

---

## 5. Composables de Foco e Atalho (`useOsFocusTab.ts`, `useShortcutRecorder.ts`, `useGlobalQuakeShortcut.ts`)

1. **`src/composables/useOsFocusTab.ts`**:
   - Montado no `App.vue`; escuta `onFocusTab` e consome `windowStore.pendingFocus` após a hidratação da store de workspace da Fase 07.
   - Verifica se a aba ainda existe na store da Fase 07; se existir, seleciona o projeto (`projectsStore.select(projectId)`) e foca a aba/painel (`tabsStore.focus(projectId, tabId, paneId ?? undefined)`). Ignora abas já removidas sem recriá-las.
2. **`src/composables/useShortcutRecorder.ts`**:
   - Captura `keydown` com `event.preventDefault()` e `event.stopPropagation()` enquanto `recording.value === true`.
   - `Escape` cancela a gravação e limpa `preview`.
   - Exige pelo menos 1 modificador (`ctrlKey`, `altKey`, `shiftKey`, `metaKey`) + 1 tecla principal (`KeyboardEvent.code` mapeada para o formato canônico `Ctrl+Alt+Backquote`, `Ctrl+Shift+F12`, etc.).
3. **`src/composables/useGlobalQuakeShortcut.ts`**:
   - Orquestra em alto nível o status do atalho global (`windowStore.shortcutStatus`), a gravação com `useShortcutRecorder`, o salvamento em `osSettingsStore.setAccelerator` e a abertura do guia/portal Wayland (`osIpc.configurePortal()`).

---

## 6. Componentes de UI (`src/components/preferences/` e `src/components/os/`)

1. **`src/components/preferences/registry.ts` e `PreferencesDialog.vue`**:
   - Registro extensível de seções (`registerPreferencesSection({ id, label, icon, component, order })`) com a seção `'system'` (`'Sistema'`, ícone `'mdi:cog-outline'`, `order: 10`) registrada por padrão.
2. **`src/components/preferences/system/PreferencesSystemSection.vue`** (e re-export `src/components/os/OsPreferencesSection.vue`):
   - Compõe os 5 grupos: **Janela e bandeja**, **Atalho global (Quake Mode)**, **Editor externo**, **Notificações nativas** (incluindo botão `"Enviar notificação de teste"`) e **Inicialização do sistema**.
   - Exibe banner de aviso quando `!windowStore.trayAvailable` (`"Bandeja do sistema indisponível"` — CA-13) e aviso informativo em Wayland quando o modo drop-down está ativo (`!windowStore.environment?.windowPositioning` — CA-20).
3. **`src/components/preferences/system/CloseBehaviorSelector.vue`, `ShortcutRecorder.vue`, `WaylandShortcutHelp.vue`, `EditorSelector.vue`**:
   - Controles tipados e acessíveis com validação imediata de `{file}` em `customArgv`.
4. **`src/components/os/WaylandShortcutGuideModal.vue`, `CloseConfirmModal.vue`, `TrayStatusPill.vue`, `ShortcutStatusBadge.vue`**:
   - `WaylandShortcutGuideModal.vue`: modal dedicado exibindo `WaylandShortcutHelp` com botão de copiar snippet para Hyprland / KDE Plasma 6 e botão `"Configurar no sistema"` (portal XDG).
   - `CloseConfirmModal.vue`: modal in-app opcional de confirmação/status de encerramento sincronizado com `os://shutdown-started` e `os://shutdown-failed`.
   - `TrayStatusPill.vue`: pílula de status na barra inferior/topo exibindo os contadores ativos (`running`, `continuous`, `errorUnseen`, `awaitingInput`) e indicador de bandeja.
   - `ShortcutStatusBadge.vue`: badge semântico (`--max-success-500` para ativo, `--max-info-500` para modo CLI, `--max-danger-500` para erro).

---

## 7. Mapeamento para Critérios de Aceite

- **CA-04**: `npm run typecheck && npm run typecheck:vue && npm run lint` passam sem erros.
- **CA-05**: Suítes Vitest passam com cobertura ≥ 90% de linhas em `parseLinkCandidates.ts`.
- **CA-13, CA-17, CA-19, CA-20**: UI reflete indisponibilidade de tray, snippet Wayland `cli_only`, erro de atalho com rollback e aviso de posicionamento Wayland.
- **CA-25, CA-38**: Lotes de ≤ 64 links, cache de 5 s (0 chamadas no segundo hover), coordenadas de células wide/wrapped corretas e invalidação por `cwdEpoch`.
- **CA-31**: Todos os rótulos, tooltips, mensagens e botões em `pt-BR`.
