# Implementação 03 — Tooling Frontend, Sync de Bibliotecas Locais e Qualidade (`plan.md`)

> Especificação técnica desta etapa: [`./spec.md`](./spec.md)
> Fase: [`../instructions.md`](../instructions.md) | [`../spec.md`](../spec.md)

---

## 1. Pré-requisitos e Regras Operacionais

- As bibliotecas locais (`MaxComponentsUi`, `MaxUse`, `MaxPinia`) devem estar disponíveis e construídas em `$MAX_LIBS_ROOT` (ou `~/GitHub`).
- Nenhum script desta fase pode modificar, instalar ou compilar arquivos dentro dos repositórios externos em `$MAX_LIBS_ROOT`.
- O pin de `vue` deve permanecer estritamente em `3.6.0-rc.13`.

---

## 2. Checklist Passo a Passo de Execução

### Passo 1 — Inspecionar Bibliotecas Locais e Criar `package.json` + `scripts/sync-local-libs.mjs`
- [ ] **1.1.** Inspecionar os arquivos `package.json` de `$MAX_LIBS_ROOT/MaxComponentsUi`, `$MAX_LIBS_ROOT/MaxUse` e `$MAX_LIBS_ROOT/MaxPinia` (somente leitura) para confirmar `peerDependencies` e o subpath exato do `presetMaxUno`.
- [ ] **1.2.** Criar `package.json` na raiz da worktree com `vue: "3.6.0-rc.13"`, `splitpanes: "4.1.2"` (sem `@types/splitpanes`), `@vitejs/plugin-vue: "6.0.10"`, `vitest`/`@vitest/coverage-v8: "5.0.5"`, `maxLocalLibs` e todos os scripts (`dev`, `build`, `preview`, `tauri`, `typecheck`, `typecheck:vue`, `lint`, `lint:fix`, `lint:style`, `test`, `test:watch`, `test:coverage`, `sync:local-libs`, `bindings`, `check`); `lint:style` com `stylelint --allow-empty-input`. Nomes canônicos únicos (sem aliases de `typecheck`) e gerenciador `npm`/`npx --no-install`.
- [ ] **1.3.** Implementar `scripts/sync-local-libs.mjs` com validação prévia integral (manifests, exports, artefatos `dist/`, peers), criação de symlinks absolutos, recusa segura diante de diretórios reais e modo `--check` somente leitura.
- [ ] **1.4.** Executar instalação npm para gerar `package-lock.json` e em seguida `npm run sync:local-libs` e `npm run sync:local-libs -- --check`.

### Passo 2 — Configuração TypeScript, Vite, Vitest e UnoCSS
- [ ] **2.1.** Criar `tsconfig.json`, `tsconfig.vue.json`, `tsconfig.node.json`, `tsconfig.test.json` e `src/env.d.ts` com todas as flags estritas (`strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noImplicitOverride`, `noFallthroughCasesInSwitch`, `verbatimModuleSyntax`).
- [ ] **2.2.** Criar `vite.config.ts` (porta `5177`, `host: '127.0.0.1'`, `strictPort: true`, `preserveSymlinks: false`, `dedupe: ['vue', 'pinia']`, allowlist restrita de `server.fs.allow` e `additionalData` SCSS).
- [ ] **2.3.** Criar `vitest.config.ts` (`happy-dom`, `include: ['tests/**/*.spec.ts', 'src/**/__tests__/**/*.spec.ts']`, `setupFiles: ['./tests/setup.ts']`, `coverage` v8 com limiar `lines: 80`) e `tests/setup.ts` (`clearMocks()` em `afterEach`).
- [ ] **2.4.** Criar `uno.config.ts` isolando o import de `presetMaxUno` a partir do subpath real de `@maxvue/max-components-ui`.

### Passo 3 — ESLint Flat Config, Regra Local, Stylelint e `scripts/check.mjs`
- [ ] **3.1.** Criar `eslint/rules/single-line-start-tag.js` utilizando `defineTemplateBodyVisitor` para inspecionar `VElement.startTag.loc`.
- [ ] **3.2.** Criar `eslint.config.js` configurando todas as regras obrigatórias (`vue/block-order`, `vue/component-api-style`, `vue/block-lang`, `vue/html-indent`, `vue/script-indent` com `baseIndent: 1`, `max/single-line-start-tag`, `@stylistic/*` e `@typescript-eslint/*`).
- [ ] **3.3.** Criar `.stylelintrc.json` com `stylelint-config-standard-scss`, `stylelint-config-recommended-vue/scss` e `custom-property-pattern: "^(max|background|mt)-[a-z0-9-]+$"`.
- [ ] **3.4.** Criar `scripts/check.mjs` encadeando `sync:local-libs --check`, `lint`, `lint:style`, `typecheck`, `typecheck:vue`, `test:coverage`, `build` e os 3 checks Cargo em `src-tauri/`.

### Passo 4 — Testes Automatizados de Lint e de `sync-local-libs`
- [ ] **4.1.** Criar as fixtures em `tests/lint/fixtures/` (`valid-component.vue`, `invalid-block-order.vue`, `invalid-multiline-tag.vue`, `invalid-comma-quotes.vue`) e o teste `tests/lint/eslint-rules.spec.ts`.
- [ ] **4.2.** Criar `tests/scripts/sync-local-libs.spec.ts` testando criação de symlinks absolutos, idempotência, falha e imutabilidade em `--check` com raiz inexistente, e rejeição de diretório real ou export ausente.

---

## 3. Comandos de Validação e Mapeamento para Critérios de Aceite

| Critério | Comando de Verificação | Resultado Esperado |
|----------|------------------------|--------------------|
| **CA-01** (Instalação limpa) | `npm ci` | Exit code `0` sem erros de peer dependency. |
| **CA-02** (Symlinks absolutos) | `npm run sync:local-libs && readlink -f node_modules/@maxvue/max-use` | Imprime o caminho absoluto de `$MAX_LIBS_ROOT/MaxUse` e retorna `0`. |
| **CA-03** (`--check` somente leitura) | `MAX_LIBS_ROOT=/caminho/inexistente npm run sync:local-libs -- --check` | Exit code `!= 0` sem criar ou remover arquivos. |
| **CA-04** (Idempotência do sync) | `npm run sync:local-libs && npm run sync:local-libs` | Ambas as execuções retornam `0` preservando os symlinks. |
| **CA-05** (ESLint zero warnings) | `npm run lint` | Exit code `0` (`--max-warnings 0`). |
| **CA-06** (Regras ESLint e fixtures) | `npx --no-install vitest run tests/lint/eslint-rules.spec.ts` | Detecta `vue/block-order`, `max/single-line-start-tag`, `@stylistic/comma-dangle` e `@stylistic/quotes`. |
| **CA-07** (Stylelint) | `npm run lint:style` | Exit code `0`, inclusive sem nenhum `.scss`/`.vue` em `src/` (`--allow-empty-input`); a verificação com arquivos reais ocorre em `01/04`. |
| **CA-08** (`tsgo` + `vue-tsc`) | `npm run typecheck && npm run typecheck:vue` | Exit code `0` nos 4 projetos TypeScript (`typecheck:vue` é válido sem `.vue` porque `tsconfig.vue.json` inclui `src/env.d.ts`; SFCs reais são verificados em `01/04`). |
| **CA-09** (Opções estritas no `tsconfig.json`) | `node -e "const c=JSON.parse(require('fs').readFileSync('tsconfig.json','utf8')).compilerOptions; for(const k of ['strict','noUncheckedIndexedAccess','exactOptionalPropertyTypes','noImplicitOverride','verbatimModuleSyntax']) if(!c[k]) process.exit(1);"` | Exit code `0`. |
