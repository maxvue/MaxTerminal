# Fase 03 — Persistência SQLite e Sanitização de Segredos (`instructions.md`)

> Fonte da verdade: [`../../AGENTS.md`](../../AGENTS.md) (§2 Banco de Dados, §3 Sanitização de Segredos, Diretriz §4)
> e contratos compartilhados de [`../README.md`](../README.md) (§2.1–§2.6).
> Especificação técnica detalhada: [`./spec.md`](./spec.md).

---

## 1. Objetivo da Fase

Entregar o crate **`mt-db`** como a única porta de entrada para persistência do MaxTerminal, com:

- Um banco SQLite local em `$XDG_DATA_HOME/maxterminal/maxterminal.db`, aberto sempre em modo **WAL** com
  `foreign_keys=ON`, `busy_timeout=5000` e `synchronous=NORMAL`.
- Arquitetura de concorrência **uma thread escritora + pool de leitores**, com API assíncrona que nunca bloqueia
  a thread principal do Tauri nem as threads de leitura de PTY (Fase 02).
- Um **runner de migrations versionadas** (`NNNN_<fase>_<desc>.sql`) com checksum, transação por migration e
  backup automático antes de aplicar migrations pendentes.
- O **schema de suporte** (`schema_migrations`, `settings`) e a **infraestrutura FTS5** (tokenizer
  `unicode61 remove_diacritics 2`, padrão *external content* + triggers, construtor seguro de consultas `MATCH`)
  para índices de domínio criados pelas Fases 05 e 07, sem antecipar tabelas nem FKs dessas fases.
- Um **escritor em lote** (`BatchWriter`) para inserts de alto volume (scrollback), com política de flush por
  tempo/quantidade/bytes e backpressure explícito.
- O módulo **`sanitizer`** completo — `sanitize(&str) -> Cow<'_, str>` e `StreamSanitizer` para fluxos
  fragmentados — aplicado antes de qualquer escrita ou envio externo, cobrindo senhas em URLs, `APP_KEY`,
  `Bearer`, cookies, tokens GitHub, AWS, `sk-`, JWT, atribuições sensíveis e chaves PEM. A garantia é sobre a passagem
  obrigatória pelo pipeline e os formatos cobertos, **não** detecção perfeita de qualquer segredo arbitrário.
- Política de **retenção, manutenção (`optimize`, checkpoint, `incremental_vacuum`) e backup** com rotação.
- Comandos IPC **`db_settings_get`** e **`db_settings_set`**, evento `db://settings-changed`, wrapper tipado
  `src/ipc/db.ts`, store `useSettingsStore` e composable `useSetting`.
- Contratos de adoção para repositórios, paginação e busca FTS futuros; fixtures independentes validam
  limites, cursores e sincronização de índices sem criar tabelas de produto nesta fase.

Ao final desta fase o app abre (ou cria) o banco, aplica migrations, persiste configurações chave/valor e expõe
os pontos de extensão (migrations, FTS, batch, retenção, registro de settings) para as fases seguintes.
Os exemplos SQL/Rust/TS são contratos para implementação futura; este planejamento não atesta código já executável.

---

## 2. Escopo

### 2.1. Incluído

| # | Item | Detalhe |
|---|------|---------|
| I-01 | Escolha do driver | `rusqlite` com feature `bundled` (SQLite compilado com FTS5, JSON1, `STRICT`) + `r2d2`/`r2d2_sqlite` para leitores. Justificativa completa em spec §1. `tauri-plugin-sql` **rejeitado**. |
| I-02 | Abertura e PRAGMAs | `journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5000`, `temp_store=MEMORY`, `cache_size=-16000`, `mmap_size=268435456`, `auto_vacuum=INCREMENTAL` (apenas em banco novo), `query_only=ON` nos leitores. |
| I-03 | Caminho e permissões | Campo `mt_core::paths::AppPaths::db_path` (Fase 01); isolamento por `MAXTERMINAL_HOME`; override adicional `MAXTERMINAL_DB_PATH` definido nesta fase, absoluto e apenas debug/testes; diretório `0700`, arquivo `.db`/`-wal`/`-shm` `0600`. |
| I-04 | Writer thread única | `std::thread` dona da `Connection` de escrita, fila `crossbeam_channel` limitada, respostas via `tokio::sync::oneshot`. |
| I-05 | Pool de leitores | `r2d2` com 4 conexões somente leitura, execução em `tokio::task::spawn_blocking`. |
| I-06 | Runner de migrations | Migrations SQL embutidas (`include_dir!`) + migrations em código (`CodeMigration`), checksum SHA-256, transação por migration, `PRAGMA user_version` espelhado, bloqueio de downgrade. |
| I-07 | Schema base | Tabela `schema_migrations` (bootstrap do runner) e migration `0001_03_base.sql` com a tabela `settings` (`STRICT`, `WITHOUT ROWID`, JSON validado). |
| I-08 | Infraestrutura FTS5 | `FtsSpec`, gerador external-content + triggers, `build_match_query()`, `rebuild`/`optimize`/`integrity-check`; índices concretos pertencem às migrations 05/07. |
| I-09 | `BatchWriter<R>` | `BatchRecord` como extensão Rust confiável; flush a cada 200 ms, 512 registros ou 1 MiB; uma transação por flush; `push()` não bloqueante; barreira de commit e erro explícito no shutdown. |
| I-10 | `sanitizer` | `sanitize()`, `sanitize_with()`, `StreamSanitizer`, newtype `SanitizedText`, regras catalogadas por `SecretKind`, `RegexSet` como pré-filtro, idempotência. |
| I-11 | Redator de logs | Implementação do ponto de extensão `LogRedactor` da Fase 01 usando `sanitize()` (plugado em `src-tauri/src/logging.rs`). |
| I-12 | Retenção e manutenção | `RetentionRule` registrável por fase, `MaintenanceScheduler` (no startup + a cada 24 h), `PRAGMA optimize`, checkpoint PASSIVE (TRUNCATE apenas ocioso/shutdown), `incremental_vacuum`, `optimize` das tabelas FTS registradas. |
| I-13 | Backup | Online Backup API do SQLite: backup pré-migration e diário, rotação (7 diários + 3 pré-migration), `quick_check` no startup; recuperação preserva banco e WAL e exige ação explícita, sem reset silencioso. |
| I-14 | Settings | `SettingsRegistry` (descritores com default e validador), namespace livre `ui.*`, repositório `SettingsRepo`, IPC `db_settings_get` / `db_settings_set`, evento `db://settings-changed`. |
| I-15 | Erros | `DbError` (`thiserror`) + conversão para `MtError` com códigos `db.*` (constantes de `ErrorCode`, padrão da Fase 01). |
| I-16 | Frontend | `src/ipc/db.ts`, `src/types/settings.ts` (interface `SettingsSchema` extensível por *declaration merging*), `src/stores/settings.ts` (`useSettingsStore`), `src/composables/useSetting.ts`. |
| I-17 | Testes e benchmarks | Testes de tabela do sanitizador, proptest, testes de migrations/concorrência/batch/backup/retenção, testes IPC, Vitest da store, benchmarks `criterion`. |
| I-18 | Extensões e ownership | `Db::read/write`, `DbSetup`, `BatchWriter` e registros de FTS/retenção; 05 cria `sessions`, `command_blocks`, `command_blocks_fts`; 07 estende sessões e cria scrollback. |
| I-19 | Sanitização nas fronteiras | Decoder incremental, máquina de estados por stream, descarte conservador em overflow/lacuna/EOF, erros estruturados, logs e payload externo final; settings JSON inspecionado recursivamente antes da escrita. |

### 2.2. Fora de escopo (e fase responsável)

| Item | Fase |
|------|------|
| `sessions` mínima, `command_blocks`, `command_blocks_fts`, `SessionProvider`, repositórios e produção de eventos OSC 133 | 05 |
| Tabelas `projects`, `project_roots` (Pasta de Projetos) e persistência de identidade (ícone/cor) | 06 |
| Abas/templates/painéis; extensão da `sessions` criada pela 05, tabela de scrollback e FTS; orquestração "Iniciar nova Sessão" e UI de histórico | 07 |
| Regras e settings concretas de retenção de sessões/scrollback, registradas na infraestrutura da 03 | 07 |
| Persistência de temas importados do Tabby | 10 |
| Transporte real ao MaxCode/MaxAiManager; obrigatório consumir `ExternalPayload` sanitizado na fronteira final definida nesta fase | 11/12 |
| Configurações de MaxAiManager (gateway, harness/modelo/raciocínio padrão) — registradas no `SettingsRegistry` | 12 |
| Memória contextual de comandos recorrentes no SQLite | 13 |
| Tabelas de grupos de comandos locais/globais | 15 |
| Cache local de issues do GitHub | 16 |
| UI de busca global FTS5, benchmarks de rajada end-to-end, hardening final e empacotamento | 17 |
| Tela de preferências/configurações visual (nenhuma fase anterior a 17 a exige; o AGENTS.md não define uma) | — |

> Regra: **cada fase adiciona apenas suas próprias migrations** (`NNNN_<fase>_<desc>.sql`). A Fase 03 entrega
> somente `0001_03_base.sql`. A 05 adiciona `NNNN_05_shell_sessions_command_blocks.sql` com o próximo número
> global disponível. A 07 estende `sessions`, sem recriá-la, e cria o próprio scrollback. Não há tabela `commands`
> nem migration `0002_03_history.sql`/`0003_03_history_fts.sql` a entregar na 03.

---

## 3. Pré-requisitos

### 3.1. Fases
- **Fase 01** concluída: workspace Cargo e `mt-core` (`MtError`, `ErrorCode`, `MtResult`,
  `AppPaths::db_path`), `tauri-specta` (`src-tauri/src/specta.rs`), ponto de extensão `LogRedactor`,
  `src/ipc/core.ts`, `src/types/bindings.ts`, `useIpcError`, Vitest com `@tauri-apps/api/mocks`.
- **Não** depende da Fase 02 (a Fase 05 é quem conecta PTY → `BatchWriter`).
- Helpers reais da 01: `call(command, request)` e `MtIpcError`, não um `unwrap` presumido; `AppPaths` está em
  `AppState.paths`; bootstrap frontend usa `registerBootstrapStep`. Dados sintéticos exercitam os repositórios
  sem PTY, shell integration, projetos ou abas.

### 3.2. Ferramentas
| Ferramenta | Versão | Verificação |
|------------|--------|-------------|
| Rust stable (edição 2024) | ≥ 1.85 | `rustc --version` |
| Compilador C (para `libsqlite3-sys` bundled) | `gcc`/`clang` do `base-devel` | `cc --version` |
| `sqlite3` CLI (inspeção) e SQLite bundled (runtime) | ≥ 3.51.3 | `sqlite3 --version`; `SELECT sqlite_version()` na conexão do app |
| `cargo-criterion` (opcional) ou `cargo bench` | — | `cargo bench --help` |
| Node/npm conforme Fase 01 | — | `npm --version` |

---

## 4. Entregáveis

### 4.1. Crate `src-tauri/crates/mt-db/`
- `Cargo.toml` (dependências: `rusqlite` [`bundled`, `backup`, `functions`, `serde_json`], `r2d2`, `r2d2_sqlite`,
  `crossbeam-channel`, `tokio` [`sync`, `rt`, `time`], `regex`, `include_dir`, `sha2`, `serde`, `serde_json`,
  `thiserror`, `tracing`, `specta`, `mt-core`; dev: `tempfile`, `proptest`, `rstest`, `criterion`)
- `migrations/0001_03_base.sql`
- `src/models.rs` — DTOs de settings/status/manutenção; histórico é definido nas fases donas
- `src/lib.rs` — re-exports e `Db` (fachada)
- `src/config.rs` — `DbConfig`, `PragmaSet`
- `src/connection.rs` — abertura, PRAGMAs, permissões, `quick_check`
- `src/error.rs` — `DbError`, códigos `db.*`, `impl From<DbError> for MtError`
- `src/writer.rs` — `DbWriter`, `WriteJob`, thread escritora
- `src/reader.rs` — `ReadPool`
- `src/batch.rs` — `BatchRecord`, `BatchWriter`, `BatchPolicy`, `BatchStats`
- `src/migrations/mod.rs`, `src/migrations/runner.rs`, `src/migrations/registry.rs`
- `src/fts.rs` — `FtsSpec`, `FtsTokenizer`, `build_match_query`, manutenção FTS
- `src/settings/mod.rs`, `src/settings/registry.rs`, `src/settings/repo.rs`
- `src/maintenance/mod.rs`, `src/maintenance/retention.rs`, `src/maintenance/backup.rs`, `src/maintenance/scheduler.rs`
- `src/sanitizer/mod.rs`, `src/sanitizer/rules.rs`, `src/sanitizer/stream.rs`
- `src/time.rs` — `now_ms()` (Unix epoch em ms, `i64`)
- `tests/migrations.rs`, `tests/concurrency.rs`, `tests/batch.rs`, `tests/backup.rs`, `tests/retention.rs`,
  `tests/settings.rs`, `tests/fts.rs`, `tests/sanitizer_table.rs`, `tests/sanitizer_props.rs`
- `benches/sanitizer.rs`, `benches/batch.rs`
- `tests/fixtures/sanitizer_cases.toml` (tabela de casos positivos/negativos)
- `tests/pagination.rs` (fixtures), `tests/sanitizer_stream.rs`, `tests/sanitizer_sinks.rs`

### 4.2. Crate binário `src-tauri/src/`
- `commands/db.rs` — settings, status e recuperação de spec §10; histórico é IPC de domínio futuro
- `events/db.rs` — payloads `SettingsChanged`, `DbRecovered`, `DbMaintenanceFinished`, `DbWriterLag`
- `state.rs` — `DbService` gerenciado separadamente de `AppState`, com prontidão e recuperação
- `lib.rs` — inicialização em worker bloqueante, estado de prontidão, agendador e shutdown assíncrono coordenado antes de sair
- `logging.rs` — `SanitizerLogRedactor` plugado no `LogRedactor`
- `specta.rs` — registro dos novos comandos e eventos

### 4.3. Frontend `src/`
- `ipc/db.ts`, `types/settings.ts`, `stores/settings.ts`, `composables/useSetting.ts`
- `types/bindings.ts` regenerado
- `tests/ipc/db.spec.ts`, `tests/stores/settings.spec.ts`, `tests/composables/useSetting.spec.ts`
- Integração mínima de loading/erro/recuperação no shell da 01

---

## 5. Critérios de Aceite

Todos executados na raiz da worktree (comandos `cargo` dentro de `src-tauri/`), com **exit code 0** salvo indicação.

### 5.1. Build e qualidade
- [ ] CA-01 `cargo build -p mt-db -p maxterminal` compila; inspecionar `ldd target/debug/maxterminal` e confirmar ausência de libsqlite3 dinâmica (driver bundled).
- [ ] CA-02 `cargo clippy -p mt-db -p maxterminal --all-targets -- -D warnings` passa; `cargo fmt --all --check` passa.
- [ ] CA-03 `cargo test -p mt-db` passa com todos os testes listados em spec §15.
- [ ] CA-04 Regeneração de bindings sem drift; exports incluem todos os comandos/DTOs de spec §10: settings, snapshots, status/recuperação e eventos de infraestrutura.
- [ ] CA-05 `npm run typecheck`, `npm run lint` e `npm run test` passam; cobertura ≥ 85% em `src/stores/settings.ts`, `src/ipc/db.ts`, `src/composables/useSetting.ts`.

### 5.2. Banco, PRAGMAs e permissões
- [ ] CA-06 Após `npm run tauri dev`, existe `~/.local/share/maxterminal/maxterminal.db` (ou `$XDG_DATA_HOME/...`).
- [ ] CA-07 `sqlite3 ~/.local/share/maxterminal/maxterminal.db 'PRAGMA journal_mode;'` imprime `wal`; `PRAGMA auto_vacuum;` imprime `2`.
- [ ] CA-08 Teste `connection::pragmas_applied` verifica, numa conexão de escrita, `foreign_keys=1`, `busy_timeout=5000`, `synchronous=1`; e numa de leitura `query_only=1`.
- [ ] CA-09 `stat -c %a` retorna `700` para o diretório e `600` para `.db`, `-wal` e `-shm`.
- [ ] CA-10 `sqlite3 ... "SELECT sqlite_compileoption_used('ENABLE_FTS5');"` executado via conexão do app (teste `fts::fts5_available`) retorna `1`.

### 5.3. Migrations
- [ ] CA-11 Banco novo: migration `1|3|base`; `PRAGMA user_version` → `1`; somente tabelas de suporte, sem `sessions`, `command_blocks`, `commands` ou scrollback; `foreign_key_check` vazio.
- [ ] CA-12 Reabrir o banco não reaplica migrations (teste `migrations::idempotent_reopen`).
- [ ] CA-13 Alterar o conteúdo de uma migration já aplicada faz `Db::open` falhar com `db.migration_checksum_mismatch` (teste).
- [ ] CA-14 Migration com SQL inválido faz rollback completo (nenhuma tabela parcial, `schema_migrations` inalterada) e retorna `db.migration_failed` com `details.version` (teste).
- [ ] CA-15 Banco com `user_version` maior que a última migration conhecida recusa abrir com `db.schema_too_new` (teste) — protege contra downgrade do app.
- [ ] CA-16 Teste `migrations::naming_contract` falha se algum arquivo em `migrations/` não casar com `^\d{4}_\d{2}_[a-z0-9_]+\.sql$`, se houver versões duplicadas ou lacunas na sequência.
- [ ] CA-17 Existindo migrations pendentes em banco não vazio, um arquivo `backups/maxterminal-premigration-v<N>-<ts>.db` é criado antes da aplicação (teste).

### 5.4. Concorrência e batching
- [ ] CA-18 Teste `concurrency::writer_and_readers`: 1 escritor inserindo 50 000 linhas enquanto 8 tarefas leem continuamente por 5 s → zero erros `SQLITE_BUSY`/`SQLITE_LOCKED`.
- [ ] CA-19 Teste `batch::coalesces`: 10 000 registros aceitos sob carga controlada resultam em ≤ 25 transações (verificado por `BatchStats.flushes`) e todas as linhas persistidas; filas cheias têm teste separado com retry explícito de registros sanitizados.
- [ ] CA-20 `BatchWriter::push()` nunca bloqueia: com a fila cheia retorna `Err(DbError::Backpressure)` em < 1 ms e emite no máximo 1 evento `db://writer-lag` por segundo.
- [ ] CA-21 `DbService` coordena parada de produtores e `finish()` dos sanitizadores antes de `Db::shutdown()`, que confirma batches aceitos, drena leitores e tenta checkpoint; sem leitores externos, WAL ausente ou vazio; ocupado/timeout é informado, nunca apagado manualmente.
- [ ] CA-22 Benchmark `benches/batch.rs`: inserção de 100 000 registros de 200 bytes em < 1,5 s (máquina de desenvolvimento, release).
- [ ] CA-23 Nenhuma chamada de `mt-db` executada a partir de um comando Tauri `async` bloqueia o runtime (revisão: todo acesso síncrono passa por `spawn_blocking` ou pela writer thread).

### 5.5. FTS5
- [ ] CA-24 Tabela-fixture: insert/update/delete/rollback/cascade mantêm FTS sincronizado; `INSERT INTO t_fts(t_fts, rank) VALUES('integrity-check', 1)` verifica índice contra conteúdo, inclusive após retenção; não criar tabelas de domínio no banco real.
- [ ] CA-25 Busca por `acao` encontra `ação` e `AÇÃO` (remove_diacritics 2); busca `npm ru` encontra `npm run dev` (prefixo no último termo).
- [ ] CA-26 `build_match_query()` neutraliza sintaxe FTS: entradas `"`, `NEAR(`, `a OR b`, `*`, `-x`, `col:val` nunca geram erro de sintaxe (proptest com 10 000 casos).

### 5.6. Sanitizador
- [ ] CA-27 `tests/fixtures/sanitizer_cases.toml` possui ≥ 3 casos positivos e ≥ 2 negativos para **cada** `SecretKind` (13 famílias, ver spec §8.2); teste de tabela passa 100%.
- [ ] CA-28 Texto sem segredos retorna `Cow::Borrowed` (teste por `matches!(out, Cow::Borrowed(_))`).
- [ ] CA-29 Idempotência: `sanitize(sanitize(x)) == sanitize(x)` (proptest, 10 000 casos) e nenhuma entrada provoca panic.
- [ ] CA-30 `StreamSanitizer`: um segredo dividido em qualquer ponto entre dois chunks (teste exaustivo de todos os pontos de corte) é redigido; bloco PEM em 50 chunks é redigido integralmente.
- [ ] CA-31 Benchmark `benches/sanitizer.rs`: ≥ 100 MiB/s para texto limpo de terminal e ≥ 30 MiB/s para texto com 1 segredo a cada 4 KiB (release).
- [ ] CA-32 Uma linha de log `tracing::info!("Authorization: Bearer abc.def.ghi123456")` aparece no arquivo de log como `Authorization: Bearer [REDACTED:bearer]`.
- [ ] CA-33 Repositórios/batches de conteúdo exigem `SanitizedText`; settings exigem inspeção contextual/recursiva antes do bind. `Db::read/write` são extensões Rust confiáveis, nunca IPC; revisar que consumidores não gravem conteúdo cru.

### 5.7. Settings e IPC
- [ ] CA-34 Wrapper `getSetting('db.retention.backup_daily_keep')` retorna `7` em banco novo (binding bruto retorna `Result` conforme 01).
- [ ] CA-35 `commands.dbSettingsSet('db.retention.backup_daily_keep', 0)` retorna `MtError` com `code = 'db.setting_invalid'` e mensagem pt-BR; valor inalterado.
- [ ] CA-36 `dbSettingsSet` com chave não registrada fora de `ui.*` retorna `db.setting_unknown`; `ui.qualquer.coisa` com JSON não sensível é aceita.
- [ ] CA-37 Valor maior que 64 KiB após serialização retorna `db.setting_too_large`.
- [ ] CA-38 Após um `set` que altera valor, evento pós-commit `{ key, value, revision }` atualiza outras janelas do mesmo processo; revisão impede resposta/evento atrasado de sobrescrever valor novo. Outra instância não compartilha eventos Tauri: escrita concorrente do app é recusada por lock.
- [ ] CA-39 `useSetting('ui.example.flag', false)` retorna um `Ref` gravável; alterações em sequência rápida geram **uma** chamada IPC (debounce 300 ms) e, em erro, o valor é revertido e o erro exposto via `useIpcError`.

### 5.8. Manutenção, retenção e backup
- [ ] CA-40 Teste `retention::applies_rules`: uma `RetentionRule` de teste remove linhas mais antigas que o limite configurado e mantém as demais.
- [ ] CA-41 Manutenção executa no startup (atrasada 60 s) e a cada 24 h; registra `db.maintenance.last_run_at` e emite `db://maintenance-finished`.
- [ ] CA-42 Rotação de backups mantém no máximo `db.retention.backup_daily_keep` diários e 3 pré-migration.
- [ ] CA-43 Corrupção simulada → estado `recovery_required`, sem exclusão/reset automático; recuperação explícita fecha conexões, preserva `.db/-wal/-shm` e restaura backup validado. Sem backup, permanece bloqueado até escolha explícita de novo banco.

### 5.9. Contratos de histórico e fronteiras sensíveis
- [ ] CA-44 Fixture de extensão usa `Db::read/write`, `DbSetup.fts` e migration própria sem importar 02/05/07; registrar FTS não cria DDL duplicado. Verificar ownership: sessões/blocos só na 05, scrollback só na 07.
- [ ] CA-45 Todas as páginas têm limite e desempate estável; 3 páginas sem repetição, consultas concorrentes e cursor inválido/expirado cobertos; FTS não devolve token secreto nem snippet cru.
- [ ] CA-46 Testar fragmentação em todos os bytes (inclusive UTF-8/ANSI), mais de dois chunks, linha >16 KiB, PEM sem END, aspas abertas, `finish()` repetido, lacuna por backpressure e EOF sem newline; nunca publicar prefixo suspeito ao esgotar buffer.
- [ ] CA-47 Fixture com segredos conhecidos ausente de `.db`, WAL, backups, logs, tabelas FTS/sombra, respostas IPC e payload externo capturado; inspecionar antes e depois de checkpoint, retenção e reabertura.
- [ ] CA-48 JSON aninhado com `password`, `APP_KEY`, `Authorization`, cookies e tokens não atravessa `settings`; rejeição não ecoa valor em `message/details/source` nem stdout/stderr.
- [ ] CA-49 Falha de commit/disco cheio e shutdown interrompido não geram evento de sucesso; fila tem limites em registros **e bytes**, sem spool de texto cru; histórico sinaliza lacunas.
- [ ] CA-50 Retenção com fixture pai/filhos respeita predicado de elegibilidade e lotes limitados, sincroniza FTS e reporta quota; regras de sessões/scrollback serão registradas pela 07. Backups podem conservar dados até expirar, sem promessa de apagamento forense.

---

## 6. Riscos e Mitigação

| # | Risco | Impacto | Mitigação |
|---|-------|---------|-----------|
| R-01 | Falsos negativos do sanitizador (segredo gravado em disco). | Alto | Catálogo de regras por família + tabela de casos versionada; `StreamSanitizer` para fronteiras de chunk; regra genérica de atribuição `*_PASSWORD/_SECRET/_TOKEN`; tipo `SanitizedText` obrigatório nas APIs de escrita de conteúdo. |
| R-02 | Falsos positivos (ex.: hashes de commit, UUIDs, `PWD=/home/...`) poluindo scrollback. | Médio | Casos negativos obrigatórios por família; regras ancoradas em prefixos/contexto; `PWD`/`OLDPWD` excluídos; valores `$VAR`/`${VAR}` e vazios não redigidos. |
| R-03 | Sequências ANSI entremeadas com o segredo impedem o match. | Médio | Decoder/parser incremental normaliza texto de arquivo antes de detectar; payloads OSC/DCS são descartados, escapes partidos retidos; testes de controles incompletos (spec §8.5). |
| R-04 | Contenção de escrita (`SQLITE_BUSY`) em rajadas de output. | Alto | Writer único, transações curtas e tratamento de Busy; WAL permite concorrência, mas leitores longos podem impedir checkpoint. |
| R-05 | Fila de escrita cheia bloqueando thread de PTY. | Alto | `push()` não bloqueante com `Backpressure`; fila limitada; evento `db://writer-lag` com rate-limit; a Fase 05 decide descartar/agregar. |
| R-06 | Crescimento ilimitado do arquivo (scrollback). | Médio | `auto_vacuum=INCREMENTAL`, `incremental_vacuum` na manutenção, `RetentionRegistry` para fases donas das tabelas, limite `db.retention.max_db_size_mb` com alerta em log. |
| R-07 | Migrations conflitantes entre fases desenvolvidas em paralelo. | Médio | Numeração global sequencial atribuída no merge + teste `naming_contract` (sem duplicatas/lacunas); checksum impede edição de migrations aplicadas. |
| R-08 | Corrupção do banco (queda de energia, disco cheio). | Médio | WAL + `synchronous=NORMAL` (commits recentes podem se perder em queda de energia); `quick_check`, backups rotativos, recuperação explícita preservando originais; filas não commitadas não são duráveis. |
| R-09 | Downgrade do app com schema mais novo. | Baixo | `db.schema_too_new` impede abertura; backup pré-migration permite rollback manual. |
| R-10 | Expor SQL arbitrário ao WebView. | Alto | `tauri-plugin-sql` rejeitado; frontend só acessa comandos tipados de domínio; nenhuma capability de SQL na `capabilities/default.json`. |
| R-11 | Settings sensíveis (tokens de fases futuras) gravados em claro. | Médio | `SettingDescriptor.sensitive = true` proíbe persistência em `settings` (retorna `db.setting_sensitive`) — a fase dona deve usar armazenamento seguro próprio; valores nunca logados. |
| R-12 | Performance de regex degradando com o crescimento do catálogo. | Baixo | `RegexSet` como pré-filtro de passe único; benchmarks com limiar no CA-31; regras compiladas uma única vez (`LazyLock`). |
