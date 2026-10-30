# Fase 01 — Setup e Fundação do Projeto (`instructions.md`)

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) e contratos compartilhados de [`../README.md`](../README.md) (§2.1–§2.6).
> Especificação técnica detalhada: [`./spec.md`](./spec.md).

> **Natureza desta entrega:** plano técnico de implementação, não comprovação de software já implementado.
> Os arquivos e comandos abaixo são entregáveis e verificações da execução futura da fase. APIs externas
> pendentes de confirmação e fontes consultadas estão identificadas em `spec.md` §14.

---

## 1. Objetivo da Fase

Entregar um esqueleto **compilável, testável, lintado e executável** do MaxTerminal que fixe, de uma vez por todas,
a stack, as convenções de código e os contratos de base sobre os quais as Fases 02–17 serão construídas:

- Workspace Cargo (`src-tauri/`) com o pacote `maxterminal` (binário + biblioteca) e `mt-core` funcional. Todos os crates (desta e das demais fases) usam `edition.workspace = true`, e cada domínio registra seu próprio estado via `app.manage(...)`: o `AppState` desta fase não agrega serviços de outras fases (D-09).
  Os demais crates do README são criados pelas fases donas; o mapa final não exige stubs nesta fase.
- Frontend Vue `3.6.0-rc.13` + TypeScript estrito (`tsgo`) + Vite 8 + UnoCSS (`presetMaxUno`) + SCSS, com as libs
  `@maxvue/*` vinculadas localmente via `sync:local-libs`.
- Pipeline de qualidade: ESLint (flat config), Stylelint, Vitest, `cargo fmt`, `clippy -D warnings`, `cargo test`.
- Geração automática de bindings TypeScript via `tauri-specta` (`src/types/bindings.ts`), com `MtError` e os newtypes
  de ids definidos em `mt-core`.
- Logging estruturado (`tracing`) com arquivo rotativo em caminho XDG e resolução centralizada de caminhos XDG.
- Janela principal com o **shell visual do app** (`App.vue` + `AppShell.vue`) vazio, no tema padrão **Max Navy/Teal**.

Ao final desta fase, um checkout em worktree, com as bibliotecas locais preparadas e versões registradas,
deve permitir `npm ci`, `npm run sync:local-libs`, `npm run bindings` e `npm run tauri dev`, nessa ordem,
para abrir o shell vazio. A instalação npm isolada não fornece os repositórios locais nem seus artefatos.

---

## 2. Escopo

### 2.1. Incluído

| # | Item | Detalhe |
|---|------|---------|
| I-01 | Workspace Cargo | `src-tauri/Cargo.toml` com `[workspace]`, `[workspace.package]`, `[workspace.dependencies]` e `[workspace.lints]` compartilhados. |
| I-02 | Crate binário `maxterminal` | `main.rs`, `lib.rs` (`run()`), `commands/`, `events/`, `logging.rs`, `specta.rs`, `state.rs`. |
| I-03 | Crate `mt-core` | Módulos `ids` (9 newtypes Uuid v7, incluindo `TabSessionId`), `error` (`MtError`, `ErrorCode`, `MtResult`), `paths` (`AppPaths` XDG), `app` (`AppInfo`). |
| I-04 | Limite do workspace | Apenas `maxterminal` e `mt-core` como membros explícitos; nenhum crate de fase posterior é necessário para compilar ou testar a fundação. |
| I-05 | Comandos IPC de fundação | `app_info` e `app_log_frontend` (ver spec §4). |
| I-06 | `tauri-specta` | Registro central de comandos/tipos e geração explícita de `src/types/bindings.ts`, sem iniciar WebView; teste de drift sem sobrescrever a referência. |
| I-07 | Logging | `tracing` + writer não bloqueante e rotação diária em `$XDG_STATE_HOME/maxterminal/logs/`; somente eventos/valores permitidos. Mensagens arbitrárias, stacks e causas brutas não são persistidas antes do sanitizador. |
| I-08 | Paths XDG | `AppPaths::resolve()` com fallbacks da especificação XDG; criação dos diretórios (`config`, `data`, `state`, `cache`, `logs`) no startup. O caminho do banco é **apenas exposto** (`db_path`), não criado. |
| I-09 | Frontend base | `src/app/{main.ts,App.vue,logger.ts,plugins.ts}`, `src/ipc/{core.ts,app.ts,index.ts}`, `src/types/{bindings.ts,errors.ts,index.ts}`, `src/stores/app.ts`, `src/composables/useIpcError.ts`, `src/components/shell/{AppShell.vue,AppTitleBar.vue,AppEmptyState.vue}`, `src/styles/{main.scss,theme.scss,params.scss}`. Não há necessidade de router com uma única tela. |
| I-10 | Tema padrão Max Navy/Teal | Tokens `--max-*` e `--background-*` vindos do `@maxvue/max-components-ui`, com overrides mínimos em `src/styles/theme.scss` e `color-scheme: dark`. |
| I-11 | `sync:local-libs` | `scripts/sync-local-libs.mjs` valida manifests, exports, artefatos e peers das três libs; cria symlinks absolutos somente no `node_modules` da worktree. Não instala nem constrói os repositórios externos. |
| I-12 | Lint/format | `eslint.config.js` (flat), `.stylelintrc.json`, `.editorconfig`, `rustfmt.toml`, `rust-toolchain.toml`. |
| I-13 | Testes | Vitest (`happy-dom` + `@vue/test-utils` + `@tauri-apps/api/mocks`) e `cargo test --workspace`. |
| I-14 | Tauri config | Janela `main`, CSP de produção e desenvolvimento distintas; capability local com apenas os dois comandos da fundação, ACL de comandos do app habilitada no build; nenhuma permissão de plugin. |
| I-15 | `.gitignore` | Inclui `.worktrees/`, `.max-code-worktrees/`, `node_modules/`, `dist/`, `src-tauri/target/`, `src-tauri/gen/` (exceto schemas), `*.log`, `coverage/`. |
| I-16 | Scripts npm | `dev`, `build`, `preview`, `tauri`, `typecheck`, `typecheck:vue`, `lint`, `lint:fix`, `lint:style`, `test`, `test:watch`, `test:coverage`, `sync:local-libs`, `bindings`, `check` (agregador). Nomes canônicos únicos (sem aliases); gerenciador `npm`/`npx --no-install`. |
| I-17 | Dependências canônicas (D-06/D-10/E-10) | Frontend: `@vitejs/plugin-vue` `6.0.10`, `vitest` e `@vitest/coverage-v8` `5.0.5`, `splitpanes` `4.1.2` (sem `@types/splitpanes`). Cargo: `[workspace.dependencies]` com `notify = "8"` e `notify-debouncer-full = "0.5"` (consumidos por `workspace = true`); `src-tauri/Cargo.toml` com `default-run = "maxterminal"`; binário auxiliar `export_bindings`. Vitest inclui `tests/**/*.spec.ts` e `src/**/__tests__/**/*.spec.ts`. |

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| Spawn/gerenciamento de PTY (`portable-pty`), leitura de `/proc`, Channels de output | 02 |
| SQLite (WAL, FTS5), migrations, repositórios, `mt_db::sanitizer::sanitize()` e ampliação de logs para texto livre sanitizado | 03 |
| `@xterm/xterm` e addons, camada de blocos, input rico e registro de gatilhos | 04 |
| OSC 133, hooks de shell, `TabStatus` e máquina de estados | 05 |
| Lista de projetos, `fs::watch`, auto-detecção de stack, badges, layout **`splitpanes` das 2 colunas** (a dependência npm `splitpanes` `4.1.2` já é instalada pela Fase 01) | 06 |
| Abas, templates, split panes internos, sessões | 07 |
| Git/worktree e detector de portas | 08 |
| System tray, atalho global (Quake), links clicáveis para editor, notificações nativas | 09 |
| Importador de temas Tabby e troca dinâmica de tema | 10 |
| Loopback HTTP/WS e URI schemes (`maxterminal://`) com MaxCode | 11 |
| MaxAiManager (`@`/`?`), diagnóstico inline, RAG | 12 |
| Agentes CLI, telemetria, MaxSkills | 13 |
| Artisan (`>`), atalhos `/` | 14 |
| Grupos de comandos (`>>`) | 15 |
| Aba de GitHub Issues | 16 |
| Testes E2E, benchmarks, empacotamento (PKGBUILD/AppImage), busca global FTS5 | 17 |

---

## 3. Pré-requisitos

### 3.1. Fases
- Nenhuma (fase raiz do DAG).

### 3.2. Ferramentas (máquina CachyOS)
| Ferramenta | Versão mínima | Verificação |
|------------|---------------|-------------|
| Rust (via `rustup`) | stable ≥ 1.85 (edição 2024) | `rustc --version` |
| Componentes Rust | `rustfmt`, `clippy` | `rustup component list --installed` |
| Node.js | 22.12+ da linha 22, ou LTS posterior suportada; fixar patch validado | `node --version`; mínimo documentado pelo Vite 8 |
| npm | ≥ 10 | `npm --version` |
| Dependências de sistema Tauri v2 | WebKitGTK 4.1, GTK3, OpenSSL, librsvg, toolchain C e pkg-config; nomes de pacotes CachyOS/Arch a confirmar na instalação | Registrar versões de WebKitGTK/GTK e diagnóstico do Tauri; AppIndicator somente se realmente exigido pelo build sem tray |
| `@tauri-apps/cli` | 2.x (devDependency) | `npx --no-install tauri --version` |
| `tsgo` | `@typescript/native-preview` (devDependency) | `npx --no-install tsgo --version` |
| Repositórios locais | `~/GitHub/MaxComponentsUi`, `~/GitHub/MaxUse`, `~/GitHub/MaxPinia`, ou `MAX_LIBS_ROOT`, com exports construídos e peers instalados | `npm run sync:local-libs -- --check`; registrar commit, versão e estado dirty de cada origem |

Rust 1.85 é o piso da edição 2024, não garantia de MSRV das dependências resolvidas. Fixar um toolchain
compatível com o `Cargo.lock` real, com `rustfmt`/`clippy`, e executar comandos Cargo em `src-tauri/`
para que `rust-toolchain.toml` seja respeitado. Nenhuma API dos pacotes privados foi confirmada nesta revisão.

---

## 4. Entregáveis

### 4.1. Raiz do repositório
- `package.json`, `package-lock.json`
- `tsconfig.json`, `tsconfig.node.json`, `tsconfig.vue.json`, `tsconfig.test.json`
- `vite.config.ts`, `vitest.config.ts`, `uno.config.ts`
- `eslint.config.js`, `.stylelintrc.json`, `.editorconfig`
- `index.html`
- `.gitignore`
- `scripts/sync-local-libs.mjs`, `scripts/check.mjs` (agregador com diretório Cargo explícito)
- `eslint/rules/single-line-start-tag.js` (regra local que exige tags de abertura em linha única)

### 4.2. Backend (`src-tauri/`)
- `Cargo.toml` (workspace + pacote `maxterminal`), `Cargo.lock`, `build.rs`, `rust-toolchain.toml`, `rustfmt.toml`
- `tauri.conf.json`, `capabilities/default.json`, `permissions/` (ACL dos comandos próprios), `icons/` (recursos mínimos necessários ao build)
- `src/main.rs`, `src/lib.rs`, `src/state.rs`, `src/logging.rs`, `src/specta.rs`
- `src/commands/mod.rs`, `src/commands/app.rs`
- `src/events/mod.rs`
- `tests/bindings.rs` (teste de drift dos bindings)
- `crates/mt-core/{Cargo.toml,src/lib.rs,src/ids.rs,src/error.rs,src/paths.rs,src/app.rs}`

### 4.3. Frontend (`src/`)
- `app/main.ts`, `app/App.vue`, `app/logger.ts`, `app/plugins.ts`
- `ipc/core.ts`, `ipc/app.ts`, `ipc/index.ts`
- `types/bindings.ts` (gerado — commitado), `types/errors.ts`, `types/index.ts`
- `stores/app.ts`
- `composables/useIpcError.ts`
- `components/shell/AppShell.vue`, `components/shell/AppTitleBar.vue`, `components/shell/AppEmptyState.vue`
- `styles/main.scss`, `styles/theme.scss`, `styles/params.scss`
- `env.d.ts`
- `tests/` → `ipc/core.spec.ts`, `stores/app.spec.ts`, `components/shell/AppShell.spec.ts`, `app/logger.spec.ts`, `lint/eslint-rules.spec.ts` + `lint/fixtures/` (na pasta `tests/` da raiz)

---

## 5. Critérios de Aceite

Comandos npm rodam na raiz da worktree e comandos Cargo em `src-tauri/`. Todos devem terminar com
**exit code 0**, salvo cenários negativos explicitamente indicados.

### 5.1. Instalação e links locais
- [ ] CA-01 `npm ci` conclui sem erros.
- [ ] CA-02 `npm run sync:local-libs` cria `node_modules/@maxvue/max-components-ui`, `max-use` e `max-pinia` como **symlinks absolutos**; `readlink -f node_modules/@maxvue/max-use` aponta para `$MAX_LIBS_ROOT/MaxUse`.
- [ ] CA-03 `--check` é somente leitura e retorna exit ≠ 0 em origem, export, peer ou link inválido; fixture com raiz inexistente dentro da worktree comprova falha sem criar/remover arquivos nas origens.
- [ ] CA-04 Executar `sync:local-libs` duas vezes seguidas é idempotente (sem erro e sem alterar links já corretos).

### 5.2. Qualidade frontend
- [ ] CA-05 `npm run lint` passa com zero erros e zero warnings (`--max-warnings 0`).
- [ ] CA-06 Um SFC de teste com ordem `<script>` antes de `<template>` gera erro `vue/block-order`; uma tag com atributos quebrados em várias linhas gera erro; vírgula final gera erro `@stylistic/comma-dangle`; aspas duplas geram erro `@stylistic/quotes`. (Verificado por fixture em `tests/lint/` executada pelo teste `eslint-rules.spec.ts` usando a API `ESLint`.)
- [ ] CA-07 `npm run lint:style` passa (Stylelint em `src/**/*.{scss,vue}`).
- [ ] CA-08 `npm run typecheck` executa `tsgo` nos projetos TS de aplicação, configuração Node e testes; `npm run typecheck:vue` executa `vue-tsc` no projeto SFC. Fixtures de erro TS e de prop no template falham no verificador correspondente.
- [ ] CA-09 `tsconfig.json` contém `strict`, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noImplicitOverride`, `verbatimModuleSyntax`.
- [ ] CA-10 `npm run test` passa; `npm run test:coverage` verifica ≥ 80% de linhas nos wrappers, store e logger autorais (exclui bindings gerados). O agregador executa a modalidade com cobertura uma vez.

### 5.3. Qualidade backend
- [ ] CA-11 `cargo fmt --all --check` (em `src-tauri/`) passa.
- [ ] CA-12 `cargo clippy --locked --workspace --all-targets -- -D warnings` passa.
- [ ] CA-13 `cargo test --workspace --locked` passa, incluindo: roundtrip e rejeição de UUIDs não-v7 em parsing/serde dos nove ids, ordenação entre timestamps distintos, erro IPC, resolução XDG via ambiente injetado, logs permitidos e teste de drift.
- [ ] CA-14 `cargo metadata --format-version 1 --no-deps` lista dois membros: `maxterminal` e `mt-core`; nenhum depende de crate de fase futura.
- [ ] CA-15 Nenhum crate contém `unsafe` (`[workspace.lints.rust] unsafe_code = "forbid"`).

### 5.4. Bindings
- [ ] CA-16 Geração explícita funciona sem servidor Vite, DISPLAY ou banco; duas execuções geram bytes idênticos. Teste de drift compara em destino temporário da worktree sem reescrever o arquivo versionado; mutação proposital da referência é detectada. `git diff --exit-code -- src/types/bindings.ts` complementa a verificação após a versão inicial estar registrada.
- [ ] CA-17 `bindings.ts` exporta wrappers dos dois comandos e tipos `AppInfo`, `AppPathsDto`, `BuildProfile`, `MtError`, `ErrorCode`, `FrontendLogEvent`, `ProjectId`, `TabId`, `PaneId`, `PtyId`, `SessionId`, `TabSessionId`, `TemplateId`, `CommandBlockId`, `CommandGroupId` (ids como `string`); nomes exatos dos wrappers são verificados contra a versão fixada do gerador.
- [ ] CA-18 Nenhum tipo espelhado de struct Rust é escrito à mão em `src/types/` (apenas re-exports e utilitários derivados).

### 5.5. Comportamento observável
- [ ] CA-19 `npm run tauri dev` abre uma janela com título **"MaxTerminal"**, tamanho inicial 1440×900, mínimo 960×600.
- [ ] CA-20 A janela exibe `AppTitleBar` (nome + versão obtida via `app_info`) e `AppEmptyState` com o texto "Nenhum projeto selecionado" sobre fundo Navy (`--background-*`) e acentos Teal (`--max-primary-*`).
- [ ] CA-21 Ao iniciar, são criados `~/.config/maxterminal/`, `~/.local/share/maxterminal/`, `~/.local/state/maxterminal/logs/` e `~/.cache/maxterminal/` (ou os equivalentes de `XDG_*_HOME`); **nenhum** arquivo `.db` é criado.
- [ ] CA-22 O arquivo `~/.local/state/maxterminal/logs/maxterminal.log.<AAAA-MM-DD>` contém a linha `MaxTerminal iniciado` com versão; `MAXTERMINAL_LOG=debug npm run tauri dev` aumenta a verbosidade.
- [ ] CA-23 Um erro Vue gera evento estático `vue_error` no log com `target=frontend`, nível `ERROR`; mensagem original, stack e valores sentinela de segredo não aparecem em arquivo ou stderr. IPC com evento desconhecido é rejeitado, fila e taxa são limitadas.
- [ ] CA-24 Se `app_info` falhar (simulado em teste com `mockIPC`), a UI exibe estado de erro em pt-BR com o `code` do `MtError`, sem tela branca.
- [ ] CA-25 `npm run tauri build -- --no-bundle` conclui e produz o binário no diretório Cargo efetivo; executar esse binário sem Vite também mostra versão/estado vazio. Registrar versão de WebKitGTK, caminho do artefato e resultado do smoke nativo; servidor Vite sozinho não comprova IPC desktop.

### 5.6. Repositório
- [ ] CA-26 `git check-ignore .worktrees/x .max-code-worktrees/x node_modules src-tauri/target` retorna as 4 entradas.
- [ ] CA-27 Todos os comentários de código, textos de UI e mensagens `MtError.message` estão em pt-BR; identificadores, arquivos e pastas em inglês.
- [ ] CA-28 `npm run check` valida links, lint, lint:style, ambos os typechecks, testes com cobertura, build frontend e Cargo (fmt/clippy/test com lockfile) e retorna 0, interrompendo no primeiro erro. Não instala dependências, não regenera bindings nem inicia GUI.
- [ ] CA-29 Capability vinculada apenas à janela `main`; allowlist contém somente os dois comandos de fundação. Invocação autorizada passa; janela não coberta e comando fora da allowlist são negados em teste do ACL, sem adicionar comandos de diagnóstico ao release.
- [ ] CA-30 Dev usa exatamente `http://127.0.0.1:5177`, porta estrita; HMR funciona com CSP de desenvolvimento. Release não permite origem do dev server, scripts remotos, `unsafe-eval`, permissões `core:default` ou plugins desnecessários.

---

## 6. Riscos e Mitigação

| # | Risco | Impacto | Mitigação |
|---|-------|---------|-----------|
| R-01 | Vue `3.6.0-rc.13` (release candidate) incompatível com plugins/peers. | Alto | Pin exato; inspecionar árvore resolvida e compiler/runtime; deduplicar Vue/Pinia; validar montagem com bibliotecas reais. Não ocultar conflitos com `--force`/`legacy-peer-deps`. |
| R-02 | `tsgo` ainda não verifica `<template>` de SFCs `.vue`. | Médio | `typecheck` (tsgo) cobre `.ts`; `typecheck:vue` (`vue-tsc`) complementa SFCs; ambos obrigatórios no `check`. |
| R-03 | `tauri-specta`/`specta` em RC e atributos incompatíveis. | Médio | Partir de `specta`/`tauri-specta =2.0.0-rc.27` e `specta-typescript =0.0.14`, documentados oficialmente; não misturar RCs. Validar features, ids/erro e drift antes de avançar. |
| R-04 | Symlinks duplicarem Vue/Pinia ou exporem arquivos extras ao dev server. | Alto | Dedupe de Vue/Pinia; resolução real dos links; allowlist de filesystem limitada à worktree e às três origens concretas, nunca todo `~/GitHub`. |
| R-05 | Worktrees em caminhos diferentes quebram links relativos (`file:../`). | Médio | `sync:local-libs` usa **caminhos absolutos** resolvidos de `MAX_LIBS_ROOT`; nada de `file:` relativo no `package.json`. |
| R-06 | `npm ci` remover symlinks locais. | Médio | Sincronização explícita após instalação, sem postinstall com efeitos externos. CI provisiona as três origens nas revisões registradas antes dos checks; ausência é bloqueio real, não sucesso parcial. |
| R-07 | Export path do `presetMaxUno` diferente do esperado. | Baixo | Inspecionar `exports` do `package.json` do `MaxComponentsUi` antes de escrever `uno.config.ts`; isolar o import em um único arquivo. |
| R-08 | WebKitGTK com GPU/DMABUF instável no CachyOS (tela branca). | Médio | Investigar suporte a `WEBKIT_DISABLE_DMABUF_RENDERER=1` como diagnóstico na versão instalada; não aplicar por padrão. Avaliação definitiva da aceleração GPU na Fase 04/17. |
| R-09 | Logs de erro vazarem segredos antes do sanitizador da Fase 03. | Alto | Somente eventos enumerados e metadados não sensíveis nesta fase; descartar `Error.message`, `source`, stack, URLs, payloads e contextos livres. Truncamento não é sanitização. |
| R-10 | Atualização implícita de ferramentas quebrar instalação limpa. | Médio | Lockfiles npm/Cargo, toolchains e revisões locais registrados; dependências incompatíveis bloqueiam a fase, sem alterar Vue obrigatório silenciosamente. |
| R-11 | Fases posteriores editarem `mt-core` de forma conflitante. | Médio | `ErrorCode` é extensível por constantes em cada crate (prefixo de domínio), sem exigir novas variantes em `mt-core`; ids já definidos todos nesta fase. |
