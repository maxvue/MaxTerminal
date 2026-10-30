# Plano de Execução — Implementação 01: Domínio Workspace, Migrations Aditivas, Templates e Backfill

## 1. Checklist Sequencial de Implementação

### Passo 1: Identificadores e Códigos de Erro em `mt-core`
1. Editar `src-tauri/crates/mt-core/src/ids.rs` para adicionar o newtype `TabSessionId(pub uuid::Uuid)` com derivações `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type` e métodos `new_v7()`, `Display` e `FromStr`.
2. Editar `src-tauri/crates/mt-core/src/error.rs` para adicionar todos os construtores de erro da Fase 07 (`template_invalid`, `template_default_required`, `template_secret_literal`, `template_cwd_not_found`, `template_conflict`, `template_not_found`, `workspace_environment_unavailable`, `workspace_request_conflict`, `workspace_operation_interrupted`, `workspace_stop_failed`, `workspace_persistence_pending`, `tab_not_found`, `tab_closing`, `tab_reorder_mismatch`, `pane_not_found`, `pane_limit_reached`, `pane_not_exited`, `layout_invalid`, `layout_conflict`, `session_not_found`, `session_query_invalid`).

### Passo 2: Modelos e Validações Puras em `mt-core::workspace`
1. Criar `src-tauri/crates/mt-core/src/workspace/template.rs` com:
   - `EnvironmentKind`, `TemplateEnvironment`, `EnvValue`, `TabTemplateInput`, `TemplateConfig`, `TabTemplate`, `CommandOverride`, `TabOpenOverrides`, `TabOpenRequest`, `LaunchSnapshot`.
   - `validate_template_input`, `validate_tab_open_overrides` e `resolve_and_validate_cwd`.
2. Criar `src-tauri/crates/mt-core/src/workspace/layout.rs` com:
   - `SplitDirection`, `PaneLayoutNode`, `LayoutSizePatch`, `RemoveLeafResult`.
   - Métodos puros `leaves`, `depth`, `find_node`, `validate_against_open_panes`, `split_leaf`, `remove_leaf` e `apply_size_patches`.
3. Criar `src-tauri/crates/mt-core/src/workspace/tab.rs` com:
   - `PaneRuntimeState`, `PaneView`, `TabLifecycle`, `TabOrigin`, `TabView`, `PaneCloseOutcome`.
4. Criar `src-tauri/crates/mt-core/src/workspace/session.rs` com:
   - `SessionEndReason`, `InitialDispatchState`, `SessionSummary`, `SessionPaneSummary`, `SessionDetail`, `ScrollbackChunk`, `CursorPage<T>`, `SessionSearchHit`, `WorkspaceRestoreFailure`, `WorkspaceRestoreReport`, `WorkspaceSnapshot`.
5. Criar `src-tauri/crates/mt-core/src/workspace/mod.rs` reexportando `template`, `layout`, `tab` e `session`, e registrar `pub mod workspace;` em `src-tauri/crates/mt-core/src/lib.rs`.

### Passo 3: Migration SQLite Aditiva e Registro FTS5
1. Criar `src-tauri/crates/mt-db/migrations/0007_07_workspace_templates_tabs_sessions.sql` com:
   - DDL de `tab_templates` e índices `ux_tab_templates_default`, `ix_tab_templates_project`, `ix_tab_templates_workspace`.
   - DDL de `tabs` e `panes` com índices parciais de abas/painéis abertos e unicidade de template workspace (`ux_tabs_workspace_template`).
   - `ALTER TABLE sessions` adicionando `archived_at`, `next_scrollback_seq`, `scrollback_bytes`, `scrollback_truncated` e índice `ix_sessions_archived`.
   - DDL de `tab_sessions` e `tab_session_panes` com índices de grupo ativo e histórico.
   - DDL de `session_scrollback`, tabela virtual `session_scrollback_fts` e triggers `trg_session_scrollback_ai/ad/au`.
   - DDL de `workspace_operations` e índice `ix_workspace_operations_pending`.
2. Registrar a `FtsSpec` de `session_scrollback_fts` no catálogo de manutenção da Fase 03 em `mt-db` sem recriar `command_blocks_fts` da Fase 05.

### Passo 4: Repositório de Templates e Contribuidor Atômico da Fase 06
1. Criar `src-tauri/crates/mt-db/src/repos/templates.rs` implementando `TemplateRepo` (`ensure_default_tx`, `list`, `get`, `create`, `update`, `delete`, `set_default`) e `DefaultTemplateContributor`.
2. Registrar `DefaultTemplateContributor` no construtor/inicializador de `ProjectsRepo` / `SqliteProjectStore` dentro de `src-tauri/crates/mt-db/src/repos/mod.rs`, garantindo que `after_insert(tx, project)` seja chamado na mesma transação de criação, importação e auto-descoberta de projetos.

### Passo 5: Code Migration de Backfill (`m07_backfill.rs`)
1. Criar `src-tauri/crates/mt-db/src/migrations/code/m07_backfill.rs` implementando:
   - Verificação estrutural da tabela `sessions` da Fase 05.
   - Execução de `TemplateRepo::ensure_default_tx` para todos os projetos existentes.
   - Vinculação não destrutiva das linhas legadas de `sessions` em agrupamentos históricos fechados quando `project_id` existir em `projects`, preservando sessões órfãs sem descarte.
   - Verificação final com `PRAGMA foreign_key_check`.
2. Registrar a migration SQL e `m07_backfill` em `src-tauri/crates/mt-db/src/migrations/mod.rs` e adicionar reparo defensivo de template padrão no startup de `DbSetup`.

### Passo 6: Testes Unitários e de Contrato SQLite
1. Escrever testes de propriedade e casos de borda da árvore de layout em `src-tauri/crates/mt-core/src/workspace/layout.rs` (split até 8 folhas, recusa da 9ª folha, limite de profundidade 7, colapso de irmão ao remover folha, eleição de `active_pane_id` vizinho, transferência de `primary_pane_id`, patches com soma ≠ 100 ou `< 5%`, `node_id` obsoleto).
2. Escrever testes de validação de template em `src-tauri/crates/mt-core/src/workspace/template.rs` (limites de nome/ícone/comando, prefixos proibidos `MT_`/`MAXTERMINAL_`, detecção de literais secretos, resolução de `cwd` com `~` e caminhos relativos).
3. Escrever testes de integração SQLite em `src-tauri/crates/mt-db/src/repos/templates.rs` e `m07_backfill.rs` (banco preenchido com dados das Fases 03/05/06 antes da migration 07, preservação de `sessions` e `command_blocks`, eleição determinística de template padrão, troca atômica de padrão, rollback de projeto quando falha o contribuidor).

---

## 2. Comandos de Verificação

```bash
# Formatação e análise estática do workspace Rust
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# Testes unitários do domínio puro (mt-core)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-core workspace::

# Testes de migrations, backfill e TemplateRepo (mt-db)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db templates
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db m07_backfill
```

---

## 3. Critérios de Aceite desta Implementação

- [ ] **CA-01.1 (Padrão Atômico em Projetos):** Projeto avulso, importado, aceito da descoberta ou autoimportado já nasce visível com exatamente um template padrão; falha forçada ao inserir o padrão desfaz a criação do projeto/lote inteiro.
- [ ] **CA-01.2 (Backfill Idempotente):** Executar o backfill duas vezes seguidas mantém o padrão existente, elege o menor `(position, created_at, id)` quando há templates sem padrão, ou cria `"Terminal"` (`mdi:console`) apenas quando o projeto não possui templates, sem abrir abas ou PTYs.
- [ ] **CA-01.3 (Invariante de Template Padrão Único):** O índice parcial `ux_tab_templates_default` impede dois padrões no mesmo projeto; `set_default` desmarca e marca na mesma transação; excluir o template padrão falha com `template.default_required`; excluir outro template mantém abas e histórico (`template_id = NULL`).
- [ ] **CA-01.4 (Sanitização e Validação de Parâmetros):** Todos os parâmetros de template (`name`, `icon`, `cwd`, `initial_command`, `add_to_workspace`, `continuous`, `environment`, `env_vars`) são validados; segredos literais são recusados com `template.secret_literal`; referências `Inherited` são aceitas sem gravar segredos em disco.
- [ ] **CA-01.5 (Árvore Canônica de Painéis):** `SplitDirection::Horizontal` e `SplitDirection::Vertical`, limite de 8 folhas, profundidade ≤ 7, tamanhos ≥ 5% somando 100 ± 0,01, colapso de nó pai ao fechar folha e transferência determinística de `active_pane_id` e `primary_pane_id` passam em 100% dos testes de domínio.
- [ ] **CA-01.6 (Preservação de Histórico da Fase 05):** A migration 07 aplicada sobre um banco contendo linhas em `sessions` e `command_blocks` preserva todos os IDs, blocos, FKs e textos, vinculando sessões identificáveis e mantendo intactas as sessões legadas sem projeto.
