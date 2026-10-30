# Implementação 02 — Plano de Execução Passo a Passo (`plan.md`)

> **Fase:** 12 — Integração MaxAiManager (`@` / `?`)
> **Unidade:** `02 - Contexto Sanitizado, Classificador de Risco, Sugestão NL-Shell, Diagnóstico Inline, RAG e Geração Assistida`
> **Especificação técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1 — Implementar o Módulo de Contexto e Sanitização Dupla (`context.rs`):**
  1. Criar `src-tauri/crates/mt-ai/src/context.rs` com `OsInfo`, `RecentCommand`, `AiContext`, `PaneBindingSnapshot`, `BlockFailure` e a trait `AiContextSource`.
  2. Implementar `strip_ansi_and_sanitize()` usando `strip_ansi_escapes::strip` + `mt_db::sanitizer::sanitize()`, `abbreviate_home()`, `truncate_utf8_head()` (`<= 1 KiB` para comandos) e `truncate_utf8_tail()` (`<= output_tail_bytes` mantendo o final da saída).
  3. Implementar `detect_linux_os_info()` lendo `/etc/os-release` e `/proc/sys/kernel/osrelease` com fallback seguro para `"Linux"`.

- [ ] **Passo 2 — Implementar o Classificador Determinístico de Risco (`risk.rs`):**
  1. Criar `src-tauri/crates/mt-ai/src/risk.rs` com `RiskLevel`, `RiskReason`, `RiskAssessment` e `RiskClassifier`.
  2. Implementar o divisor de segmentos (`&&`, `||`, `;`, `|`, `&`, subshells `$(...)` e crases), o tokenizador via `shell-words` (tratando erro de parse como `Caution`), e o removedor de prefixos (`sudo`, `doas`, `env`, `nice`, `time`, `xargs`).
  3. Cadastrar todas as regras `Destructive` e `Caution` detalhadas em §3.2 da `spec.md`, incluindo proteção contra falsos positivos em `echo "rm -rf"` (`Caution`) e `grep rm file` (`Safe`).
  4. Implementar `RiskClassifier::combine(local, hint)` garantindo `level = max(local.level, hint)` e preenchendo `confirm_token` com o 1º token do comando quando `level == RiskLevel::Destructive`.

- [ ] **Passo 3 — Implementar os Templates de Prompt em Português (`prompts/`):**
  1. Criar `src-tauri/crates/mt-ai/src/prompts/mod.rs`, `suggest.rs`, `diagnose.rs` e `structured.rs`.
  2. Garantir que o output do terminal anexado nos prompts de sugestão e diagnóstico seja sempre delimitado por `<<<OUTPUT\n...\nOUTPUT>>>` com instrução explícita ao modelo para tratá-lo como dado bruto não confiável (prevenção de prompt injection — §10.6 da spec da Fase 12).

- [ ] **Passo 4 — Implementar o Serviço de Sugestão NL→Shell (`suggest.rs`):**
  1. Criar `src-tauri/crates/mt-ai/src/suggest.rs` com `SuggestRequest`, `AiTrigger`, `CommandSuggestion`, `AiStreamEvent` e `parse_suggestion()`.
  2. Implementar `parse_suggestion()` aceitando exclusivamente JSON válido (direto ou em bloco cercado único ```` ```json ````), rejeitando prosa livre, comandos vazios (`AI_EMPTY_COMMAND`), quebras de linha `\n`/`\r` ou bytes de controle `NUL`/`ESC` (`AI_BAD_RESPONSE`).
  3. Integrar `suggest_command` com `CatalogService::resolve()`, `AiContextSource::for_pane()`, `GatewayClient::chat()`, `RiskClassifier::combine()` e `AiRepo::insert_history()`.

- [ ] **Passo 5 — Implementar o Serviço de Diagnóstico Rápido Inline (`diagnose.rs`):**
  1. Criar `src-tauri/crates/mt-ai/src/diagnose.rs` com `InlineDiagnosis`, `DiagnosisSkip`, `DiagnosisOutcome`, `DiagnoseRequest`, `DiagnosisCacheKey` e `DiagnosisService`.
  2. Implementar `normalize()` e `error_hash()` (SHA-256 canônico sobre `DiagnosisCacheKey`).
  3. Implementar a máquina de execução: filtros de elegibilidade (`Disabled`, `Interactive`, `IgnoredExitCode`, `ProjectOptOut`), substituição por painel (`Superseded`), consulta ao cache SQLite (`from_cache: true` e reclassificação local de risco do `fix_command`), rate limiter `max_per_minute`, fila limitada de capacidade 3 + `Semaphore(1)`, chamada JSON ao gateway usando o menor nível de raciocínio suportado pelo modelo (`lowest_reasoning`), truncamento de `summary` em 160 caracteres e gravação no cache.

- [ ] **Passo 6 — Implementar o Serviço de Consulta RAG (`rag.rs`):**
  1. Criar `src-tauri/crates/mt-ai/src/rag.rs` com `RagSlug`, `RagSearchRequest`, `RagResult`, `RagSearchResponse` e `RagService`.
  2. Implementar `validate_slug()` (`^[a-z0-9][a-z0-9_-]{0,63}$` → `AI_INVALID_SLUG`), construção segura de URL com `path_segments_mut()`, mapeamento de `404` na busca para `AI_RAG_NOT_FOUND`, sanitização de resultados, truncamento de `content` em 2 KiB, validação de URLs `http`/`https` e ordenação estável por `score`.

- [ ] **Passo 7 — Implementar o Gerador Estruturado (`structured.rs`) e Fachada `AiService` (`lib.rs`):**
  1. Criar `src-tauri/crates/mt-ai/src/structured.rs` com `StructuredRequest`, `StructuredResponse` e `StructuredGenerator`.
  2. Implementar `validate_schema_security()` recusando schemas `> 32 KiB` ou com `$ref` externo, compilação com `jsonschema::Validator` cacheada em `DashMap` (máx. 64), e fluxo de validação com até 1 tentativa de reparo automático dentro do deadline total.
  3. Completar `src-tauri/crates/mt-ai/src/lib.rs` implementando `AiService` e o worker de health check com `tokio::sync::watch`.

- [ ] **Passo 8 — Escrever e Executar a Suíte Completa de Testes Rust (`mt-ai`):**
  1. Criar `src-tauri/crates/mt-ai/tests/risk_rules.rs` com `>= 40` casos tabelados (`CA-19`), teste de não-rebaixamento (`CA-17`), verificação de `confirm_token` (`CA-16`) e benchmark `< 1 ms` para comandos `<= 1 KiB`.
  2. Criar `src-tauri/crates/mt-ai/tests/suggest_parse.rs` testando JSON direto, bloco cercado `json`, rejeição de prosa, comando vazio, multilinha, NUL/ESC e verificação de sanitização no contexto enviado (`CA-18`).
  3. Criar `src-tauri/crates/mt-ai/tests/diagnose_cache.rs` testando `error_hash`, cache hit incrementando `hits` sem HTTP (`CA-21`), `exit 130` e modo interativo ignorados (`CA-22`), substituição `Superseded` no mesmo painel, independência entre dois painéis da mesma aba (`CA-34`) e fila limitada (3) + rate limit.
  4. Criar `src-tauri/crates/mt-ai/tests/structured.rs` testando JSON válido de primeira, reparo em 1 tentativa, falha persistente retornando `AI_SCHEMA_VALIDATION` com `details.errors` (`CA-27`) e bloqueio de `$ref` remoto (`CA-36`).
  5. Adicionar testes de RAG (`CA-25`, `CA-26`) em `src-tauri/crates/mt-ai/tests/gateway_mock.rs`.

---

## 2. Comandos de Verificação e Validação

Execute os comandos abaixo a partir da raiz da worktree:

```bash
# 1. Lint estrito sem warnings em todos os alvos de mt-ai (CA-01)
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-ai --all-targets -- -D warnings

# 2. Testar as >= 40 regras do classificador de risco e performance < 1ms (CA-16, CA-17, CA-19)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai --test risk_rules

# 3. Testar o parser de sugestões NL->Shell e sanitização de contexto (CA-15, CA-18)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai --test suggest_parse

# 4. Testar diagnóstico inline, cache por hash, fila, Superseded e isolamento por painel (CA-20, CA-21, CA-22, CA-34)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai --test diagnose_cache

# 5. Testar geração estruturada, tentativa única de reparo e bloqueio de $ref externo (CA-27, CA-36)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai --test structured

# 6. Executar toda a suíte do crate mt-ai (CA-01)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-ai
```

---

## 3. Mapeamento dos Critérios de Aceite da Fase 12

| Critério | Cobertura nesta Implementação | Teste / Verificação |
|----------|-------------------------------|---------------------|
| `CA-01` | `cargo clippy -p mt-ai --all-targets -- -D warnings` e `cargo test -p mt-ai` 100% verdes. | Suíte completa de `mt-ai` |
| `CA-16` (backend) | `rm -rf ./build` classificado como `destructive` com `confirm_token = Some("rm")`; `ls -la` como `safe`. | `tests/risk_rules.rs` |
| `CA-17` | Modelo retornando `risk: safe` para `git push --force` não rebaixa o nível local (`destructive`). | `tests/risk_rules.rs::combine_never_downgrades_local_risk` |
| `CA-18` (backend) | Comando recente contendo `Bearer abc123` é mascarado no `AiContext` e no payload HTTP enviado ao mock. | `tests/suggest_parse.rs::context_masks_bearer_token_in_preview_and_http` |
| `CA-19` | `>= 40` casos de teste incluindo `rm -rf /`, `dd`, `mkfs.ext4`, `git push --force`, `git reset --hard`, `git clean -fdx`, `chmod -R 777 /`, fork bomb, `curl ... \| sh`, `> /dev/sda`, `DROP DATABASE`, `php artisan migrate:fresh`, encadeamento `ls && rm -rf x` (`destructive`), `echo "rm -rf"` (`caution`) e `grep rm file` (`safe`). | `tests/risk_rules.rs::exhaustive_risk_table` |
| `CA-20` (backend) | Requisições concorrentes no mesmo painel cancelam a anterior (`Superseded`). | `tests/diagnose_cache.rs::supersedes_previous_diagnosis_on_same_pane` |
| `CA-21` | Mesmo `error_hash` na 2ª vez é servido do `ai_diagnosis_cache` com `from_cache = true` e `hits` incrementado sem HTTP. | `tests/diagnose_cache.rs::second_identical_error_hits_cache_without_http` |
| `CA-22` (backend) | `exit 130`, modo interativo/TUI e projetos com opt-out retornam `DiagnosisOutcome::Skipped`. | `tests/diagnose_cache.rs::skips_ignored_exit_interactive_and_opted_out_project` |
| `CA-25` (backend) | Busca RAG em `/v1/rags/{slug}/search` com `{ query, top_k }` funciona mesmo sem `default_selection`. | `tests/gateway_mock.rs::rag_search_works_without_default_selection` |
| `CA-26` | Slug `404` retorna `AI_RAG_NOT_FOUND` com mensagem pt-BR; slug fora de `[a-z0-9-_]` falha antes do HTTP com `AI_INVALID_SLUG`. | `tests/gateway_mock.rs::rag_invalid_slug_and_404_handling` |
| `CA-27` | `generate_structured` valida JSON Schema, repara em 1 tentativa e falha com `AI_SCHEMA_VALIDATION` + `details.errors` se persistir inválido. | `tests/structured.rs` |
| `CA-34` (backend) | Dois painéis da mesma aba diagnosticam independentemente; `error_hash` inclui gateway/projeto/cwd/shell/seleção/versões; dispensar um `block_id` não afeta outro bloco com o mesmo hash. | `tests/diagnose_cache.rs::split_panes_and_block_dismiss_isolation` |
| `CA-36` (schema) | JSON Schema com `$ref` remoto (`http://...`) é rejeitado sem acesso à rede; strings da resposta estruturada são sanitizadas. | `tests/structured.rs::rejects_remote_ref_and_sanitizes_output` |
