# Implementação 02 — Plano de Execução Passo a Passo (`plan.md`)

> Parte 02 de 05 da **Fase 06: Projetos e Layout de 2 Colunas**.
> Especificação técnica correspondente: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Implementação

### Passo 1 — Criar a Migration SQLite da Fase 06 (`src-tauri/crates/mt-db/migrations/NNNN_06_projects.sql`)
1. Verificar no diretório `src-tauri/crates/mt-db/migrations/` o próximo número sequencial `NNNN` livre após as migrations das Fases 03 e 05 (ex.: `0003_06_projects.sql`).
2. Escrever o DDL completo de `project_roots`, `projects`, `project_root_entries` e `project_discovery_dismissed` exatamente como especificado em `spec.md` §3.1:
   - `STRICT` em todas as tabelas; `WITHOUT ROWID` em `project_root_entries` e `project_discovery_dismissed`.
   - `ON DELETE SET NULL` em `projects(root_id)` e `ON DELETE CASCADE` nas tabelas filhas de `project_roots`.
   - Índices `idx_projects_sort_order`, `idx_projects_root_id`, `idx_root_entries_canonical` e `idx_discovery_dismissed_path`.

### Passo 2 — Implementar o Repositório Tipado em `mt-db` (`src-tauri/crates/mt-db/src/repos/projects.rs`)
1. Criar `src-tauri/crates/mt-db/src/repos/projects.rs` e registrá-lo em `src-tauri/crates/mt-db/src/repos/mod.rs` e `src-tauri/crates/mt-db/src/lib.rs`.
2. Definir as structs de persistência `StoredProject`, `StoredNewProject`, `StoredProjectUpdate`, `StoredRoot`, `StoredRootEntry`, `StoredDismissed`, `StoredCatalog`, `StoredReconcileBatch`, `ProjectMutation` e `ProjectCommit`.
3. Definir a trait interna `pub(crate) trait ProjectInsertContributor` e o registro interno de contributors (vazio na Fase 06, mais helper `#[cfg(test)]` ou `pub(crate)` para testes de rollback conjunto).
4. Implementar `ProjectsRepo::load_catalog` e `ProjectsRepo::find_project_by_path` usando `self.db.read(...)` com `stmt.prepare_cached(...)`.
5. Implementar `ProjectsRepo::commit` usando `self.db.write(...)`:
   - Aplicar todas as mutações em uma única transação `IMMEDIATE`.
   - Usar `SanitizedText` em títulos gravados.
   - Mapear violações de `UNIQUE(projects.path)` para `DbError::Constraint("project.duplicate_path".into())` e `UNIQUE(project_roots.path)` para `DbError::Constraint("project_root.duplicate_path".into())`.
   - Implementar `Reorder` validando conjunto completo e sem duplicatas antes de reescrever `0..n-1` (retornando `project.reorder_mismatch` em divergência).
   - Invocar `after_insert` dos contributors internos apenas quando um projeto novo for efetivamente inserido.

### Passo 3 — Implementar `SqliteProjectStore` e `settings.rs` em `mt-projects`
1. Criar `src-tauri/crates/mt-projects/src/store.rs` implementando a trait `ProjectStore` e `SqliteProjectStore`, convertendo `DbError` e códigos de constraint (`project.duplicate_path`, `project_root.duplicate_path`, `project.reorder_mismatch`) para `MtError` com mensagens em pt-BR.
2. Criar `src-tauri/crates/mt-projects/src/settings.rs` com `pub fn register_db(setup: &mut DbSetup)`, registrando `projects.selected_id` (default `null`, validador de UUID/null) e `layout.columns.left_size` (default `22`, validador numérico finito `15.0..=45.0`).

### Passo 4 — Implementar Varredura de Nível 1 e Diff (`src-tauri/crates/mt-projects/src/scanner.rs`)
1. Implementar `scan_level1(root: &Path) -> Result<ScanSnapshot, MtError>`:
   - Filtrar entradas ocultas (`is_hidden`), arquivos regulares, sockets e FIFOs.
   - Resolver symlinks de diretórios mantendo `entry_path` (caminho lexical na raiz) e `canonical_path` (destino real).
   - Ignorar symlinks quebrados/circulares sem marcar `complete = false`, mas marcar `complete = false` em `PermissionDenied` ou erros de I/O de entradas.
   - Ignorar destinos proibidos (`/`, `$HOME`, a própria raiz/ancestrais e caminhos dentro de `.git`, `.worktrees`, `.max-code-worktrees`).
   - Ordenar `entries` por `(name, entry_path)`.
2. Implementar `diff(current: &ScanSnapshot, previous: &[RootEntryRow]) -> ScanDiff`:
   - Retornar `removed_entry_paths` vazio sempre que `current.complete == false`.

### Passo 5 — Implementar `RootWatcher` (`src-tauri/crates/mt-projects/src/watcher.rs`)
1. Implementar `WatchSignal` e `RootWatcher` com `DEBOUNCE = 500ms` e `SETTLE = 1500ms`.
2. Configurar `notify_debouncer_full::new_debouncer` observando a raiz e o diretório pai em `RecursiveMode::NonRecursive`.
3. Garantir inserção em `dirty_roots` antes de `tx.try_send` para não perder invalidações se o canal de 64 mensagens lotar.
4. Atualizar `src-tauri/crates/mt-projects/src/lib.rs` exportando `scanner`, `settings`, `store`, `watcher` e `pub use settings::register_db;`.

### Passo 6 — Escrever Testes de Integração (`store_and_scanner.rs` e `watcher.rs`)
1. Em `src-tauri/crates/mt-projects/tests/store_and_scanner.rs`:
   - Testar migration aplicada sobre banco contendo apenas Fases 03 + 05 (sem tabelas da 07/16), verificando `PRAGMA foreign_key_check` vazio, reopen idempotente em arquivo temporário WAL e validação das 2 settings (`projects.selected_id` e `layout.columns.left_size`).
   - Testar CRUD em `SqliteProjectStore`: `duplicate_path`, `reorder` transacional e `reorder_mismatch`, `ON DELETE SET NULL` ao remover raiz (`root_id` vira `None` nos projetos importados), `ON DELETE CASCADE` em `project_root_entries` e `project_discovery_dismissed`.
   - Testar rollback transacional conjunto usando um contributor de teste que falha propositalmente.
   - Testar `scanner.rs`: pastas ocultas, arquivos, subníveis ignorados, symlink válido para fora da raiz, dois aliases apontando para o mesmo destino, link quebrado vs `PermissionDenied` simulado (`complete = false` preserva origens).
2. Em `src-tauri/crates/mt-projects/tests/watcher.rs`:
   - Testar criação de subpasta isolada emitindo `WatchSignal::RootChanged` em `< 3 s`.
   - Testar rajada de 20 pastas criadas em sequência sinalizando a raiz em `dirty_roots` / `WatchSignal`.
   - Testar criação de `.oculta`, arquivo regular e pasta em subnível (`a/b`) sem disparo indevido.
   - Testar remoção da própria pasta raiz emitindo `WatchSignal::RootMissing`.

---

## 2. Comandos de Verificação

```bash
# Compilar mt-db e mt-projects
cargo check -p mt-db -p mt-projects

# Rodar os testes de persistência, scanner e watcher
cargo test -p mt-db
cargo test -p mt-projects --test store_and_scanner
cargo test -p mt-projects --test watcher

# Verificar lints Rust
cargo clippy -p mt-db -p mt-projects --all-targets -- -D warnings
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 06)

- **CA-3 (Parcial — camada de watcher e scanner):** Criação isolada detectada rapidamente; rajada de 20 pastas converge; ocultas/arquivos/subníveis ignorados; erro de permissão marca `complete = false` e não remove origens.
- **CA-4 (Completo):** `path` duplicado retorna `MtError { code: "project.duplicate_path" }`; `project_reorder` é transacional; remover raiz mantém projetos com `root_id = None`.
- **CA-9 (Parcial — scanner e store):** Dois aliases na mesma raiz ou em duas raízes mapeiam para o mesmo `canonical_path` com entradas separadas em `project_root_entries`; scan parcial não exclui origens.
- **CA-12 (Completo):** Migration funciona em banco com 03+05 sem 07/16; `foreign_key_check` vazio, reopen idempotente em WAL, settings válidas aceitas e inválidas recusadas.
