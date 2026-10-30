# Implementação 04 — Store Pinia, Applier CSS Sem FOUC, Bridge Xterm e UI de Preferências (`plan.md`)

> Fase 10 — Temas e Importador Tabby
> Especificação desta unidade: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Execução

### Passo 1 — Tipos Derivados, Mensagens de Erro e Cliente IPC (`src/types/theme.ts`, `src/ipc/themes.ts`)
- [ ] Criar `src/types/theme.ts` com `CachedThemeSnapshot`, `ThemeResolver`, `PaneThemeContext` e `MAPPING_VERSION`.
- [ ] Atualizar `src/types/errors.ts` adicionando as mensagens em `pt-BR` para todos os códigos `theme.*` (`theme.not_found`, `theme.invalid_color`, `theme.invalid_palette`, `theme.invalid_scheme`, `theme.parse_failed`, `theme.unsupported_format`, `theme.file_too_large`, `theme.file_unreadable`, `theme.path_invalid`, `theme.dir_limit_exceeded`, `theme.builtin_readonly`, `theme.export_failed`, `theme.invalid_font`, `theme.invalid_appearance`, `theme.invalid_token`, `theme.project_not_found`, `theme.ligatures_unavailable`).
- [ ] Implementar `src/ipc/themes.ts` com `themeIpc`, `onThemeChanged`, `onThemeLibraryChanged`, `pickThemeFile`, `pickThemeDir` e `pickExportPath`.

### Passo 2 — Motor de Tokens CSS, Bootstrap Sem FOUC e Utilitários de Fonte (`src/theme/{cssTokens,bootstrap,fonts}.ts`)
- [ ] Implementar `src/theme/cssTokens.ts` com allowlist das 102 chaves, `sanitizeVars`, `applyTokens`, `clearTokens` e `tokensToStyle`.
- [ ] Implementar `src/theme/bootstrap.ts` com `applyCachedThemeBeforeMount` e `saveSnapshot` (limite 32 KiB).
- [ ] Implementar `src/theme/fonts.ts` com `MONO_FONT_SUGGESTIONS`, `isFontAvailable` e `quoteFontFamily`.
- [ ] Criar os testes `tests/theme/cssTokens.spec.ts` e `tests/theme/bootstrap.spec.ts`.

### Passo 3 — Bridge com o Motor de Terminal (`src/theme/terminalBridge.ts`)
- [ ] Implementar `toTerminalTheme`, `toTerminalPreferences` e `installTerminalBridge` em `src/theme/terminalBridge.ts`.
- [ ] Criar `tests/theme/terminalTheme.typecheck.ts` verificando em tempo de compilação que `XtermTheme` é atribuível a `ITheme` e `ReturnType<typeof toTerminalTheme>` é atribuível a `TerminalTheme` (CA-05).
- [ ] Criar `tests/theme/terminalBridge.spec.ts` testando aplicação por painel com e sem override de projeto, panes ocultas, `clearTextureAtlas()` e fallback de ligaduras.

### Passo 4 — Store `useThemeStore` e Composables (`src/stores/theme.ts`, `src/composables/*.ts`)
- [ ] Implementar `src/stores/theme.ts` com LRU de 24 temas inativos, fixação de temas em uso, cursores monotônicos de revisão, reação a `project://changed` (`dropProjects`) e rollback otimista em `updateAppearance`.
- [ ] Implementar `src/composables/useThemeApplier.ts`, `src/composables/useTerminalAppearance.ts` (com debounce de 150 ms na fonte e reversão de ligaduras com mensagem `"Não foi possível ativar as ligaduras nesta fonte"`) e `src/composables/useThemePreview.ts` (debounce de 60 ms e isolamento de escopo local).
- [ ] Integrar `applyCachedThemeBeforeMount()` e `registerBootstrapStep({ name: 'themes', ... })` em `src/app/main.ts` e montar `useThemeApplier()` + `useTerminalAppearance()` em `src/app/App.vue`.
- [ ] Criar `tests/stores/theme.spec.ts` e `tests/composables/useThemeApplier.spec.ts`.

### Passo 5 — Componentes de Preferências de Aparência (`src/components/preferences/appearance/*.vue`)
- [ ] Verificar se `src/components/preferences/registry.ts` e `PreferencesDialog.vue` já existem (Fase 09); caso não existam, criá-los conforme `spec.md` I-15.
- [ ] Implementar `ThemeSwatch.vue`, `ThemePreview.vue`, `ThemePicker.vue`, `TerminalFontSettings.vue`, `ProjectThemeOverrides.vue`, `ThemeImportReport.vue` e `AppearancePreferences.vue` seguindo todas as regras de SFC Vue (ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, tags de abertura em linha única, 4 espaços, sem cores literais).
- [ ] Criar os testes de componentes em `tests/components/preferences/appearance/*.spec.ts`.

---

## 2. Comandos de Verificação e Validação

```bash
# Linting de código e estilos (garante CA-03 e CA-32)
npm run lint
npm run lint:style

# Verificação de tipos TypeScript e Vue (inclui CA-05 terminalTheme.typecheck.ts)
npm run typecheck
npm run typecheck:vue

# Testes unitários e de componentes com cobertura >= 85% em stores/theme.ts, src/theme/ e useThemeApplier.ts
npx --no-install vitest run tests/theme/ tests/stores/theme.spec.ts tests/composables/useThemeApplier.spec.ts tests/components/preferences/appearance/ --coverage

# Verificação final integrada (Rust + Frontend + Bindings)
npm run bindings && git diff --exit-code src/types/bindings.ts
cargo test --manifest-path src-tauri/Cargo.toml --workspace
npm run test
```

---

## 3. Critérios de Aceite Atendidos

| Critério | Descrição | Verificação |
|----------|-----------|-------------|
| **CA-03** | `npm run lint`, `npm run lint:style`, `npm run typecheck`, `npm run typecheck:vue` e `npm run test` passam com cobertura ≥ 85%. | Comandos acima retornam exit 0. |
| **CA-05** | Teste TS de compilação valida `XtermTheme`  assignable a `ITheme` e `toTerminalTheme` assignable a `TerminalTheme`. | `tests/theme/terminalTheme.typecheck.ts`. |
| **CA-24** | Confirmar **Aplicar** atualiza tokens e terminais elegíveis no próximo frame sem reiniciar PTY ou perder scrollback. | `tests/stores/theme.spec.ts` + `tests/theme/terminalBridge.spec.ts`. |
| **CA-25** | Hover/foco atualiza somente o `ThemePreview` em escopo local; sair restaura o ativo sem alterar o estado global. | `tests/components/preferences/appearance/ThemePicker.spec.ts`. |
| **CA-26** | Tema claro remove a classe `dark` de `<html>` e define `color-scheme: light`; tema escuro restaura. | `tests/composables/useThemeApplier.spec.ts`. |
| **CA-27** | Sobreposição de tema por projeto com e sem `applyProjectThemeToUi`. | `tests/stores/theme.spec.ts` + `ProjectThemeOverrides.spec.ts`. |
| **CA-28** | Alterar fonte/tamanho/altura de linha reflete nos terminais em ≤ 200 ms com `clearTextureAtlas()` e re-fit. | `tests/theme/terminalBridge.spec.ts`. |
| **CA-29** | Ligaduras carregadas sob demanda; falha reverte opção e exibe `"Não foi possível ativar as ligaduras nesta fonte"`. | `tests/theme/terminalBridge.spec.ts` + `tests/stores/theme.spec.ts`. |
| **CA-30** | Reiniciar com tema claro ativo aplica snapshot de `localStorage` antes do `mount()` sem FOUC. | `tests/theme/bootstrap.spec.ts`. |
| **CA-31** | `ThemeImportReport` exibe contagens exatas (`totalFailures`/`totalWarnings`), aviso de `truncatedDetails` e badge "Contraste baixo". | `tests/components/preferences/appearance/ThemeImportReport.spec.ts`. |
| **CA-32** | Textos de UI, `MtError` e comentários em `pt-BR`; nenhum `--max-*` literal fora de `cssTokens.ts`/builtins. | `npm run lint` + `npm run lint:style`. |
| **CA-33** | Cancelar/fechar preview não grava SQLite nem snapshot; falha de persistência faz rollback visual e de estado. | `tests/stores/theme.spec.ts`. |
| **CA-34** | Respostas/eventos fora de ordem e mutações concorrentes convergem pela revisão monotônica. | `tests/stores/theme.spec.ts`. |
