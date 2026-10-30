# Plano de Execução — Implementação 01: Contratos de Domínio, Lifecycle de Spawn, Migrations SQLite e Repositórios de Sessões e Blocos

> **Fase:** 05 — Shell Integration OSC 133 e Máquina de Estados das Abas
> **Implementação:** `01 - Contratos de Domínio, Migrations SQLite e Repositórios de Sessões e Blocos`
> **Especificação de referência:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1: Implementar tipos de domínio da Fase 05 e códigos de erro em `mt-core`**
  1. Criar `src-tauri/crates/mt-core/src/shell.rs` com todos os enums e structs especificados na §3.1 da `spec.md` (`ContinuousPolicy`, `IntegrationMode`, `ObservationSource`, `ObservationConfidence`, `CommandTextQuality`, `PersistenceState`, `BlockStatus`, `BlockEndReason`, `OutputKind`, `TabStatus`, `ShellIntegrationRequest`, `RuntimeBinding`, `ExecutionRef`, `StreamPosition`, `CompletionSnapshot`, `AttentionSnapshot`, `PaneStateDto`, `TabStateSnapshot`, `TabStateListResponse`, `SeenRequest`, `FocusSnapshot`, `CommandStarted`, `CommandFinished`, `CommandBlockDto`, `Page<T>`, `CommandBlockHit`, `ErrorContext`, `IntegrationInfo`).
  2. Implementar conversões `as_str(&self) -> &'static str` e `FromStr` para os enums persistidos no SQLite (`IntegrationMode`, `ObservationSource`, `ObservationConfidence`, `CommandTextQuality`, `BlockStatus`, `BlockEndReason`).
  3. Adicionar em `src-tauri/crates/mt-core/src/error.rs` os códigos estáticos `shell.tab_not_found`, `shell.pane_not_found`, `shell.pty_not_found`, `shell.binding_conflict`, `shell.session_mismatch`, `shell.invalid_argument`, `shell.block_not_found`, `shell.block_running`, `shell.block_not_failed` e `shell.context_unavailable` com mensagens em `pt-BR`.
  4. Exportar `pub mod shell;` em `src-tauri/crates/mt-core/src/lib.rs`.

- [ ] **Passo 2: Adicionar `SpawnLifecycle` e `spawn_with_lifecycle` em `mt-pty`**
  1. Criar `src-tauri/crates/mt-pty/src/spawn/lifecycle.rs` com `ResolvedSpawn`, a trait `SpawnLifecycle` (`prepare`, `started`, `failed`) e `NoopSpawnLifecycle`.
  2. Atualizar `src-tauri/crates/mt-pty/src/spawn/mod.rs` para construir `ResolvedSpawn` logo após resolver `shell`, `args`, `cwd` e `env`, invocar `lifecycle.prepare(&mut resolved)`, revalidar o ambiente resultante e, após obter o `pid`/`start_time` do filho bloqueado no launcher e registrar a instância, invocar `lifecycle.started(&summary)` **antes** de iniciar as threads `reader`, `writer` e `waiter`.
  3. Garantir que qualquer erro após `prepare` acione o rollback de contenção/processo da Fase 02 e chame `lifecycle.failed(pty_id)`.
  4. Expor `PtyManager::spawn_with_lifecycle` em `src-tauri/crates/mt-pty/src/manager.rs` e manter `PtyManager::spawn` delegando para `NoopSpawnLifecycle`.
  5. Adicionar testes unitários em `mt-pty` comprovando a ordem exata `prepare` → criação do processo → `started` → primeiro frame no `OutputTap`, bem como a chamada de `failed` quando `prepare` ou `started` retornam erro.

- [ ] **Passo 3: Criar a Migration SQLite `0002_05_shell_sessions_command_blocks.sql` e registrar `FtsSpec`**
  1. Criar `src-tauri/crates/mt-db/migrations/0002_05_shell_sessions_command_blocks.sql` exatamente com o DDL da §3.4 da `spec.md` (`sessions`, `command_blocks`, índices parciais, `command_blocks_fts` e triggers `command_blocks_fts_ai`, `command_blocks_fts_ad`, `command_blocks_fts_au`).
  2. Criar `src-tauri/crates/mt-db/src/repos/mod.rs` expondo `pub const fn shell_fts_spec() -> FtsSpec` com `content_table: "command_blocks"`, `fts_table: "command_blocks_fts"`, `columns: &["command", "cwd", "output_text"]`, `tokenizer: FtsTokenizer::Text` e `prefix: &[2, 3, 4]`.

- [ ] **Passo 4: Implementar `SessionsRepo` e `CommandBlocksRepo` em `mt-db`**
  1. Implementar `src-tauri/crates/mt-db/src/repos/sessions.rs` (`prepare`, `close`, `abort`, `get`) usando `Db::write` e `Db::read` com `prepare_cached`.
  2. Validar em `SessionsRepo::prepare` a reutilização segura de sessão (`status == 'active'`, mesmo `tab_id` e `pane_id`, preservando `next_seq` incremental sem reiniciar em 1).
  3. Implementar `src-tauri/crates/mt-db/src/repos/command_blocks.rs` (`start`, `finish`, `get`, `get_error_tail`, `list`, `search`, `close_dangling`).
  4. Garantir em `CommandBlocksRepo::start` a reserva/incremento transacional de `sessions.next_seq` e idempotência em caso de retry com o mesmo `(id, session_id, seq, pty_id, generation)`.
  5. Garantir em `CommandBlocksRepo::finish` a guarda `status = 'running'`, idempotência para retry idêntico e preenchimento de `output_text` e `error_tail` exclusivamente via `Option<SanitizedText>`.
  6. Garantir em `CommandBlocksRepo::search` o uso de `build_match_query(query)` da Fase 03, validação do tamanho da query (`<= 4096` bytes) e do limite (`1..=100`), e extração de snippets com marcadores `«` e `»`.
  7. Exportar `pub mod repos;` em `src-tauri/crates/mt-db/src/lib.rs`.

- [ ] **Passo 5: Criar Suíte de Testes de Integração de Banco e FTS5**
  1. Criar `src-tauri/crates/mt-db/tests/shell_repos_and_fts.rs` cobrindo:
     - Aplicação da migration `0002_05` sobre banco vazio e sobre banco com `0001_03_base.sql`, mais reaplicação idempotente pelo `MigrationRunner`.
     - Violação das constraints `STRICT` e `CHECK` (ex.: `succeeded` com `exit_code = 1`, `failed` com `exit_code = 0`, `unknown` com `exit_code = Some(0)`, `running` com `ended_at` preenchido).
     - `ON DELETE CASCADE` ao remover uma sessão, verificando que `command_blocks` e `command_blocks_fts` ficam vazios e passam em `INSERT INTO command_blocks_fts(command_blocks_fts, rank) VALUES('integrity-check', 1)`.
     - Sincronização FTS5 em `INSERT`, `UPDATE` (quando `finish` grava `output_text`), `DELETE` e `rebuild`, além de estabilidade do `rowid` após `VACUUM`.
     - Sanitização ponta a ponta: tentativa de gravar `Authorization: Bearer segredo123456` em `command` e `output_text` via `SanitizedText::new`, comprovando que a busca FTS por `segredo123456` retorna 0 resultados e a busca por `REDACTED` ou pelo comando base retorna o hit mascarado.
     - Recuperação de crash com `close_dangling`: blocos `running` passam para `unknown` com `end_reason = 'recovery'` e `exit_code = None`.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificar compilação e tipos de mt-core, mt-pty e mt-db
cargo check -p mt-core -p mt-pty -p mt-db --all-targets

# 2. Executar testes unitários e de integração dos 3 crates
cargo test -p mt-core -p mt-pty -p mt-db

# 3. Executar especificamente a suíte de migrations, repositórios e FTS5 da Fase 05
cargo test -p mt-db --test shell_repos_and_fts -- --nocapture

# 4. Validar lints estritos no workspace
cargo clippy -p mt-core -p mt-pty -p mt-db --all-targets -- -D warnings
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 05)

- [x] **CA-DB-01 (instructions.md §5):** Migration aplicada sobre base03/vazio, reaplicação pelo runner, rollback em erro, FK/cascade, FTS insert/update/delete/rebuild; ids textuais e `rowid` estável.
- [x] **CA-DB-02 (instructions.md §5):** Segredos nunca entram crus no repositório; uso estrito de `SanitizedText` em `SessionsRepo` e `CommandBlocksRepo`.
- [x] **CA-EST-04 (instructions.md §5):** `exitCode=null` mapeado e validado como `unknown` no banco (`CHECK (status = 'unknown' AND ended_at IS NOT NULL AND exit_code IS NULL)`), nunca sucesso fabricado em `close_dangling`.
- [x] **CA-PRE-02 (spec.md §2.1):** `SpawnLifecycle` genérico em `mt-pty` executa `prepare` antes de criar o filho e `started` antes de iniciar o reader, sem acoplar `mt-pty` a `mt-shell`.
