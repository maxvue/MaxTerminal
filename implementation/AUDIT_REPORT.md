# Relatório de Auditoria Técnica Independente — Especificações e Planos (`AUDIT_REPORT.md`)

- **Projeto:** MaxTerminal (`~/GitHub/MaxTerminal`)
- **Worktree Ativa:** `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-4afa22c6` (branch `wt-4afa22c6`)
- **Fonte Canônica da Verdade:** [`../AGENTS.md`](../AGENTS.md) e [`./README.md`](./README.md)
- **Escopo Auditado:** 100% dos **205 arquivos** em `./implementation/*` (`README.md`, 17 `instructions.md`, 17 `spec.md` de fase, 85 `spec.md` de sub-implementação e 85 `plan.md` de sub-implementação).
- **Metodologia:** Auditoria independente conduzida por **6 subagentes especializados em paralelo** (Fases 01–03, Fases 04/05/10, Fases 06–08, Fases 09/11/17, Fases 12–13, Fases 14–16) combinada com verificação estática transversal (migrations SQLite, contratos IPC/Specta, `tauri::State`, `mt-core`, `mt-db`, `mt-bridge`, `mt-ai`, `mt-commands`, `package.json` e `Cargo.toml`).

---

## Sumário Executivo

A arquitetura documentada nas 17 fases demonstra maturidade técnica elevada e cobre em profundidade os requisitos funcionais do [`AGENTS.md`](../AGENTS.md). Entretanto, como as 17 fases e suas 85 unidades de implementação foram detalhadas de forma distribuída, a auditoria cruzada identificou **divergências concretas de contratos entre fases, omissões normativas e incompatibilidades de pacotes/schemas** que causariam falhas imediatas de compilação (`cargo check`, `tsgo`, `npm ci`) ou falhas de integração em runtime se não forem saneadas antes ou durante a execução.

| Categoria de Severidade | Total de Achados | Impacto Principal |
| :--- | :---: | :--- |
| **1. Crítico** *(Omissões ou Contradições com o `AGENTS.md` e Conflitos Bloqueantes entre Fases)* | **14** | Quebra do runner de migrations SQLite (`1..=N`), ausência literal das 24 labels canônicas na spec da Fase 16, falta de protocolo `BUG FIX` / ciclo de bugs em lote na Fase 11, conflitos de status HTTP/erros/capabilities entre Fases 09, 11 e 17, e incompatibilidades de geração assistida entre Fases 12, 14 e 15. |
| **2. Técnico** *(Especificações rasas, divergências de assinaturas IPC/Rust/TS, schemas ou pacotes)* | **18** | Conflitos de `peerDependencies` (`vite@8.0.0` vs `@vitejs/plugin-vue@5.2.3`, `vue@3.6.0-rc.13` vs `vue-web-terminal`), `rusqlite@0.34` vs `r2d2_sqlite@0.27`, omissão de `splitpanes` no `package.json`, métodos `todo!()` em `12/01` e `12/03`, e divergências de assinaturas entre `spec.md` da fase e `spec.md` das subpastas. |
| **3. Estrutural** *(Ordem de implementação, dependências quebradas entre fases/sub-fases e caminhos)* | **11** | Referência antecipada de `ScrollbackRecorder` em `07/03` (criado apenas em `07/04`), importação de `mt_core::pty` inexistente na Fase 07, colisão de `AppTitleBar.vue` e `TerminalPlayground.vue` em caminhos distintos, fragmentação de pastas Vitest e 25 arquivos `plan.md` com caminhos hardcoded da worktree antiga `wt-6f18b3d0`. |

---

## Matriz Completa de Conformidade com o `AGENTS.md`

| Eixo do `AGENTS.md` | Requisito Específico | Fases | Status | Evidência e Achados Relacionados |
| :--- | :--- | :---: | :---: | :--- |
| **1. Core & Stack** | Tauri v2, `portable-pty`, monitoramento `/proc` no Linux, zero processos órfãos | 01, 02 | ⚠️ Parcial | `portable-pty` + `/proc/<pid>/stat` + `pidfd_open` + `PR_SET_CHILD_SUBREAPER` + watchdog em `02/01..04`. Ressalvas: contradição de cgroup v2 (**C-08**) e erro de chamada de `MtError::with_details` (**T-04**). |
| **1. Core & Stack** | Vue `v3.6.0-rc.13`, TypeScript estrito com `tsgo`, Vite 8, UnoCSS (`presetMaxUno`), SCSS Scoped | 01 | ⚠️ Parcial | Fixado em `01/03/spec.md`, porém com conflito de `peerDependencies` entre `vite: "8.0.0"`, `@vitejs/plugin-vue: "5.2.3"` e `vitest: "3.1.2"` (**T-01**) e divergência no nome do script `typecheck` vs `check-types` vs `typecheck:tsgo` (**E-08**). |
| **1. Core & Stack** | Script `sync:local-libs` para `@maxvue/max-components-ui` (`MaxIcon`), `@maxvue/max-use` e `@maxvue/max-pinia` | 01 | ✅ Conforme | `scripts/sync-local-libs.mjs` em `01/03/spec.md` valida `package.json`, `dist/` e `exports`, vinculando as 3 bibliotecas com modo `--check` somente leitura. |
| **1. Core & Stack** | Diretrizes de idioma (`pt-BR` / `en-US`), ordem SFC (`<template>`, `<script setup>`, `<style scoped>`), 4 espaços, `baseIndent: 1`, `'single'`, `;`, `comma-dangle: 'never'`, tag em 1 linha | 01 | ✅ Conforme | Regra autoral AST `eslint/rules/single-line-start-tag.js` + `eslint.config.js` + `.stylelintrc.json` em `01/03/spec.md`. Pequena divergência visual em `AppTitleBar.vue` (**T-03**). |
| **2. Motor Híbrido & DB** | `@xterm/xterm` + addons (`webgl`, `fit`, `search`, `web-links`) convivendo sem conflito com `vue-web-terminal` (camada de blocos e input rico) | 04, 05 | ⚠️ Parcial | Coexistência via `TerminalModeDetector` (`?1049h`/`?1047h` alternate screen) e `InputModeResolver` excelente. Porém, `VueWebTerminalAdapter` (`04/05/spec.md`) não foi conectado a `useCommandBlocks.ts` / `CommandBlockLayer.vue` (**C-09**). |
| **2. Motor Híbrido & DB** | `splitpanes` para as 2 colunas principais e para Split Panes (horizontal/vertical) dentro da aba | 06, 07 | ⚠️ Parcial | Especificado em `AppLayout.vue` (`06/05`) e `PaneLayout.vue` (`07/05`), mas o pacote `"splitpanes"` foi omitido das adições ao `package.json` nas Fases 01, 06 e 07 (**T-02**). |
| **2. Motor Híbrido & DB** | SQLite em modo WAL (`PRAGMA journal_mode=WAL;`) + índice FTS5 para histórico de sessões, comandos e saídas | 03, 05, 07, 17 | ⚠️ Parcial | WAL + `DbWriter` + `ReadPool` + FTS5 (`command_blocks_fts`, `session_scrollback_fts`, `search_index_fts`) especificados. Porém, há quebra crítica na numeração contígua das migrations (**C-01**), conflito `rusqlite 0.34` vs `r2d2_sqlite 0.27` e versão SQLite `3.51.3` vs `3.45.0` (**C-07**). |
| **2. Motor Híbrido & DB** | Importador de temas Tabby (YAML/JSON) mapeado para tokens `--max-*` / `--background-*` (tema padrão Max Navy/Teal) | 10 | ⚠️ Parcial | Conversão OKLCH, contraste WCAG, anti-YAML-bomb e tema Max Navy/Teal excelentes em `10/01..03`. Porém, `10/04/spec.md` §3.4 contradiz `10/spec.md` §3.13 e `04/01/spec.md` nos campos de `TerminalTheme.ui` (**C-10**). |
| **3. OS Experience (CachyOS)** | System Tray condicional (minimiza mantendo vivo quando houver abas `Executando` ou `Execução Contínua`) | 09 | ✅ Conforme | `LifecycleController::decide_close` (`09/02/spec.md`) verifica `running > 0 \|\| continuous > 0`, oculta janela com `api.prevent_close()` e preserva PTYs; menu da bandeja possui saída explícita coordenada. |
| **3. OS Experience (CachyOS)** | Atalho Global (Quake Mode / Quick Toggle) em qualquer workspace | 09 | ✅ Conforme | `QuakeController` + `EnvironmentDetector` suportando X11 nativo, Wayland via XDG Desktop Portal `GlobalShortcuts` (`zbus`) e fallback guiado para Hyprland/KWin (`maxterminal --toggle`). |
| **3. OS Experience (CachyOS)** | Links clicáveis `arquivo:linha` abrindo na linha correspondente no VS Code / MaxCode | 09, 11 | ⚠️ Parcial | `LinkParser` + `LinkResolver` (`09/04/spec.md`) validam e abrem no editor, mas o ponto de extensão `EditorOpener` previsto para a ponte loopback do MaxCode não foi implementado na Fase 11 (**T-13**). |
| **3. OS Experience (CachyOS)** | Sanitização de Segredos (`APP_KEY`, senhas, cookies, `Bearer`) antes do SQLite ou envio externo | 03, 05, 07, 11, 12, 13, 14, 15, 16, 17 | ✅ Conforme | `mt_db::sanitizer::{sanitize, StreamSanitizer, SanitizedText, ExternalPayload}` cobre 13 famílias de segredos inclusive partidos entre chunks de streaming. Ressalva de assinatura em `LogRedactor::redact` (**T-05**). |
| **4. Coluna Esquerda (Projetos)** | Adição individual e "Pasta de Projetos" (`~/GitHub`), `fs::watch` auto-descoberta, auto-detecção de stack (Laravel, Vue, Tauri, VS Code, Python, Rust) com Ícone Iconify + Cor + fallback | 06 | ✅ Conforme | `mt-projects` (`06/01..05`) implementa detector determinístico para as 6 stacks, FNV-1a fallback, `RootWatcher` (`notify`) em nível 1 e modais de criação/importação. |
| **4. Coluna Esquerda (Projetos)** | Os 6 Badges Semânticos exatos: 🟢 Executando (`--max-success-500`), 🔵 Execução Contínua (`--max-info-500`), 🟠 Finalizado não visto (`--max-warning-500`), 🔴 Com Erro/Intervenção não visto (`--max-danger-500`), ⚪ Parado, 🟣 Issues Abertas | 05, 06, 13, 16 | ✅ Conforme | `useProjectCounters.ts` + `ProjectStatusBadges.vue` (`06/04..05`) + `useProjectBadgeRegistry` (recebendo o 6º badge 🟣 Issues Abertas da Fase 16 em `16/04/spec.md`). |
| **5. Coluna Direita (Workspace)** | Botões "Adicionar nova aba" (Template Padrão) e "Iniciar Aba Template", Split Panes, parâmetros completos de Templates e botão "Iniciar nova Sessão" arquivando no SQLite | 07 | ⚠️ Parcial | Funcionalidade completa em `07/01..05`. Há porém incompatibilidade de assinatura em `ProjectInsertContributor` com a Fase 06 (**C-11**), uso indevido de `ProjectActivityGuard` (**C-12**) e referência antecipada de `ScrollbackRecorder` entre `07/03` e `07/04` (**E-01**). |
| **5. Coluna Direita (Indicadores)** | Indicadores de Git/Worktree (branch, `dirty`, `.worktrees/`, `.max-code-worktrees/`) e Detector de Portas Ativas (badge clicável para abrir navegador ou encerrar processo) | 08 | ⚠️ Parcial | `mt-git` e `mt-ports` (`08/01..04`) completos com validação `pidfd` anti-TOCTOU. Porém, faltam métodos públicos em `WorkspaceManager` (Fase 07) exigidos por `IndicatorWorkspaceObserver` e `PtyProcessTreeAdapter` em `08/03` (**C-13**). |
| **6. IA & Agentes CLI** | Gateway `MaxAiManager` (`X-Agent-Name: MaxTerminal`), cascata `@` (Harness → Modelo → Raciocínio) e `?` padrão, Diagnóstico Rápido Inline ao lado de "Inspecionar", Geração Assistida e RAG (`/v1/rags/{slug}/search`) | 12, 14, 15 | ❌ Conflitos de Contrato | `X-Agent-Name: MaxTerminal`, cascata `@`/`?`, diagnóstico inline e RAG especificados. Porém, `OpenAiCompatDialect::default()` viola `CA-35` (**C-14**) e os contratos de Geração Assistida (`useAiGenerate`, `schemaId` e `AiService::generate_structured`) divergem entre a Fase 12 e as Fases 14 e 15 (**C-05**, **C-06**). |
| **6. IA & Agentes CLI** | 4 Agentes CLI (`Claude Code`, `Codex`, `Antigravity (agy)`, `OpenCode`), Templates/Atalhos, isolamento em Worktree, `StatusLine` (modelo, contexto, cotas 5h/semanal), alerta de permissão com notificação nativa CachyOS, `MaxSkills` + memória contextual | 13 | ❌ Conflitos de Contrato | Arquitetura de `mt-agents` + sidecar `mt-agent-relay` completa, mas com conflito de constraint SQLite `'max_code'` vs `"maxCode"` (**T-15**), divergência de assinatura `TabStateRegistry::apply` com a Fase 05 (**T-16**), divergência de `SpawnContext`/`ResolvedSpawn` com a Fase 07 (**T-17**) e acoplamento de `agent_tab_open` impedindo reuso pela Fase 16 (**E-05**). |
| **7. Integração `MaxCode` & Issues** | Loopback HTTP/WS (`127.0.0.1`) + URI Schemes (`vscode://johnattas.maxcode/...` e `maxterminal://...`), sync de projeto ativo, Shell Integration OSC 133 + `/proc` gerando botão "Inspecionar" com prompt exato em Plan Mode | 05, 11 | ⚠️ Parcial | Prompt `"Ao executar o comando \"${comando}\" tive o seguinte erro: \"${erro}\""` idêntico letra por letra e sanitizado (`11/03/spec.md`). Contudo, há conflito de HTTP `403` vs `421` e código `bridge.deeplink_rejected` com a Fase 17 (**C-04**) e duplicação do roteador de Deep Link com a Fase 09 (**E-04**). |
| **7. Integração `MaxCode` & Issues** | Aba Fixa de Issues com as **24 labels canônicas** do MaxCode, ações 1-clique (`Abrir`, `BUG PLAN`, `BUG FIX`), abrir aba na worktree da issue, gestão local completa (`## PLANO DE IMPLEMENTAÇÃO`) e botão global "Corrigir todos os Bugs" | 11, 16 | ❌ Omissão e Quebra Crítica | 1) `16/spec.md` e `16/02/spec.md` omitem a lista literal das 24 labels canônicas, exigindo leitura externa de `~/GitHub/MaxCode/src/core/labelsDeBugs.ts` (**C-02**). 2) A Fase 11 (`mt-bridge`) suporta apenas `ConversationMode::Plan` e não implementa o protocolo para `BUG FIX` nem para `"Corrigir todos os Bugs"` que a Fase 16 invoca (**C-03**). |
| **8. Comandos Rápidos (`>`, `>>`, `/`)** | Artisan `>` (`php artisan list --format=json` + cache `.max-code/commands/artisan.json`), filtro e flags `-`, opção final "Criar comando" com modal completo e 2 ações ("Criar" e "Criar com MaxCode": `"Criar comando artisan:\n${dados_formatados_do_formulario}"`) | 14 | ⚠️ Parcial | Cache compatível, modal e prompt MaxCode exatos em `14/01..05`. Ressalvas: `Schedule` não grava automaticamente quando `routes/console.php` já existe no Laravel 11/12 (**T-18**) e `artisanArgsTrigger` só ativa para 4 prefixos fixos de 3 caracteres (**T-19**). |
| **8. Comandos Rápidos (`>`, `>>`, `/`)** | Grupos `>>` (Locais antes de Globais, 3 modos: Sequencial, Paralelo em Background, Multi-Abas, e 2 opções finais: "Criar grupo de comandos local" e "Criar grupo de comandos global") e atalhos `/` (`composer`, `npm`, `pnpm`, `yarn`, `bun`, `cargo`) | 14, 15 | ⚠️ Conflito no Crate | Regras funcionais de `>>` e `/` completas, mas as Fases 14 e 15 colidem na definição do crate compartilhado `mt-commands` (`edition = "2021"` vs `"2024"`, `mt-db` sem features vs com SQLite, `QuickCommandsState` vs `AppCommandGroupState`) (**E-06**). |

---

## 1. Achados Críticos (Omissões ou Contradições com o `AGENTS.md` e Conflitos Bloqueantes)

### `[C-01]` Quebra Fatal de Contiguidade e Ordem nas Migrations SQLite (`0001` a `0017`) entre a Fase 03 e as Fases 05, 06, 07, 09, 10, 12, 13, 15, 16 e 17
- **Severidade:** Crítica (Bloqueante de inicialização do banco a partir da Fase 07).
- **Arquivos Afetados:**
  - `03 - Persistência SQLite e Sanitização de Segredos/instructions.md` (linha 118) vs `03/02/spec.md` (§6.1 linha 402, §6.2 linha 460 e §8 linha 595 `naming_contract`)
  - `05/01/spec.md` (linha 39: `0002_05_shell_sessions_command_blocks.sql`)
  - `06/02/spec.md` (linha 30: `0003_06_projects.sql`)
  - `07/01/spec.md` (linhas 34 e 736) e `07/01/plan.md` (linha 23): `0007_07_workspace_templates_tabs_sessions.sql`
  - `09/01/spec.md` (linha 93: `NNNN_09_os_settings.sql`)
  - `10/03/spec.md` (linha 35: `NNNN_10_themes.sql`)
  - `12/01/spec.md` (linhas 29 e 96) e `12/01/plan.md` (linha 13): `0006_12_ai.sql`
  - `13/01/spec.md` (linha 23: `0009_13_agents_skills_memory.sql`)
  - `15/01/spec.md` (linha 23) e `15/01/plan.md` (linha 14): `0015_15_command_groups.sql`
  - `16/02/spec.md` (linha 24: `NNNN_16_github.sql`)
  - `17/01/spec.md` (linha 26) e `17/01/plan.md` (linha 11): `0017_17_search.sql`
- **Evidência:**
  1. Na Fase 03 (`03/02/spec.md` linha 460 e `CA-16` linha 595), `MigrationRunner::new` carrega todos os arquivos de `src-tauri/crates/mt-db/migrations/` e **exige que as versões `NNNN` formem uma sequência estritamente contígua `1..=N` sem lacunas**, além de validar `PRAGMA user_version == MAX(version)`.
  2. Internamente na própria Fase 03, `03/instructions.md` (linha 118) lista 3 arquivos (`0001_03_base.sql`, `0002_03_history.sql`, `0003_03_history_fts.sql`), enquanto `03/spec.md` e `03/02/spec.md` criam **apenas** `0001_03_base.sql` (transferindo `sessions` e `command_blocks` para a Fase 05).
  3. As Fases 07, 15 e 17 confundiram o prefixo sequencial `NNNN` da migration com o número da fase (`0007_07_...`, `0015_15_...`, `0017_17_...`), criando lacunas (`0003` salta para `0007`; `0009` salta para `0015`), já que as Fases 01, 02, 04, 08, 11 e 14 **não possuem migrations SQLite**.
  4. Pior ainda: a Fase 12 (implementada após a Fase 07) fixa `0006_12_ai.sql` (versão `6`), que é **menor** que `0007_07_workspace_templates_tabs_sessions.sql` (versão `7`) criada na Fase 07!
- **Correção Obrigatória:** Padronizar nas especificações e planos das 11 fases com banco a tabela canônica contígua `0001` a `0011`:
  - `0001_03_base.sql` (Fase 03)
  - `0002_05_shell_sessions_command_blocks.sql` (Fase 05)
  - `0003_06_projects.sql` (Fase 06)
  - `0004_07_workspace_templates_tabs_sessions.sql` (Fase 07 — substituir `0007_07_...`)
  - `0005_09_os_settings.sql` (Fase 09 — substituir `NNNN_09_...`)
  - `0006_10_themes.sql` (Fase 10 — substituir `NNNN_10_...`)
  - `0007_12_ai.sql` (Fase 12 — substituir `0006_12_...`)
  - `0008_13_agents_skills_memory.sql` (Fase 13 — substituir `0009_13_...`)
  - `0009_15_command_groups.sql` (Fase 15 — substituir `0015_15_...`)
  - `0010_16_github.sql` (Fase 16 — substituir `NNNN_16_...`)
  - `0011_17_search.sql` (Fase 17 — substituir `0017_17_...`)

---

### `[C-02]` Omissão da Definição Literal das 24 Labels Canônicas do Workflow MaxCode na Especificação da Fase 16
- **Severidade:** Crítica (Omissão de contrato exigido pelo `AGENTS.md` §7).
- **Arquivos Afetados:**
  - `16 - Aba Fixa de GitHub Issues/spec.md` (§2.1 linhas 50–88, §7 linhas 343–371, §12 linha 739)
  - `16 - Aba Fixa de GitHub Issues/02 - Catalogo-de-Labels-Extracao-de-Plano-Cache-SQLite-e-Sync/spec.md` (§1 item 1, §2 linha 25, §3.1 linhas 100–127)
  - `16 - Aba Fixa de GitHub Issues/02 - Catalogo-de-Labels-Extracao-de-Plano-Cache-SQLite-e-Sync/plan.md` (Passo 1 linha 12)
- **Evidência:**
  - O `AGENTS.md` §7 exige suporte integral às **24 labels canônicas do workflow do MaxCode (15 de estado, 3 de encerramento e 6 de categoria)**.
  - Contudo, `16/spec.md` (§2.1 linha 83) recusa-se a listar as 24 labels no próprio documento (*"Este documento não transcreve nem completa um suposto catálogo"*), e `16/02/plan.md` manda o executor criar `src-tauri/crates/mt-github/fixtures/maxcode/labels.v1.json` lendo um repositório externo fora da worktree (`~/GitHub/MaxCode/src/core/labelsDeBugs.ts`).
- **Correção Obrigatória:** Incorporar diretamente em `16/spec.md` §7 e `16/02/spec.md` §3.1 o catálogo completo das **24 labels canônicas** verificadas em `/home/johnattas/GitHub/MaxCode/src/core/labelsDeBugs.ts` (preservando a grafia byte a byte, inclusive `Waiting user test` e `Evidence not Found`):
  - **15 Labels de Estado (`state`):**
    1. `Waiting Plan`
    2. `Waiting More Info`
    3. `Planned`
    4. `Waiting Execute`
    5. `Executed`
    6. `Waiting Check`
    7. `Checked Approved`
    8. `Checked Rejected`
    9. `Waiting Re-plan`
    10. `Fix Merged`
    11. `Waiting Deploy`
    12. `Deployed`
    13. `Waiting user test` *(atenção à caixa baixa em `user test`)*
    14. `User Rejected`
    15. `User Approved`
  - **3 Labels de Encerramento (`closure`):**
    16. `Duplicated`
    17. `Evidence not Found` *(atenção à caixa baixa em `not`)*
    18. `Dismissed`
  - **6 Labels de Categoria (`category`):**
    19. `Regra de Negocio`
    20. `Testes`
    21. `Desempenho`
    22. `Seguranca`
    23. `Audit`
    24. `Documentacao`

---

### `[C-03]` Incompatibilidade Bilateral entre Fase 11 (`mt-bridge`) e Fase 16 (`mt-github`) para as Ações "Abrir", "Analisar (`BUG PLAN`)", "Corrigir (`BUG FIX`)" e "Corrigir todos os Bugs"
- **Severidade:** Crítica (Quebra funcional dos requisitos do `AGENTS.md` §7).
- **Arquivos Afetados:**
  - `11 - Integração Bidirecional com MaxCode/spec.md` (§5.3, §19) e `11/01/spec.md` (§3.6 `protocol.rs`)
  - `11 - Integração Bidirecional com MaxCode/02 - Servidor Loopback HTTP e WebSocket, Sessões, Cliente MaxCode e URIs no mt-bridge/spec.md` (§3.4 `MaxCodeClient`)
  - `16 - Aba Fixa de GitHub Issues/instructions.md` (§3.1 linhas 125–133)
  - `16 - Aba Fixa de GitHub Issues/spec.md` (§2.2 linhas 99–103, §8.1 linhas 409–448)
  - `16 - Aba Fixa de GitHub Issues/03 - Worktrees-Acoes-MaxCode-Orquestrador-em-Lote-e-IPC-Tauri/spec.md` (§2 linha 36, §3.2 e §3.3)
- **Evidência:**
  - O `AGENTS.md` §7 exige enviar uma issue ao MaxCode com 1 clique nas 3 modalidades (**Abrir**, **Analisar (`BUG PLAN`)** e **Corrigir (`BUG FIX`)**) e o botão global **"Corrigir todos os Bugs"**.
  - Na Fase 11 (`11/01/spec.md` §3.6), `ConversationMode` possui exclusivamente a variante `Plan`, `Capability` não inclui `issues_protocol_v1` / `bugs_cycle_v1`, e `MaxCodeClient` só expõe `open_plan` e `open_project`.
  - Na Fase 16 (`16/03/spec.md` §3.2–§3.3), `IssueActionDispatcher` e `BugCycleOrchestrator` exigem que o adaptador concreto de `MaxCodePort` envie mensagens `issue.command` (`open | plan | fix`), `bugs.cycle.preview`, `bugs.cycle.start`, `bugs.cycle.status`, escute `bugs.cycle.progress` e monte URIs `vscode://johnattas.maxcode/issue?...`, mas `16/instructions.md` §3.1 proíbe editar arquivos da Fase 11!
- **Correção Obrigatória:** Adicionar as mensagens `issue.command` (`open | plan | fix`), `bugs.cycle.{preview,start,status,progress}`, as capabilities `issues_protocol_v1` e `bugs_cycle_v1` e o builder de URI `vscode://johnattas.maxcode/issue` diretamente em `mt-bridge` (na Fase 11 `11/01/spec.md` e `11/02/spec.md`, ou autorizando sua adição aditiva em `16/03/spec.md`).

---

### `[C-04]` Tripla Contradição de Segurança e Hardening entre as Fases 09, 11 e 17 (Status HTTP `403` vs `421`, Código `bridge.deeplink_rejected` e Capabilities `autostart:allow-*`)
- **Severidade:** Crítica (Falha garantida nos testes de integração/hardening da Fase 17 e quebra do requisito `CA-37` da Fase 09).
- **Arquivos Afetados:**
  - `09/instructions.md` (`CA-37`), `09/03/spec.md` (§2.2 `OsError::ActivationInvalid`), `09/04/spec.md` (§4.3 `src-tauri/capabilities/os.json`)
  - `11/instructions.md` (`CA-10`), `11/01/spec.md` (§3.2 `BridgeError::HostForbidden`, §3.5 `HostPolicy`), `11/02/spec.md` (§3.1 `MaxTerminalUri`, §3.2 `auth_middleware`)
  - `17/instructions.md` (`CA-33`, `CA-34`), `17/04/spec.md` (§3.1 `capabilities/main.json` e §3.4 `bridge_hardening.rs`, `deep_link_allowlist.rs`)
- **Evidência:**
  1. **Status HTTP do Header `Host` inválido (`403` vs `421`):** A Fase 11 (`11/01/spec.md` e `11/02/plan.md`) especifica que um header `Host` inválido retorna **`403 Forbidden`** (`bridge.host_forbidden`) e aceita `127.0.0.1:<port>` e `localhost:<port>`. Já a Fase 17 (`17/instructions.md` `CA-33` e `17/04/spec.md` `tests/bridge_hardening.rs`) exige que `Host` diferente de `127.0.0.1:<port>` retorne **`421 Misdirected Request`**.
  2. **Código de erro de Deep Link rejeitado:** A Fase 09 retorna `"os.activation_invalid"`, a Fase 11 retorna `"bridge.invalid_uri"` / `"bridge.invalid_path"`, mas a Fase 17 (`CA-34` e `tests/deep_link_allowlist.rs`) exige o código **`"bridge.deeplink_rejected"`** (inexistente nas Fases 09 e 11).
  3. **Violação de Capabilities da WebView (`CA-37`):** A Fase 09 (`CA-37` e `09/04/spec.md` §4.3) proíbe conceder `autostart:*` à WebView e exige um teste provando que `autostart` direto via JS é negado. A Fase 17 (`17/04/spec.md` §3.1) sobrescreve `capabilities` com `capabilities/main.json` concedendo explicitamente `"autostart:allow-enable"`, `"autostart:allow-disable"` e `"autostart:allow-is-enabled"`.
- **Correção Obrigatória:**
  - Unificar o status HTTP de `Host` inválido (padronizar `403 Forbidden` ou `421 Misdirected Request` em ambas as Fases 11 e 17).
  - Alinhar o código de erro esperado em `tests/deep_link_allowlist.rs` (`17/04/spec.md`) com as variantes reais de `BridgeError` (`bridge.invalid_uri` / `bridge.invalid_path`).
  - Remover `"autostart:allow-*"` de `capabilities/main.json` em `17/04/spec.md`, preservando o acesso exclusivamente pelos comandos Rust validados.

---

### `[C-05]` Quebra de Contrato de Geração Assistida entre Fase 12 (`useAiGenerate` / `StructuredSchemaRegistry`) e Fase 14 (Modal de Comando Artisan)
- **Severidade:** Crítica (Quebra de compilação TypeScript e falha de validação de schema em runtime).
- **Arquivos Afetados:**
  - `12/02/spec.md` (§3.6 linhas 514–536) e `12/03/spec.md` (§3.6 linhas 515–547)
  - `14/spec.md` (§2.2 linha 127) e `14/04/spec.md` (§3.1 linhas 62–76 e §3.4 linhas 172–177)
- **Evidência:**
  1. **Assinatura incompatível de `useAiGenerate<T>`:** Em `12/03/spec.md` §3.6 (linha 543), `useAiGenerate(schemaId, schema, decode)` exige **3 parâmetros obrigatórios** (sendo `decode: (value: unknown) => T` obrigatório por `CA-36`) e `run(prompt, opts?: AiGenerateRunOptions)` retorna `Promise<T | null>`. Em `14/04/spec.md` §3.1 (linha 73), `createAiGenerator` chama `useAiGenerate` com **apenas 2 parâmetros** (sem `decode`), espera retorno `Promise<T>` e passa `{ signal?: AbortSignal }`.
  2. **Nome do `schemaId` divergente:** A Fase 12 (`12/02/spec.md` linha 521) registra no backend o schema builtin `'artisan.command.v1'`, enquanto a Fase 14 (`14/spec.md` linha 127) invoca `'artisan.command'` (sem `.v1`), o que causará rejeição por schema desconhecido.
- **Correção Obrigatória:** Padronizar `'artisan.command.v1'` (ou aceitar alias `'artisan.command'` em `StructuredSchemaRegistry`) e fornecer a função `decode` em `src/composables/quick-commands/adapters.ts` (`14/04/spec.md`).

---

### `[C-06]` Quebra Tripla de Contrato de Geração Assistida entre Fase 12 (`AiService`) e Fase 15 (`GroupSuggester` / Grupos de Comandos)
- **Severidade:** Crítica (Erro de compilação Rust + falha de validação de schema).
- **Arquivos Afetados:**
  - `12/spec.md` (§2.9 linha 566), `12/02/spec.md` (§3.6 linha 522, §3.7 linhas 600 e 624)
  - `15/instructions.md` (linha 87), `15/spec.md` (§2.1 linha 27), `15/02/spec.md` (§3.1 linhas 256–277), `15/03/spec.md` (§1 linha 6, §2 linha 28, §3.1 linha 52)
- **Evidência:**
  1. **Três nomes diferentes para o mesmo `schema_id`:**
     - `12/spec.md` §2.9 (linha 566): `"command-group.suggestion.v1"`
     - `12/02/spec.md` §3.6 (linha 522): `"command_group.v1"`
     - `15/03/spec.md` §3.1 (linha 52): `"command-group.suggestion"`
  2. **Acesso a campo privado `AiService.structured.generate`:** A Fase 15 (`15/instructions.md` linha 87, `15/spec.md` linha 27, `15/03/spec.md` linha 6) manda invocar `ai_service.structured.generate(...)`. Na Fase 12 (`12/02/spec.md` §3.7 linhas 600 e 624), o campo `structured` de `AiService` é privado e o método público é `AiService::generate_structured(&self, req: StructuredRequest) -> MtResult<StructuredResponse>`.
  3. **Omissão de `src-tauri/src/command_groups/adapters.rs` em `15/03/spec.md`:** Embora listado na tabela de arquivos a criar em `15/03/spec.md` §2 (linha 28), o corpo da especificação omite a implementação de `TauriGroupAiAdapter` e `TauriRecurringCommandsAdapter`.
- **Correção Obrigatória:** Padronizar o `schema_id` como `"command-group.suggestion.v1"` em ambas as Fases 12 e 15, usar `AiService::generate_structured` e detalhar `TauriGroupAiAdapter` e `TauriRecurringCommandsAdapter` em `15/03/spec.md`.

---

### `[C-07]` Incompatibilidade de Dependências Cargo (`rusqlite 0.34` vs `r2d2_sqlite 0.27`) e Contradição de Versão do SQLite (`3.51.3` vs `3.45.0`) na Fase 03
- **Severidade:** Crítica (Falha de resolução/linkagem do `Cargo.lock` em `mt-db`).
- **Arquivos Afetados:**
  - `03/instructions.md` (§3.2 linha 106)
  - `03/spec.md` (§1.2 linha 26)
  - `03/01/spec.md` (§3.1 linhas 53–55 e 67)
  - `03/02/spec.md` (§1 linha 12 vs §4.2 linha 247)
- **Evidência:**
  1. Em `03/01/spec.md` §3.1 (linhas 53–55), o `[workspace.dependencies]` declara `rusqlite = "0.34"` junto com `r2d2_sqlite = "0.27"`. No crates.io, `r2d2_sqlite 0.27.0` depende de `rusqlite ^0.33` (enquanto `rusqlite 0.34` requer `r2d2_sqlite 0.28+`). Como o crate `libsqlite3-sys` possui `links = "sqlite3"`, ter duas versões de `rusqlite` no grafo causa erro fatal de compilação no Cargo (`multiple crates link to native library sqlite3`), violando a própria invariante da linha 67!
  2. Além disso, `03/01/spec.md` linha 67 e `03/02/spec.md` linha 12 exigem SQLite bundled `>= 3.51.3`, mas `rusqlite 0.34` empacota SQLite `3.48.x`, e `verify_sqlite_capabilities` (`03/02/spec.md` linha 247) verifica `sqlite_version_number() >= 3045000` (`3.45.0`).
- **Correção Obrigatória:** Alinhar as versões compatíveis em `03/01/spec.md` (ex.: `rusqlite = "0.34"` com `r2d2_sqlite = "0.28"`, ou `rusqlite = "0.33"` com `r2d2_sqlite = "0.27"`) e unificar a verificação de versão mínima do SQLite bundled em `3.45.0` (`3045000`).

---

### `[C-08]` Contradição de Contenção Cgroup v2 na Fase 02 (`02/spec.md` vs `02/01/spec.md`)
- **Severidade:** Crítica (Contradição arquitetural direta e risco de `EBUSY`/`EACCES` no systemd do Linux).
- **Arquivos Afetados:**
  - `02 - Core PTY e Supervisão de Processos/spec.md` (§7.6 linha 587)
  - `02 - Core PTY e Supervisão de Processos/01 - Fundação do Crate mt-pty, Contratos, Spawn e Contenção Cgroup v2/spec.md` (§3.7 linha 719)
- **Evidência:**
  - `02/spec.md` §7.6 (linha 587) estabelece: *"App/watchdog ficam fora dos cgroups das cargas. Não mover o app inteiro temporariamente para um cgroup de PTY."*
  - `02/01/spec.md` §3.7 (linha 719) contradiz a spec da fase determinando: *"Move o próprio processo app e o watchdog para um subgrupo `<subtree>/app` (respeitando a regra 'no internal processes' do cgroup v2) e habilita controladores necessários na raiz delegada"*.
- **Correção Obrigatória:** Alinhar `02/01/spec.md` §3.7 com `02/spec.md` §7.6: criar subgrupos filhos apenas dentro de um escopo delegado dedicado às cargas PTY (ou degradar graciosamente para `CgroupDriver::Disabled` quando o cgroup atual do app tiver processos internos e não permitir habilitar `subtree_control`, sem mover o processo principal da UI/Tauri).

---

### `[C-09]` `VueWebTerminalAdapter` Desconectado da Renderização de Blocos na Fase 04 (`AGENTS.md` §2)
- **Severidade:** Crítica (Requisito de stack do `AGENTS.md` §2 implementado apenas como classe órfã sem uso em produção).
- **Arquivos Afetados:**
  - `AGENTS.md` §2 (linha 60: `vue-web-terminal / Camada de Blocos e Input Rico`)
  - `04 - Motor de Terminal Híbrido/05 - Camada de Blocos de Comando, Adaptador VueWebTerminal e Ações de Bloco/spec.md` (§3.1 linhas 46–105, §3.2 `useCommandBlocks.ts`, §3.4 `CommandBlockLayer.vue`)
- **Evidência:**
  - Em `04/05/spec.md` §3.1, `VueWebTerminalAdapter` é definido em `src/terminal/blocks/vueWebTerminalAdapter.ts`, mas nem `useCommandBlocks.ts` (§3.2), nem `CommandBlockLayer.vue` (§3.4), nem `HybridTerminal.vue` (`04/06/spec.md`) importam ou instanciam `VueWebTerminalAdapter`.
- **Correção Obrigatória:** Integrar `VueWebTerminalAdapter` ao pipeline de projeção de `useCommandBlocks.ts` / `CommandBlockLayer.vue` e adicionar `"vue-web-terminal"` na seção `overrides` do `package.json` para aceitar `"vue": "3.6.0-rc.13"`.

---

### `[C-10]` Contradição no Contrato `TerminalTheme.ui` entre `04/01/spec.md`, `10/spec.md` e `10/04/spec.md`
- **Severidade:** Crítica (Erro de compilação TypeScript em `toTerminalTheme` e ausência de aplicação local por painel).
- **Arquivos Afetados:**
  - `04/01/spec.md` (§3.1 linhas 54–60) e `04/02/spec.md` (§3.5 linha 175)
  - `10/spec.md` (§3.13 linhas 480–487 e §7.3 linha 1181)
  - `10/04/spec.md` (§3.4 linhas 201–202)
- **Evidência:**
  1. `04/01/spec.md` §3.1 e `10/spec.md` §3.13 definem `TerminalTheme.ui` com os 5 campos `{ blockBackground, blockBorder, blockHeaderBackground, richInputBackground, richInputBorder }`.
  2. Em contradição direta, `10/04/spec.md` §3.4 (linha 202) instrui que `toTerminalTheme` preencha `ui` com `--mt-terminal-background`, `--mt-terminal-foreground` e `--mt-terminal-selection`, quebrando o typecheck `tests/theme/terminalTheme.typecheck.ts` (`CA-05`).
  3. Além disso, `TerminalInstance.applyTheme` na Fase 04 (`04/02/spec.md` §3.5) aplica apenas `this.xterm.options.theme = toXtermTheme(theme)` e descarta `theme.ui`, impedindo que abas de projetos com temas distintos estilizen localmente seus próprios `CommandBlockLayer` e `RichInput`.
- **Correção Obrigatória:** Corrigir `10/04/spec.md` §3.4 para usar as 5 propriedades de `TerminalTheme.ui` e expor `theme.ui` como variáveis CSS no container `.hybrid-terminal` de cada painel.

---

### `[C-11]` Incompatibilidade de Assinatura da Trait `ProjectInsertContributor` entre Fase 06 (`06/02`) e Fase 07 (`07/01`)
- **Severidade:** Crítica (Erro de compilação Rust em `mt-db`).
- **Arquivos Afetados:**
  - `06/02/spec.md` (§3.2 linhas 228–236)
  - `07/01/spec.md` (§3.6 linhas 932–936)
- **Evidência:**
  - Na Fase 06 (`06/02/spec.md` linha 231): `fn after_insert(&self, tx: &rusqlite::Transaction<'_>, project: &StoredProject) -> Result<(), DbError>;`
  - Na Fase 07 (`07/01/spec.md` linha 932): `fn after_insert(&self, tx: &Transaction<'_>, project_id: ProjectId, now_ms: i64) -> Result<(), MtError>` (3 parâmetros em vez de 2 e retorno `MtError` em vez de `DbError`).
- **Correção Obrigatória:** Ajustar `DefaultTemplateContributor::after_insert` em `07/01/spec.md` para receber `(tx: &rusqlite::Transaction<'_>, project: &StoredProject) -> Result<(), DbError>`, lendo `project.id` e `project.created_at_ms`.

---

### `[C-12]` Uso Incorreto de `ProjectActivityGuard` como Campo Permanente de `WorkspaceManager` na Fase 07 (`07/03`)
- **Severidade:** Crítica (Erro de compilação Rust + deadlock na remoção de projetos).
- **Arquivos Afetados:**
  - `06/spec.md` (§2.10 linha 650) e `06/03/spec.md` (§3.1 linhas 72–75 e 108)
  - `07/03/spec.md` (§3.3 linhas 275, 285 e §3.4 linha 372)
- **Evidência:**
  - Na Fase 06, `ProjectActivityGuard` é um guard RAII temporário de leitura (`OwnedRwLockReadGuard<()>`) retornado por `ProjectService::acquire_activity(&self, id: ProjectId)`.
  - Na Fase 07 (`07/03/spec.md` linha 275), `WorkspaceManager` declara `project_guard: Arc<mt_projects::ProjectActivityGuard>` como campo permanente e chama `ProjectActivityGuard::acquire_shared(project_id)` (método inexistente).
- **Correção Obrigatória:** Substituir o campo em `WorkspaceManager` por `project_service: Arc<mt_projects::service::ProjectService>` e chamar `self.project_service.acquire_activity(project_id).await?` apenas durante a transação de abertura da aba.

---

### `[C-13]` Acoplamento Quebrado entre Fase 08 (`08/03`) e `WorkspaceManager` da Fase 07 (`07/03`)
- **Severidade:** Crítica (Erro de compilação e perda de rastreamento de `pty_id` em `git_track_pane` e `verify_pane_ownership`).
- **Arquivos Afetados:**
  - `07/03/spec.md` (§3.3 `WorkspaceManager`)
  - `08/01/spec.md` (§3.8 linha 597) e `08/03/spec.md` (§3.1 linhas 68–75, 102–113; §3.5 linhas 355–362; §3.6 linhas 489–497)
- **Evidência:**
  - `08/03/spec.md` assume que `WorkspaceManager` (Fase 07) expõe `WorkspaceObserver`, `reserve_cwd` e consulta de ownership `(tab_id, pane_id) -> Option<PtyId>`, mas em `07/03/spec.md` o campo `runtime` de `WorkspaceManager` é privado e a trait chama-se `WorkspaceLifecycleObserver`. Além disso, o comando IPC `git_track_pane` em `08/03/spec.md` (§3.5 linha 355) não recebe `pty_id` nem consulta o `WorkspaceManager`, passando sempre `pty_id = None` para `GitService::track_pane`.
- **Correção Obrigatória:** Expor `pub async fn pane_pty(&self, tab_id: TabId, pane_id: PaneId) -> Option<PtyId>` em `WorkspaceManager` (`07/03/spec.md`) e injetar `Arc<WorkspaceManager>` em `PtyProcessTreeAdapter` e `git_track_pane` (`08/03/spec.md`).

---

### `[C-14]` Violação do Gate `CA-35` em `OpenAiCompatDialect::default()` na Fase 12 (`12/01/spec.md`)
- **Severidade:** Crítica (Contradição interna direta com `12/instructions.md` `CA-35` e `12/spec.md` §4.0).
- **Arquivos Afetados:**
  - `12/instructions.md` (`CA-35` linha 184), `12/spec.md` (§4.0 linhas 702–718) e `12/01/spec.md` (§3.6 linhas 1123–1127)
- **Evidência:**
  - `CA-35` exige que as rotas propostas do gateway (`S-01..S-07`) não sejam marcadas como validadas por padrão sem confirmação do gateway real. Contudo, `impl Default for OpenAiCompatDialect` em `12/01/spec.md` (linha 1125) retorna `Self::new(GatewayCapabilities::all_validated())`.
- **Correção Obrigatória:** Inicializar `OpenAiCompatDialect::default()` com capacidades pendentes de probe/configuração e restringir `GatewayCapabilities::all_validated()` a `OpenAiCompatDialect::for_tests()`.

---

## 2. Achados Técnicos (Especificações Rasas, Falta de Contratos IPC/Schemas ou Incompatibilidades de Bibliotecas)

### `[T-01]` Conflito de `peerDependencies` no `package.json` da Fase 01 (`vite@8.0.0` vs `@vitejs/plugin-vue@5.2.3` e `vitest@3.1.2`)
- **Onde:** `01/03/spec.md` §2 (linhas 65–85) e `01/instructions.md` (R-01 linha 189).
- **Detalhe:** `@vitejs/plugin-vue@5.2.3` e `vitest@3.1.2` exigem `vite@^5.0.0 || ^6.0.0` nas suas `peerDependencies`. Combiná-los com `"vite": "8.0.0"` sem `@vitejs/plugin-vue@^6.0.0+` / `vitest@^3.2+` (ou `overrides`) faz `npm install` falhar com `ERESOLVE`, especialmente porque `01/instructions.md` proíbe `--legacy-peer-deps`.

### `[T-02]` Omissão da Dependência `"splitpanes"` no `package.json` nas Fases 01, 06 e 07
- **Onde:** `01/03/spec.md` §2, `06/04/plan.md` linha 11, `06/05/spec.md` linha 212 e `07/05/spec.md` linha 27.
- **Detalhe:** As Fases 06 e 07 importam `Splitpanes`, `Pane` e `'splitpanes/dist/splitpanes.css'`, mas nenhum passo de `spec.md` ou `plan.md` das Fases 01, 06 ou 07 adiciona `"splitpanes"` (e `"@types/splitpanes"`) ao `package.json`.

### `[T-03]` Divergência no Template de `AppTitleBar.vue` entre `01/spec.md` e `01/04/spec.md`
- **Onde:** `01/spec.md` §7.12 (linhas 914–921) vs `01/04/spec.md` §5.2 (linhas 575–577).
- **Detalhe:** `01/spec.md` coloca `<div ml-auto><slot name="actions" /></div>` em linha única, o que aciona o warning/error `vue/singleline-html-element-content-newline` do `pluginVue.configs['flat/recommended']` se copiado sem a quebra de linha de `01/04/spec.md`.

### `[T-04]` Uso Incorreto de `MtError::new` e `MtError::with_details` na Fase 02 (`02/01` e `02/05`) e Formato de `ErrorCode` na Fase 12 (`12/spec.md`)
- **Onde:**
  - `01/01/spec.md` §4.4 (linhas 358, 415–430)
  - `02/01/spec.md` §3.3 (linhas 248–252) e `02/05/spec.md` §3.2 (linhas 174, 201, 235)
  - `12/spec.md` §2.1 (linhas 120–142) vs `12/01/spec.md` (§3.1)
- **Detalhe:**
  1. Em `mt-core` (`01/01/spec.md`), `MtError::new(code: ErrorCode, message)` exige `ErrorCode` (que valida formato `domínio.motivo` minúsculo) e `.with_details(details)` é método fluente de 2 argumentos (`self, details`). Já `02/01/spec.md` chama `MtError::with_details(code, message, d)` com 3 argumentos e `02/05/spec.md` passa `&str` direto para `MtError::new("pty.io", ...)`.
  2. Em `12/spec.md` §2.1 (linhas 120–142), as constantes `codes::AI_*` estão em `UPPER_SNAKE_CASE` (`"AI_GATEWAY_UNAVAILABLE"`), violando a validação `ErrorCode::try_from` da Fase 01 (que exige `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`, como `"ai.gateway_unavailable"`).

### `[T-05]` Divergência de Assinatura na Trait `LogRedactor::redact` entre `01/02/spec.md` e `03/04/spec.md`
- **Onde:** `01/spec.md` §4.7 (linha 552), `01/02/spec.md` §3.2 (linha 284) e `03/04/spec.md` §3 (linha 51).
- **Detalhe:** `01/02/spec.md` declara `fn redact<'a>(&self, field_name: &str, input: &'a str) -> Cow<'a, str>` (2 parâmetros), enquanto `01/spec.md` e `03/04/spec.md` implementam `fn redact<'a>(&self, input: &'a str) -> Cow<'a, str>` (1 parâmetro).

### `[T-06]` Especificação Rasa de `impl specta::Type for BinaryPtyFrame` na Fase 02 (`02/05/spec.md`)
- **Onde:** `02/05/spec.md` §3.1 (linhas 48–58).
- **Detalhe:** A implementação de `specta::Type` para `BinaryPtyFrame(pub Bytes)` está resumida a um comentário `// Implementação de specta::Type exportando ArrayBuffer...`. Como `specta = "=2.0.0-rc.27"` não possui primitiva `ArrayBuffer`, deve-se especificar `#[derive(specta::Type)] #[specta(transparent)] pub struct BinaryPtyFrame(pub Vec<u8>);` (com serialização customizada via `InvokeResponseBody::Raw` no canal) para que `export_bindings` compile sem erro.

### `[T-07]` Modo Fallback `/proc` (`IntegrationMode::Proc`) na Fase 05 Nunca Produz Estado de Erro nem Habilita "Inspecionar"
- **Onde:** `05/03/spec.md` §1.5 (linhas 268–317), §1.6 (linha 359) e `05/04/spec.md` (linha 324).
- **Detalhe:** Em shells sem OSC 133 (`IntegrationMode::Proc`), `ProcFallbackMonitor` finaliza blocos com `BlockStatus::Unknown` (`exit_code: None`), que transita para `TabStatus::Idle` e é recusado por `command_block_error_context` (`shell.block_not_failed`). Se o próprio PTY encerrar com `exit_code != 0` durante uma execução em modo `/proc`, o bloco ativo deve ser finalizado como `BlockStatus::Failed` com o `exit_code` do PTY.

### `[T-08]` Erro de Tipagem no Retorno do Gatilho `/` no Playground da Fase 04 (`04/06/spec.md`) e Descarte de `LigaturesAddon` na Fase 10 (`10/04/spec.md`)
- **Onde:** `04/06/spec.md` §3.2 (linha 103) e `10/04/spec.md` §3.4 (linhas 204–209).
- **Detalhe:**
  1. `04/06/spec.md` linha 103 instrui retornar `{ kind: ' true', passthrough: true }` em `provide()`, mas `CompletionResult` não possui campo `kind` (deve ser `{ items: [], passthrough: true }`).
  2. `10/04/spec.md` §3.4 carrega `@xterm/addon-ligatures` quando `font.ligatures === true`, mas não guarda o `IDisposable` em um `WeakMap` para descartá-lo quando o usuário alternar `font.ligatures` para `false`.

### `[T-09]` Divergência na Assinatura de `useProjectsStore().detachTab` entre Fase 06 (`06/04`) e Fase 07 (`07/05`)
- **Onde:** `06/04/spec.md` §3.5 (linha 348) vs `07/05/spec.md` §3.2 (linha 230).
- **Detalhe:** `06/04/spec.md` define `detachTab(tabId: TabId): void` (1 argumento), mas `07/05/spec.md` chama `useProjectsStore().detachTab(tabId, projectId)` (2 argumentos).

### `[T-10]` Typo Case-Sensitive no Ícone Iconify `'mdi:source-Commit'` e Divergência de Assinatura `GitService::track_pane` na Fase 08
- **Onde:** `08/04/spec.md` §3.5.1 (linha 257) e `08/spec.md` §2.6 (linha 298) vs `08/01/spec.md` §3.8 (linha 597).
- **Detalhe:**
  1. `08/04/spec.md` linha 257 usa `'mdi:source-Commit'` (com `C` maiúsculo) em vez de `'mdi:source-commit'`.
  2. `08/spec.md` §2.6 omite `pty_id: Option<PtyId>` na assinatura de `GitService::track_pane`, divergindo de `08/01/spec.md` §3.8.

### `[T-11]` Divergências Internas de Assinaturas e Nomes de Funções na Fase 11 (`11/spec.md` vs `11/01–03/spec.md` e `11/03/plan.md`)
- **Onde:**
  - `11/spec.md` (§3.3–§3.5, §8) vs `11/01/spec.md` (§3.4, §3.5, §3.8) e `11/02/spec.md` (§3.4)
  - `11/spec.md` (linha 111) vs `11/03/spec.md` (linha 26) vs `11/03/plan.md` (linha 21)
- **Detalhe:**
  1. As funções de truncamento UTF-8 em `mt-bridge/src/prompt.rs` têm 3 nomes distintos nos 3 documentos: `truncate_utf8` (`11/spec.md`), `truncate_utf8_prefix` / `truncate_utf8_suffix` (`11/03/spec.md`) e `truncate_utf8_keep_start` / `truncate_utf8_keep_end` (`11/03/plan.md`).
  2. `MaxCodeClient::open_plan` tem 3 argumentos em `11/spec.md` e 4 argumentos (`pre_truncated: bool`) em `11/02/spec.md`; `BridgeHost` tem 6 métodos em `11/spec.md` e 7 métodos (`list_projects`) em `11/01/spec.md`.
  3. DTOs Tauri da Fase 11 (`ProjectFocusRequested`, `DeliveryReport`, `BridgeStatusDto`) não possuem `#[serde(rename_all = "camelCase")]`, misturando `snake_case` e `camelCase` no TypeScript.

### `[T-12]` Duplicação e Conflito no Subsistema de Autostart entre a Fase 09 (`--start-hidden`) e a Fase 17 (`--minimized`)
- **Onde:** `09/04/spec.md` (§3 `mt-os/src/autostart.rs`, `--start-hidden`, comandos `os_get_autostart` / `os_set_autostart`) vs `17/05/spec.md` (§3.3 `src-tauri/src/autostart.rs`, `--minimized`, comandos `os_autostart_get` / `os_autostart_set`).
- **Detalhe:** A Fase 17 reimplementa o autostart já entregue pela Fase 09 com nomes de comandos IPC diferentes e flag de linha de comando incompatível (`--minimized` em vez de `--start-hidden` esperado pelo `setup` da Fase 09).

### `[T-13]` Ponto de Extensão `EditorOpener` da Fase 09 Não Implementado na Fase 11
- **Onde:** `09/04/spec.md` (§2.2 `EditorLauncher::set_bridge_opener`) e `11/04/spec.md`.
- **Detalhe:** A Fase 09 define a trait `EditorOpener` para que a Fase 11 envie cliques em links `arquivo:linha` ao MaxCode via ponte loopback quando `EditorKind::MaxCode` estiver selecionado, mas a Fase 11 nunca implementa nem registra esse `EditorOpener`.

### `[T-14]` Métodos `AiService::prepare_suggest` e `run_suggest_stream` Indefinidos e Corpos `todo!()` na Fase 12 (`12/01`, `12/02`, `12/03`)
- **Onde:** `12/01/spec.md` (§3.5 linhas 1044–1106), `12/02/spec.md` (§3.7 linha 611) e `12/03/spec.md` (§3.1 linhas 66–113 e §3.2 linhas 195–201).
- **Detalhe:**
  1. `12/03/spec.md` chama `ai.prepare_suggest(&request)` e `ai.run_suggest_stream(...)`, mas `12/02/spec.md` define apenas `ai.suggest_command(...)`.
  2. `AiRepo` (`12/01/spec.md`) e `TauriAiContextSource` (`12/03/spec.md`) deixam os corpos dos métodos com `todo!()` na especificação.
  3. `InlineDiagnosis.vue` tem props `{ blockId, tabId }` em `12/spec.md` e `{ blockId, tabId, paneId }` em `12/05/spec.md`.

### `[T-15]` Conflito de Constraint SQLite `CHECK (location IN ('project', 'max_code'))` com `#[serde(rename_all = "camelCase")]` em `WorktreeLocationDto` na Fase 13 (`13/01/spec.md`)
- **Onde:** `13/01/spec.md` (§3.2 linha 130 vs §3.7 linhas 658–662).
- **Detalhe:** `WorktreeLocationDto::MaxCode` serializa para `"maxCode"` devido ao `camelCase`, violando a constraint `CHECK (location IN ('project', 'max_code'))` da tabela `agent_template_options` se não houver conversão explícita `as_db_str() -> "max_code"`.

### `[T-16]` Divergência de Assinatura em `TabStateRegistry::apply` entre Fase 05 (`05/03`) e Fase 13 (`13/01` e `13/03`)
- **Onde:** `05/03/spec.md` (§3.2–§3.3) vs `13/spec.md` (§1.2 linha 71) e `13/01/spec.md` (§3.4 linhas 376–388).
- **Detalhe:** `TabStateRegistry::apply` na Fase 05 recebe `(pane_id: PaneId, input: StateInput)` exigindo `RuntimeBinding { tab_id, pane_id, pty_id, session_id, generation }` tanto em `AttentionRequired` quanto em `AttentionCleared`, mas `TabStatePort` na Fase 13 não recebe nem repassa `pty_id` e `generation`.

### `[T-17]` Divergência nos Campos de `SpawnContext`, `ResolvedSpawn` e `WorkspaceLifecycleObserver` entre Fase 07 (`07/03`) e Fase 13 (`13/02`)
- **Onde:** `07/03/spec.md` (§3.2 linhas 167–204, §3.3 linhas 229–242) vs `13/02/spec.md` (§3.2 linhas 157–218).
- **Detalhe:** Na Fase 07, `ResolvedSpawn` possui `{ spawn_spec, shell_integration_enabled, initial_command }`, enquanto `AgentResolver` na Fase 13 tenta acessar `resolved.spec` e `resolved.initial_input`, além de ler `ctx.template_env` e `ctx.agent_id` na raiz de `SpawnContext` (quando na Fase 07 estão em `ctx.snapshot`).

### `[T-18]` Limitações Técnicas na Fase 14 (`routes/console.php` existente no Laravel 11/12 e 4 prefixos fixos de `artisanArgsTrigger`) e na Fase 15/16 (`DatabasePool` vs `DatabaseHandle` vs `mt_db::Db`)
- **Onde:**
  - `14/02/spec.md` (§3.4 linha 317) e `14/04/spec.md` (§3.5 linha 196)
  - `15/01/spec.md` (linha 1007: `mt_db::DatabasePool`), `15/02/spec.md` (§3.2 linha 160: `ChainPolicy::Semicolon`) e `16/02/spec.md` (linha 197: `mt_db::DatabaseHandle`)
- **Detalhe:**
  1. Em `14/02/spec.md`, `plan_console_routes` recusa-se a gravar automaticamente em `routes/console.php` se o arquivo já existir (forçando passos manuais em 100% dos projetos Laravel 11/12 padrão, que já vêm com `routes/console.php`).
  2. Em `14/04/spec.md`, `artisanArgsTrigger` só registra os 4 prefixos `['php', './v', 'ven', 'sai']`.
  3. Em `15/01/spec.md` e `16/02/spec.md`, os repositórios referenciam tipos `mt_db::DatabasePool` e `mt_db::DatabaseHandle`, enquanto a Fase 03 (`03/03/spec.md`) exporta a fachada `mt_db::Db`.
  4. Em `15/02/spec.md`, no fallback sem OSC 133 para `ChainPolicy::Semicolon` (`cmd1 ; cmd2`), o shell retorna apenas o `exit_code` do último comando, mascarando falhas de comandos intermediários.

---

## 3. Achados Estruturais (Ordem de Implementação, Dependências Quebradas entre Fases e Conflitos de Caminhos)

### `[E-01]` Ordem de Implementação Quebrada entre `07/03` e `07/04` (`ScrollbackRecorder` importado antes de existir)
- **Onde:** `07/03/spec.md` (§3.1 linha 52, §3.3 linhas 250 e 273) e `07/03/plan.md` (linha 51) vs `07/04/spec.md` (§2.1 linha 26).
- **Impacto:** `07/03` declara `pub mod recorder;` e usa `Arc<ScrollbackRecorder>` dentro de `WorkspaceManager`, mas `src-tauri/src/workspace/recorder.rs` só é criado em `07/04`, impedindo `07/03` de compilar e passar nos seus testes.

### `[E-02]` Importação de Módulo Inexistente `mt_core::pty` na Fase 07 (`07/03` e `07/05`) e Falta de `TabSessionId` na Fase 01 (`01/01`)
- **Onde:**
  - `01/01/spec.md` (§4.3 `mt-core/src/ids.rs`)
  - `07/01/spec.md` (§3.1 linhas 62–85)
  - `07/03/spec.md` (linhas 49, 159, 224) e `07/05/spec.md` (linha 79)
- **Impacto:**
  1. `07/03/spec.md` e `07/05/spec.md` fazem `use mt_core::pty::{ExitInfo, KillMode, PtyOwner, SpawnSpec};`, mas esses tipos residem em **`mt_pty`** (Fase 02) e o módulo `mt_core::pty` nunca foi criado!
  2. `TabSessionId` (usado nas Fases 07 e 17) falta na lista dos IDs gerados por `define_id!` na Fase 01 (`01/01/spec.md`) e é definido manualmente sem validação UUID v7 em `07/01/spec.md`.

### `[E-03]` Ordem de Verificação Invertida entre `01/03/plan.md` e `01/04/plan.md`
- **Onde:** `01/03/plan.md` (Seção 2 linhas 38–42).
- **Impacto:** O plano de `01/03` executa `npm run lint:style` (`stylelint "src/**/*.{scss,vue}"`) e `npm run typecheck:vue` antes de a etapa `01/04` criar os primeiros arquivos `.vue` e `.scss` em `src/`.

### `[E-04]` Duplicação e Colisão do Roteador de Deep Links (`ActivationRouter` da Fase 09 vs `DeepLinkRouter` da Fase 11)
- **Onde:** `09/03/spec.md` (§2 `mt-os/src/activation.rs`) vs `11/04/spec.md` (§3.3 `src-tauri/src/bridge/deep_link.rs`).
- **Impacto:** Ambas as fases registram listeners globais em `app.deep_link().on_open_url(...)` e leem `get_current()`, e o parser preliminar da Fase 09 rejeita os query params `worktree` e `correlationId` usados pela Fase 11. A Fase 11 deve implementar a trait `ActivationHandler` da Fase 09 em vez de instanciar um segundo listener do plugin `deep-link`.

### `[E-05]` Acoplamento de `agent_tab_open` ao Handler Tauri na Fase 13 (`13/02`) Impedindo Reuso pelo `AgentPort` da Fase 16 (`16/03`)
- **Onde:** `13/02/spec.md` (§3.4 linhas 276–307) vs `16/03/spec.md` (§3.1 linha 133).
- **Impacto:** Toda a orquestração de abertura de aba de Agente CLI com isolamento de worktree está dentro da função `#[tauri::command] pub async fn agent_tab_open(state: State<'_, AppAgentState>, ...)`, impedindo que o adaptador de `AgentPort` da Fase 16 a invoque sem extrair um método de serviço em `AppAgentState`.

### `[E-06]` Conflito Estrutural no Crate Compartilhado `mt-commands`, no Modelo de `tauri::State` e nas Pastas de Triggers entre as Fases 12, 14 e 15
- **Onde:**
  - `14/01/spec.md` (§3.1 `Cargo.toml`) vs `15/01/spec.md` (§2) e `15/03/spec.md` (§3.3)
  - `12/03/spec.md` (linha 27) e `14/03/spec.md` (linha 431) vs Fases 02, 03, 05, 06, 07, 08, 13, 15
  - `14/04/spec.md` (`src/composables/quick-commands/`) vs `15/04/spec.md` (`src/terminal/triggers/providers/`)
- **Impacto:**
  1. `mt-commands/Cargo.toml` nasce na Fase 14 com `edition = "2021"` e `mt-db` sem SQLite (`default-features = false`), conflitando com a Fase 15 que exige `edition = "2024"` e `mt-db` completo + `rusqlite`.
  2. As Fases 12 e 14 injetam seus serviços dentro da struct `AppState` da Fase 01 (`state.ai`, `state.commands`), enquanto as Fases 02, 03, 05, 06, 07, 08, 13 e 15 registram cada serviço separadamente via `app.manage(...)`.
  3. A Fase 14 registra triggers em `src/composables/quick-commands/` via `main.ts`, enquanto a Fase 15 assume a existência de `src/terminal/triggers/providers/index.ts`.

### `[E-07]` Colisões de Caminhos de Componentes Vue entre Fases (`TerminalPlayground.vue` e `AppTitleBar.vue`)
- **Onde:**
  - `TerminalPlayground`: criado em `src/components/terminal/__dev__/TerminalPlayground.vue` na Fase 04 (`04/06/spec.md` linha 26), mas referenciado como `src/views/dev/TerminalPlaygroundView.vue` na Fase 05 (`05/05/spec.md` linha 44).
  - `AppTitleBar`: criado em `src/components/shell/AppTitleBar.vue` na Fase 01 (`01/04/spec.md` linha 34), referenciado como `src/components/layout/AppTitleBar.vue` na Fase 11 (`11/05/spec.md` linha 33) e como `src/components/app/AppTitleBar.vue` na Fase 17 (`17/02/spec.md` linha 41).

### `[E-08]` Inconsistência nos Nomes de Scripts `package.json`, Binário `export_bindings` e Gerenciador de Pacotes (`npm` vs `pnpm`)
- **Onde:**
  - `01/03/spec.md` define `"typecheck"`, `"typecheck:vue"` e binário `export_bindings`.
  - `02/05/plan.md` chama `cargo run --bin export-bindings` (com hífen) e `02/06/plan.md` e `08/04/plan.md` usam `pnpm` em vez de `npm`.
  - `05/04/plan.md`, `06/04/plan.md`, `07/05/plan.md` e `11/05/plan.md` chamam `npm run check-types`, enquanto `17/04/plan.md` chama `npm run typecheck:tsgo`.
- **Correção Obrigatória:** Incluir no `package.json` da Fase 01 os aliases `"check-types": "npm run typecheck"` e `"typecheck:tsgo": "npm run typecheck"`, adicionar `default-run = "maxterminal"` em `src-tauri/Cargo.toml` e padronizar `npm` / `npx --no-install` em todos os `plan.md`.

### `[E-09]` Fragmentação dos Diretórios de Testes Frontend Vitest (`tests/` vs `src/tests/` vs `src/**/__tests__/`)
- **Onde:**
  - `01/03/spec.md` §4.3 configura `vitest.config.ts` com `include: ['tests/**/*.spec.ts']`.
  - Porém, `02/06/spec.md` e `14/04/spec.md` colocam testes em `src/**/__tests__/*.spec.ts`, e `13/05/spec.md` coloca testes em `src/tests/**/*.spec.ts` — diretórios que **não são cobertos** pelo glob `tests/**/*.spec.ts` do `vitest.config.ts` da Fase 01!

### `[E-10]` Duas Versões Major Distintas do Crate `notify` no Workspace Cargo (`notify = "8"` na Fase 06 vs `notify = "6"` na Fase 13)
- **Onde:** `06/02/spec.md` (`notify = "8"`, `notify-debouncer-full = "0.5"`) vs `13/spec.md` linha 126 (`notify = "6"`, `notify-debouncer-full = "0.3"`).
- **Correção Obrigatória:** Centralizar `notify = "8"` e `notify-debouncer-full = "0.5"` em `[workspace.dependencies]` do `src-tauri/Cargo.toml` para evitar duplicar a árvore de watchers inotify.

### `[E-11]` Caminhos Absolutos Hardcoded da Worktree Antiga (`wt-6f18b3d0`) em 25 Arquivos `plan.md`
- **Onde:** Todos os arquivos `plan.md` das Fases **08** (`08/01..04/plan.md`), **11** (`11/01/plan.md`), **13** (`13/01..05/plan.md`), **14** (`14/01..05/plan.md`), **15** (`15/01..05/plan.md`) e **17** (`17/01..05/plan.md`).
- **Impacto:** Os comandos de verificação instruem executar a partir de `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0`, podendo induzir agentes executores a rodar comandos fora da worktree ativa da tarefa.

---

## Inventário dos 205 Arquivos Auditados

- `./AGENTS.md` (Fonte Canônica)
- `./implementation/README.md` (Roadmap e Contratos Compartilhados)
- **Fase 01 (`01 - Setup e Fundação do Projeto`):** `instructions.md`, `spec.md` + 4 sub-implementações (`01` a `04`, cada uma com `spec.md` e `plan.md`) — 10 arquivos.
- **Fase 02 (`02 - Core PTY e Supervisão de Processos`):** `instructions.md`, `spec.md` + 6 sub-implementações (`01` a `06`, cada uma com `spec.md` e `plan.md`) — 14 arquivos.
- **Fase 03 (`03 - Persistência SQLite e Sanitização de Segredos`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
- **Fase 04 (`04 - Motor de Terminal Híbrido`):** `instructions.md`, `spec.md` + 6 sub-implementações (`01` a `06`, cada uma com `spec.md` e `plan.md`) — 14 arquivos.
- **Fase 05 (`05 - Shell Integration OSC 133 e Máquina de Estados das Abas`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
- **Fase 06 (`06 - Projetos e Layout de 2 Colunas`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
- **Fase 07 (`07 - Abas, Templates, Split Panes e Sessões`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
- **Fase 08 (`08 - Indicadores Contextuais Git, Worktree e Portas Ativas`):** `instructions.md`, `spec.md` + 4 sub-implementações (`01` a `04`, cada uma com `spec.md` e `plan.md`) — 10 arquivos.
- **Fase 09 (`09 - Integração com o Sistema Operacional (CachyOS)`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
- **Fase 10 (`10 - Temas e Importador Tabby`):** `instructions.md`, `spec.md` + 4 sub-implementações (`01` a `04`, cada uma com `spec.md` e `plan.md`) — 10 arquivos.
- **Fase 11 (`11 - Integração Bidirecional com MaxCode`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
- **Fase 12 (`12 - Integração MaxAiManager`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
- **Fase 13 (`13 - Agentes CLI, Telemetria e MaxSkills`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
- **Fase 14 (`14 - Comandos Rápidos Artisan e Atalhos Universais`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
- **Fase 15 (`15 - Grupos de Comandos`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
- **Fase 16 (`16 - Aba Fixa de GitHub Issues`):** `instructions.md`, `spec.md` + 6 sub-implementações (`01` a `06`, cada uma com `spec.md` e `plan.md`) — 14 arquivos.
- **Fase 17 (`17 - Busca Global, Qualidade, Performance e Empacotamento`):** `instructions.md`, `spec.md` + 5 sub-implementações (`01` a `05`, cada uma com `spec.md` e `plan.md`) — 12 arquivos.
