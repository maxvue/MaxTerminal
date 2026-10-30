# Especificação Técnica — Implementação 02: Catálogo de Labels, Extração de Plano, Cache SQLite e Sincronização (`mt-github` + `mt-db`)

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `02 - Catalogo-de-Labels-Extracao-de-Plano-Cache-SQLite-e-Sync`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §4.1 e §7, [`../instructions.md`](../instructions.md) §2.1 e §5.1, [`../spec.md`](../spec.md) §2, §4.3, §4.4, §7, §10.4 e §11.

---

## 1. Contexto, Objetivo e Escopo

Esta segunda implementação constrói o núcleo de persistência, validação do workflow MaxCode, extração estrutural de planos e sincronização resiliente no crate `mt-github` e na migration de banco de dados `mt-db`:

1. **Catálogo Canônico das 24 Labels do Workflow MaxCode (`labels.rs` + `fixtures/maxcode/*`):** Carregamento e gate de validação estrita do export versionado de `~/GitHub/MaxCode/src/core/labelsDeBugs.ts` (commit de referência `1054eb8a886de412de8912ad56f64b1367da8b09`), garantindo exatamente **24 labels** (**15 de estado**, **3 de encerramento** e **6 de categoria**), preservação byte a byte da grafia original (incluindo caixa e espaçamento), verificação de integridade SHA-256 (`sourceSha256` e `exportSha256`), preview puro de transições e sincronização seletiva (`ensure_labels`) que cria apenas labels faltantes no repositório sem sobrescrever cores ou descrições existentes.
2. **Extração Estrutural de `## PLANO DE IMPLEMENTAÇÃO` (`plan.rs`):** Parser Markdown baseado em eventos AST (`pulldown-cmark`) que localiza a última seção H2 cujo texto exato é `PLANO DE IMPLEMENTAÇÃO` (ignorando blocos de código cercados/indentados, HTML e citações), compara corpo da issue e comentários pelo maior `(updated_at_ms, created_at_ms, source_id)` e sinaliza `provisional: true` quando nem todas as páginas de comentários foram carregadas.
3. **Migration SQLite e Repositório de Cache (`0010_16_github.sql`, `cache.rs`):** Criação das 10 tabelas e índices em modo WAL com chaves estrangeiras, sanitização obrigatória (`mt_db::sanitizer::sanitize()`) de títulos, corpos, comentários e payloads JSON, cache HTTP por página com ETag e `sanitizer_version`, controle de mutações com digest HMAC efêmero (`github_mutations`) e política de retenção (`RetentionRegistry`).
4. **Serviço de Sincronização e Reconciliação (`sync.rs`):** `IssueSyncService` com área de staging paginada (`state=all`), publicação atômica em transação única após todas as páginas, sync incremental com janela de sobreposição de 60s (`since`), reconciliação completa periódica (24h), coalescência de requisições concorrentes (N consumidores aguardam a mesma operação paginada), scheduler limitado a 2 sincronizações simultâneas com prioridade para o projeto ativo (TTL 120s lista, 300s detalhe, ≥ 10 min em background) e cálculo exato de `OpenIssueCount` excluindo Pull Requests.

---

## 2. Arquivos a Criar ou Modificar

| Caminho | Ação | Descrição |
|---------|------|-----------|
| `src-tauri/crates/mt-db/migrations/0010_16_github.sql` | Criar | Migration SQLite com as 10 tabelas e índices do domínio GitHub Issues; versão 10 fixa (D-01) |
| `src-tauri/crates/mt-github/fixtures/maxcode/labels.v1.json` | Criar | Export versionado das 24 labels canônicas (15 state, 3 closure, 6 category), transições e hashes SHA-256 de procedência |
| `src-tauri/crates/mt-github/fixtures/maxcode/labels-cases.v1.json` | Criar | Vetores de teste de normalização, comparação byte a byte e transições válidas/inválidas exportados do MaxCode |
| `src-tauri/crates/mt-github/src/labels.rs` | Criar | `LabelCatalog`, `CatalogStatus`, validador de gate 24/15/3/6, normalizador compatível, preview de transição e `ensure_labels` |
| `src-tauri/crates/mt-github/src/plan.rs` | Criar | `extract_latest_issue_plan` usando `pulldown-cmark` sobre o corpo da issue e comentários |
| `src-tauri/crates/mt-github/src/cache.rs` | Criar | `GithubCacheRepo` para leitura/escrita transacional sanitizada no SQLite e integração com `RetentionRegistry` |
| `src-tauri/crates/mt-github/src/sync.rs` | Criar | `IssueSyncService` (coalescência, staging paginado, catch-up incremental, watermark, emissão de eventos e mutações) |
| `src-tauri/crates/mt-github/src/model.rs` | Modificar | Adicionar DTOs `LabelCatalog`, `CatalogStatus`, `MissingLabelMetadata`, `LabelEnsureResult`, `SyncTicket`, `SyncTarget` |
| `src-tauri/crates/mt-github/src/lib.rs` | Modificar | Registrar e reexportar `labels`, `plan`, `cache` e `sync` |
| `src-tauri/crates/mt-github/tests/labels.rs` | Criar | Testes de cardinalidade (24 = 15 + 3 + 6), hash SHA-256, colisões, preview puro e `ensure_labels` |
| `src-tauri/crates/mt-github/tests/plan.rs` | Criar | Testes de extração com múltiplos comentários, código cercado, blockquote, delimitador H1/H2 e flag `provisional` |
| `src-tauri/crates/mt-github/tests/cache.rs` | Criar | Testes de SQLite WAL, sanitização, TTL, falha em página intermediária preservando snapshot, troca de `auth_scope` e retenção |
| `src-tauri/crates/mt-github/tests/sync.rs` | Criar | Testes de coalescência (10 chamadas concorrentes → 1 operação paginada), reconciliação de exclusões e contagem de issues abertas |

---

## 3. Assinaturas Exatas, Schema SQL e Regras de Negócio

### 3.1. Catálogo de Labels Canônicas (`src-tauri/crates/mt-github/src/labels.rs`)

Catálogo literal D-02, obrigatório em `labels.v1.json` e nos testes de igualdade byte a byte:

| Tipo | Quantidade | Nomes exatos |
|------|------------|-------------|
| `state` | 15 | `Waiting Plan`, `Waiting More Info`, `Planned`, `Waiting Execute`, `Executed`, `Waiting Check`, `Checked Approved`, `Checked Rejected`, `Waiting Re-plan`, `Fix Merged`, `Waiting Deploy`, `Deployed`, `Waiting user test`, `User Rejected`, `User Approved` |
| `closure` | 3 | `Duplicated`, `Evidence not Found`, `Dismissed` |
| `category` | 6 | `Regra de Negocio`, `Testes`, `Desempenho`, `Seguranca`, `Audit`, `Documentacao` |

Preservar fixture versionada, transições e hashes de procedência. Teste adicional de paridade com
`MaxCode/src/core/labelsDeBugs.ts` roda quando a fonte estiver disponível; sua ausência não bloqueia os testes
locais da fixture nem exige checkout pessoal em CI. Cores/descrições/transições não são inferidas pelos nomes.

```rust
use serde::{Deserialize, Serialize};
use specta::Type;
use crate::error::GithubError;
use crate::model::{LabelDefinition, LabelInfo, LabelKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogStatus {
    Compatible,
    Missing,
    Invalid,
    PeerMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LabelTransitionRule {
    pub from: String,
    pub to: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LabelCatalog {
    pub version: u16,
    pub hash: String,
    pub status: CatalogStatus,
    pub labels: Vec<LabelDefinition>,
    pub transitions: Vec<LabelTransitionRule>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MissingLabelMetadata {
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LabelEnsureResult {
    pub created: Vec<LabelInfo>,
    pub existing: Vec<LabelInfo>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TransitionPreview {
    pub resulting_labels: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct LabelCatalogValidator;

impl LabelCatalogValidator {
    /// Carrega e valida `fixtures/maxcode/labels.v1.json`.
    /// Exige exatamente 24 entradas distintas: 15 State, 3 Closure e 6 Category.
    /// Verifica `sourceSha256` e `exportSha256` (hash canônico sem o campo `exportSha256`).
    pub fn load_embedded() -> LabelCatalog;

    pub fn validate_raw_json(raw_json: &str) -> Result<LabelCatalog, GithubError>;

    /// Normaliza o nome de uma label seguindo exatamente a regra de comparação exportada pelo MaxCode
    /// (trim + colapso de espaços internos + casefold/lowercase conforme `labels-cases.v1.json`).
    pub fn normalize_label_name(name: &str) -> String;

    /// Classifica uma label remota se corresponder a uma das 24 labels canônicas.
    pub fn classify(&self, catalog: &LabelCatalog, label_name: &str) -> Option<LabelKind>;

    /// Preview puro e idempotente de adição/remoção manual de labels:
    /// preserva todas as labels externas e de categoria não tocadas e emite aviso não-bloqueante
    /// se a combinação resultante de labels de estado não constar nas transições conhecidas.
    pub fn preview_patch(
        catalog: &LabelCatalog,
        current_labels: &[String],
        add: &[String],
        remove: &[String],
    ) -> TransitionPreview;
}
```

Regras para `ensure_labels`:
- Percorre todas as páginas de `GET /repos/{owner}/{repo}/labels?per_page=100`.
- Compara cada label das 24 do catálogo com as labels existentes no repositório usando `normalize_label_name`.
- Se uma label já existir no repositório (mesmo com outra cor ou descrição), é adicionada em `existing` e **nunca** é alterada.
- Para cada label faltante, exige `confirmed == true` e cor hexadecimal de 6 dígitos válida (`^[0-9a-fA-F]{6}$`, vinda do catálogo ou de `missing_metadata`).
- Em caso de corrida (`422 Unprocessable Entity` por `already_exists`), relê as labels do repositório e move o item para `existing` se confirmado.

### 3.2. Extrator de `## PLANO DE IMPLEMENTAÇÃO` (`src-tauri/crates/mt-github/src/plan.rs`)

```rust
use crate::model::{IssueComment, IssuePlan};

#[derive(Debug, Clone)]
pub struct PlanCandidateSource<'a> {
    pub comment_id: Option<&'a str>,
    pub markdown: &'a str,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Extrai a última seção H2 cujo texto plano é exatamente "PLANO DE IMPLEMENTAÇÃO"
/// dentro de um documento Markdown, usando o parser de eventos `pulldown-cmark`.
pub fn extract_plan_from_markdown(
    markdown: &str,
    source_comment_id: Option<String>,
    source_updated_at_ms: i64,
    provisional: bool,
) -> Option<IssuePlan>;

/// Seleciona o plano mais recente entre o corpo da issue e os comentários carregados.
/// Regra de desempate determinística: maior `(updated_at_ms, created_at_ms, comment_id)`.
/// Se dentro de um mesmo corpo/comentário houver mais de um H2 `## PLANO DE IMPLEMENTAÇÃO`,
/// seleciona a última ocorrência daquele documento.
pub fn resolve_issue_plan(
    issue_body: &str,
    issue_created_at_ms: i64,
    issue_updated_at_ms: i64,
    comments: &[IssueComment],
    comments_complete: bool,
) -> Option<IssuePlan>;
```

Regras estritas de parsing em `plan.rs`:
1. Usa `pulldown_cmark::Parser::new_ext(markdown, Options::all()).into_offset_iter()`.
2. Ignora qualquer heading que esteja dentro de `Tag::CodeBlock(_)` (cercado com crases/tils ou indentado), `Tag::BlockQuote(_)` ou blocos HTML.
3. Identifica `Event::Start(Tag::Heading { level: HeadingLevel::H2, .. })` e acumula o texto plano até `Event::End(TagEnd::Heading(HeadingLevel::H2))`.
4. Se `heading_text.trim() == "PLANO DE IMPLEMENTAÇÃO"`, registra o offset byte inicial do heading. A seção se estende até o byte imediatamente anterior ao próximo `Event::Start(Tag::Heading { level, .. })` ( fora de blockquote/codeblock) com `level <= HeadingLevel::H2` (`H1` ou `H2`), ou até o fim do documento (`markdown.len()`).
5. Calcula `start_line` e `end_line` (1-indexed) contando quebras de linha `\n` até os offsets inicial e final da fatia extraída.
6. Se `comments_complete == false`, o `IssuePlan` retornado recebe `provisional = true`.

### 3.3. Migration SQLite (`src-tauri/crates/mt-db/migrations/0010_16_github.sql`)

O arquivo SQL implementa integralmente o DDL especificado em [`../spec.md`](../spec.md) §7:
- `github_repos` (`id`, `host CHECK (host = 'github.com')`, `owner COLLATE NOCASE`, `name COLLATE NOCASE`, `html_url`, `UNIQUE (host, owner, name)`).
- `github_project_repos` (`project_id PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE`, `repo_id REFERENCES github_repos(id) ON DELETE CASCADE`, `generation INTEGER NOT NULL DEFAULT 1 CHECK (generation > 0)`).
- `github_sync_state` (`repo_id`, `auth_scope`, `revision`, `checked_at_ms`, `full_sync_at_ms`, `watermark_ms`, `open_count`, `complete`, `PRIMARY KEY (repo_id, auth_scope)`).
- `github_issues` (chave primária `(repo_id, auth_scope, number)`, `UNIQUE (repo_id, auth_scope, node_id)`, índices `idx_github_issues_state_updated` e `idx_github_issues_updated`).
- `github_issue_comments` (chave primária `(repo_id, auth_scope, issue_number, id)`, índice `idx_github_comments_order`).
- `github_labels` (chave primária `(repo_id, auth_scope, id)`, `UNIQUE (repo_id, auth_scope, normalized_name)`).
- `github_issue_labels` (chave primária `(repo_id, auth_scope, issue_number, label_id)`, índice `idx_github_issue_labels_filter`).
- `github_http_cache` (chave primária `(repo_id, auth_scope, request_key)`, `etag`, `next_url`, `sanitized_json`, `sanitizer_version`, `checked_at_ms`, índice `idx_github_http_expiry`).
- `github_bug_cycles` (chave primária `request_id`, `UNIQUE (repo_id, peer_id, cycle_id)` e índice parcial único `idx_github_one_active_cycle ON github_bug_cycles(repo_id) WHERE status IN ('pending', 'accepted', 'running', 'unknown')`).
- `github_mutations` (chave primária `request_id`, `repo_id`, `auth_scope`, `kind`, `payload_digest`, `status`, `result_json`, `created_at_ms`, `updated_at_ms`, índice `idx_github_mutations_pending`).

### 3.4. Repositório SQLite (`src-tauri/crates/mt-github/src/cache.rs`) e Sincronização (`src-tauri/crates/mt-github/src/sync.rs`)

```rust
pub struct GithubCacheRepo {
    db: mt_db::Db,
    session_hmac_key: [u8; 32],
}

impl GithubCacheRepo {
    pub fn new(db: mt_db::Db) -> Self;

    pub async fn bind_project_repo(
        &self,
        project_id: &ProjectId,
        repo: &GithubRepoRef,
    ) -> Result<u32, GithubError>; // Retorna `generation`

    pub async fn commit_full_issues_sync(
        &self,
        repo_id: &RepoId,
        auth_scope: &AuthScopeId,
        expected_generation: u32,
        staged_issues: Vec<RemoteIssueItem>,
        staged_labels: Vec<LabelInfo>,
        sync_started_at_ms: i64,
        checked_at_ms: i64,
    ) -> Result<SyncCommitSummary, GithubError>;

    pub async fn commit_incremental_issues_sync(
        &self,
        repo_id: &RepoId,
        auth_scope: &AuthScopeId,
        staged_issues: Vec<RemoteIssueItem>,
        sync_started_at_ms: i64,
        checked_at_ms: i64,
    ) -> Result<SyncCommitSummary, GithubError>;

    pub async fn query_issues_page(
        &self,
        repo_id: &RepoId,
        auth_scope: &AuthScopeId,
        catalog: &LabelCatalog,
        query: &IssueListQuery,
        now_ms: i64,
    ) -> Result<Cached<IssuePage>, GithubError>;

    pub async fn get_issue_detail(
        &self,
        repo_id: &RepoId,
        auth_scope: &AuthScopeId,
        catalog: &LabelCatalog,
        number: u32,
        comments_cursor: Option<&str>,
        now_ms: i64,
    ) -> Result<Option<Cached<IssueDetail>>, GithubError>;

    pub async fn get_open_counts_by_projects(
        &self,
        project_ids: &[ProjectId],
        auth_scope: &AuthScopeId,
        now_ms: i64,
    ) -> Result<std::collections::HashMap<ProjectId, OpenIssueCount>, GithubError>;
}
```

Regras críticas de `cache.rs` e `sync.rs`:
1. **Sanitização Pré-Gravação:** Todo `title`, `body`, `comment.body`, `label.description` e payload JSON é sanitizado com `mt_db::sanitizer::sanitize()` antes de entrar no `rusqlite::Transaction`.
2. **Preservação de Corpo em Upsert de Lista:** Quando um upsert de lista ou incremental atualiza `github_issues`, só substitui `body` se `new.updated_at_ms >= existing.updated_at_ms` e `new.body.is_some()`. Nunca retrocede uma issue que já possui versão mais recente no cache.
3. **Transação Única por Sincronização Paginada:** Se a listagem tiver 15 páginas e a página 9 falhar por erro de rede ou rate limit, **nenhuma** alteração em `github_issues`, `open_count` ou `watermark_ms` é comitada; o snapshot anterior permanece íntegro e `stale = true`.
4. **Coalescência (`IssueSyncService`):** Mantém um `Mutex<HashMap<SyncResourceKey, SharedSyncFuture>>`. Se 10 consumidores solicitarem `sync_issues_list` para o mesmo `(repo_id, auth_scope)` simultaneamente, apenas **1** varredura paginada na rede é executada e todos recebem o mesmo `SyncTicket { request_id, coalesced: true/false }`.
5. **Orçamento de Concorrência e TTL:** Semáforo global de 2 sincronizações simultâneas (`tokio::sync::Semaphore::new(2)`), com fila prioritária para o projeto selecionado na UI. TTL de lista = `120_000 ms`, TTL de detalhe = `300_000 ms`, intervalo mínimo para projetos em segundo plano = `600_000 ms` (10 min).
6. **Catch-up Incremental e Reconciliação Completa:** Sincronizações subsequentes usam `since = watermark_ms - 60_000` com `state=all`. A cada 24h (`now_ms - full_sync_at_ms >= 86_400_000`), executa uma varredura completa seguida de catch-up desde o início da varredura para reconciliar issues transferidas ou deletadas sem falsos positivos.
7. **Idempotência de Mutações (`github_mutations`):** `create_issue` e `patch_labels` calculam `HMAC-SHA256(session_hmac_key, canonical_payload)` e gravam `status = 'pending'` antes do envio. Se o mesmo `request_id` for reenviado com digest diferente, retorna `GithubErrorKind::Conflict`. Se a rede cair após o envio (`GithubErrorKind::MutationUnknown`), grava `status = 'unknown'` e retorna `MutationReceipt { outcome: MutationOutcome::Unknown, value: None }`.
8. **Escala em Repositórios com 1000+ Issues:** Na reconciliação completa (`commit_full_issues_sync`), a remoção de issues que deixaram de existir no remoto **nunca** constrói `WHERE number NOT IN (?1, ?2, ...)` (que violaria `SQLITE_MAX_VARIABLE_NUMBER` com mais de 999 issues). Em vez disso, insere os números vistos na varredura completa em uma tabela temporária `TEMP TABLE seen_issue_numbers(number INTEGER PRIMARY KEY)` dentro da mesma transação do writer e remove ausências com `DELETE FROM github_issues WHERE repo_id = ?1 AND auth_scope = ?2 AND NOT EXISTS (SELECT 1 FROM seen_issue_numbers s WHERE s.number = github_issues.number)`.
9. **Invariante de Chaves Estrangeiras (`github_sync_state` Pai):** Como `github_issues`, `github_labels` e `github_http_cache` possuem `FOREIGN KEY (repo_id, auth_scope) REFERENCES github_sync_state(repo_id, auth_scope) ON DELETE CASCADE`, toda transação de escrita em `GithubCacheRepo` (seja sync de lista, sync de labels, escrita em `github_http_cache`, `create_issue` ou `patch_labels` antes do primeiro sync completo) executa primeiro `INSERT INTO github_sync_state (repo_id, auth_scope, revision, complete) VALUES (?1, ?2, 0, 0) ON CONFLICT(repo_id, auth_scope) DO NOTHING`.
10. **Registro de Retenção (`RetentionRegistry` da Fase 03):** `GithubCacheRepo::register_retention(registry: &mut mt_db::RetentionRegistry)` registra a rotina de limpeza periódica que:
    - Expira entradas de `github_http_cache` com `checked_at_ms < now_ms - 86_400_000` (24 horas).
    - Expira comentários e detalhes de issues fechadas (`state = 'closed'`) com `updated_at_ms < now_ms - 2_592_000_000` (30 dias).
    - Remove partições (`github_sync_state` em cascata) cujo `auth_scope` não é mais o ativo e não é acessado há mais de 7 dias, preservando sempre issues abertas do escopo ativo, repositórios com ciclos ativos em `github_bug_cycles` e registros incertos em `github_mutations` (`status IN ('pending', 'unknown')`).
    - Caso qualquer linha de `github_issues` de uma partição ativa seja removida por limpeza manual, marca `complete = 0` e `open_count = NULL` em `github_sync_state`, exigindo nova sincronização antes de exibir total exato no badge.
