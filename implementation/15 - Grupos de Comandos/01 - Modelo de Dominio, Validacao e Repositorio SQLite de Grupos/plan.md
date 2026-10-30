# Implementação 01 — Plano de Execução Passo a Passo (`plan.md`)

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1: Criar a Migration SQLite (`0015_15_command_groups.sql`)**
  1. Inspecionar `src-tauri/crates/mt-db/migrations/` para confirmar o próximo número sequencial livre (`NNNN_15_command_groups.sql`).
  2. Criar o arquivo SQL contendo as 5 tabelas (`command_groups`, `command_group_labels`, `command_group_items`, `command_group_runs`, `command_group_run_items`), todas as constraints `CHECK`, chaves estrangeiras (`ON DELETE CASCADE` e `ON DELETE SET NULL`) e todos os índices definidos na seção 3 da `spec.md`.
  3. Registrar a migration no catálogo de migrations do `mt-db` (caso o crate utilize lista explícita de migrations embutidas).

- [ ] **Passo 2: Configurar Dependências e Tipos de Domínio (`model.rs`)**
  1. Em `src-tauri/crates/mt-commands/Cargo.toml`, verificar/adicionar `unicode-normalization`, `sha2`, `uuid`, `serde`, `serde_json`, `specta`, `rusqlite`, `mt-core` e `mt-db`.
  2. Criar `src-tauri/crates/mt-commands/src/groups/model.rs` com todos os newtypes (`GroupItemId`, `GroupRunId`, `GroupRunItemId`, `GroupRequestId`, `OutputSubscriptionId`), enums (`GroupScope`, `ExecutionMode`, `SequentialSeparator`, `ShellDialect`, `ItemStatus`, `RunStatus`), structs de entrada/saída (`CommandGroupInput`, `CommandGroupItemInput`, `CommandGroup`, `CommandGroupItem`, `CommandGroupCatalog`, `GroupRunRequest`, `GroupRunSnapshot`, `GroupRunItemSnapshot`, `GroupRunCounts`, `GroupRun`, `GroupRunItem`, `GroupRunSummary`, `GroupRunListPage`) e `GroupErrorCode`.
  3. Garantir que todas as structs IPC tenham `#[derive(Serialize, Deserialize, Type)]` e `#[serde(rename_all = "camelCase")]` (com `deny_unknown_fields` nos DTOs de entrada) e que os enums utilizem `#[serde(rename_all = "snake_case")]`.

- [ ] **Passo 3: Implementar Validação e Detecção Bloqueante de Segredos (`validate.rs` e `resolve.rs`)**
  1. Criar `src-tauri/crates/mt-commands/src/groups/validate.rs` implementando:
     - `normalize_key` com Unicode NFC + lowercase.
     - `validate_env_key` validando `[A-Za-z_][A-Za-z0-9_]*` e rejeitando o prefixo `MAXTERMINAL_`.
     - `validate_relative_cwd_syntax` rejeitando caminhos vazios, caracteres de controle (`\n`, `\r`, `\0`), caminhos absolutos (`/etc`) e subida acima da raiz (`../..`).
     - `validate_group_input` validando:
       - `scope == Local` exige `project_id.is_some()` (`commands.group.project_required`); `scope == Global` exige `project_id.is_none()` (`commands.group.scope_mismatch`).
       - `name` aparado entre 1 e 80 caracteres; `description` ≤ 2048 caracteres; 1 a 50 itens; `item.title` aparado entre 1 e 80 caracteres; `item.command` entre 1 e 8192 bytes sem `\n`, `\r` ou `\0`.
       - Até 20 labels de 1 a 32 caracteres sem duplicatas após `normalize_key`.
       - Até 64 entradas em `env` por nível e tamanho total mesclado ≤ 64 KiB, valores sem `\0`.
       - Passagem em `mt_db::sanitizer::sanitize()` para comandos e valores de `env`: se houver alteração, acumular o caminho do campo (`env.<KEY>`, `items[i].command`, `items[i].env.<KEY>`) e retornar `Err(GroupErrorCode::SecretDetected.into_error_with_fields(..., fields))`.
  2. Criar `src-tauri/crates/mt-commands/src/groups/resolve.rs` implementando `CwdResolver::resolve_effective_cwd` (canonicalização da raiz e do alvo, retorno de `commands.group.cwd_not_found` quando inexistente e `commands.group.cwd_outside_project` quando symlink ou caminho escapar da raiz), `CwdResolver::merge_env`, `detect_shell_dialect` (`bash`, `zsh`, `fish`) e `compute_request_hash` (SHA-256 canônico).

- [ ] **Passo 4: Implementar Máquina de Estados e Redutor (`aggregate.rs`)**
  1. Criar `src-tauri/crates/mt-commands/src/groups/aggregate.rs` implementando `compute_counts`, `reduce_run_status` e `can_transition_item`.
  2. Garantir que estados terminais nunca regredam para estados ativos ou para outro estado terminal, e que a prioridade de redução siga exatamente: `cancelling` → `running`/`queued` → `interrupted` → `cancelled` → `partial` → `failed` → `succeeded`.

- [ ] **Passo 5: Implementar Repositório Transacional SQLite (`repo.rs`)**
  1. Criar `src-tauri/crates/mt-commands/src/groups/repo.rs` implementando `CommandGroupRepo` e `SqliteCommandGroupRepo`.
  2. Em `create`: validar entrada, calcular `next_sort_order` dentro do escopo/projeto, inserir grupo, labels e itens na mesma transação; mapear violação de `ux_groups_local_name` / `ux_groups_global_name` para `commands.group.name_conflict`.
  3. Em `update`: conferir existência (`commands.group.not_found`), imutabilidade de `scope` e `project_id` (`commands.group.scope_mismatch`), igualdade de `expected_revision` (`commands.group.revision_conflict`), validar todos os `item.id` enviados contra o `group_id` atual, deletar e reinserir itens/labels na mesma transação e incrementar `revision = revision + 1`.
  4. Em `list`: retornar `local_groups` do `project_id` ordenados por `(sort_order ASC, id ASC)` e `global_groups` ordenados por `(sort_order ASC, id ASC)`.
  5. Em `reorder`: validar que `entries` corresponde exatamente aos grupos do escopo/projeto e às suas `revision` atuais; atualizar `sort_order` e incrementar `revision` atomicamente.
  6. Em `accept_run`, `mark_cancel_requested`, `update_run_item`, `recover_interrupted_runs` e `prune_finished_runs_older_than`: manter consistência transacional entre `command_group_runs` e `command_group_run_items`, truncando `output_tail` em até 256 KiB em fronteira UTF-8 válida.
  7. Expor todos os submódulos em `src-tauri/crates/mt-commands/src/groups/mod.rs` e `src-tauri/crates/mt-commands/src/lib.rs`.

- [ ] **Passo 6: Criar Testes Automatizados (`groups_validate.rs` e `groups_repo.rs`)**
  1. Criar `src-tauri/crates/mt-commands/tests/groups_validate.rs` testando todos os casos de `CA-04` (comando com `\n`, `cwd` absoluto, `cwd` com `../..`, symlink apontando para fora da raiz do projeto, chave env `1ABC` e `MAXTERMINAL_FOO`, > 50 itens, comando com `Bearer eyJ...` e `APP_KEY=base64:...` verificando `code == "commands.group.secret_detected"` e `details.fields`, além da preservação de `$TOKEN`).
  2. Criar `src-tauri/crates/mt-commands/tests/groups_repo.rs` testando `CA-02`, `CA-03`, `CA-23` e `CA-24` (constraints `CHECK` de escopo/projeto, conflito de nome case-insensitive e Unicode NFC no mesmo projeto, permissão do mesmo nome em outro projeto e em global, `ON DELETE CASCADE` ao excluir projeto, atomicidade de `update`, `reorder` com mismatch e conflito de revisão, ordenação `list`, idempotência e conflito de `request_id` mesmo após edição/exclusão do grupo, matriz completa do redutor `reduce_run_status` incluindo `partial`, e `recover_interrupted_runs`).

---

## 2. Comandos Exatos de Verificação

Execute os comandos abaixo a partir de `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0/src-tauri`:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p mt-db
cargo test -p mt-commands --test groups_validate
cargo test -p mt-commands --test groups_repo
```

---

## 3. Mapeamento dos Critérios de Aceite

| Critério da Fase 15 | Como é atendido e verificado nesta Implementação |
|---------------------|--------------------------------------------------|
| **CA-01** | `cargo fmt --check`, `cargo clippy --workspace -- -D warnings` e `cargo test -p mt-commands` verdes. |
| **CA-02** | Testado em `groups_repo.rs`: `CHECK` de `local`/`global` vs `project_id`, unicidade por `ux_groups_local_name` e `ux_groups_global_name` com `commands.group.name_conflict`, e `ON DELETE CASCADE` ao remover o projeto. |
| **CA-03** | Testado em `groups_repo.rs`: substituição transacional de itens em `update`, `commands.group.reorder_mismatch` em `reorder` inválido e ordenação `local_groups` → `global_groups` por `sort_order`. |
| **CA-04** | Testado em `groups_validate.rs`: rejeição de `\n`, `cwd` absoluto, `../..`, symlink externo, env `1ABC`, > 50 itens e segredos literais (`commands.group.secret_detected` com `details.fields`). |
| **CA-22** | Todas as mensagens de `MtError` e comentários em `pt-BR`; identificadores Rust em `en-US`. |
| **CA-23** | Testado em `groups_repo.rs`: `accept_run` + `find_run_by_request_id` devolvem a execução existente para o mesmo `(request_id, request_hash)` e retornam `commands.group.request_conflict` para payload divergente; edição/exclusão do grupo preserva `command_group_runs.snapshot_json`. |
| **CA-24 (parcial)** | Testado em `groups_repo.rs`: redutor `reduce_run_status` e `compute_counts` validados contra toda a matriz de transições (`succeeded`, `failed`, `partial`, `cancelled`, `interrupted`, `skipped`). |
| **CA-25 (parcial)** | Normalização Unicode NFC para `name_key` e `label_key` persistida e indexada. |
| **CA-26 (parcial)** | Controle de concorrência otimista por `revision` nos grupos (`commands.group.revision_conflict`) e `version` monotônica por execução e item. |
