# Implementação 01 — Plano de Execução Passo a Passo (`plan.md`)

> **Fase:** 12 — Integração MaxAiManager (`@` / `?`)
> **Unidade:** `01 - Fundação do Crate mt-ai, Persistência SQLite, Cliente HTTP, Catálogo e Registro`
> **Especificação técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1 — Criar a Migration SQLite da Fase 12 (`mt-db`):**
  1. Verificar o último número sequencial em `src-tauri/crates/mt-db/migrations/`.
  2. Criar `src-tauri/crates/mt-db/migrations/0006_12_ai.sql` (ou próximo índice livre `_12_ai.sql`) com as tabelas `ai_settings`, `ai_catalog_cache`, `ai_diagnosis_cache` e `ai_prompt_history` e seus respectivos índices e constraints `CHECK` conforme §3.2 da `spec.md`.
  3. Registrar a nova migration no runner de migrations em `src-tauri/crates/mt-db/src/migrations.rs`.

- [ ] **Passo 2 — Estruturar o Crate `mt-ai` no Workspace Cargo:**
  1. Criar `src-tauri/crates/mt-ai/Cargo.toml` com as dependências exatas de §3.1 da `spec.md` (garantindo zero dependência de `mt-pty`, `mt-shell`, `mt-projects` ou `tauri`).
  2. Adicionar `"crates/mt-ai"` em `[workspace.members]` no arquivo `src-tauri/Cargo.toml`.
  3. Criar `src-tauri/crates/mt-ai/assets/catalog.fallback.json` com os 4 harnesses canônicos (`claude-code`, `codex`, `agy`, `opencode`) e `models: []`.

- [ ] **Passo 3 — Implementar Identificadores, Códigos de Erro e Configurações (`ids.rs`, `error.rs`, `config.rs`):**
  1. Implementar `AiRequestId` (Uuid v7) em `src-tauri/crates/mt-ai/src/ids.rs`.
  2. Implementar todas as constantes `codes::AI_*` e construtores de `MtError` em `src-tauri/crates/mt-ai/src/error.rs`.
  3. Implementar `AiSettings`, `AiSelection`, `DiagnosisSettings`, `RagSettings`, `ContextSettings`, `TimeoutSettings`, `AiSettings::validate()`, `AiSettings::sanitized()`, `AiSettings::gateway_key()` e `is_loopback_host()` em `src-tauri/crates/mt-ai/src/config.rs`.

- [ ] **Passo 4 — Implementar o Registro de Requisições e Cancelamento (`registry.rs`):**
  1. Implementar `AiRequestRegistry`, `RequestMeta`, `RequestGuard` e `TerminalState` em `src-tauri/crates/mt-ai/src/registry.rs`.
  2. Garantir transição atômica `STATE_RUNNING -> STATE_COMPLETED | STATE_FAILED | STATE_CANCELLED`, suporte a tombstones de cancelamento precoce (antes de `begin`) com TTL de 120 s e limite de 256 entradas, recusa de IDs duplicados (`AI_REQUEST_CONFLICT`), `cancel_pane()` e `cancel_all()`.

- [ ] **Passo 5 — Implementar o Repositório SQLite (`repo.rs`):**
  1. Implementar `AiRepo` em `src-tauri/crates/mt-ai/src/repo.rs` com contadores atômicos `history_generation` e `cache_generation`.
  2. Implementar `get_settings()` e `save_settings()` com controle de concorrência otimista por `revision` (`AI_SETTINGS_CONFLICT`) e sanitização prévia dos campos textuais.
  3. Implementar `get_catalog_cache()`, `put_catalog_cache()`, `get_diagnosis_cache()` (filtrando `expires_at > now` e incrementando `hits` sem prorrogar `expires_at`), `put_diagnosis_cache()`, `insert_history()` (respeitando `save_history`, aplicando `mt_db::sanitizer::sanitize()` e podando acima de 5.000 linhas), `list_history()` (paginação keyset `WHERE id < :before_id ORDER BY id DESC LIMIT :limit`), `clear_history()` e `clear_cache()`.

- [ ] **Passo 6 — Implementar Dialeto, Streaming SSE e Cliente HTTP (`client/{mod.rs,dialect.rs,sse.rs,http.rs}`):**
  1. Implementar a trait `GatewayDialect` e `OpenAiCompatDialect` em `src-tauri/crates/mt-ai/src/client/dialect.rs`.
  2. Implementar o consumidor de stream SSE em `src-tauri/crates/mt-ai/src/client/sse.rs` com coalescência de deltas a cada `32 ms`, janela de retenção de cauda de `128 bytes` para sanitização incremental entre fronteiras de chunks (`CA-36`), timeout de inatividade de `15 s`, limite de `64 KiB` por frame e `1 MiB` total, e erro `AI_BAD_RESPONSE` em caso de EOF sem `[DONE]`.
  3. Implementar `GatewayClient` em `src-tauri/crates/mt-ai/src/client/http.rs` com header obrigatório `X-Agent-Name: MaxTerminal`, `reconfigure()` atômico (`CA-06`), desativação de redirects automáticos, mapeamento de erros (`AI_GATEWAY_UNAVAILABLE`, `AI_TIMEOUT`, `AI_AUTH_REQUIRED`, `AI_RATE_LIMITED`, `AI_HTTP_STATUS`) e retry restrito a `Op::Health` e `Op::Catalog`.

- [ ] **Passo 7 — Implementar o Serviço de Catálogo (`catalog/{mod.rs,fallback.rs}`):**
  1. Implementar o carregamento e parse de `assets/catalog.fallback.json` via `include_str!` em `src-tauri/crates/mt-ai/src/catalog/fallback.rs`.
  2. Implementar `CatalogService` em `src-tauri/crates/mt-ai/src/catalog/mod.rs` com cache em memória + SQLite (TTL 6h, `If-None-Match` / `ETag` por `gateway_key`), mesclagem garantindo sempre os 4 harnesses canônicos, `resolve()` e `lowest_reasoning()`.
  3. Expor os módulos criados em `src-tauri/crates/mt-ai/src/lib.rs`.

- [ ] **Passo 8 — Escrever e Executar a Suíte de Testes da Implementação 01:**
  1. Criar `src-tauri/crates/mt-ai/tests/no_pty_dependency.rs` inspecionando `cargo metadata` para provar que `mt-ai` não depende de `mt-pty` nem `mt-shell`.
  2. Criar `src-tauri/crates/mt-ai/tests/repo.rs` testando migration, `PRAGMA foreign_key_check`, sanitização de `APP_KEY=base64:...` e `Bearer ...`, `save_history = false`, poda de 5.000 linhas e geração de limpeza contra requisições concorrentes.
  3. Criar `src-tauri/crates/mt-ai/tests/gateway_mock.rs` com `wiremock` cobrindo header `X-Agent-Name: MaxTerminal` em todos os endpoints, troca de URL em runtime, rejeição de host não-loopback, SSE coalescido, EOF prematuro, cancelamento `< 200 ms`, timeout de 300 ms e segredo dividido entre dois chunks SSE.
  4. Criar `src-tauri/crates/mt-ai/tests/catalog.rs` cobrindo `Gateway`, `Cache`, `Fallback`, TTL sem HTTP, `ETag` + `304`, e `resolve()` rejeitando modelos/níveis inventados com `AI_UNKNOWN_SELECTION`.

---

## 2. Comandos de Verificação e Validação

Execute os comandos abaixo a partir da raiz da worktree:

```bash
# 1. Verificar compilação e lint estrito do crate mt-ai e mt-db
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-db -p mt-ai --all-targets -- -D warnings

# 2. Executar testes de isolamento estrutural (sem dependência de PTY)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai --test no_pty_dependency

# 3. Executar testes de persistência SQLite, sanitização e migrations
cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai --test repo

# 4. Executar testes de contrato HTTP, SSE, cancelamento e sanitização de chunks com wiremock
cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai --test gateway_mock

# 5. Executar testes de catálogo, TTL, ETag 304 e fallback dos 4 harnesses
cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai --test catalog
```

---

## 3. Mapeamento dos Critérios de Aceite da Fase 12

| Critério | Cobertura nesta Implementação | Teste / Verificação |
|----------|-------------------------------|---------------------|
| `CA-01` (parcial) | Compilação limpa no `clippy -D warnings` e testes dos módulos base de `mt-ai`. | `cargo clippy -p mt-ai` e `cargo test -p mt-ai` |
| `CA-05` | Header `X-Agent-Name: MaxTerminal` presente em toda requisição HTTP. | `tests/gateway_mock.rs::every_request_has_agent_header` |
| `CA-06` | Alteração de `base_url` via `reconfigure()` passa a valer na próxima requisição sem reiniciar. | `tests/gateway_mock.rs::reconfigure_switches_base_url_atomically` |
| `CA-07` | Host não-loopback rejeitado com `AI_URL_NOT_LOOPBACK` salvo `allow_remote = true`. | `tests/gateway_mock.rs::rejects_non_loopback_unless_allowed` |
| `CA-08` | Stream SSE de 5 deltas coalescido + `Completed`; JSON único; EOF prematuro gera `Failed`/`AI_BAD_RESPONSE`. | `tests/gateway_mock.rs::sse_deltas_json_single_and_premature_eof` |
| `CA-09` | Cancelamento encerra stream em `< 200 ms` e nenhuma mensagem chega após `Cancelled`. | `tests/gateway_mock.rs::cancel_aborts_stream_under_200ms` |
| `CA-10` | Timeout de 300 ms gera `AI_TIMEOUT`; gateway desligado gera `AI_GATEWAY_UNAVAILABLE` com mensagem em pt-BR. | `tests/gateway_mock.rs::timeout_and_unavailable_errors` |
| `CA-11` | Catálogo do gateway (`Gateway`), cache (`Cache`) ou fallback de identidades (`claude-code`, `codex`, `agy`, `opencode`) sem inventar modelos/níveis. | `tests/catalog.rs::catalog_sources_and_strict_resolve` |
| `CA-12` | Segunda chamada dentro do TTL não faz HTTP; `ETag` + `304` reaproveita cache. | `tests/catalog.rs::ttl_skips_http_and_etag_304_reuses_cache` |
| `CA-29` | Migration aplica sem erros e `PRAGMA foreign_key_check` retorna vazio. | `tests/repo.rs::migration_and_foreign_key_check` |
| `CA-30` | `save_history = false` não grava linhas; `clear_history` e `clear_cache` esvaziam tabelas. | `tests/repo.rs::history_opt_out_and_clear_operations` |
| `CA-31` | Nenhum texto gravado nas tabelas `ai_*` contém segredos (`APP_KEY=base64:...` → `[REDACTED]`). | `tests/repo.rs::sanitizes_all_text_columns_before_insert` |
| `CA-32` | Logs `tracing` de `mt-ai` não registram prompts nem respostas em nível `>= info`. | `tests/gateway_mock.rs::tracing_logs_contain_no_prompt_or_response` |
| `CA-33` (backend) | Tombstones de cancelamento precoce, recusa de IDs duplicados (`AI_REQUEST_CONFLICT`) e geração monotônica de cache/histórico. | `tests/repo.rs::concurrent_clear_does_not_repopulate` |
| `CA-35` | Capacidades não validadas permanecem `Unavailable` com motivo em pt-BR. | `tests/gateway_mock.rs::unvalidated_capabilities_report_unavailable` |
| `CA-36` (stream) | Segredo dividido entre dois chunks SSE (`Bearer ` + `secret_token_123`) é sanitizado antes de `on_delta`. | `tests/gateway_mock.rs::sanitizes_secret_split_across_sse_chunks` |
