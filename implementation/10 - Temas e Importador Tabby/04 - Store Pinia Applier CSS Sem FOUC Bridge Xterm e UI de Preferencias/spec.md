# Implementação 04 — Store Pinia, Applier CSS Sem FOUC, Bridge Xterm e UI de Preferências (`spec.md`)

> Fase 10 — Temas e Importador Tabby
> Referências: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§7, §8, §9, §10, §11, §12.2).
> Pré-requisito: Implementações 01, 02 e 03 concluídas.

---

## 1. Contexto, Objetivo e Escopo

Esta quarta e última implementação entrega toda a camada Frontend (Vue 3.6 + TypeScript + UnoCSS + SCSS Scoped) do subsistema de temas e aparência do MaxTerminal:

1. **Wrappers IPC e Tipos Derivados (`src/types/theme.ts`, `src/ipc/themes.ts`)**: cliente tipado `themeIpc` usando `call` de `src/ipc/core.ts`, listeners `onThemeChanged` / `onThemeLibraryChanged`, helpers de diálogo nativo (`pickThemeFile`, `pickThemeDir`, `pickExportPath` via `@tauri-apps/plugin-dialog`) e registro de mensagens de erro em `pt-BR` para todos os códigos `theme.*`.
2. **Motor de Tokens CSS, Bootstrap Anti-FOUC e Fontes (`src/theme/{cssTokens.ts,bootstrap.ts,fonts.ts}`)**:
   - `cssTokens.ts`: allowlist estrita das 102 chaves CSS (`--background-0..950`, `--max-{primary,success,warning,danger,info}-{50..950,content}`, `--mt-terminal-{background,foreground,selection}`), validação por regex (`TOKEN_NAME_RE`, `TOKEN_VALUE_RE`), aplicação via CSSOM (`root.style.setProperty`) com diff sobre `AppliedTokens` e escopo local via `tokensToStyle`.
   - `bootstrap.ts`: leitura e validação de `localStorage.getItem('mt.theme.lastApplied')` (limite 32 KiB) **antes** de `app.mount()` em `main.ts`, eliminando qualquer flash de tema incorreto (CA-30).
   - `fonts.ts`: catálogo `MONO_FONT_SUGGESTIONS`, verificação `isFontAvailable` e serialização segura `quoteFontFamily` contra injeção de CSS.
3. **Bridge com o Motor de Terminal (`src/theme/terminalBridge.ts`)**: adaptação de `ResolvedTheme` para `TerminalTheme` (Fase 04), registro do `setThemeProvider` global e da extensão `theme.project-override` via `registerTerminalExtension`, propagação reativa para todas as instâncias vivas sem reiniciar PTY nem perder scrollback, atualização de fonte com `clearTextureAtlas()` + re-fit e carregamento sob demanda de `@xterm/addon-ligatures` com fallback automático em caso de erro.
4. **Store Pinia e Composables (`src/stores/theme.ts`, `src/composables/{useThemeApplier,useTerminalAppearance,useThemePreview}.ts`)**: gerenciamento de estado reativo com cache LRU de 24 temas inativos e fixação de temas em uso, cursores monotônicos de revisão para aparência e biblioteca, resolução de tema por projeto (`effectiveUiThemeId`, `applyProjectThemeToUi`), preview isolado com geração monotônica e rollback otimista em falhas de persistência.
5. **Componentes de Interface (`src/components/preferences/appearance/*.vue`)**: `AppearancePreferences.vue`, `ThemePicker.vue`, `ThemeSwatch.vue`, `ThemePreview.vue`, `TerminalFontSettings.vue`, `ProjectThemeOverrides.vue` e `ThemeImportReport.vue` (mais `PreferencesDialog.vue` e `registry.ts` caso não tenham sido criados pela Fase 09).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Modificados
- `package.json` — adição de `@xterm/addon-ligatures` e `@tauri-apps/plugin-dialog` (se ausentes).
- `src/types/errors.ts` — inclusão das mensagens `pt-BR` dos códigos `theme.*` no mapa `errorMessages`.
- `src/app/main.ts` — chamada de `applyCachedThemeBeforeMount()` antes de `app.mount('#app')` e registro do bootstrap step `themes`.
- `src/app/App.vue` — montagem de `useThemeApplier()` e `useTerminalAppearance()` (e `PreferencesDialog` se aplicável).

### 2.2. Arquivos Criados
- `src/types/theme.ts`
- `src/ipc/themes.ts`
- `src/theme/cssTokens.ts`
- `src/theme/bootstrap.ts`
- `src/theme/fonts.ts`
- `src/theme/terminalBridge.ts`
- `src/stores/theme.ts`
- `src/composables/useThemeApplier.ts`
- `src/composables/useTerminalAppearance.ts`
- `src/composables/useThemePreview.ts`
- `src/components/preferences/registry.ts` (se ainda não existir da Fase 09)
- `src/components/preferences/PreferencesDialog.vue` (se ainda não existir da Fase 09)
- `src/components/preferences/appearance/AppearancePreferences.vue`
- `src/components/preferences/appearance/ThemePicker.vue`
- `src/components/preferences/appearance/ThemeSwatch.vue`
- `src/components/preferences/appearance/ThemePreview.vue`
- `src/components/preferences/appearance/TerminalFontSettings.vue`
- `src/components/preferences/appearance/ProjectThemeOverrides.vue`
- `src/components/preferences/appearance/ThemeImportReport.vue`
- `tests/theme/terminalTheme.typecheck.ts`
- `tests/theme/cssTokens.spec.ts`
- `tests/theme/bootstrap.spec.ts`
- `tests/theme/terminalBridge.spec.ts`
- `tests/stores/theme.spec.ts`
- `tests/composables/useThemeApplier.spec.ts`
- `tests/components/preferences/appearance/ThemePicker.spec.ts`
- `tests/components/preferences/appearance/ThemePreview.spec.ts`
- `tests/components/preferences/appearance/ThemeImportReport.spec.ts`
- `tests/components/preferences/appearance/TerminalFontSettings.spec.ts`
- `tests/components/preferences/appearance/ProjectThemeOverrides.spec.ts`

---

## 3. Especificação Técnica de Módulos TypeScript e Vue

### 3.1. `src/types/theme.ts`

```ts
import type { ProjectId, ResolvedTheme, ThemeId } from './bindings';

export const MAPPING_VERSION = 1;

export interface CachedThemeSnapshot {
    v: 1;
    themeId: ThemeId;
    revision: number;
    mappingVersion: number;
    appearance: 'dark' | 'light';
    vars: Record<string, string>;
    savedAt: number;
}

export type ThemeResolver = (id: ThemeId) => ResolvedTheme | undefined;

export interface PaneThemeContext {
    projectId: ProjectId | null;
}
```

### 3.2. `src/theme/cssTokens.ts`

```ts
export const TOKEN_NAME_RE = /^--(max|background|mt)-[a-z0-9-]+$/;
export const TOKEN_HEX6_RE = /^#[0-9a-f]{6}$/;
export const TOKEN_HEX8_RE = /^#[0-9a-f]{6}([0-9a-f]{2})?$/;
export const LAST_APPLIED_KEY = 'mt.theme.lastApplied';

const BACKGROUND_STEPS = [
    0, 25, 50, 75, 100, 125, 150, 175, 200, 225, 250, 275, 300, 325, 350, 375, 400, 425, 450, 475,
    500, 525, 550, 575, 600, 625, 650, 675, 700, 725, 750, 775, 800, 825, 850, 875, 900, 925, 950
] as const;

const SCALE_NAMES = ['primary', 'success', 'warning', 'danger', 'info'] as const;
const SCALE_STEPS = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950] as const;

export const ALLOWED_TOKEN_KEYS: ReadonlySet<string> = new Set<string>([
    ...BACKGROUND_STEPS.map((s) => `--background-${s}`),
    ...SCALE_NAMES.flatMap((name) => [
        ...SCALE_STEPS.map((step) => `--max-${name}-${step}`),
        `--max-${name}-content`
    ]),
    '--mt-terminal-background',
    '--mt-terminal-foreground',
    '--mt-terminal-selection'
]);

export interface AppliedTokens {
    keys: ReadonlySet<string>;
    values: Readonly<Record<string, string>>;
    appearance: 'dark' | 'light';
}

export function isValidTokenEntry(key: string, value: unknown): value is string {
    if (typeof value !== 'string' || !TOKEN_NAME_RE.test(key) || !ALLOWED_TOKEN_KEYS.has(key)) {
        return false;
    }
    if (key === '--mt-terminal-selection') {
        return TOKEN_HEX8_RE.test(value);
    }
    return TOKEN_HEX6_RE.test(value);
}

export function sanitizeVars(input: Record<string, unknown>): Record<string, string>;
export function hasCompleteAllowedTokens(vars: Record<string, unknown>): vars is Record<string, string>;
export function applyTokens(
    vars: Record<string, string>,
    appearance: 'dark' | 'light',
    root?: HTMLElement,
    previous?: AppliedTokens
): AppliedTokens;
export function clearTokens(root?: HTMLElement, previous?: AppliedTokens): void;
export function tokensToStyle(vars: Record<string, string>): Record<string, string>;
```

Regras de `applyTokens`:
- Sanitiza `vars` com `sanitizeVars`.
- Para cada chave presente em `previous?.keys` que não exista no novo conjunto sanitizado, executa `target.style.removeProperty(key)`.
- Para cada `[key, value]` do novo conjunto cujo valor difira de `previous?.values[key]`, executa `target.style.setProperty(key, value)`.
- Atualiza `target.classList.toggle('dark', appearance === 'dark')` e `target.style.colorScheme = appearance`.

### 3.3. `src/theme/bootstrap.ts`

```ts
import type { CachedThemeSnapshot } from '@/types/theme';
import { MAPPING_VERSION } from '@/types/theme';
import { applyTokens, hasCompleteAllowedTokens, LAST_APPLIED_KEY } from './cssTokens';

export const MAX_SNAPSHOT_BYTES = 32 * 1024;

export function applyCachedThemeBeforeMount(
    storage: Storage | undefined = typeof window !== 'undefined' ? window.localStorage : undefined,
    root: HTMLElement | undefined = typeof document !== 'undefined' ? document.documentElement : undefined
): boolean;

export function saveSnapshot(
    snapshot: CachedThemeSnapshot,
    storage: Storage | undefined = typeof window !== 'undefined' ? window.localStorage : undefined
): void;
```

Regras de `applyCachedThemeBeforeMount`:
- Envolve leitura de `storage` em `try / catch` (Storage indisponível ou bloqueado retorna `false` sem lançar erro).
- Se `raw === null` ou `raw.length > MAX_SNAPSHOT_BYTES`, remove a chave e retorna `false`.
- Faz `JSON.parse(raw)` e valida: `v === 1`, `mappingVersion === MAPPING_VERSION`, `appearance === 'dark' || appearance === 'light'`, `typeof themeId === 'string'`, `typeof revision === 'number'` e `hasCompleteAllowedTokens(parsed.vars)`. Se falhar, remove `LAST_APPLIED_KEY` e retorna `false`.
- Chama `applyTokens(parsed.vars, parsed.appearance, root)` e retorna `true`.

### 3.4. `src/theme/terminalBridge.ts`

```ts
import type { ProjectId, ResolvedTheme, TerminalFontSettings } from '@/types/bindings';
import type { TerminalPreferences, TerminalTheme } from '@/types/terminal';

export function toTerminalTheme(resolved: ResolvedTheme): TerminalTheme;
export function toTerminalPreferences(
    font: TerminalFontSettings
): Pick<TerminalPreferences, 'fontFamily' | 'fontSize' | 'lineHeight' | 'letterSpacing'>;

export interface TerminalBridge {
    applyThemes(resolve: (projectId: ProjectId | null) => ResolvedTheme | null): void;
    applyFont(font: TerminalFontSettings): Promise<{ ligaturesFailed: boolean }>;
    dispose(): void;
}

export function installTerminalBridge(
    resolve: (projectId: ProjectId | null) => ResolvedTheme | null
): TerminalBridge;
```

Detalhes de `toTerminalTheme` e `installTerminalBridge`:
- Constrói um **novo objeto** `TerminalTheme` a cada chamada, com tupla `ansi` de 16 strings hexadecimais validada, `source: resolved.summary.builtin ? 'builtin' : (resolved.summary.origin.startsWith('tabby_') ? 'tabby' : 'custom')`, e `ui` contendo `--mt-terminal-background`, `--mt-terminal-foreground` e `--mt-terminal-selection` para os blocos e `RichInput`.
- Nunca muta `xterm.options.theme` in-place; atribui um novo objeto `ITheme` (omitindo chaves opcionais ausentes como `selectionForeground` para não herdar valor residual do tema anterior).
- Em `applyFont`:
  - Atualiza `fontFamily` (serializada por `quoteFontFamily`), `fontSize`, `lineHeight`, `letterSpacing` via `inst.applyPreferences(...)`.
  - Atualiza `inst.xterm.options.fontWeight = font.fontWeight` e `inst.xterm.options.fontWeightBold = font.fontWeightBold`.
  - Chama `inst.xterm.clearTextureAtlas?.()` seguido do re-fit da instância (que dispara `pty_resize(cols, rows)`).
  - Se `font.ligatures === true`: importa dinamicamente `@xterm/addon-ligatures` e ativa nas instâncias; se o `import()` ou `loadAddon` lançar exceção, descarta o addon parcial e retorna `{ ligaturesFailed: true }`.

### 3.5. Store `useThemeStore` (`src/stores/theme.ts`)

Implementa todos os estados, getters e actions de `spec.md` §7.4 e fluxos de §8.5:
- `DEFAULT_THEME_ID = 'b5907b11-f15c-5886-b803-f1fb94bc4c9d'`.
- `DEFAULT_FONT: TerminalFontSettings = { fontFamily: 'JetBrainsMono Nerd Font, monospace', fontSize: 13, lineHeight: 1.15, letterSpacing: 0, fontWeight: 'normal', fontWeightBold: 'bold', ligatures: false }`.
- Mantém `appearanceRevisionCursor` e `libraryRevisionCursor` independentes para processar eventos `theme://changed` e `theme://library-changed` sem colisão de revisão.
- Fixa no mapa `resolved` o tema ativo global, o tema padrão e todos os temas usados em `project_overrides`, mantendo um LRU de até 24 temas inativos para previsualização rápida.
- Em `updateAppearance(patch)`: aplica atualização otimista em `appearance.font` / `appearance.applyProjectThemeToUi` guardando o snapshot confirmado anterior e um contador de geração; se a chamada IPC `themeIpc.updateAppearance(patch)` falhar, reverte para o último estado confirmado caso nenhuma revisão mais nova tenha sido confirmada (CA-33, CA-34).

### 3.6. Convenções Obrigatórias dos Componentes Vue (`src/components/preferences/appearance/*.vue`)

Todos os SFCs devem obedecer rigorosamente às regras do `AGENTS.md`:
1. Ordem dos blocos: `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`.
2. Recuo de 4 espaços (`baseIndent: 1` no `<script setup>`), aspas simples, ponto e vírgula obrigatório, sem vírgula final (`comma-dangle: 'never'`).
3. Tags de abertura no `<template>` em **linha única** (nome da tag, atributos, props, diretivas, eventos e `>` ou `/>` na mesma linha).
4. Uso estrito de tokens `--max-*` e `--background-*` nos blocos `<style lang="scss" scoped>`, sem cores hexadecimais literais (CA-32).
5. `ThemeImportReport.vue` usa sempre `report.totalFailures` e `report.totalWarnings` nos contadores de resumo e exibe aviso indicando `report.truncatedDetails` quando `report.truncatedDetails > 0` (CA-31).
6. Quando `ligaturesFailed` retornar `true` no `useTerminalAppearance`, `useThemeStore` reverte `ligatures = false` e exibe toast com o texto exato: `"Não foi possível ativar as ligaduras nesta fonte"` (CA-29).
