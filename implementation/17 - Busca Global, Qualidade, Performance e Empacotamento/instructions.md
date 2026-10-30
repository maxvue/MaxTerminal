# Fase 17 — Busca Global, Qualidade, Performance e Empacotamento

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) (§2 "Banco de Dados" — busca instantânea FTS5,
> §3 "Sanitização de Segredos", Diretrizes §2–§4) e [`../README.md`](../README.md) (contratos §2.1–§2.6).
> Especificação técnica detalhada: [`spec.md`](./spec.md).

---

## 1. Objetivo da Fase

Fase final e transversal. Ela **consome** os contratos das Fases 01–16 e entrega:

1. **Busca Global FTS5 instantânea** — paleta global (`GlobalSearchPalette.vue`) que pesquisa em paralelo
   blocos de comando (Fase 05), scrollback de sessões (Fase 07), projetos (06), templates de aba (07),
   grupos/execuções (15) e issues/comentários em cache (16), com ranking `bm25` por fonte, trechos destacados,
   filtros, paginação consistente em snapshot limitado, cancelamento e navegação até a origem.
2. **Performance mensurável** — benchmarks reprodutíveis (rajada de output, 20 abas, TUIs), baseline
   por ambiente, limites calibrados e aprovados antes da comparação, profiling e orçamento de bundle em CI.
3. **Qualidade** — matriz de testes completa (unit, integração IPC, E2E, não-órfãos, sanitização ponta a
   ponta, migrations sobre banco populado), pipeline de CI no GitHub Actions, acessibilidade e pt-BR.
4. **Hardening de segurança** — CSP final, capabilities Tauri v2 mínimas por janela, auditoria de
   dependências e revisão automatizada do bridge loopback e dos deep links.
5. **Empacotamento CachyOS/Arch** — `PKGBUILD` validado em chroot limpo, AppImage e `.deb`, `.desktop` com
   `x-scheme-handler/maxterminal`, ícones hicolor, autostart, atualização opcional, migração de dados e
   backup/restore do banco.
6. **Rastreabilidade** — matriz de requisitos funcionais do `AGENTS.md` com fase responsável, teste e evidência;
   existência de arquivos não equivale a funcionalidade implementada nem a teste aprovado.

Ao final desta fase o MaxTerminal é instalável via `makepkg -si`, passa em toda a matriz de testes e
atende às metas de desempenho documentadas.

---

## 2. Escopo

### 2.1. Incluído

| ID | Item | Descrição |
|----|------|-----------|
| I-01 | Motor de busca | `mt_db::search`: `SearchEngine`, `SearchSource`, consultas parametrizadas, `bm25` por seção, snapshots limitados e cursor ordinal opaco; cancelamento isolado por requisição/conexão. Sem truncar candidatos silenciosamente. |
| I-02 | Índices FTS auxiliares | Consumir `command_blocks_fts` (05) e `session_scrollback_fts` (07) pela infraestrutura da 03. `0011_17_search.sql` adiciona somente índices/projeções sanitizadas necessários às demais fontes; tabela de sessões permanece única. |
| I-03 | IPC de busca | Comandos `search_global`, `search_cancel`, `search_index_stats`, `search_index_rebuild`; evento `search://index-progress`. |
| I-04 | UI de busca | `GlobalSearchPalette.vue`, `SearchFilterBar.vue`, `SearchResultSection.vue`, `SearchResultItem.vue`, `HighlightedText.vue`; store `useSearchStore`; composables `useGlobalSearch`, `useSearchNavigation`, `useSearchShortcut`. |
| I-05 | Navegação | Resultado → seleciona projeto (06) → abre aba viva ou visualização de sessão arquivada (07) → rola até o bloco (04/05) ou linha de scrollback, com realce temporário. |
| I-06 | Atalhos | `Ctrl+K` e `Ctrl+Shift+F` (segundo toque/escalonamento) registrados no `keybindings.ts` da Fase 04 sem roubar teclas de TUIs (regras em spec §7.4). |
| I-07 | Benchmarks | `criterion`, corpus determinístico com 1M linhas e bytes conhecidos, produtor com sequência/checksum, rajadas finitas, 20 abas e TUIs; relatório JSON com ambiente, amostras e contabilidade de perdas. |
| I-08 | Profiling | Spans `tracing` padronizados, feature `tokio-console`, overlay de desempenho somente em dev (`PerfOverlay.vue`), guia de uso do Vue Devtools. |
| I-09 | Orçamento de bundle | `scripts/check-bundle-budget.mjs` e analisador compatível com Vite 8; baseline medido para JS/CSS/recursos, sem presumir compatibilidade de plugins Rollup. |
| I-10 | Matriz de testes | Vitest/mocks de contrato, Rust com SQLite/PTY reais, E2E frontend simulado e E2E Tauri real (`tauri-driver`/WebKitWebDriver + WebdriverIO) publicados separadamente; mocks não comprovam IPC nativo. |
| I-11 | CI | `.github/workflows/ci.yml`, `audit.yml`, `release.yml`, `bench.yml`. |
| I-12 | Acessibilidade e pt-BR | ARIA (combobox/listbox/dialog), foco preso e restaurado, contraste AA com tokens `--max-*`, `axe-core` no navegador real; Vitest verifica semântica. Revisão de textos e `Intl` `pt-BR`. |
| I-13 | Hardening | CSP final em `tauri.conf.json`, `capabilities/*.json` por janela, `cargo audit`, `cargo deny`, `npm audit`, testes de bridge loopback (Host/Origin/token) e de deep links (allowlist). |
| I-14 | Empacotamento | `packaging/arch/PKGBUILD`, `packaging/arch/PKGBUILD-bin`, `.SRCINFO`, `maxterminal.desktop`, ícones hicolor, `bundle` AppImage/deb no `tauri.conf.json`, validação do autostart da Fase 09 (`os_get_autostart`/`os_set_autostart`, `--start-hidden`), `tauri-plugin-updater` atrás de feature `updater` (desligada no pacote Arch). |
| I-15 | Dados entre versões | Teste de upgrade a partir de bancos-fixture de versões anteriores; comandos `db_backup_list`, `db_backup_create`, `db_backup_restore` (restauração aplicada no próximo boot). |
| I-16 | Rastreabilidade | Matriz em `spec.md` §14 + manifesto com requisito, fase, teste, execução e evidência; `scripts/check-traceability.mjs` valida referências e pendências. |

### 2.2. Fora de escopo

| Item | Motivo / Fase responsável |
|------|---------------------------|
| Criar novamente tabelas de sessões/comandos/saída e seus FTS | 03 fornece infraestrutura; 05 cria `sessions`, `command_blocks` e `command_blocks_fts`; 07 estende sessões e cria `session_scrollback`/FTS; 17 apenas consome esses domínios |
| Busca local no buffer de um terminal (`@xterm/addon-search`) | Fase 04 (a paleta apenas escalona a partir dela) |
| Busca semântica/RAG | Fase 12 (`/v1/rags/{slug}/search`); a busca global é léxica (FTS5) |
| Reimplementar bridge loopback, deep links, sanitizador ou supervisor de PTY | Fases 11, 11/09, 03 e 02 — aqui apenas auditoria, testes e correções pontuais via adaptadores |
| Tela genérica de preferências | Não definida no `AGENTS.md` |
| Pacotes para outras distribuições além de Arch (PKGBUILD), AppImage e `.deb` | Plataforma alvo é CachyOS |
| Assinatura/notarização de binários para macOS/Windows | Fora da plataforma alvo |

---

## 3. Pré-requisitos

### 3.1. Fases
- **01–16 concluídas.** Em especial:
   - **03**: infraestrutura `Db` (escritor único + pool de leitores), `FtsSpec`/`FtsTokenizer`,
     `build_match_query()`, `DbSetup`/`CodeMigration`/runner, backups e sanitização.
     **Não cria tabelas de domínio `sessions`, `commands` ou `scrollback_chunks`.**
  - **04**: `keybindings.ts`, `TerminalSearchBar.vue`, `CommandBlockLayer` com `scrollToBlock()`.
   - **05**: cria `sessions` mínima, `command_blocks` e `command_blocks_fts` em
     `0002_05_shell_sessions_command_blocks.sql` (spec §7); `useTabStatusStore`.
  - **06**: tabela `projects`; `useProjectsStore.select()`.
   - **07**: `tabs`, `tab_templates`, extensão da `sessions` criada pela 05, vínculos
     `tab_sessions`/`tab_session_panes`, `session_scrollback` + `session_scrollback_fts`;
     stores e visualizador de histórico com consultas paginadas, sem recriar sessões ou comandos.
   - **09**: janela principal alternada pelo modo Quake, tray e deep link `maxterminal://`;
     capability adicional somente se existir outra janela no contrato efetivamente integrado.
  - **11**: servidor loopback e handler de URI.
  - **15**: `command_groups`/`command_group_items`. **16**: cache `github_issues`.
- Adaptadores de leitura/navegação absorvem diferenças de nomes confirmadas por schema/bindings. Diferenças
  semânticas (sanitização, posse de processos, cancelamento) exigem correção na fase dona antes do aceite.
  Não criar tabelas alternativas nem fingir que um adaptador fornece garantias ausentes.
- As Fases 15/16 precisam fornecer o DDL definitivo de execuções/comentários, chaves estáveis e a barreira de
  sanitização. Enquanto esses contratos não forem verificados, registrar a fonte como pendente (spec §2).

### 3.2. Ferramentas

| Ferramenta | Uso | Verificação |
|------------|-----|-------------|
| `cargo-nextest` | Execução paralela de testes Rust | `cargo nextest --version` |
| `cargo-audit`, `cargo-deny` | Auditoria de dependências/licenças | `cargo audit --version`, `cargo deny --version` |
| `tauri-driver` + `WebKitWebDriver` | E2E nativo Linux; pacote varia por distro | registrar versões e localizar executável compatível com WebKitGTK 4.1 |
| `@wdio/cli` + `mocha` | Runner E2E | `npx wdio --version` |
| `tokio-console` | Profiling assíncrono | `tokio-console --version` |
| `hyperfine`, `pidstat` (`sysstat`), `xdotool` | Benchmarks de sistema | `hyperfine --version` |
| `xvfb-run` | E2E headless na CI | `which xvfb-run` |
| `namcap`, `makepkg` (`base-devel`, `pacman-contrib`) | Validação do PKGBUILD | `namcap -h` |
| `linuxdeploy`/`appimagetool` usados pelo bundler | AppImage | versões/checksums registrados; `npm run tauri -- build --bundles appimage` |
| `axe-core` / `vitest-axe` | Acessibilidade | `npx vitest --run tests/a11y` |

Build Linux requer GTK3, WebKitGTK 4.1, libsoup3, compilador/linker, `pkg-config`, OpenSSL e suporte
AppIndicator conforme a Fase 09. No CachyOS/Arch, confirmar `webkit2gtk-4.1`, `gtk3`, `libsoup3`,
`libappindicator-gtk3`, `librsvg`, `openssl` e `xdotool` quando requerido pelo grafo nativo; `base-devel`,
Rust e Node são ferramentas de build, não dependências obrigatórias do usuário final. Registrar versões
do kernel, glibc, GPU/driver, compositor e escala. Testes headless usam D-Bus de sessão e Xvfb;
tray/atalho global/Wayland exigem também sessão gráfica real. Detalhes em spec §11.

---

## 4. Entregáveis

### 4.1. Backend (`src-tauri/`)
- `crates/mt-db/src/search/mod.rs` — reexports, `SearchEngine`.
- `crates/mt-db/src/search/model.rs` — `SearchRequest`, `SearchFilters`, `SearchHit`, `SearchTarget`,
  `SearchSection`, `SearchResponse`, `SearchCursor`, `SearchSourceKind`, `SortMode`.
- `crates/mt-db/src/search/query.rs` — `QueryBuilder` (sobre `build_match_query()` da Fase 03), `ParsedQuery`.
- `crates/mt-db/src/search/cursor.rs` — cursor opaco vinculado ao snapshot, janela, consulta e expiração.
- `crates/mt-db/src/search/rank.rs` — `bm25` por fonte e desempate determinístico; sem comparar scores entre FTS.
- `crates/mt-db/src/search/snapshot.rs`, `projection.rs` — cache limitado e projeções sanitizadas derivadas.
- `crates/mt-db/src/search/cancel.rs` — `CancelRegistry` sobre `rusqlite::InterruptHandle`.
- `crates/mt-db/src/search/sources/{command_blocks,scrollback,sessions,projects,templates,groups,issues}.rs`
  — grupos incluem execuções; issues incluem comentários.
- `crates/mt-db/src/search/error.rs` — `SearchError` → `MtError` (códigos `search.*`).
- `crates/mt-db/migrations/0011_17_search.sql` (versão 11, após `0010_16_github.sql`, conforme D-01; sequência contígua `1..=11`).
- `crates/mt-db/benches/search.rs`, `crates/mt-db/examples/gen_search_corpus.rs`.
- `crates/mt-db/tests/search_*.rs`, `crates/mt-db/tests/upgrade_from_fixtures.rs`,
  `crates/mt-db/tests/fixtures/db/*.db.zst`.
- `src/commands/search.rs`, `src/commands/backup.rs`, `src/events/search.rs`.
- `src/perf.rs` — spans padronizados, comando de dev `perf_snapshot` (apenas `debug_assertions`).
- `tests/no_orphans.rs`, `tests/sanitizer_e2e.rs`, `tests/bridge_hardening.rs`, `tests/deep_link_allowlist.rs`.
- `tauri.conf.json` (CSP final, `bundle`), capabilities das janelas efetivamente existentes.
- `deny.toml`, `.cargo/audit.toml`.

### 4.2. Frontend (`src/`)
- `ipc/search.ts`, `ipc/backup.ts`; `types/bindings.ts` regenerado.
- `stores/search.ts` (`useSearchStore`).
- `composables/search/useGlobalSearch.ts`, `useSearchNavigation.ts`, `useSearchShortcut.ts`, `useHighlightSegments.ts`.
- `components/search/GlobalSearchPalette.vue`, `SearchFilterBar.vue`, `SearchResultSection.vue`,
  `SearchResultItem.vue`, `HighlightedText.vue`.
- `components/dev/PerfOverlay.vue` (somente `import.meta.env.DEV`).
- Testes `tests/unit/search/*.spec.ts`, `tests/a11y/*.spec.ts`, `tests/i18n/ptbr-literals.spec.ts`.

### 4.3. Qualidade, CI e empacotamento (raiz)
- `e2e/wdio.conf.ts`, `e2e/specs/{search,tabs,no-orphans,sanitizer,deeplink,tray}.e2e.ts`,
  configuração separada `e2e/frontend/` (IPC simulado), fixtures e diretórios XDG isolados por execução.
- `scripts/bench/{burst-yes.sh,burst-cat.sh,tabs-20.sh,tui.sh,report.mjs}`,
  `scripts/check-bundle-budget.mjs`, `scripts/check-traceability.mjs`, `bench/baseline.json`,
  `bench/environment.json`, amostras brutas, manifesto de recursos e `release-manifest.json`.
- `.github/workflows/{ci.yml,audit.yml,release.yml,bench.yml}`.
- `packaging/arch/{PKGBUILD,PKGBUILD-bin,.SRCINFO,maxterminal.install}`,
  `packaging/linux/maxterminal.desktop`, `packaging/linux/icons/hicolor/<N>x<N>/apps/maxterminal.png`
  (16, 32, 48, 64, 128, 256, 512) + `scalable/apps/maxterminal.svg`.
- `docs/` **não** é criado; instruções de profiling ficam em comentários dos scripts e em `spec.md` §9.

---

## 5. Critérios de Aceite

Executados na raiz da worktree (comandos `cargo` em `src-tauri/`), exit code 0 salvo indicação.

### 5.1. Busca global — corretude
- [ ] CA-01 `cargo nextest run -p mt-db search_` passa (testes listados em spec §12).
      `schema_contract` comprova a sequência: infraestrutura 03 sem tabelas de histórico; 05 cria
      `sessions`/`command_blocks`/FTS; 07 preserva ids e acrescenta vínculos/scrollback; 17 consome tudo
      sem criar tabelas substitutas `commands`/`scrollback_chunks`.
- [ ] CA-02 Proptest `query::never_syntax_error` (20 000 casos, incluindo `"`, `*`, `^`, `NEAR(`, `AND`,
      `OR`, `NOT`, `col:`, `(`, `)`, `-`, `+`, NUL, emoji, RTL) nunca produz `SQLITE_ERROR` em `MATCH`.
- [ ] CA-03 Corpus-oráculo contém explicitamente `migrate` em comando, saída, metadados de sessão,
      template e grupo; resultados por fonte coincidem com os ids esperados e segmentos destacados.
      Não exigir que `migrate` encontre "Migrar" por stemming inexistente. Incluir comentários/execuções.
- [ ] CA-04 Filtros combinados (`project_id`, período, `exit_code = 'nonzero'`, `tab_id`) retornam apenas
      linhas que satisfazem todos os predicados (ids esperados explícitos). `LIKE` não é oráculo de FTS;
      saída sem vínculo inequívoco a comando não recebe inferência de exit code.
- [ ] CA-05 Paginação: percorrer todas as páginas de 25 itens de uma consulta com 10 000 resultados retorna
      cada chave de domínio do snapshot exatamente uma vez, inclusive com inserts/updates/deletes
      concorrentes; expiração devolve `search.cursor_expired`, não muda a ordem silenciosamente.
- [ ] CA-06 Cursor adulterado (base64 alterado) → `MtError { code: 'search.invalid_cursor' }`.
- [ ] CA-07 Nenhum texto retornado contém segredo da tabela `sanitizer_cases.toml` (a fonte já é sanitizada;
      teste insere via API pública e busca pelo prefixo do segredo → 0 ocorrências do valor original).

### 5.2. Busca global — desempenho
- [ ] CA-08 `cargo bench -p mt-db --bench search` usa corpus determinístico de **1 000 000 linhas**,
      com seed, bytes, distribuição por fonte e 200 consultas versionadas (raras/comuns/prefixos/filtros).
      Publicar p50/p95/p99, cache frio/quente, throughput do writer concorrente e espaço FTS/WAL;
      atender os limites previamente aprovados em `bench/baseline.json` no mesmo ambiente (§9 da spec).
- [ ] CA-09 Com debounce configurado em 120 ms, 10 teclas separadas por 30 ms produzem exatamente
      uma consulta após a última tecla no teste de timers controlados. Consultas já iniciadas, respostas
      invertidas, cancelamento antes do registro e fechar/reabrir nunca sobrescrevem a geração atual.
- [ ] CA-10 Medir última tecla→pintura, debounce, fila, SQL e IPC separadamente no Tauri real; percentis
      atendem o orçamento aprovado para o ambiente. Cancelar A nunca interrompe B na conexão reutilizada.

### 5.3. Busca global — UI e navegação
- [ ] CA-11 `Ctrl+K` abre a paleta com foco no campo quando o foco está fora de um terminal em modo
      `passthrough`; `Ctrl+Shift+F` com a barra local da Fase 04 já aberta escalona para a paleta global
      levando o texto digitado; em TUI (alt-screen) nenhuma das teclas é interceptada.
- [ ] CA-12 Setas navegam entre itens de todas as seções; `Enter` abre; `Ctrl+Enter` abre em nova aba
      quando o alvo é template; `Esc` fecha e devolve o foco ao elemento anterior.
- [ ] CA-13 Abrir resultado de bloco de uma aba viva seleciona o projeto, ativa a aba e rola até o bloco
      com realce de 1,5 s; de sessão arquivada abre a visualização de histórico da Fase 07 posicionada no
      bloco; de scrollback posiciona na linha do chunk.
- [ ] CA-14 Destaques são renderizados por `HighlightedText.vue` sem `v-html` (regra ESLint
      `vue/no-v-html` em `error`; teste com payload `<img src=x onerror=alert(1)>` exibe texto literal).
- [ ] CA-15 Todos os textos da paleta estão em pt-BR ("Buscar em tudo…", "Nenhum resultado para “x”",
      "Carregar mais", "Blocos de comando", "Histórico de sessões", "Projetos", "Templates",
      "Grupos de comandos", "Issues") e datas são formatadas com `Intl.DateTimeFormat('pt-BR')`.

### 5.4. Performance do terminal
- [ ] CA-16 Rajadas finitas de 1 GiB e arquivo de 500 MiB medem bytes produzidos/lidos/enviados/consumidos,
      hashes e lacunas por sequência. Zero perda/duplicação/reordenação no canal anexado com ACK saudável;
      perdas permitidas no replay desanexado são contabilizadas por `Gap`, e truncamentos de persistência
      ficam explícitos. Cliques, crescimento de filas, CPU, PSS/RSS e recuperação pós-rajada atendem baseline.
- [ ] CA-17 `tabs-20.sh`: 20 abas, 200 linhas/s/aba com tamanho de linha e duração registrados; medir
      app, WebKit e filhos separadamente, abas visíveis/ocultas, busca simultânea e término de todos os filhos.
- [ ] CA-18 Input→echo usa id de correlação e relógio monotônico do frontend (ou relógios sincronizados
      com erro estimado); callback de escrita xterm mede consumo, não pintura. Reportar ambos quando disponíveis.
- [ ] CA-19 TUIs `htop`/`vim` com fixture de 10 000 linhas: medir intervalos de frame, input e troca de aba
      em WebGL e fallback; não alegar FPS de Chromium medido em WebKitGTK nem frames visíveis no Xvfb.
- [ ] CA-20 Ociosidade: 5 abas paradas, janela visível, 60 s após aquecimento; medir CPU por núcleo,
      despertares, PSS/RSS total e backlog, confrontando limites do ambiente.
- [ ] CA-21 Bundle inicial/assíncrono/CSS/recursos: tamanhos crus e gzip e diferenças contra baseline
      aprovado. Sem baseline válido ou com ambiente diferente, status "não comparável", nunca aprovação.

### 5.5. Qualidade e CI
- [ ] CA-22 `npm run check` passa (lint, lint:style, typecheck `tsgo`, `vue-tsc`, Vitest, fmt, clippy
      `-D warnings`, nextest).
- [ ] CA-23 Publicar cobertura Rust/Vitest por módulo e branches críticos; não regredir o baseline
      aprovado. Casos de perda, segredo, órfãos, cancelamento e migrations são obrigatórios independentemente
      da porcentagem; exclusões documentadas não podem esconder esses caminhos.
- [ ] CA-24 `npm run e2e` (WebdriverIO + `tauri-driver`, `xvfb-run` na CI) passa com as 6 specs de §4.3.
- [ ] CA-25 `cargo nextest run -p maxterminal --test no_orphans`: após fechar aba, "Iniciar nova Sessão",
      saída explícita e `SIGTERM` no app, nenhum descendente supervisionado permanece, incluindo zumbis,
      double-fork e `setsid`, identificado por `(pid, start_time)` em ambiente isolado. Fechar janela com
      tarefas vivas minimiza para tray e preserva processos; `SIGKILL` exige teste de recuperação no próximo
      boot, sem prometer limpeza síncrona impossível (spec §8).
- [ ] CA-26 `cargo nextest run -p maxterminal --test sanitizer_e2e`: segredos digitados no PTY
      (`Bearer`, `APP_KEY=base64:`, cookie `laravel_session`, senha em URL) não aparecem em
      `command_blocks`, `session_scrollback`, projeções, FTS (incluindo vocabulário), DB/WAL/backups,
      logs, snippets e payload do "Inspecionar". Cobrir todos os cortes UTF-8/ANSI e flush/shutdown;
      usar apenas segredos sintéticos e verificar também páginas livres, não só SELECT lógico.
- [ ] CA-27 `upgrade_from_fixtures`: cada banco-fixture de versão anterior populado (≥ 10 000 blocos)
      migra até a última versão, `PRAGMA integrity_check` = `ok`, `integrity-check` de todas as FTS sem
      erro, contagens preservadas e backup pré-migration criado.
- [ ] CA-28 Workflow `ci.yml` executa em PR e push para a branch de integração, com jobs `frontend`, `rust`, `e2e`,
      `audit`, `bundle-budget`, `traceability`, `package` e é obrigatório (branch protection documentada
      no próprio workflow em comentário).
- [ ] CA-29 `axe-core` sem violações `serious`/`critical` na paleta, lista de projetos, barra de abas e
      modais (Vitest + E2E).

### 5.6. Segurança
- [ ] CA-30 CSP efetiva de produção sem `unsafe-eval`, origens remotas de script ou CDNs. Rede da IA/GitHub
      no Rust não exige liberá-la em `connect-src` do WebView; testar IPC, recursos e bloqueios no pacote real.
- [ ] CA-31 `capabilities_minimal` inspeciona permissões efetivas e labels reais (spec §10.2), inclusive
      restrições dos comandos próprios registradas no manifesto do aplicativo; teste negativo de janela/
      origem não autorizada. Não supor que capabilities de plugins restringem automaticamente todo comando próprio.
- [ ] CA-32 `cargo audit` e `cargo deny check` sem vulnerabilidades não justificadas; `npm audit
      --omit=dev --audit-level=high` com 0 achados.
- [ ] CA-33 `bridge_hardening`: requisição ao loopback sem token → 401; `Host` diferente de
      `127.0.0.1:<porta>` ou `localhost:<porta>` → 403 `bridge.host_forbidden`; `Origin` não permitido → 403; servidor nunca escuta em `0.0.0.0`/`::`.
- [ ] CA-34 `deep_link_allowlist`: URIs `maxterminal://` fora das rotas permitidas, com `..`, caminhos
      fora de projetos cadastrados ou comandos embutidos são rejeitadas pelos códigos da camada proprietária: `os.activation_invalid` (estrutura), `bridge.invalid_uri` (rota/parâmetros), `bridge.invalid_path` (caminho inválido), `bridge.project_not_found` (sem cadastro) ou `bridge.cwd_outside_project` (escape de cwd), sem efeitos colaterais.

### 5.7. Empacotamento e dados
- [ ] CA-35 Build Arch em chroot limpo e fontes verificadas gera `maxterminal-<ver>-1-x86_64.pkg.tar.zst`;
      `namcap PKGBUILD` e `namcap *.pkg.tar.zst` sem erros (`E:`).
- [ ] CA-36 Pacote instalado: `desktop-file-validate /usr/share/applications/maxterminal.desktop` passa;
      associação MIME é verificada em usuário de teste que a selecionou (pacote não altera preferência global);
      URI válida da Fase 09/11 abre/foca o app, argumento inválido é rejeitado; ícones presentes em
      `/usr/share/icons/hicolor/{16x16..512x512,scalable}/apps/`.
- [ ] CA-37 `npm run tauri -- build --bundles appimage,deb` gera AppImage e `.deb` em base compatível
      declarada; testar instalação/execução em VM limpa sem `~/GitHub`, Node ou symlinks de desenvolvimento.
      Recursos, ícones, fontes e hooks funcionam offline; testar FUSE e extração da AppImage.
- [ ] CA-38 Autostart: ativar/desativar na bandeja cria/remove `~/.config/autostart/maxterminal.desktop`
      com `--start-hidden` quando `start_minimized`; consumir `os_get_autostart`/`os_set_autostart` da 09, iniciar na bandeja (ou fallback acessível sem tray) e restaurar abas "Adicionar ao Workspace" pela 07.
- [ ] CA-39 Build do pacote Arch compila sem a feature `updater`; build AppImage com `updater` verifica
      assinatura minisign e nunca instala atualização sem confirmação do usuário.
- [ ] CA-40 `db_backup_create` gera arquivo íntegro em `$XDG_DATA_HOME/maxterminal/backups/`;
      `db_backup_restore` agenda a restauração no próximo boot; validar schema/checksums/integridade,
      interrupção de energia, rollback e sidecars WAL antes de reabrir conexões. Reusar backup da 03.
- [ ] CA-42 Duas builds limpas do mesmo manifesto reproduzem os hashes dos payloads não assinados;
      divergências são investigadas com comparação binária e bloqueiam alegação de reprodução bit a bit.
      Manifesto inclui lockfiles, commits das libs Max, toolchains, imagem base, recursos, SBOM e hashes.

### 5.8. Rastreabilidade
- [ ] CA-41 `node scripts/check-traceability.mjs` valida ids estáveis dos requisitos da spec §14,
      fase responsável, teste executável e artefato da execução; nenhum requisito obrigatório fica sem
      evidência. Check estrutural não certifica comportamento. Toda pendência tem responsável explícito.

---

## 6. Riscos e Mitigação

| ID | Risco | Impacto | Mitigação |
|----|-------|---------|-----------|
| R-01 | `bm25` sobre termos comuns é caro | Alto | Medir plano real; índice e filtros corretos, deadline com erro explícito; nunca confundir `rowid` com tempo nem cortar resultados silenciosamente. |
| R-02 | Conflito de atalhos com shell/TUIs (`Ctrl+K` = kill-line no readline; `Ctrl+Shift+F` = busca local da Fase 04) | Alto | Regras de precedência (spec §7.4): nunca interceptar em `passthrough`; `Ctrl+K` só fora do xterm ou com input rico vazio; `Ctrl+Shift+F` escalona apenas com a barra local aberta; teclas configuráveis em `keybindings.ts`. |
| R-03 | Nomes de colunas divergentes nas Fases 05/07/15/16 | Médio | Adaptadores isolados em `sources/*.rs` + teste `schema_contract` que valida colunas via `PRAGMA table_info` e falha com mensagem clara. |
| R-04 | Cancelamento atingir outra consulta | Alto | Lease exclusivo, registro por janela/id e descarte coordenado do handle antes de devolver conexão; deadline cooperativo configurado/medido, fila limitada. |
| R-05 | E2E instável com WebKitGTK/`tauri-driver` | Médio | Retentativa apenas em nível de spec (máx. 1), `xvfb-run` com resolução fixa, esperas explícitas por `data-testid`; testes críticos (não-órfãos, sanitização) também existem em Rust puro. |
| R-06 | Libs `@maxvue/*` locais indisponíveis na CI/pacote | Alto | Fontes/tarballs por commit e checksum em manifesto; bundle autocontido. Publicação AUR depende de fontes redistribuíveis sem credenciais privadas. |
| R-07 | Benchmarks ruidosos em runners compartilhados | Médio | Runner dedicado, séries repetidas e tolerância por métrica derivada da variabilidade; CI comum faz smoke sem aprovar limites de hardware. |
| R-08 | CSP restritiva quebrar WebGL/estilos do xterm ou UnoCSS | Médio | `style-src 'self' 'unsafe-inline'` (necessário a xterm e Vue) documentado; `script-src` sem exceções; E2E de renderização WebGL. |
| R-09 | `tauri-plugin-updater` conflitar com pacman | Médio | Feature desligada no PKGBUILD; ativa só no AppImage; nunca auto-instala. |
| R-10 | Restauração de backup corromper dados ativos | Alto | Restauração só no boot, antes de abrir conexões; backup do banco atual antes; `quick_check` no arquivo restaurado; rollback automático em falha. |
| R-11 | Matriz de rastreabilidade desatualizar | Baixo | Requisitos identificados explicitamente; CI valida referências/evidências; revisão semântica confirma cobertura. |
| R-12 | Índices FTS auxiliares divergirem da tabela de origem | Médio | Triggers `ai/ad/au` gerados pelo `FtsSpec` da Fase 03 + `search_index_rebuild` + `integrity-check` na manutenção diária. |
