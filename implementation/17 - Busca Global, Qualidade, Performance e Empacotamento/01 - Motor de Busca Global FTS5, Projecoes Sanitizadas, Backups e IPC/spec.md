# Implementação 01 — Motor de Busca Global FTS5, Projeções Sanitizadas, Backups e IPC

> **Fase:** 17 — Busca Global, Qualidade, Performance e Empacotamento
> **Itens de Escopo Cobertos:** `I-01`, `I-02`, `I-03`, `I-15`
> **Critérios de Aceite Cobertos:** `CA-01`, `CA-02`, `CA-03`, `CA-04`, `CA-05`, `CA-06`, `CA-07`, `CA-27`, `CA-40`

---

## 1. Contexto, Objetivo e Escopo

Esta implementação constrói o núcleo backend em Rust (`mt-db` e crate binário `maxterminal`) para a **Busca Global FTS5 instantânea** e para a **gestão de backups/upgrades de banco de dados entre versões**.

O motor consome estritamente os domínios já instituídos nas fases anteriores:
- Infraestrutura de conexões (`Db::{read, write}`), `FtsSpec`, `FtsRegistry`, `build_match_query()`, runner de migrations e `StreamSanitizer` / `SanitizedText` da **Fase 03**.
- Tabela `sessions` mínima, `command_blocks` e `command_blocks_fts` criados pela **Fase 05**.
- Extensão de `sessions`, vínculos `tab_sessions` / `tab_session_panes`, `tabs`, `tab_templates` e `session_scrollback` + `session_scrollback_fts` criados pela **Fase 07**.
- Tabelas `projects` (**Fase 06**), `command_groups` / itens / execuções (**Fase 15**) e cache `github_issues` / comentários (**Fase 16**).

Nenhuma tabela de domínio (`sessions`, `command_blocks`, `session_scrollback`) é recriada ou duplicada. Para as fontes de metadados que não possuem FTS5 dedicado nas fases anteriores (`sessions`, `projects`, `templates`, `groups`, `group_runs`, `issues`, `comments`), esta unidade introduz a projeção auxiliar única, sanitizada e reconstruível `search_metadata` indexada via `search_metadata_fts`.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

### 2.1. Criar em `src-tauri/crates/mt-db/`
- `src-tauri/crates/mt-db/migrations/0011_17_search.sql` — DDL da tabela `search_metadata` e índices B-tree complementares; versão 11 após `0010_16_github.sql` (D-01).
- `src-tauri/crates/mt-db/src/search/mod.rs` — Módulo raiz de busca, `SearchEngine`, `SearchConfig` e `SearchOwner`.
- `src-tauri/crates/mt-db/src/search/model.rs` — DTOs exportados (`SearchRequest`, `SearchFilters`, `ExitFilter`, `SearchSort`, `SearchSourceKind`, `HighlightSegment`, `SearchHit`, `SearchTarget`, `SearchSection`, `SearchResponse`, `SearchIndexStat`, `RebuildJobHandle`).
- `src-tauri/crates/mt-db/src/search/error.rs` — Enum `SearchError` e conversão para `MtError` com códigos `search.*`.
- `src-tauri/crates/mt-db/src/search/query.rs` — `ParsedQuery`, `QueryBuilder` sobre `build_match_query()` da Fase 03, validação de limites e geração de sentinelas livres de colisão para `snippet()`.
- `src-tauri/crates/mt-db/src/search/rank.rs` — Ordenação determinística por fonte (`bm25` ascendente + timestamp descendente + chave de domínio crescente) sem comparar scores entre tabelas FTS distintas.
- `src-tauri/crates/mt-db/src/search/cursor.rs` — Codificação/decodificação autenticada (HMAC-SHA256 efêmero por boot + Base64 URL-safe) de `SearchCursorPayload`.
- `src-tauri/crates/mt-db/src/search/snapshot.rs` — `SnapshotCache`, `MaterializedSnapshot`, controle de TTL (120 s), limite por janela (máximo 2 snapshots e 16 MiB) e invalidação por evento de domínio.
- `src-tauri/crates/mt-db/src/search/cancel.rs` — `CancelRegistry`, `RequestKey`, `RequestState`, `CancelOutcome`, tombstones para cancelamento pré-registro e coordenação com `rusqlite::InterruptHandle`.
- `src-tauri/crates/mt-db/src/search/projection.rs` — `SanitizedProjection`, `ProjectionSource`, `upsert_projection`, `delete_projection`, `backfill_all_projections` e `rebuild_search_fts`.
- `src-tauri/crates/mt-db/src/search/sources/mod.rs` — Trait `SearchSource`, `SearchContext`, `CollectedSection`, `RawCandidate`.
- `src-tauri/crates/mt-db/src/search/sources/command_blocks.rs` — Consulta parametrizada em `command_blocks_fts` + `command_blocks` + `sessions`.
- `src-tauri/crates/mt-db/src/search/sources/scrollback.rs` — Consulta parametrizada em `session_scrollback_fts` + `session_scrollback` + `tab_session_panes` + `sessions`.
- `src-tauri/crates/mt-db/src/search/sources/sessions.rs` — Consulta parametrizada em `search_metadata_fts` (`source = 'sessions'`) com suporte a sessões ativas, arquivadas e legadas sem vínculo em `tab_sessions`.
- `src-tauri/crates/mt-db/src/search/sources/projects.rs` — Consulta parametrizada em `search_metadata_fts` (`source = 'projects'`).
- `src-tauri/crates/mt-db/src/search/sources/templates.rs` — Consulta parametrizada em `search_metadata_fts` (`source = 'templates'`).
- `src-tauri/crates/mt-db/src/search/sources/groups.rs` — Consulta parametrizada em `search_metadata_fts` (`source IN ('groups', 'group_runs')`).
- `src-tauri/crates/mt-db/src/search/sources/issues.rs` — Consulta parametrizada em `search_metadata_fts` (`source IN ('issues', 'comments')`).
- `src-tauri/crates/mt-db/tests/search_query.rs` — Testes unitários e Proptest (20.000 casos) de sintaxe FTS5.
- `src-tauri/crates/mt-db/tests/search_sources.rs` — Testes de contrato por fonte com corpus-oráculo e filtros combinados.
- `src-tauri/crates/mt-db/tests/search_snapshot.rs` — Testes de paginação sobre 10.000 hits, mutações concorrentes, expiração e cursor adulterado.
- `src-tauri/crates/mt-db/tests/search_cancel.rs` — Testes de cancelamento isolado por conexão e corrida `A → release → B → cancel(A)`.
- `src-tauri/crates/mt-db/tests/search_index.rs` — Testes `schema_contract`, triggers FTS, sanitização (`sanitizer_cases.toml`), cascata e rebuild.
- `src-tauri/crates/mt-db/tests/upgrade_from_fixtures.rs` — Testes de migração sobre bancos populados (≥ 10.000 blocos) e restauração transacional no boot.
- `src-tauri/crates/mt-db/tests/fixtures/db/v05_populated.db.zst`, `v07_populated.db.zst`, `v16_populated.db.zst` — Fixtures compactadas geradas deterministicamente no setup de teste.

### 2.2. Modificar em `src-tauri/crates/mt-db/`
- `src-tauri/crates/mt-db/src/lib.rs` — Exportar `pub mod search;` e registrar `FtsSpec` de `search_metadata_fts` no `DbSetup`.
- `src-tauri/crates/mt-db/src/backup.rs` — Adicionar variante `BackupKind::Manual`, catálogo listável `list_backups()`, `schedule_restore(backup_id)` com manifesto de intenção assinado e `apply_pending_restore_on_boot()` antes de abrir o pool WAL.

### 2.3. Criar ou Modificar no crate binário `src-tauri/src/`
- `src-tauri/src/search/mod.rs` e `src-tauri/src/search/adapters.rs` — Hooks transacionais que alimentam `upsert_projection` / `delete_projection` nos repositórios das Fases 05, 06, 07, 15 e 16.
- `src-tauri/src/commands/search.rs` — Comandos Tauri `search_global`, `search_cancel`, `search_release`, `search_index_stats`, `search_index_rebuild`.
- `src-tauri/src/commands/backup.rs` — Comandos Tauri `db_backup_list`, `db_backup_create`, `db_backup_restore`.
- `src-tauri/src/events/search.rs` — Payloads e emissores para `search://index-progress` e `search://invalidated`.
- `src-tauri/src/lib.rs` — Registrar `SearchEngine` no `tauri::Builder::manage`, conectar `apply_pending_restore_on_boot()` na inicialização e exportar os comandos no `tauri_specta::collect_commands!`.

---

## 3. Especificação Técnica Exaustiva

### 3.1. Migration SQL (`0011_17_search.sql`) e Registro `FtsSpec`

```sql
-- Migration 0011_17_search.sql
-- Projeção auxiliar sanitizada e reconstruível para fontes de metadados sem FTS dedicado.

CREATE TABLE IF NOT EXISTS search_metadata (
    rowid INTEGER PRIMARY KEY,
    source TEXT NOT NULL CHECK (source IN (
        'sessions',
        'projects',
        'templates',
        'groups',
        'group_runs',
        'issues',
        'comments'
    )),
    entity_key TEXT NOT NULL,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    tab_id TEXT,
    session_id TEXT,
    pane_id TEXT,
    exit_code INTEGER,
    source_truncated INTEGER NOT NULL DEFAULT 0 CHECK (source_truncated IN (0, 1)),
    fetched_at INTEGER,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    target_json TEXT NOT NULL,
    occurred_at INTEGER,
    source_revision TEXT NOT NULL,
    UNIQUE (source, entity_key)
) STRICT;

CREATE INDEX IF NOT EXISTS ix_search_metadata_project_source
    ON search_metadata(project_id, source, occurred_at DESC, entity_key ASC);

CREATE INDEX IF NOT EXISTS ix_search_metadata_source_time
    ON search_metadata(source, occurred_at DESC, entity_key ASC);

-- Índices B-tree auxiliares nas tabelas das Fases 05/07 (criados com IF NOT EXISTS caso ausentes)
CREATE INDEX IF NOT EXISTS ix_sessions_project_started_id
    ON sessions(project_id, started_at DESC, id ASC);

CREATE INDEX IF NOT EXISTS ix_command_blocks_session_started_id
    ON command_blocks(session_id, started_at DESC, id ASC);

CREATE INDEX IF NOT EXISTS ix_command_blocks_exit_started_id
    ON command_blocks(exit_code, started_at DESC, id ASC);
```

No `DbSetup` da Fase 03 (`src-tauri/crates/mt-db/src/lib.rs`), registrar:
```rust
pub const SEARCH_METADATA_FTS: FtsSpec = FtsSpec {
    content_table: "search_metadata",
    fts_table: "search_metadata_fts",
    columns: &["title", "body"],
    tokenizer: FtsTokenizer::Text,
    prefix: &[2, 3],
};
```
O `CodeMigration` pós-DDL executa `backfill_all_projections(tx)` lendo os registros existentes de `sessions`, `projects`, `tab_templates`, `command_groups`, `command_group_runs`, `github_issues` e `github_issue_comments`, passando cada campo textual por `SanitizedText::new(&sanitizer, raw)` antes de inserir em `search_metadata`.

### 3.2. Estruturas de Dados e Modelos (`mt-db/src/search/model.rs`)

```rust
use mt_core::ids::{
    CommandBlockId, CommandGroupId, PaneId, ProjectId, SessionId, TabId, TemplateId,
};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub request_id: String,
    pub generation: u32,
    pub query: String,
    pub filters: SearchFilters,
    pub sort: SearchSort,
    pub page_size: u16,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilters {
    pub project_id: Option<ProjectId>,
    pub tab_id: Option<TabId>,
    pub session_id: Option<SessionId>,
    pub pane_id: Option<PaneId>,
    pub from_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub exit: ExitFilter,
    pub sources: Vec<SearchSourceKind>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExitFilter {
    #[default]
    Any,
    Zero,
    Nonzero,
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchSort {
    #[default]
    Relevance,
    Recent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchSourceKind {
    Commands,
    Output,
    Sessions,
    Projects,
    Templates,
    Groups,
    GroupRuns,
    Issues,
    Comments,
}

impl SearchSourceKind {
    /// Ordem canônica fixa das seções na UI e no motor de busca.
    pub const ALL_ORDERED: [Self; 9] = [
        Self::Commands,
        Self::Output,
        Self::Sessions,
        Self::Projects,
        Self::Templates,
        Self::Groups,
        Self::GroupRuns,
        Self::Issues,
        Self::Comments,
    ];
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct HighlightSegment {
    pub text: String,
    pub matched: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub key: String,
    pub source: SearchSourceKind,
    pub target: SearchTarget,
    pub title: Vec<HighlightSegment>,
    pub excerpt: Vec<HighlightSegment>,
    pub occurred_at_ms: Option<i64>,
    pub source_truncated: bool,
    pub fetched_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchTarget {
    #[serde(rename_all = "camelCase")]
    Command {
        project_id: Option<ProjectId>,
        tab_id: TabId,
        session_id: SessionId,
        pane_id: Option<PaneId>,
        block_id: CommandBlockId,
    },
    #[serde(rename_all = "camelCase")]
    Output {
        project_id: ProjectId,
        tab_id: TabId,
        session_id: SessionId,
        pane_id: PaneId,
        chunk_seq: i64,
    },
    #[serde(rename_all = "camelCase")]
    Session {
        project_id: Option<ProjectId>,
        tab_id: Option<TabId>,
        session_id: SessionId,
    },
    #[serde(rename_all = "camelCase")]
    Project {
        project_id: ProjectId,
    },
    #[serde(rename_all = "camelCase")]
    Template {
        project_id: ProjectId,
        template_id: TemplateId,
    },
    #[serde(rename_all = "camelCase")]
    Group {
        group_id: CommandGroupId,
        project_id: Option<ProjectId>,
    },
    #[serde(rename_all = "camelCase")]
    GroupRun {
        run_key: String,
        item_key: Option<String>,
        project_id: Option<ProjectId>,
    },
    #[serde(rename_all = "camelCase")]
    Issue {
        repo_key: String,
        number: u32,
        comment_key: Option<String>,
        project_id: ProjectId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchSection {
    pub source: SearchSourceKind,
    pub items: Vec<SearchHit>,
    pub next_cursor: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub request_id: String,
    pub generation: u32,
    pub snapshot_id: String,
    pub expires_at_ms: i64,
    pub sections: Vec<SearchSection>,
    pub indexed_through_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct CancelOutcome {
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexStat {
    pub source: SearchSourceKind,
    pub ready: bool,
    pub row_count: u64,
    pub updated_at_ms: Option<i64>,
    pub active_job_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct RebuildJobHandle {
    pub job_id: String,
}
```

### 3.3. Erros de Domínio (`mt-db/src/search/error.rs`)

```rust
use mt_core::error::MtError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("A consulta informada é inválida: {reason}")]
    InvalidQuery { reason: &'static str },
    #[error("Os filtros de busca informados são inválidos: {reason}")]
    InvalidFilters { reason: &'static str },
    #[error("O cursor de paginação é inválido ou foi adulterado.")]
    InvalidCursor,
    #[error("Os resultados desta busca expiraram. Atualize a busca para continuar.")]
    CursorExpired,
    #[error("A busca foi cancelada.")]
    Cancelled,
    #[error("O tempo limite da busca foi excedido. Refine os filtros e tente novamente.")]
    Timeout,
    #[error("O motor de busca está ocupado processando outras consultas. Tente novamente.")]
    Busy,
    #[error("O índice de busca da fonte '{source}' está indisponível no momento.")]
    IndexUnavailable { source: &'static str },
    #[error("A fonte de dados '{source}' não está disponível.")]
    SourceUnavailable { source: &'static str },
    #[error("Este resultado não está mais disponível.")]
    TargetMissing,
    #[error("Erro interno ao executar a busca.")]
    Internal(#[from] rusqlite::Error),
}

impl From<SearchError> for MtError {
    fn from(err: SearchError) -> Self {
        let code = match &err {
            SearchError::InvalidQuery { .. } => "search.invalid_query",
            SearchError::InvalidFilters { .. } => "search.invalid_filters",
            SearchError::InvalidCursor => "search.invalid_cursor",
            SearchError::CursorExpired => "search.cursor_expired",
            SearchError::Cancelled => "search.cancelled",
            SearchError::Timeout => "search.timeout",
            SearchError::Busy => "search.busy",
            SearchError::IndexUnavailable { .. } => "search.index_unavailable",
            SearchError::SourceUnavailable { .. } => "search.source_unavailable",
            SearchError::TargetMissing => "search.target_missing",
            SearchError::Internal(_) => "search.internal",
        };
        MtError::new(code, err.to_string())
    }
}
```

### 3.4. Construção de Query FTS5 e Sentinelas de Destaque (`mt-db/src/search/query.rs`)

```rust
use crate::fts::build_match_query;
use crate::search::error::SearchError;
use crate::search::model::HighlightSegment;

pub const MAX_QUERY_CHARS: usize = 256;
pub const MAX_QUERY_BYTES: usize = 1024;

#[derive(Debug, Clone)]
pub struct ParsedQuery {
    pub raw_trimmed: String,
    pub fts_match_expr: String,
    pub tokens_lower: Vec<String>,
    pub open_sentinel: String,
    pub close_sentinel: String,
}

impl ParsedQuery {
    /// Valida a query do usuário e constrói uma expressão MATCH blindada contra SQLITE_ERROR.
    pub fn parse(input: &str, seed_nonce: u64) -> Result<Option<Self>, SearchError> {
        if input.as_bytes().contains(&0) {
            return Err(SearchError::InvalidQuery {
                reason: "caractere nulo não permitido",
            });
        }
        if input.len() > MAX_QUERY_BYTES || input.chars().count() > MAX_QUERY_CHARS {
            return Err(SearchError::InvalidQuery {
                reason: "consulta excede o limite de 256 caracteres ou 1024 bytes",
            });
        }
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let Some(fts_match_expr) = build_match_query(trimmed) else {
            // Consulta composta apenas por pontuação ou separadores
            return Ok(None);
        };
        let tokens_lower = extract_search_tokens(trimmed);
        let (open_sentinel, close_sentinel) = generate_sentinel_pair(seed_nonce);
        Ok(Some(Self {
            raw_trimmed: trimmed.to_owned(),
            fts_match_expr,
            tokens_lower,
            open_sentinel,
            close_sentinel,
        }))
    }

    /// Converte o texto retornado por `snippet()` com sentinelas em `Vec<HighlightSegment>`.
    /// Se o texto original já contiver qualquer sentinela (colisão residual), faz fallback seguro
    /// destacando os tokens por casamento case-insensitive sobre limites UTF-8 válidos, ou retorna
    /// um único segmento `matched: false`.
    pub fn parse_snippet_segments(&self, raw_source: &str, snippet_out: &str) -> Vec<HighlightSegment>;
}
```

### 3.5. Snapshot Materializado, Paginação e Cancelamento (`snapshot.rs`, `cursor.rs`, `cancel.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SearchOwner {
    pub window_label: String,
}

#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub max_total_hits: usize,          // default: 10_000
    pub max_snapshot_bytes: usize,      // default: 16 * 1024 * 1024 (16 MiB)
    pub default_page_size: u16,         // default: 25
    pub max_page_size: u16,             // default: 100
    pub snapshot_ttl_ms: i64,           // default: 120_000 (120 s)
    pub max_snapshots_per_owner: usize, // default: 2
    pub max_concurrent_searches: usize, // default: 4
    pub query_deadline_ms: u64,         // default: 1_500
}

pub struct SearchEngine {
    db: Db,
    config: SearchConfig,
    sources: Vec<Box<dyn SearchSource>>,
    cancel_registry: Arc<CancelRegistry>,
    snapshot_cache: Arc<SnapshotCache>,
    cursor_signer: Arc<CursorSigner>,
    semaphore: Arc<tokio::sync::Semaphore>,
}

impl SearchEngine {
    pub fn new(db: Db, config: SearchConfig) -> Self;
    pub async fn search(&self, owner: SearchOwner, request: SearchRequest) -> Result<SearchResponse, SearchError>;
    pub fn cancel(&self, owner: &SearchOwner, request_id: &str) -> CancelOutcome;
    pub fn release(&self, owner: &SearchOwner, snapshot_id: &str);
    pub fn invalidate_all(&self, reason: &'static str) -> Vec<(String, String)>;
    pub async fn index_stats(&self) -> Result<Vec<SearchIndexStat>, SearchError>;
    pub async fn rebuild_indexes(
        &self,
        sources: Vec<SearchSourceKind>,
        progress_tx: tokio::sync::mpsc::UnboundedSender<SearchIndexProgressEvent>,
    ) -> Result<RebuildJobHandle, SearchError>;
}
```

Regras estritas de execução em `SearchEngine::search`:
1. Se `request.cursor` for `Some(token)`:
   - Decodificar e validar assinatura HMAC via `CursorSigner::decode(&token)` → falha retorna `SearchError::InvalidCursor`.
   - Conferir `payload.owner == owner.window_label` e `payload.query_fingerprint == fingerprint(&request.query, &request.filters, request.sort)` → divergência retorna `SearchError::InvalidCursor`.
   - Buscar snapshot no `SnapshotCache` → se ausente ou `now_ms >= snapshot.expires_at_ms`, retorna `SearchError::CursorExpired`.
   - Fatiar `snapshot.sections[payload.source]` a partir de `payload.next_ordinal` com tamanho `page_size.clamp(1, config.max_page_size)` e retornar somente a seção solicitada com `next_cursor` atualizado (se houver mais itens).
2. Se `request.cursor` for `None` (nova consulta):
   - Registrar `(owner.window_label, request.request_id, request.generation)` no `CancelRegistry`. Se já houver tombstone de cancelamento prévio, retornar `SearchError::Cancelled` imediatamente.
   - Tentar adquirir permissão do `semaphore` dentro do `deadline`; se a fila estiver esgotada, retornar `SearchError::Busy`.
   - Executar em `tokio::task::spawn_blocking`: reservar conexão exclusiva de leitura (`db.checkout_exclusive_reader()`), registrar `conn.get_interrupt_handle()` no `CancelRegistry` sob mutex e configurar `conn.progress_handler(1024, ...)` para verificar `cancellation` e `Instant::now() >= deadline`.
   - Iniciar transação `BEGIN DEFERRED;` para fixar um snapshot WAL consistente entre todas as fontes consultadas.
   - Iterar pelas fontes solicitadas (na ordem `SearchSourceKind::ALL_ORDERED`), coletando hits até `max_total_hits` e `max_snapshot_bytes`, respeitando a compatibilidade de filtros:
     - `ExitFilter != Any` exclui automaticamente `Output`, `Sessions`, `Projects`, `Templates`, `Groups`, `Issues`, `Comments` (apenas `Commands` e `GroupRuns` possuem exit code inequívoco).
     - `pane_id.is_some()` mantém apenas `Commands` e `Output`.
     - `tab_id.is_some()` ou `session_id.is_some()` mantém apenas `Commands`, `Output` e `Sessions`.
   - Encerrar a transação `COMMIT;` e limpar o `progress_handler` e o `InterruptHandle` do `CancelRegistry` **antes** de devolver a conexão ao pool (garantindo que um `cancel(A)` tardio jamais interrompa uma consulta `B` subsequente na mesma conexão).
   - Armazenar o `MaterializedSnapshot` no `SnapshotCache` (evictando o mais antigo daquela janela se ultrapassar `max_snapshots_per_owner`) e retornar a primeira página de cada seção.

### 3.6. Backup Manual e Restauração no Boot (`mt-db/src/backup.rs` e `commands/backup.rs`)

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackupKind {
    Daily,
    PreMigration,
    PreRestore,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntryDto {
    pub backup_id: String,
    pub kind: BackupKind,
    pub created_at_ms: i64,
    pub size_bytes: u64,
    pub schema_version: u32,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreScheduledDto {
    pub pending: bool,
    pub backup_id: String,
}
```

Fluxo de `db_backup_restore(backup_id)`:
1. Valida que `backup_id` consta no manifesto do diretório `$XDG_DATA_HOME/maxterminal/backups/` (rejeita qualquer `..`, `/` ou symlink).
2. Abre o arquivo de backup em modo somente leitura, verifica `sha256`, `PRAGMA quick_check`, `PRAGMA foreign_key_check`, versão do schema (`<= CURRENT_SCHEMA_VERSION`) e checksums das migrations conhecidas.
3. Grava atomicamente `$XDG_DATA_HOME/maxterminal/backups/pending-restore.json` com `{ backupId, sha256, requestedAtMs, stage: "scheduled" }` e `fsync`.
4. No próximo boot (`apply_pending_restore_on_boot()`), com o file lock de instância adquirido e **antes** de abrir conexões do app:
   - Etapa 1 (`PreRestoreBackup`): gera backup consistente do banco atual (`BackupKind::PreRestore`).
   - Etapa 2 (`StageVerifiedCopy`): copia o arquivo de backup para `maxterminal.db.restore_staging`, revalida `PRAGMA integrity_check` e FTS `integrity-check`.
   - Etapa 3 (`QuiesceSidecars`): move `maxterminal.db`, `maxterminal.db-wal` e `maxterminal.db-shm` atuais para quarentena temporária `.rollback/`, garantindo que nenhum WAL antigo seja associado ao banco restaurado.
   - Etapa 4 (`AtomicSwap`): renomeia `maxterminal.db.restore_staging` para `maxterminal.db`, executa `fsync` no diretório pai, remove `.rollback/` e `pending-restore.json`.
   - Em caso de falha em qualquer etapa: restaura os arquivos de `.rollback/` para os nomes originais, registra o erro e mantém o banco anterior íntegro.

---

## 4. Regras de Negócio, Metas de Latência e Casos de Borda

- **Zero vazamento de segredos (`CA-07`):** Nenhuma coluna indexada em `command_blocks_fts`, `session_scrollback_fts` ou `search_metadata_fts` pode receber texto bruto. `SanitizedProjection` exige valores do tipo `SanitizedText`.
- **Comandos parciais ou nulos:** Quando `command_blocks.command` for `NULL` ou `command_quality IN ('partial', 'unavailable')`, o título do hit exibe `"Comando indisponível"` (`matched: false`), enquanto o bloco permanece encontrável pelo `output_text` ou `cwd`.
- **Histórico legado sem vínculo da Fase 07:** Consultas em `command_blocks` e `sessions` usam `LEFT JOIN tab_session_panes` / `LEFT JOIN tab_sessions` para não descartar sessões criadas na Fase 05 sem agrupamento de aba.
- **Limites de proteção:** `max_total_hits = 10_000`, `max_snapshot_bytes = 16 MiB`, `page_size` padrão `25` (máximo `100`), `TTL = 120 s`. Quando o limite é atingido, `SearchSection.truncated = true`.
