# Plano de Execução — Implementação 02: Catálogo de Labels, Extração de Plano, Cache SQLite e Sincronização (`mt-github` + `mt-db`)

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `02 - Catalogo-de-Labels-Extracao-de-Plano-Cache-SQLite-e-Sync`
> **Especificação Técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Criar os Artefatos do Catálogo MaxCode e Implementar `labels.rs`
1. Criar `src-tauri/crates/mt-github/fixtures/maxcode/labels.v1.json` contendo o export estruturado de `~/GitHub/MaxCode/src/core/labelsDeBugs.ts` (commit `1054eb8a886de412de8912ad56f64b1367da8b09`) com as 24 labels exatas (15 `state`, 3 `closure`, 6 `category`), as regras de transição exportadas e os hashes SHA-256 (`sourceSha256` e `exportSha256`).
2. Criar `src-tauri/crates/mt-github/fixtures/maxcode/labels-cases.v1.json` com vetores de comparação (caixa mista, espaços extras, strings vazias, labels externas e combinações válidas/inválidas de estado).
3. Implementar `src-tauri/crates/mt-github/src/labels.rs`:
   - `LabelCatalogValidator::load_embedded()` e `validate_raw_json()` verificando cardinalidade exata (24 total, 15 `State`, 3 `Closure`, 6 `Category`), unicidade byte a byte e por chave normalizada, integridade das referências em `transitions` e verificação do `exportSha256`.
   - `normalize_label_name()` reproduzindo 100% dos casos de `labels-cases.v1.json`.
   - `preview_patch()` calculando o conjunto resultante de labels (preservando labels de categoria e externas) e retornando avisos não-bloqueantes quando a transição de estado violar as regras conhecidas.
   - `ensure_labels()` listando todas as labels remotas do repositório, identificando as faltantes entre as 24 canônicas e criando exclusivamente as ausentes mediante confirmação explícita (`confirmed == true`), tratando corrida `422` por releitura.

### Passo 2: Implementar o Extrator Estrutural de Plano (`plan.rs`)
1. Implementar `src-tauri/crates/mt-github/src/plan.rs` utilizando `pulldown-cmark`:
   - Percorrer `Parser::new_ext(markdown, Options::all()).into_offset_iter()`.
   - Rastrear profundidade de `CodeBlock`, `BlockQuote` e blocos HTML para ignorar falsos cabeçalhos dentro de exemplos de código ou citações.
   - Detectar headings `H2` cujo texto plano (após trim) seja exatamente `"PLANO DE IMPLEMENTAÇÃO"`.
   - Delimitar o fim da seção no início do próximo heading `H1` ou `H2` de nível superior ou no fim do texto.
   - Calcular `start_line` e `end_line` (base 1) e retornar a última ocorrência no documento.
2. Implementar `resolve_issue_plan()` avaliando o corpo da issue e todos os comentários carregados, selecionando o plano de maior `(updated_at_ms, created_at_ms, source_comment_id)` e atribuindo `provisional = !comments_complete`.

### Passo 3: Criar a Migration SQLite (`NNNN_16_github.sql`) e o Repositório `GithubCacheRepo` (`cache.rs`)
1. Criar a migration `src-tauri/crates/mt-db/migrations/NNNN_16_github.sql` (usando o próximo prefixo numérico sequencial em `mt-db/migrations/`) com as 10 tabelas e todos os índices definidos em `spec.md` §3.3.
2. Implementar `src-tauri/crates/mt-github/src/cache.rs`:
   - Utilizar o writer thread do `mt-db` para transações de escrita e `spawn_blocking` com conexões de leitura para consultas.
   - Passar todos os campos textuais (`title`, `body`, `comment.body`, `label.description`, `sanitized_json`, `sanitized_progress_json`, `result_json`) por `mt_db::sanitizer::sanitize()` antes de executar `INSERT`/`UPDATE`.
   - Garantir que `commit_full_issues_sync` e `commit_incremental_issues_sync` executem em uma única transação SQLite: atualizar `github_labels`, `github_issues`, `github_issue_labels`, recalcular `open_count` (contando apenas `state = 'open' AND unavailable = 0`), incrementar `revision` monotônica e atualizar `watermark_ms`.
   - Implementar paginação local parametrizada em `query_issues_page` suportando filtro por `state`, interseção (`AND`) de múltiplas labels normalizadas e busca textual com escape de caracteres curinga (`%`, `_`, `\`) no `LIKE ... ESCAPE '\'`, ordenada por `(updated_at_ms DESC, number DESC)`.
   - Integrar a política de limpeza no `RetentionRegistry` da Fase 03 (expiração de HTTP cache em 24h e detalhes de issues fechadas em 30 dias, preservando issues abertas e ciclos/mutações pendentes ou incertos).

### Passo 4: Implementar o Serviço de Sincronização (`sync.rs`)
1. Implementar `IssueSyncService` em `src-tauri/crates/mt-github/src/sync.rs`:
   - Coalescência de operações em andamento por chave `(repo_id, auth_scope, SyncTarget)` para que chamadas simultâneas compartilhem a mesma execução paginada.
   - Semáforo global limitando a 2 sincronizações simultâneas, priorizando o projeto ativo.
   - Revalidação de cada página com `ETag` guardado em `github_http_cache` (por `request_key` que inclui versão do sanitizer): se a página 1 retornar `304 Not Modified`, reutilizar o `next_url` persistido naquela página para inspecionar as páginas seguintes.
   - Cálculo de `OpenIssueCount`: `None` enquanto `complete == 0` (nunca sincronizou todas as páginas), `Some(n)` após sincronização completa reconciliada, excluindo Pull Requests antes de persistir.
   - Execução de mutações (`create_issue` e `update_issue_labels`) registrando intenção em `github_mutations` com digest HMAC efêmero, aplicando diff incremental (`add_issue_labels` / `remove_issue_label`), verificando `expected_updated_at_ms` e emitindo `github://issues-updated` e `github://issue-updated` após commit local.

### Passo 5: Escrever Testes Automatizados (`tests/labels.rs`, `tests/plan.rs`, `tests/cache.rs`, `tests/sync.rs`)
1. Criar `src-tauri/crates/mt-github/tests/labels.rs`:
   - Verificar cardinalidade exata 24 (15/3/6), hashes de procedência e todos os vetores de `labels-cases.v1.json`.
   - Verificar que adulteração de hash, label faltante ou duplicada resulta em `CatalogStatus::Invalid`.
   - Testar `preview_patch` idempotente preservando labels externas e categorias.
   - Testar `ensure_labels` garantindo que apenas labels ausentes são criadas e labels existentes com outra cor não sofrem alteração.
2. Criar `src-tauri/crates/mt-github/tests/plan.rs`:
   - Issue com 3 comentários contendo `## PLANO DE IMPLEMENTAÇÃO` retorna o último.
   - `## PLANO DE IMPLEMENTAÇÃO` dentro de bloco cercado ```` ```markdown ```` ou blockquote `>` é ignorado.
   - Issue sem seção de plano retorna `None`; comentários incompletos marcam `provisional: true`.
3. Criar `src-tauri/crates/mt-github/tests/cache.rs`:
   - Verificar aplicação da migration, constraints FK/CASCADE e índices via `EXPLAIN QUERY PLAN`.
   - Verificar TTL (`stale = true` após 120s para lista e 300s para detalhe).
   - Verificar que `sanitize()` mascara tokens `Bearer` e `APP_KEY` no corpo e comentários antes de gravar no disco.
   - Verificar que falha simulada na página 2 de 3 preserva intactos o `open_count` e `watermark_ms` anteriores.
   - Verificar isolamento de `auth_scope` (troca de conta não lê cache privado da conta anterior) e reconciliação em lote com > 1000 issues (`TEMP TABLE seen_issue_numbers` sem estourar limite de parâmetros SQLite).
4. Criar `src-tauri/crates/mt-github/tests/sync.rs`:
   - Disparar 10 chamadas concorrentes de `sync_issues_list` contra um servidor `wiremock` de 3 páginas e comprovar que o servidor recebe exatamente 1 sequência paginada (3 requisições GET no total, não 30).

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Compilar mt-db e mt-github com todos os testes
cargo check -p mt-db -p mt-github --all-targets

# 2. Executar testes de catálogo de labels, extração de plano, cache SQLite e sincronização
cargo test -p mt-github --test labels --test plan --test cache --test sync

# 3. Executar clippy sem warnings
cargo clippy -p mt-db -p mt-github --all-targets -- -D warnings
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

| Critério (Fase 16) | Como é atendido e verificado |
|--------------------|------------------------------|
| **CA-1 (`labels`)** | `tests/labels.rs` comprova 24 entradas (15/3/6) byte a byte idênticas ao export de `labelsDeBugs.ts`, bloqueio em divergência, preview puro idempotente e `ensure_labels` sem sobrescrever existentes |
| **CA-1 (`plan`)** | `tests/plan.rs` comprova seleção do último plano entre 3 comentários, ignorando headings em blocos de código e retornando `None` quando ausente |
| **CA-1 (`cache`)** | `tests/cache.rs` comprova `stale` por TTL, remoção apenas após reconciliação completa, sanitização com `sanitize()`, preservação de contagem em falha parcial e isolamento por `auth_scope` |
| **CA-1 (`sync`)** | `tests/sync.rs` comprova que 10 chamadas concorrentes do mesmo recurso geram apenas 1 operação paginada |
| **CA-8 (Contagem > 1000 issues)** | `tests/cache.rs` e `tests/sync.rs` validam paginação e contagem exata acima de 1000 issues excluindo PRs e filtros locais |
