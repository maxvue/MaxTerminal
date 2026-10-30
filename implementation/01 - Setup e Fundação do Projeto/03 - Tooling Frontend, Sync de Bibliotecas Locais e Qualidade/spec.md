# Implementação 03 — Tooling Frontend, Sync de Bibliotecas Locais e Qualidade (`spec.md`)

> Fase: [`../instructions.md`](../instructions.md) | Especificação da Fase: [`../spec.md`](../spec.md)
> Escopo desta sub-implementação: `package.json`, `tsconfig.json`, `tsconfig.node.json`, `tsconfig.vue.json`, `tsconfig.test.json`, `src/env.d.ts`, `vite.config.ts`, `vitest.config.ts`, `uno.config.ts`, `eslint.config.js`, `eslint/rules/single-line-start-tag.js`, `.stylelintrc.json`, `scripts/sync-local-libs.mjs`, `scripts/check.mjs`, `tests/setup.ts`, `tests/lint/eslint-rules.spec.ts` (+ fixtures em `tests/lint/fixtures/`) e testes do sincronizador (`tests/scripts/sync-local-libs.spec.ts`).

---

## 1. Objetivo e Fronteira Arquitetural

Configurar todo o pipeline de build, vinculação de bibliotecas locais (`@maxvue/*`), tipagem estrita (`tsgo` + `vue-tsc`), linting (`ESLint` flat config + regra autoral de tags em linha única + `Stylelint`), suíte de testes (`Vitest` + cobertura `v8`) e o orquestrador de qualidade (`scripts/check.mjs`):

1. **Manifesto e Vinculação Local (`package.json`, `scripts/sync-local-libs.mjs`):**
   - Fixar `vue` em **`3.6.0-rc.13`**, Vite 8 e Tauri CLI 2.
   - Declarar o mapa `maxLocalLibs` para `@maxvue/max-components-ui` (`MaxComponentsUi`), `@maxvue/max-use` (`MaxUse`) e `@maxvue/max-pinia` (`MaxPinia`), criando **symlinks absolutos** em `node_modules/@maxvue/*` a partir de `MAX_LIBS_ROOT` (ou `~/GitHub`), com modo `--check` estritamente somente leitura.
2. **Tipagem Estrita em 4 Projetos (`tsconfig*.json`):**
   - `tsconfig.json` (código TS da aplicação verificado por `tsgo`), `tsconfig.node.json` (configs Vite/Vitest/UnoCSS verificados por `tsgo`), `tsconfig.test.json` (testes Vitest verificados por `tsgo`) e `tsconfig.vue.json` (SFCs `.vue` + TS verificados por `vue-tsc`), todos com `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noImplicitOverride`, `noFallthroughCasesInSwitch` e `verbatimModuleSyntax`.
3. **Qualidade de Código e Estilo (`eslint.config.js`, `eslint/rules/single-line-start-tag.js`, `.stylelintrc.json`):**
   - Impor a ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, recuo de 4 espaços (`baseIndent: 1` no `<script setup>`), aspas simples no TS e duplas nos atributos HTML, ponto e vírgula obrigatório, sem vírgula final (`comma-dangle: 'never'`) e tag de abertura em linha única via visitante AST do `vue-eslint-parser`.

---

## 2. Manifesto `package.json`

```json
{
  "name": "maxterminal",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "engines": {
    "node": ">=22.12.0",
    "npm": ">=10.0.0"
  },
  "maxLocalLibs": {
    "@maxvue/max-components-ui": "MaxComponentsUi",
    "@maxvue/max-use": "MaxUse",
    "@maxvue/max-pinia": "MaxPinia"
  },
  "scripts": {
    "dev": "vite --host 127.0.0.1 --port 5177 --strictPort",
    "build": "vite build",
    "preview": "vite preview --host 127.0.0.1 --port 4173 --strictPort",
    "tauri": "tauri",
    "typecheck": "tsgo --noEmit -p tsconfig.json && tsgo --noEmit -p tsconfig.node.json && tsgo --noEmit -p tsconfig.test.json",
    "typecheck:vue": "vue-tsc --noEmit -p tsconfig.vue.json",
    "lint": "eslint --max-warnings 0 .",
    "lint:fix": "eslint --fix .",
    "lint:style": "stylelint --allow-empty-input \"src/**/*.{scss,vue}\"",
    "test": "vitest run",
    "test:watch": "vitest",
    "test:coverage": "vitest run --coverage",
    "sync:local-libs": "node scripts/sync-local-libs.mjs",
    "bindings": "cargo test --manifest-path src-tauri/Cargo.toml --locked --test bindings export -- --ignored --exact",
    "check": "node scripts/check.mjs"
  },
  "dependencies": {
    "@tauri-apps/api": "2.5.0",
    "pinia": "3.0.1",
    "splitpanes": "4.1.2",
    "vue": "3.6.0-rc.13"
  },
  "devDependencies": {
    "@stylistic/eslint-plugin": "4.2.0",
    "@tauri-apps/cli": "2.5.0",
    "@types/node": "22.14.0",
    "@typescript/native-preview": "7.0.0-dev.20250520.1",
    "@vitejs/plugin-vue": "6.0.10",
    "@vitest/coverage-v8": "5.0.5",
    "@vue/test-utils": "2.4.6",
    "eslint": "9.24.0",
    "eslint-plugin-vue": "10.0.0",
    "globals": "16.0.0",
    "happy-dom": "17.4.4",
    "postcss-html": "1.8.0",
    "postcss-scss": "4.0.9",
    "sass-embedded": "1.86.3",
    "stylelint": "16.18.0",
    "stylelint-config-recommended-vue": "1.6.0",
    "stylelint-config-standard-scss": "14.0.0",
    "typescript": "5.8.3",
    "typescript-eslint": "8.29.1",
    "unocss": "66.0.0",
    "vite": "8.0.0",
    "vitest": "5.0.5",
    "vue-eslint-parser": "10.1.3",
    "vue-tsc": "2.2.8"
  },
  "overrides": {
    "vue": "3.6.0-rc.13"
  }
}
```

> **Regras sobre `package.json`:**
> - Antes de fechar `package-lock.json`, inspecionar os `peerDependencies` declarados nos manifests reais de `$MAX_LIBS_ROOT/{MaxComponentsUi,MaxUse,MaxPinia}/package.json` e adicionar ao `dependencies` do `maxterminal` quaisquer peers obrigatórios de runtime exigidos pelas 3 bibliotecas.
> - Fixar versões exatas (sem `^` ou `~`) compatíveis com o registro npm disponível na máquina de execução, mantendo `vue` estritamente em `3.6.0-rc.13`.
> - **Compatibilidade de peers (T-01/D-10):** `@vitejs/plugin-vue` `6.0.10` e `vitest` `5.0.5` aceitam `vite@8` e `vue@3.6.0-rc` nos `peerDependencies`; por isso `npm ci` não usa `--legacy-peer-deps` nem `overrides` além do pin de `vue`. Todos os pacotes `@vitest/*` (hoje apenas `@vitest/coverage-v8`) acompanham a **mesma versão** de `vitest` (`5.0.5`); ao atualizar um, atualizar o outro no mesmo commit.
> - **`splitpanes` (T-02/D-10):** `splitpanes` `4.1.2` fica em `dependencies` porque é consumido em runtime pelo layout das Fases 06 e 07 (`Splitpanes`, `Pane`, `splitpanes/dist/splitpanes.css`). **Não** adicionar `@types/splitpanes`: o pacote já distribui tipos próprios.
> - **Scripts canônicos (E-08/D-10):** `typecheck`, `typecheck:vue`, `lint`, `lint:style`, `test` (além de `test:coverage`, `bindings` e `check`) são os únicos nomes válidos para essas verificações em todas as fases; nenhum alias adicional (variações de `typecheck`) existe e nenhum deve ser usado em `plan.md`. Gerenciador único: `npm` (`npx --no-install` para binários locais); outros gerenciadores de pacotes Node são proibidos neste repositório.
> - **Tolerância a árvore vazia (E-03):** `lint:style` usa `stylelint --allow-empty-input` e `typecheck:vue` permanece válido sem `.vue` porque `tsconfig.vue.json` inclui `src/env.d.ts`. Assim `npm run lint:style` e `npm run typecheck:vue` retornam `0` antes de a sub-implementação 01/04 criar os primeiros `.vue`/`.scss`; a validação com arquivos reais ocorre em `01/04` (`npm run check`, CA-28).

---

## 3. Configuração TypeScript (`tsconfig*.json` e `src/env.d.ts`)

### 3.1. `tsconfig.json` (Aplicação TS — verificado por `tsgo`)

```json
{
  "compilerOptions": {
    "target": "ES2023",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2023", "DOM", "DOM.Iterable"],
    "types": ["vite/client"],
    "paths": {
      "@/*": ["./src/*"]
    },
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,
    "noImplicitOverride": true,
    "noFallthroughCasesInSwitch": true,
    "verbatimModuleSyntax": true,
    "isolatedModules": true,
    "moduleDetection": "force",
    "resolveJsonModule": true,
    "noEmit": true,
    "skipLibCheck": false
  },
  "include": [
    "src/**/*.ts",
    "src/env.d.ts"
  ],
  "exclude": [
    "node_modules",
    "dist",
    "src-tauri"
  ]
}
```

### 3.2. `tsconfig.vue.json` (Aplicação TS + SFCs `.vue` — verificado por `vue-tsc`)

```json
{
  "extends": "./tsconfig.json",
  "compilerOptions": {
    "jsx": "preserve"
  },
  "include": [
    "src/**/*.ts",
    "src/**/*.vue",
    "src/env.d.ts"
  ]
}
```

### 3.3. `tsconfig.node.json` (Arquivos de Configuração — verificado por `tsgo`)

```json
{
  "compilerOptions": {
    "target": "ES2023",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2023"],
    "types": ["node"],
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "exactOptionalPropertyTypes": true,
    "noImplicitOverride": true,
    "noFallthroughCasesInSwitch": true,
    "verbatimModuleSyntax": true,
    "isolatedModules": true,
    "moduleDetection": "force",
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": [
    "vite.config.ts",
    "vitest.config.ts",
    "uno.config.ts"
  ]
}
```

### 3.4. `tsconfig.test.json` (Suíte de Testes — verificado por `tsgo`)

```json
{
  "extends": "./tsconfig.json",
  "compilerOptions": {
    "types": ["vite/client", "vitest/globals", "node"]
  },
  "include": [
    "src/**/*.ts",
    "src/env.d.ts",
    "tests/**/*.ts"
  ]
}
```

### 3.5. `src/env.d.ts`

```ts
/// <reference types="vite/client" />

declare module '*.vue' {
    import type { DefineComponent } from 'vue';
    const component: DefineComponent<Record<string, never>, Record<string, never>, unknown>;
    export default component;
}
```

---

## 4. Configuração Vite, Vitest e UnoCSS

### 4.1. `vite.config.ts`

```ts
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import vue from '@vitejs/plugin-vue';
import UnoCSS from 'unocss/vite';
import { defineConfig } from 'vite';

const workspaceRoot = fileURLToPath(new URL('.', import.meta.url));
const srcDir = path.resolve(workspaceRoot, 'src');

function resolveAllowedFsRoots(): string[] {
    const rawLibsRoot = process.env.MAX_LIBS_ROOT ?? path.join(os.homedir(), 'GitHub');
    const libsRoot = rawLibsRoot.startsWith('~/')
        ? path.join(os.homedir(), rawLibsRoot.slice(2))
        : rawLibsRoot;

    const folders = ['MaxComponentsUi', 'MaxUse', 'MaxPinia'];
    const roots = [workspaceRoot];

    for (const folder of folders) {
        const candidate = path.resolve(libsRoot, folder);
        try {
            roots.push(fs.realpathSync(candidate));
        } catch {
            roots.push(candidate);
        }
    }

    return roots;
}

export default defineConfig({
    clearScreen: false,
    envPrefix: ['VITE_'],
    plugins: [
        vue(),
        UnoCSS()
    ],
    resolve: {
        preserveSymlinks: false,
        alias: {
            '@': srcDir
        },
        dedupe: ['vue', 'pinia']
    },
    optimizeDeps: {
        exclude: [
            '@maxvue/max-components-ui',
            '@maxvue/max-use',
            '@maxvue/max-pinia'
        ]
    },
    css: {
        preprocessorOptions: {
            scss: {
                api: 'modern-compiler',
                additionalData: "@use '@/styles/params' as *;\n"
            }
        }
    },
    server: {
        host: '127.0.0.1',
        port: 5177,
        strictPort: true,
        fs: {
            strict: true,
            allow: resolveAllowedFsRoots()
        }
    },
    build: {
        target: 'es2022',
        sourcemap: true
    }
});
```

### 4.2. `vitest.config.ts`

> **Globs de teste (E-09/D-10):** `include` cobre `tests/**/*.spec.ts` (testes transversais: lint, scripts, componentes de shell) **e** `src/**/__tests__/**/*.spec.ts` (testes colocalizados por domínio). Diretórios como `src/tests/` **não** são cobertos e não devem ser criados; testes pertencem a `tests/` ou a `__tests__/` ao lado do código.

```ts
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import vue from '@vitejs/plugin-vue';
import { defineConfig } from 'vitest/config';

const workspaceRoot = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
    plugins: [vue()],
    resolve: {
        preserveSymlinks: false,
        alias: {
            '@': path.resolve(workspaceRoot, 'src')
        },
        dedupe: ['vue', 'pinia']
    },
    test: {
        globals: true,
        environment: 'happy-dom',
        setupFiles: ['./tests/setup.ts'],
        include: ['tests/**/*.spec.ts', 'src/**/__tests__/**/*.spec.ts'],
        coverage: {
            provider: 'v8',
            reporter: ['text', 'json', 'html'],
            include: [
                'src/ipc/**/*.ts',
                'src/stores/**/*.ts',
                'src/app/logger.ts',
                'src/composables/**/*.ts'
            ],
            exclude: [
                'src/types/bindings.ts',
                'src/**/*.d.ts',
                'src/**/index.ts'
            ],
            thresholds: {
                lines: 80
            }
        }
    }
});
```

### 4.3. `uno.config.ts`

Inspecionar o `package.json#exports` e `.d.ts` de `@maxvue/max-components-ui` após executar `sync:local-libs` e importar `presetMaxUno` pelo subpath real exportado pela biblioteca local:

```ts
import { defineConfig } from 'unocss';
import { presetMaxUno } from '@maxvue/max-components-ui/preset'; // Ajustar para o subpath exato confirmado em MaxComponentsUi/package.json

export default defineConfig({
    presets: [
        presetMaxUno()
    ]
});
```

---

## 5. Linting (`eslint.config.js`, Regra Local e `.stylelintrc.json`)

### 5.1. `eslint/rules/single-line-start-tag.js`

```js
/**
 * Regra ESLint autoral `max/single-line-start-tag`:
 * Exige que toda tag de abertura no `<template>` de um SFC Vue tenha seu nome,
 * atributos, props, diretivas, eventos e fechamento `>` ou `/>` na mesma linha.
 */
export default {
    meta: {
        type: 'layout',
        docs: {
            description: 'Exige que tags de abertura no <template> permaneçam em uma única linha.'
        },
        schema: [],
        messages: {
            multilineStartTag: 'A tag de abertura <{{name}}> deve permanecer inteiramente em uma única linha (AGENTS.md §3).'
        }
    },
    create(context) {
        const parserServices = context.sourceCode?.parserServices ?? context.parserServices;
        if (!parserServices || typeof parserServices.defineTemplateBodyVisitor !== 'function') {
            return {};
        }

        return parserServices.defineTemplateBodyVisitor({
            VElement(node) {
                const { startTag } = node;
                if (!startTag || !startTag.loc) {
                    return;
                }
                if (startTag.loc.start.line !== startTag.loc.end.line) {
                    context.report({
                        node: startTag,
                        loc: startTag.loc,
                        messageId: 'multilineStartTag',
                        data: {
                            name: node.rawName || node.name
                        }
                    });
                }
            }
        });
    }
};
```

### 5.2. `eslint.config.js`

```js
import stylistic from '@stylistic/eslint-plugin';
import pluginVue from 'eslint-plugin-vue';
import globals from 'globals';
import tseslint from 'typescript-eslint';
import vueParser from 'vue-eslint-parser';
import singleLineStartTag from './eslint/rules/single-line-start-tag.js';

export default [
    {
        ignores: [
            'dist/**',
            'node_modules/**',
            'coverage/**',
            'src-tauri/target/**',
            'src-tauri/gen/**',
            'src/types/bindings.ts',
            '.worktrees/**',
            '.max-code-worktrees/**',
            'tests/lint/fixtures/invalid-*.vue',
            'tests/lint/fixtures/invalid-*.ts'
        ]
    },
    ...tseslint.configs.recommended,
    ...pluginVue.configs['flat/recommended'],
    {
        files: ['**/*.{js,mjs,ts,vue}'],
        plugins: {
            '@stylistic': stylistic,
            max: {
                rules: {
                    'single-line-start-tag': singleLineStartTag
                }
            }
        },
        languageOptions: {
            ecmaVersion: 'latest',
            sourceType: 'module',
            globals: {
                ...globals.browser,
                ...globals.node
            }
        },
        rules: {
            '@stylistic/indent': ['error', 4, { SwitchCase: 1 }],
            '@stylistic/quotes': ['error', 'single', { avoidEscape: true }],
            '@stylistic/semi': ['error', 'always'],
            '@stylistic/comma-dangle': ['error', 'never'],
            '@typescript-eslint/no-unused-vars': [
                'error',
                {
                    argsIgnorePattern: '^_',
                    varsIgnorePattern: '^_'
                }
            ],
            '@typescript-eslint/consistent-type-imports': 'error',
            '@typescript-eslint/no-explicit-any': 'error'
        }
    },
    {
        files: ['**/*.vue'],
        languageOptions: {
            parser: vueParser,
            parserOptions: {
                parser: tseslint.parser,
                extraFileExtensions: ['.vue'],
                sourceType: 'module'
            }
        },
        rules: {
            '@stylistic/indent': 'off',
            'vue/block-order': [
                'error',
                {
                    order: ['template', 'script[setup]', 'style[scoped]']
                }
            ],
            'vue/component-api-style': ['error', ['script-setup']],
            'vue/block-lang': [
                'error',
                {
                    script: { lang: 'ts' },
                    style: { lang: 'scss' }
                }
            ],
            'vue/enforce-style-attribute': ['error', { allow: ['scoped'] }],
            'vue/html-indent': ['error', 4],
            'vue/script-indent': [
                'error',
                4,
                {
                    baseIndent: 1,
                    switchCase: 1
                }
            ],
            'vue/html-quotes': ['error', 'double'],
            'vue/max-attributes-per-line': 'off',
            'vue/singleline-html-element-content-newline': 'off',
            'vue/multiline-html-element-content-newline': 'off',
            'vue/first-attribute-linebreak': [
                'error',
                {
                    singleline: 'beside',
                    multiline: 'beside'
                }
            ],
            'vue/html-closing-bracket-newline': [
                'error',
                {
                    singleline: 'never',
                    multiline: 'never'
                }
            ],
            'vue/define-props-declaration': ['error', 'type-based'],
            'vue/define-emits-declaration': ['error', 'type-literal'],
            'max/single-line-start-tag': 'error'
        }
    }
];
```

### 5.3. `.stylelintrc.json`

```json
{
  "extends": [
    "stylelint-config-standard-scss",
    "stylelint-config-recommended-vue/scss"
  ],
  "rules": {
    "selector-pseudo-class-no-unknown": [
      true,
      {
        "ignorePseudoClasses": ["deep", "global", "slotted"]
      }
    ],
    "selector-pseudo-element-no-unknown": [
      true,
      {
        "ignorePseudoElements": ["v-deep"]
      }
    ],
    "scss/at-rule-no-unknown": [
      true,
      {
        "ignoreAtRules": ["unocss"]
      }
    ],
    "custom-property-pattern": "^(max|background|mt)-[a-z0-9-]+$"
  }
}
```

---

## 6. Scripts `scripts/sync-local-libs.mjs` e `scripts/check.mjs`

### 6.1. `scripts/sync-local-libs.mjs`

Contrato e assinatura exportável para testes:
- Exporta `syncLocalLibs(options?: { workspaceRoot?: string; libsRoot?: string; checkOnly?: boolean; logger?: (line: string) => void }): { ok: boolean; results: Array<{ pkg: string; target?: string; error?: string }> }`.
- Regras estritas:
  1. Resolve `libsRoot = options?.libsRoot ?? process.env.MAX_LIBS_ROOT ?? path.join(os.homedir(), 'GitHub')` (expande `~` inicial se presente; falha se não for caminho absoluto).
  2. Lê `package.json#maxLocalLibs` da worktree; aceita exclusivamente `@maxvue/max-components-ui`, `@maxvue/max-use` e `@maxvue/max-pinia`.
  3. **Fase de validação (100% leitura antes de qualquer escrita):**
     - Verifica que `srcDir = path.join(libsRoot, folder)` existe, resolve `realpathSync(srcDir)` e confirma que `package.json` interno tem `"name" === pkg` e `"version"` preenchida.
     - Percorre recursivamente no campo `exports` (ou `main`/`module`/`types`) do `package.json` da biblioteca local e verifica com `fs.existsSync` que todos os artefatos apontados em `dist/` existem e são arquivos não vazios.
     - Verifica que todas as `peerDependencies` obrigatórias da biblioteca local estão instaladas/resolvíveis no `node_modules` da worktree.
     - Se `checkOnly === true`, verifica ainda se `node_modules/<pkg>` já é um symlink simbólico cujo `fs.readlinkSync` é um caminho absoluto idêntico a `realSrcDir` e nunca cria diretórios ou links.
  4. **Fase de aplicação (somente se `checkOnly === false` e todas as 3 origens passaram na validação):**
     - Garante `node_modules/@maxvue` dentro da worktree.
     - Se `dest` existir:
       - Usa `fs.lstatSync(dest)` (sem seguir symlink).
       - Se for `isSymbolicLink()` e `fs.readlinkSync(dest) === realSrcDir`, preserva o link sem alterá-lo (idempotência).
       - Se for `isSymbolicLink()` apontando para outro lugar, cria um symlink temporário `node_modules/@maxvue/.tmp-<nome>-<pid>` para `realSrcDir` e faz `fs.renameSync` atômico sobre `dest`.
       - Se for diretório real ou arquivo regular, **recusa** sobrescrever e falha com mensagem explicativa em pt-BR (`"Destino já existe como diretório/arquivo real; remova manualmente ou execute npm ci"`).
  5. Imprime em pt-BR: `✔ @maxvue/max-use → /caminho/absoluto/MaxUse` ou `✖ @maxvue/max-use (motivo em pt-BR)` e encerra com código `1` se `ok === false`.

### 6.2. `scripts/check.mjs`

Executa sequencialmente via `child_process.spawnSync` com `stdio: 'inherit'`, interrompendo no primeiro status diferente de `0`:
1. `node scripts/sync-local-libs.mjs --check` (`cwd`: raiz da worktree)
2. `npm run lint` (`cwd`: raiz da worktree)
3. `npm run lint:style` (`cwd`: raiz da worktree)
4. `npm run typecheck` (`cwd`: raiz da worktree)
5. `npm run typecheck:vue` (`cwd`: raiz da worktree)
6. `npm run test:coverage` (`cwd`: raiz da worktree)
7. `npm run build` (`cwd`: raiz da worktree)
8. `cargo fmt --all --check` (`cwd`: `<worktree>/src-tauri`)
9. `cargo clippy --locked --workspace --all-targets -- -D warnings` (`cwd`: `<worktree>/src-tauri`)
10. `cargo test --workspace --locked` (`cwd`: `<worktree>/src-tauri`)

---

## 7. Testes de Qualidade e Tooling (`tests/`)

### 7.1. `tests/setup.ts`

```ts
import { clearMocks } from '@tauri-apps/api/mocks';
import { afterEach } from 'vitest';

afterEach(() => {
    clearMocks();
});
```

### 7.2. `tests/lint/eslint-rules.spec.ts` e Fixtures (`tests/lint/fixtures/`)

Usa a classe `ESLint` da API programática (`overrideConfigFile: path.resolve('eslint.config.js')`, `ignore: false`) para inspecionar:
- `tests/lint/fixtures/valid-component.vue`: SFC válido com `<template>`, `<script setup lang="ts">`, `<style lang="scss" scoped>` e tags em linha única → **0 erros, 0 warnings**.
- `tests/lint/fixtures/invalid-block-order.vue`: `<script setup lang="ts">` antes de `<template>` → reporta `vue/block-order`.
- `tests/lint/fixtures/invalid-multiline-tag.vue`: tag `<div\n class="x">` quebrada em 2 linhas → reporta `max/single-line-start-tag`.
- `tests/lint/fixtures/invalid-comma-quotes.vue`: objeto com vírgula final e string TS com aspas duplas → reporta `@stylistic/comma-dangle` e `@stylistic/quotes`.

### 7.3. `tests/scripts/sync-local-libs.spec.ts`

Testa `syncLocalLibs` contra diretórios temporários criados dentro da worktree:
- Cria links simbólicos absolutos para as 3 libs quando válidas; segunda execução seguida é idempotente.
- `checkOnly: true` falha quando link está ausente ou incorreto e **não cria nem remove** nenhum arquivo/pasta.
- Falha sem alterar nada quando um export declarado no `package.json` da lib não existe em `dist/` ou quando o destino em `node_modules/@maxvue/*` é um diretório real.
