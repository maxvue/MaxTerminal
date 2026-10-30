# Plano de Execução — Implementação 02: Repositórios de Abas, Painéis, Sessões, Scrollback e Operações

## 1. Checklist Sequencial de Implementação

### Passo 1: Repositórios de Abas e Painéis (`tabs.rs` e `panes.rs`)
1. Criar `src-tauri/crates/mt-db/src/repos/tabs.rs` implementando `TabsRepo`:
   - `create_tab_with_root_pane_tx`, `find_workspace_tab_by_template_tx`, `get_open_tab`, `get_tab_any_state`, `list_open_by_project`, `list_all_open`, `reorder`, `update_focus` e `apply_layout_patches`.
   - Validar invariantes da árvore (`validate_against_open_panes`), pertencimento de `active_pane_id` e `primary_pane_id`, e correspondência de `template_id` ao `project_id` na mesma transação.
2. Criar `src-tauri/crates/mt-db/src/repos/panes.rs` implementando `PanesRepo`:
   - `split_pane_tx`, `bump_generation_tx`, `close_intermediate_pane_tx` e `mark_all_tab_panes_closed_tx`.

### Passo 2: Repositório de Agrupamentos de Sessão (`tab_sessions.rs`)
1. Criar `src-tauri/crates/mt-db/src/repos/tab_sessions.rs` implementando `TabSessionsRepo`:
   - `start_group_tx` com alocação monotônica via `tabs.last_session_seq`.
   - `prepare_pane_session_tx` integrando `SessionsRepo::prepare` da Fase 05 e inserindo em `tab_session_panes`.
   - Transições atômicas de `initial_dispatch` (`claim_initial_dispatch`, `mark_initial_dispatch_waiting`, `mark_initial_dispatch_sent`).
   - `archive_closed_pane_session_tx` verificando `sessions.status = 'closed'`, `ended_at IS NOT NULL` e zero blocos `running` em `command_blocks`.
   - `archive_group_tx`, `list_by_tab` (com cursor `base64url` validado por `tab_id`) e `get_detail`.

### Passo 3: Repositório de Scrollback e Busca FTS5 (`scrollback.rs`)
1. Criar `src-tauri/crates/mt-db/src/repos/scrollback.rs` implementando `ScrollbackRepo`:
   - `insert_batch` reservando `sessions.next_scrollback_seq` e acumulando `scrollback_bytes` e `scrollback_truncated`.
   - `get_scrollback_page` com cursor `(tab_session_id, session_id, seq)`, validação de vínculo em `tab_session_panes`, limite de 200 itens e corte em 1 MiB.
   - `search_in_tab` utilizando `build_match_query` da Fase 03 em `session_scrollback_fts` com `JOIN` prévio em `tab_session_panes` e `tab_sessions`.

### Passo 4: Repositório de Operações / Ledger (`workspace_operations.rs`)
1. Criar `src-tauri/crates/mt-db/src/repos/workspace_operations.rs` implementando `WorkspaceOperationsRepo`:
   - `compute_args_hash` com SHA-256 sobre JSON canônico.
   - `acquire_or_inspect`, `update_stage`, `mark_completed` e `mark_stale_boots_interrupted`.

### Passo 5: Settings, Retenção e Cascata de Remoção de Projeto
1. Editar `src-tauri/crates/mt-db/src/settings.rs` para registrar `workspace.sessions.keep_per_tab=50`, `workspace.sessions.max_age_days=90` e `workspace.scrollback.limit_mib=8`.
2. Editar `src-tauri/crates/mt-db/src/retention.rs` para adicionar a limpeza por idade de sessões arquivadas e o job `enforce_keep_per_tab` (incluindo limpeza de abas manuais fechadas sem histórico e preservação de abas `origin = 'workspace'`).
3. Expor `delete_project_workspace_data_tx` em `src-tauri/crates/mt-db/src/repos/mod.rs` para remover sessões base, vínculos, grupos, painéis e abas na ordem correta sem violar `ON DELETE RESTRICT` de `tab_session_panes(pane_id)`.

### Passo 6: Testes de Integração dos Repositórios
1. Escrever testes em `mt-db` cobrindo:
   - Ciclo completo de abrir aba → split de painel → fechar painel intermediário → nova sessão (`seq+1`) → fechar aba.
   - Reordenação de abas e recusa com `tab.reorder_mismatch` quando falta ou sobra uma aba.
   - Conflito de `expected_layout_revision` retornando `layout.conflict`.
   - Paginação de `session_list` e `session_scrollback_get` (incluindo corte de 1 MiB com chunks de 64 KiB e rejeição de cursor de outra aba/sessão).
   - Busca FTS5 em `session_scrollback_fts` isolada por `tab_id` e sincronização dos triggers de INSERT/DELETE.
   - Retenção `keep_per_tab` e `max_age_days` preservando sessão ativa e abas `origin = 'workspace'`, e cascade completo ao remover projeto.

---

## 2. Comandos de Verificação

```bash
# Formatação e lint
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# Testes dos repositórios da Fase 07 em mt-db
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db repos::tabs
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db repos::panes
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db repos::tab_sessions
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db repos::scrollback
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db repos::workspace_operations
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db retention
```

---

## 3. Critérios de Aceite desta Implementação

- [ ] **CA-02.1 (Agrupamento Único e Sequências Independentes):** Cada aba aberta possui no máximo um registro ativo em `tab_sessions` (`ux_tab_sessions_active`); cada painel possui seu próprio `SessionId` em `sessions` com sequências `next_seq` (comandos) e `next_scrollback_seq` (scrollback) independentes e sem colisão.
- [ ] **CA-02.2 (Paginação, Teto de 1 MiB e Busca FTS5):** `get_scrollback_page` respeita o teto de 200 itens e 1 MiB por resposta; cursores adulterados ou de outra sessão falham com `session.query_invalid`; `search_in_tab` retorna apenas hits sanitizados da aba consultada.
- [ ] **CA-02.3 (Idempotência por `request_id`):** `WorkspaceOperationsRepo` detecta reutilização de `request_id` com argumentos divergentes (`workspace.request_conflict`), retorna o resultado salvo quando `completed` e bloqueia retomada silenciosa quando `interrupted` (`workspace.operation_interrupted`).
- [ ] **CA-02.4 (Retenção e Remoção de Projeto):** A retenção remove apenas sessões arquivadas excedentes (por idade e `keep_per_tab`), preserva sessões ativas e identidades `origin = 'workspace'`, e a remoção de projeto apaga todas as sessões, blocos, scrollback, vínculos, painéis e abas do projeto sem violar FKs (`PRAGMA foreign_key_check` vazio).
