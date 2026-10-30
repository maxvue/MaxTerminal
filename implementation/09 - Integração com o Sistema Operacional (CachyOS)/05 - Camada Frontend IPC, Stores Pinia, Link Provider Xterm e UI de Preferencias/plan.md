# Implementação 05 — Camada Frontend IPC, Stores Pinia, Link Provider Xterm e UI de Preferências (`plan.md`)

> Plano de execução passo a passo para a **Implementação 05** da Fase 09.
> Especificação técnica correspondente: [`./spec.md`](./spec.md).

---

## 1. Pré-requisitos

1. Implementações 01 a 04 concluídas e `src/types/bindings.ts` atualizado via `npm run bindings`.
2. Componentes base do `@maxvue/max-components-ui` (`MaxModal`, `MaxButton`, `MaxIcon`, `Toast`) e ponto de extensão de link provider da Fase 04 disponíveis.

---

## 2. Etapas de Implementação

### Etapa 2.1 — Wrapper IPC e Listeners Tipados (`src/ipc/os.ts`)
- **Arquivos**:
  - `src/ipc/os.ts`
- **Ações**:
  1. Implementar `unwrap` convertendo `{ status: 'error', error }` em rejeição tipada com `MtError`.
  2. Exportar `osIpc` com os 16 métodos mapeados para `commands.os*`.
  3. Exportar os 10 helpers `onFocusTab`, `onWindowVisibility`, `onSettingsChanged`, `onShortcutStatus`, `onCounters`, `onOpenPreferences`, `onShutdownStarted`, `onShutdownFailed`, `onEnvironmentChanged` e `onLinkContextChanged`.

### Etapa 2.2 — Stores Pinia (`window.ts` / `osWindow.ts` e `osSettings.ts`)
- **Arquivos**:
  - `src/stores/window.ts`
  - `src/stores/osWindow.ts`
  - `src/stores/osSettings.ts`
- **Ações**:
  1. Em `src/stores/window.ts`: implementar `useWindowStore` com protocolo de inicialização livre de condição de corrida (assinar eventos → bufferizar → chamar `osIpc.uiReady()` → aplicar apenas eventos com `revision > snapshot.revision`), getters (`isWayland`, `trayAvailable`, `shortcutHealthy`, `needsAttention`) e `dispose()` seguro contra registros tardios.
  2. Em `src/stores/osWindow.ts`: re-exportar `useWindowStore` e `useOsWindowStore`.
  3. Em `src/stores/osSettings.ts`: implementar `useOsSettingsStore` com `load()`, `update(patch)` otimista com rollback automático e `Toast.show` em caso de erro, debounce de 400 ms para ajustes numéricos e `setAccelerator(acc)`.

### Etapa 2.3 — Parser de Links, Cache LRU e Link Provider do Xterm (`src/composables/`)
- **Arquivos**:
  - `src/composables/editorLinks/parseLinkCandidates.ts`
  - `src/composables/editorLinks/linkCache.ts`
  - `src/composables/useEditorLinks.ts`
  - `src/composables/useTerminalEditorLinks.ts`
- **Ações**:
  1. Em `parseLinkCandidates.ts`: implementar scanner linear com suporte a aspas, Unicode (`\p{L}`, `\p{N}`), exclusão integral de URLs (`://`, `mailto:`), horários (`10:30`), versões (`1.2.3:4`) e diretórios (`src/`), extração de `:linha[:coluna]` (`1..=10_000_000`) e limite de 200 candidatos / 16 KiB por linha.
  2. Em `linkCache.ts`: implementar cache LRU de 512 entradas com TTL de 5 s indexado por `tabId|paneId|ptyId|cwdEpoch|path|line|column` e invalidação por painel.
  3. Em `useEditorLinks.ts` (e re-export `useTerminalEditorLinks.ts`): registrar `terminal.registerLinkProvider`, reconstruir linhas wrapped, mapear índices UTF-16 para coordenadas 1-based de células xterm (tratando células de largura 0/1/2), fatiar resoluções em lotes de ≤ 64 itens com debounce de 60 ms e abrir com `osIpc.openLink(capturedContext, candidate)`.

### Etapa 2.4 — Composables de Foco e Gravador de Atalho (`src/composables/`)
- **Arquivos**:
  - `src/composables/useOsFocusTab.ts`
  - `src/composables/useShortcutRecorder.ts`
  - `src/composables/useGlobalQuakeShortcut.ts`
- **Ações**:
  1. Implementar `useOsFocusTab.ts` aguardando hidratação da store de abas (Fase 07), consumindo `pendingFocus` e ouvindo `onFocusTab`.
  2. Implementar `useShortcutRecorder.ts` convertendo combinações `keydown` (`Ctrl`, `Alt`, `Shift`, `Super` + `KeyboardEvent.code`) em string canônica e cancelando com `Escape`.
  3. Implementar `useGlobalQuakeShortcut.ts` integrando `useShortcutRecorder`, `useOsSettingsStore` e `useWindowStore`.

### Etapa 2.5 — Componentes Vue de Preferências e OS (`src/components/`)
- **Arquivos**:
  - `src/components/preferences/registry.ts`
  - `src/components/preferences/PreferencesDialog.vue`
  - `src/components/preferences/system/PreferencesSystemSection.vue`
  - `src/components/preferences/system/CloseBehaviorSelector.vue`
  - `src/components/preferences/system/ShortcutRecorder.vue`
  - `src/components/preferences/system/WaylandShortcutHelp.vue`
  - `src/components/preferences/system/EditorSelector.vue`
  - `src/components/os/OsPreferencesSection.vue`
  - `src/components/os/WaylandShortcutGuideModal.vue`
  - `src/components/os/CloseConfirmModal.vue`
  - `src/components/os/TrayStatusPill.vue`
  - `src/components/os/ShortcutStatusBadge.vue`
- **Ações**:
  1. Seguir estritamente a ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, tags de abertura em linha única, recuo de 4 espaços, aspas simples, ponto e vírgula e sem vírgula final.
  2. Implementar todos os componentes com textos em `pt-BR` e tokens `--max-*`.
  3. Conectar `useOsFocusTab` e `PreferencesDialog` no `App.vue`.

### Etapa 2.6 — Suítes de Testes Automatizados no Frontend (Vitest)
- **Arquivos**:
  - `tests/composables/editorLinks/parseLinkCandidates.spec.ts`
  - `tests/composables/useEditorLinks.spec.ts`
  - `tests/stores/window.spec.ts`
  - `tests/stores/osSettings.spec.ts`
  - `tests/composables/useShortcutRecorder.spec.ts`
  - `tests/components/preferences/PreferencesSystemSection.spec.ts`
- **Ações**:
  1. Em `parseLinkCandidates.spec.ts`: testar ≥ 40 casos reais (`php artisan`, `tsc`, `cargo`, `eslint`, `phpunit`, caminhos relativos/absolutos/`~`, espaços entre aspas, caracteres Unicode/acentos, URLs rejeitadas, versões e horários rejeitados, linha 0/overflow rejeitados, truncamento em 200 candidatos), atingindo ≥ 90% de cobertura de linhas.
  2. Em `useEditorLinks.spec.ts`: testar loteamento de 200 candidatos em 4 chamadas de ≤ 64 itens (CA-25), cache hit em 5 s (0 chamadas no segundo hover), invalidação ao mudar `cwdEpoch`, cálculo de coordenadas em caracteres largos (emojis/CJK) e linhas wrapped.
  3. Em `window.spec.ts` e `osSettings.spec.ts`: testar ordenação por `revision`, preservação de `--minimized` em `os_ui_ready`, atualização otimista e rollback com erro `os.shortcut_conflict`.
  4. Em `useShortcutRecorder.spec.ts` e `PreferencesSystemSection.spec.ts`: testar gravação de teclas, cancelamento com `Escape`, exibição do snippet Wayland (`cli_only`) e aviso de bandeja indisponível (`!trayAvailable`).

---

## 3. Checklist de Validação

- [ ] Todos os arquivos `.vue` obedecem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>` e possuem tags de abertura em linha única.
- [ ] `parseLinkCandidates.ts` possui cobertura de linhas ≥ 90% no Vitest.
- [ ] Uma linha com 200 candidatos gera no máximo 4 chamadas `os_link_resolve` e reutiliza o cache em hovers subsequentes dentro de 5 s.
- [ ] `useWindowStore.init()` chama `os_ui_ready` sem forçar `os_window_show` incondicional e descarta eventos com `revision` anterior ao snapshot.
- [ ] `npm run typecheck`, `npm run typecheck:vue` e `npm run lint` executam com código de saída 0.

---

## 4. Comandos de Verificação (Mapeados para `CA-*`)

```bash
# CA-04: Verificação estrita de tipos TypeScript/Vue e ESLint/Stylelint
npm run typecheck && npm run typecheck:vue && npm run lint

# CA-05: Suíte de testes unitários e de componentes com cobertura de parseLinkCandidates.ts >= 90%
npx vitest run tests/composables/editorLinks tests/stores/window.spec.ts tests/stores/osSettings.spec.ts tests/composables/useShortcutRecorder.spec.ts tests/components/preferences --coverage
```
