# Implementação 03 — Persistência SQLite, ThemeService, Comandos IPC e Eventos Tauri (`plan.md`)

> Fase 10 — Temas e Importador Tabby
> Especificação desta unidade: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Execução

### Passo 1 — Migration SQLite e Repositório em `mt-db`
- [ ] Confirmar que `src-tauri/crates/mt-db/migrations/` contém `0005_09_os_settings.sql` (D-01) e criar `src-tauri/crates/mt-db/migrations/0006_10_themes.sql` com as tabelas `themes`, `appearance_settings`, `project_themes`, índices e triggers.
- [ ] Criar `src-tauri/crates/mt-db/src/repos/themes.rs` (e registrar em `repos/mod.rs` e `lib.rs`) implementando `list`, `get`, `find_by_hashes`, `insert`, `upsert_builtin`, `ensure_appearance_row`, `delete`, `projects_using`, `appearance`, `save_appearance`, `project_overrides` e `set_project_theme`.

### Passo 2 — Cache LRU e `ThemeService` em `mt-themes`
- [ ] Implementar `ResolvedCache` em `src-tauri/crates/mt-themes/src/cache.rs`.
- [ ] Implementar `ThemeStore`, `SqliteThemeStore`, `ThemeEventSink` e `ThemeService` em `src-tauri/crates/mt-themes/src/service.rs`.
- [ ] Garantir que `import_file` e `import_dir` executem parse e normalização via `tokio::task::spawn_blocking`, façam deduplicação por `ContentHash` intra-lote e contra o banco numa única transação e emitam `theme://library-changed` somente após o commit.

### Passo 3 — Testes de Integração com Banco Real (`tests/service_db.rs`)
- [ ] Criar `src-tauri/crates/mt-themes/tests/service_db.rs` cobrindo:
   - Aplicação da sequência completa `0001`–`0006` em banco vazio, banco até `0003` (Fase 06) e banco até `0005` (Fase 09), verificando `PRAGMA foreign_key_check` vazio (CA-17). O runner aplica `0004`/`0005` quando pendentes antes de `0006`, sem renumerar ou preencher lacunas com placeholders.
  - `start()` semeando os 6 builtins sem duplicar no reinício e atualizando linha ao incrementar `builtin_version` (CA-18).
  - Reimportação do mesmo arquivo retornando `imported = []` e `duplicates.len() == N` (CA-19).
  - Exclusão de builtin retornando `theme.builtin_readonly`; exclusão do tema ativo voltando para `DEFAULT_THEME_ID` e limpando `project_themes` em cascata (CA-20).
  - Exclusão de projeto na tabela `projects` removendo automaticamente a linha em `project_themes` (CA-21).
  - Exportação nos 4 formatos, reimportação gerando duplicado por hash idêntico e rejeição de extensão inválida com `theme.export_failed` (CA-23).

### Passo 4 — Comandos IPC, Eventos Tauri, Capabilities e Bindings
- [ ] Implementar `src-tauri/src/events/themes.rs` (`ThemeChangedEvent`, `ThemeLibraryChangedEvent`, `TauriThemeSink`).
- [ ] Implementar `src-tauri/src/commands/themes.rs` com os 9 comandos Tauri + testes unitários com `tauri::test::mock_builder`.
- [ ] Registrar comandos e eventos em `src-tauri/src/commands/mod.rs`, `src-tauri/src/events/mod.rs` e `src-tauri/src/specta.rs`.
- [ ] Configurar a inicialização de `Arc<ThemeService>` em `src-tauri/src/lib.rs` e adicionar `"dialog:allow-open"` e `"dialog:allow-save"` em `src-tauri/capabilities/default.json`.
- [ ] Executar `npm run bindings` para atualizar `src/types/bindings.ts` e verificar que todos os comandos e tipos exigidos pelo CA-04 estão exportados.

---

## 2. Comandos de Verificação e Validação

```bash
# Formatação, clippy e suíte completa de testes Rust (workspace + mt-themes >= 60 testes)
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml -p mt-themes
cargo test --manifest-path src-tauri/Cargo.toml --workspace

# Geração e verificação de bindings TypeScript
npm run bindings
git diff --exit-code src/types/bindings.ts
```

---

## 3. Critérios de Aceite Atendidos

| Critério | Descrição | Verificação |
|----------|-----------|-------------|
| **CA-01** | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace` passam. | Comandos acima retornam exit 0. |
| **CA-02** | `cargo test -p mt-themes` executa ≥ 60 testes, incluindo snapshots `insta` sem pendências. | `cargo test -p mt-themes`. |
| **CA-04** | `npm run bindings && git diff --exit-code src/types/bindings.ts` retorna 0 com todos os 9 comandos e tipos exportados. | `npm run bindings`. |
| **CA-17** | Migrations aplicam sem erro e `PRAGMA foreign_key_check` retorna vazio. | `tests/service_db.rs`. |
| **CA-18** | Startup semeia 6 builtins sem duplicar no reinício e atualiza quando `builtin_version` sobe. | `tests/service_db.rs`. |
| **CA-19** | Importar o mesmo arquivo duas vezes retorna `imported = []` e `duplicates.len() = N`. | `tests/service_db.rs`. |
| **CA-20** | `theme_delete` bloqueia builtin, restaura "Max Navy/Teal" se ativo for excluído e limpa `project_themes`. | `tests/service_db.rs`. |
| **CA-21** | Remover projeto remove sua linha em `project_themes` via `ON DELETE CASCADE`. | `tests/service_db.rs`. |
| **CA-22** | `theme_import_dir` roda em `spawn_blocking`, respeita limites de 500 arquivos e profundidade 4 em < 3 s (release). | `tests/service_db.rs` + `tests/import_tabby.rs`. |
| **CA-23** | `theme_export` exporta `tabby_yaml`, `tabby_json`, `xterm_json`, `css_tokens`, reimporta como duplicado e valida extensão de `destPath`. | `tests/service_db.rs`. |
