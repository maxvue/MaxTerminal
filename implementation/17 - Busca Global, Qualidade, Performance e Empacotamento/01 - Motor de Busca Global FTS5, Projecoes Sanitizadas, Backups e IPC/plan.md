# Plano de Execução — Implementação 01: Motor de Busca Global FTS5, Projeções Sanitizadas, Backups e IPC

> **Fase:** 17 — Busca Global, Qualidade, Performance e Empacotamento
> **Especificação de Referência:** [`spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Migration `0011_17_search.sql`, `FtsSpec` e Projeções Sanitizadas
1. Criar `src-tauri/crates/mt-db/migrations/0011_17_search.sql` com a tabela `search_metadata` (`STRICT`) e os índices `ix_search_metadata_project_source`, `ix_search_metadata_source_time`, `ix_sessions_project_started_id`, `ix_command_blocks_session_started_id` e `ix_command_blocks_exit_started_id`. Validar sequência `1..=11`, banco novo e upgrade de fixture v10 sem modificar migrations anteriores.
2. Criar `src-tauri/crates/mt-db/src/search/projection.rs` implementando `SanitizedProjection`, `upsert_projection`, `delete_projection`, `backfill_all_projections` e `rebuild_search_fts` (restrito à allowlist do `FtsRegistry`).
3. Registrar `SEARCH_METADATA_FTS` (`search_metadata_fts` com `prefix = &[2, 3]`) no `DbSetup` de `src-tauri/crates/mt-db/src/lib.rs` e conectar a `CodeMigration` para popular `search_metadata` a partir de bancos já existentes.

### Passo 2: Modelos, Erros, `QueryBuilder` e Sentinelas de Snippet
1. Criar `src-tauri/crates/mt-db/src/search/model.rs` e `src-tauri/crates/mt-db/src/search/error.rs` com todos os DTOs derivados com `specta::Type` e códigos `search.*`.
2. Criar `src-tauri/crates/mt-db/src/search/query.rs` reutilizando `build_match_query()` da Fase 03, validando comprimento máximo (256 chars / 1024 bytes), ausência de bytes NUL e gerando par de sentinelas unicode privadas verificadas contra colisão para extrair `Vec<HighlightSegment>`.
3. Criar `src-tauri/crates/mt-db/src/search/rank.rs` implementando a ordenação determinística por seção (`Relevance`: score `bm25` ASC, `occurred_at_ms` DESC, `key` ASC; `Recent`: `occurred_at_ms` DESC, `key` ASC).

### Passo 3: Fontes de Busca (`sources/*.rs`)
1. Implementar a trait `SearchSource` em `src-tauri/crates/mt-db/src/search/sources/mod.rs`.
2. Implementar `command_blocks.rs` (`command_blocks_fts` JOIN `command_blocks` JOIN `sessions`) e `scrollback.rs` (`session_scrollback_fts` JOIN `session_scrollback` JOIN `tab_session_panes` JOIN `sessions`).
3. Implementar `sessions.rs`, `projects.rs`, `templates.rs`, `groups.rs` e `issues.rs` consultando `search_metadata_fts` JOIN `search_metadata` com predicados estáticos bindados para `project_id`, `tab_id`, `session_id`, `pane_id`, `[from_ms, until_ms)` e `exit_code`.

### Passo 4: `SnapshotCache`, `CursorSigner` e `CancelRegistry`
1. Implementar `src-tauri/crates/mt-db/src/search/cursor.rs` com assinatura HMAC-SHA256 em memória por instância, serializando `{ owner, snapshot_id, source, next_ordinal, query_fingerprint, expires_at_ms }` em Base64 URL-safe sem padding.
2. Implementar `src-tauri/crates/mt-db/src/search/snapshot.rs` com limite de 10.000 hits, 16 MiB por janela, TTL de 120 s e máximo de 2 snapshots simultâneos por `SearchOwner`.
3. Implementar `src-tauri/crates/mt-db/src/search/cancel.rs` com estados `Queued`, `Running`, `Completed`, `Cancelled`, tombstones com TTL para cancelamentos que chegam antes do início da consulta e coordenação sob mutex com `rusqlite::InterruptHandle` + `progress_handler`.
4. Unificar tudo em `src-tauri/crates/mt-db/src/search/mod.rs` (`SearchEngine`).

### Passo 5: Extensão de Backups (`BackupKind::Manual`) e Restauração Transacional no Boot
1. Atualizar `src-tauri/crates/mt-db/src/backup.rs` adicionando `BackupKind::Manual` e `BackupKind::PreRestore`, listagem ordenada de backups validados, agendamento atômico (`pending-restore.json`) e máquina de estados de restauração no boot (`apply_pending_restore_on_boot`) com proteção contra associação indevida de sidecars `-wal`/`-shm`.

### Passo 6: Adaptadores Transacionais, Comandos IPC e Eventos Tauri
1. Criar `src-tauri/src/search/adapters.rs` conectando os hooks de escrita de projetos (06), templates/sessões (07), grupos/execuções (15) e issues/comentários (16) a `upsert_projection` / `delete_projection` e à invalidação de snapshots quando projetos ou políticas mudarem.
2. Criar `src-tauri/src/events/search.rs`, `src-tauri/src/commands/search.rs` e `src-tauri/src/commands/backup.rs`.
3. Registrar comandos e estado no `src-tauri/src/lib.rs` e regenerar os bindings TypeScript via `tauri-specta`.

### Passo 7: Suíte de Testes Rust (`mt-db/tests/search_*.rs` e `upgrade_from_fixtures.rs`)
1. Implementar `search_query.rs` com Proptest de 20.000 entradas hostis (`"`, `*`, `^`, `NEAR(`, `AND`, `OR`, `NOT`, `col:`, `(`, `)`, `-`, `+`, NUL, emoji, RTL).
2. Implementar `search_sources.rs` com corpus-oráculo contendo `migrate` em todas as fontes e validação exata de IDs e filtros combinados.
3. Implementar `search_snapshot.rs` testando paginação completa de 10.000 resultados em páginas de 25 sob escritas/deleções concorrentes, expiração de TTL (`search.cursor_expired`) e adulteração de Base64 (`search.invalid_cursor`).
4. Implementar `search_cancel.rs` testando cancelamento em fila, em execução, antes do registro e reutilização segura da conexão (`A → release → B → cancel(A)`).
5. Implementar `search_index.rs` validando `schema_contract` (sequência 03 → 05 → 07 → 17 sem tabelas duplicadas) e ausência total de segredos de `sanitizer_cases.toml`.
6. Implementar `upgrade_from_fixtures.rs` validando migração de bancos populados (≥ 10.000 blocos), `PRAGMA integrity_check`, `integrity-check` FTS5 e falha injetada em cada etapa do restore no boot.

---

## 2. Comandos Exatos de Verificação e Testes

Executar a partir da raiz da worktree ativa (`cd "$(git rev-parse --show-toplevel)"`):

```bash
# 1. Verificar compilação e lint estrito do workspace Rust
cargo check --manifest-path src-tauri/Cargo.toml --workspace --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# 2. Executar todos os testes de busca global em mt-db (CA-01 a CA-07)
cargo nextest run --manifest-path src-tauri/Cargo.toml -p mt-db search_

# 3. Executar Proptest com 20.000 casos para o parser FTS5 (CA-02)
PROPTEST_CASES=20000 cargo nextest run --manifest-path src-tauri/Cargo.toml -p mt-db --test search_query

# 4. Executar testes de migração e backup/restore em bancos populados (CA-27, CA-40)
cargo nextest run --manifest-path src-tauri/Cargo.toml -p mt-db --test upgrade_from_fixtures
```

---

## 3. Mapeamento para os Critérios de Aceite da Fase 17

| Critério | Teste / Verificação Responsável | Resultado Esperado |
|----------|---------------------------------|--------------------|
| `CA-01` | `cargo nextest run -p mt-db search_` + `search_index::schema_contract` | Sequência 03→05→07→17 validada sem tabelas paralelas; todos os testes `search_*` passam. |
| `CA-02` | `search_query::never_syntax_error` (20.000 casos) | Zero ocorrências de `SQLITE_ERROR` em consultas `MATCH`. |
| `CA-03` | `search_sources::oracle_corpus_migrate` | Hits por seção coincidem com os IDs esperados e segmentos destacados para `migrate`. |
| `CA-04` | `search_sources::combined_filters_exact_match` | Predicados `project_id`, período, `exit_code = Nonzero` e `tab_id` filtram exatamente os IDs esperados e excluem fontes sem exit code. |
| `CA-05` | `search_snapshot::paginate_10k_under_concurrent_writes` | 400 páginas de 25 itens retornam cada uma das 10.000 chaves do snapshot exatamente uma vez; TTL expirado retorna `search.cursor_expired`. |
| `CA-06` | `search_snapshot::tampered_cursor_rejected` | Alteração de 1 bit no cursor Base64 retorna `MtError { code: "search.invalid_cursor" }`. |
| `CA-07` | `search_index::no_secrets_in_search_results_or_fts_vocab` | Inserção dos casos de `sanitizer_cases.toml` via API pública resulta em 0 ocorrências do segredo original nos hits e em `fts5vocab`. |
| `CA-27` | `upgrade_from_fixtures::migrate_populated_fixtures` | Fixtures ≥ 10.000 blocos migram até a versão 17 com `integrity_check = ok`, FTS íntegro, contagens preservadas e backup pré-migration criado. |
| `CA-40` | `upgrade_from_fixtures::backup_create_and_boot_restore_stages` | `db_backup_create` gera arquivo íntegro; `db_backup_restore` aplica no próximo boot com rollback automático em falha injetada e limpeza de sidecars WAL. |
