# Implementação 01 — Fundação do Crate `mt-ai`, Persistência SQLite, Cliente HTTP, Catálogo e Registro (`spec.md`)

> **Fase:** 12 — Integração MaxAiManager (`@` / `?`)
> **Unidade:** `01 - Fundação do Crate mt-ai, Persistência SQLite, Cliente HTTP, Catálogo e Registro`
> **Pré-requisitos:** Fase 01 (`mt-core`), Fase 03 (`mt-db` e `mt_db::sanitizer::sanitize()`)
> **Cobertura de Escopo da Fase 12:** `I-01`, `I-02`, `I-03`, `I-04`, `I-13`, `I-15` (parcial)
> **Cobertura de Critérios de Aceite:** `CA-01` (parcial), `CA-05`, `CA-06`, `CA-07`, `CA-08`, `CA-09`, `CA-10`, `CA-11`, `CA-12`, `CA-29`, `CA-30`, `CA-31`, `CA-32`, `CA-33` (invariantes backend), `CA-35`, `CA-36` (sanitização em fronteiras de chunks)

---

## 1. Contexto, Objetivo e Escopo

Esta primeira unidade estabelece a fundação do domínio de Inteligência Artificial do MaxTerminal no backend Rust:
1. **Isolamento estrutural (`mt-ai`):** Cria o crate `src-tauri/crates/mt-ai` dependendo exclusivamente de `mt-core` e `mt-db` (sem dependência de `mt-pty`, `mt-shell`, `mt-projects` ou `tauri`), garantindo estruturalmente que a camada de IA jamais possa escrever bytes diretamente em um PTY (`CA-15`).
2. **Persistência SQLite (`NNNN_12_ai.sql` e `AiRepo`):** Cria as tabelas `ai_settings`, `ai_catalog_cache`, `ai_diagnosis_cache` e `ai_prompt_history`, garantindo sanitização obrigatória (`mt_db::sanitizer::sanitize()`) antes de toda gravação, retenção automática de no máximo 5.000 itens de histórico, expiração de cache e contador monotônico de geração de limpeza para impedir que requisições em voo repovoem dados recém-limpos.
3. **Registro de Requisições e Cancelamento (`AiRequestRegistry`):** Gerencia o ciclo de vida atômico (`Pending/Running → Completed | Failed | Cancelled`) por `AiRequestId` (Uuid v7), suportando tombstones para cancelamentos que cheguem antes do início da requisição, cancelamento por painel (`cancel_pane`) e cancelamento global (`cancel_all`).
4. **Cliente HTTP e Streaming Condicional (`GatewayClient`, `GatewayDialect`, `sse.rs`):** Conecta ao gateway local do **MaxAiManager** injetando obrigatoriamente o header `X-Agent-Name: MaxTerminal` em **todas** as requisições (`CA-05`), validando que a URL seja loopback salvo `allow_remote = true` (`CA-07`), aplicando timeouts por operação, retry restrito a leituras idempotentes homologadas (`Health`/`Catalog`), streaming SSE condicional com coalescência de 32 ms e sanitização incremental capaz de reter caudas parciais para impedir vazamento de segredos divididos entre chunks (`CA-36`).
5. **Catálogo de Harnesses (`CatalogService`):** Gerencia a cascata **Harness → Modelo → Nível de Raciocínio** com cache em memória + SQLite (TTL de 6 horas, revalidação `If-None-Match` / `ETag` separada por `gateway_key`) e fallback estático contendo exclusivamente as identidades visuais dos 4 harnesses canônicos (`claude-code`, `codex`, `agy`, `opencode`) com `models: []`, sem jamais inventar modelos ou níveis de raciocínio (`CA-11`, `CA-12`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

### 2.1. Arquivos a Modificar
- `src-tauri/Cargo.toml` — inclusão de `crates/mt-ai` em `[workspace.members]` e dependências compartilhadas no workspace (`eventsource-stream`, `strip-ansi-escapes`, `jsonschema`, `shell-words`, `wiremock`).
- `src-tauri/crates/mt-db/src/migrations.rs` — registro da migration da Fase 12 (`NNNN_12_ai.sql`) no runner de migrations da Fase 03.

### 2.2. Arquivos a Criar
- `src-tauri/crates/mt-db/migrations/0006_12_ai.sql` *(ajustar prefixo numérico caso o próximo índice livre na pasta `migrations/` seja diferente, preservando o sufixo `_12_ai.sql`)*
- `src-tauri/crates/mt-ai/Cargo.toml`
- `src-tauri/crates/mt-ai/assets/catalog.fallback.json`
- `src-tauri/crates/mt-ai/src/lib.rs` *(estrutura base e re-exports desta unidade; expandido na Implementação 02)*
- `src-tauri/crates/mt-ai/src/ids.rs`
- `src-tauri/crates/mt-ai/src/error.rs`
- `src-tauri/crates/mt-ai/src/config.rs`
- `src-tauri/crates/mt-ai/src/registry.rs`
- `src-tauri/crates/mt-ai/src/repo.rs`
- `src-tauri/crates/mt-ai/src/client/mod.rs`
- `src-tauri/crates/mt-ai/src/client/dialect.rs`
- `src-tauri/crates/mt-ai/src/client/sse.rs`
- `src-tauri/crates/mt-ai/src/client/http.rs`
- `src-tauri/crates/mt-ai/src/catalog/mod.rs`
- `src-tauri/crates/mt-ai/src/catalog/fallback.rs`
- `src-tauri/crates/mt-ai/tests/no_pty_dependency.rs`
- `src-tauri/crates/mt-ai/tests/repo.rs`
- `src-tauri/crates/mt-ai/tests/gateway_mock.rs`
- `src-tauri/crates/mt-ai/tests/catalog.rs`

---

## 3. Especificação Técnica Exaustiva

### 3.1. `src-tauri/crates/mt-ai/Cargo.toml`

```toml
[package]
name = "mt-ai"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
mt-core = { path = "../mt-core" }
mt-db = { path = "../mt-db" }
reqwest = { workspace = true, default-features = false, features = ["json", "stream", "rustls-tls"] }
eventsource-stream = "0.2"
async-trait = "0.1"
strip-ansi-escapes = "0.2"
futures-util = "0.3"
tokio = { workspace = true, features = ["rt", "macros", "time", "sync"] }
tokio-util = { version = "0.7", features = ["rt"] }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
specta = { workspace = true }
jsonschema = { version = "0.26", default-features = false }
regex = "1"
 once_cell = "1"
shell-words = "1"
sha2 = "0.10"
hex = "0.4"
url = "2"
dashmap = "6"
parking_lot = "0.12"
tracing = { workspace = true }
uuid = { workspace = true, features = ["v7", "serde"] }
time = { workspace = true, features = ["formatting", "parsing", "serde"] }

[dev-dependencies]
wiremock = "0.6"
tokio = { workspace = true, features = ["full", "test-util"] }
tempfile = "3"
```

---

### 3.2. Migration SQLite (`src-tauri/crates/mt-db/migrations/0006_12_ai.sql`)

```sql
-- Fase 12: configurações, caches e histórico de IA (todo texto gravado já sanitizado via mt_db::sanitizer::sanitize)
CREATE TABLE IF NOT EXISTS ai_settings (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    payload     TEXT    NOT NULL CHECK (json_valid(payload)),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS ai_catalog_cache (
    gateway_key TEXT    PRIMARY KEY,
    payload     TEXT    NOT NULL CHECK (json_valid(payload)),
    etag        TEXT,
    fetched_at  TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS ai_diagnosis_cache (
    error_hash        TEXT    PRIMARY KEY CHECK (length(error_hash) = 64),
    gateway_key       TEXT    NOT NULL,
    project_id        TEXT    REFERENCES projects (id) ON DELETE CASCADE,
    summary           TEXT    NOT NULL,
    fix_command       TEXT,
    fix_risk          TEXT    CHECK (fix_risk IN ('safe', 'caution', 'destructive')),
    harness_id        TEXT    NOT NULL,
    model_id          TEXT    NOT NULL,
    reasoning_id      TEXT,
    prompt_version    INTEGER NOT NULL,
    sanitizer_version INTEGER NOT NULL,
    hits              INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_hit_at       TEXT,
    expires_at        TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ai_diagnosis_cache_last_hit ON ai_diagnosis_cache (last_hit_at);
CREATE INDEX IF NOT EXISTS idx_ai_diagnosis_cache_expiry   ON ai_diagnosis_cache (expires_at);
CREATE INDEX IF NOT EXISTS idx_ai_diagnosis_cache_project  ON ai_diagnosis_cache (project_id, gateway_key);

CREATE TABLE IF NOT EXISTS ai_prompt_history (
    id                 TEXT    PRIMARY KEY,
    kind               TEXT    NOT NULL CHECK (kind IN ('suggest', 'diagnosis', 'rag', 'structured')),
    trigger            TEXT    CHECK (trigger IN ('at', 'question')),
    project_id         TEXT    REFERENCES projects (id) ON DELETE SET NULL,
    tab_id             TEXT,
    pane_id            TEXT,
    session_id         TEXT,
    block_id           TEXT,
    harness_id         TEXT,
    model_id           TEXT,
    reasoning_id       TEXT,
    rag_slug           TEXT,
    schema_id          TEXT,
    prompt_sanitized   TEXT    NOT NULL,
    response_sanitized TEXT,
    risk_level         TEXT    CHECK (risk_level IN ('safe', 'caution', 'destructive')),
    status             TEXT    NOT NULL CHECK (status IN ('completed', 'failed', 'cancelled')),
    error_code         TEXT,
    latency_ms         INTEGER,
    created_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_ai_history_project      ON ai_prompt_history (project_id, id DESC);
CREATE INDEX IF NOT EXISTS idx_ai_history_kind         ON ai_prompt_history (kind, id DESC);
CREATE INDEX IF NOT EXISTS idx_ai_history_project_kind ON ai_prompt_history (project_id, kind, id DESC);
```

---

### 3.3. Identificadores, Erros e Configuração (`ids.rs`, `error.rs`, `config.rs`)

#### `src-tauri/crates/mt-ai/src/ids.rs`
```rust
use serde::{Deserialize, Serialize};
use std::fmt;

/// Identificador único de uma requisição de IA (Uuid v7 ordenável no tempo).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct AiRequestId(pub uuid::Uuid);

impl AiRequestId {
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }

    pub fn parse(raw: &str) -> Result<Self, uuid::Error> {
        uuid::Uuid::parse_str(raw).map(Self)
    }
}

impl Default for AiRequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AiRequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

#### `src-tauri/crates/mt-ai/src/error.rs`
```rust
use mt_core::{ErrorCode, MtError};
use serde_json::json;

pub mod codes {
    pub const AI_GATEWAY_UNAVAILABLE: &str = "AI_GATEWAY_UNAVAILABLE";
    pub const AI_TIMEOUT: &str = "AI_TIMEOUT";
    pub const AI_CANCELLED: &str = "AI_CANCELLED";
    pub const AI_HTTP_STATUS: &str = "AI_HTTP_STATUS";
    pub const AI_BAD_RESPONSE: &str = "AI_BAD_RESPONSE";
    pub const AI_SCHEMA_VALIDATION: &str = "AI_SCHEMA_VALIDATION";
    pub const AI_URL_INVALID: &str = "AI_URL_INVALID";
    pub const AI_URL_NOT_LOOPBACK: &str = "AI_URL_NOT_LOOPBACK";
    pub const AI_INVALID_SLUG: &str = "AI_INVALID_SLUG";
    pub const AI_RAG_NOT_FOUND: &str = "AI_RAG_NOT_FOUND";
    pub const AI_NO_DEFAULT_SELECTION: &str = "AI_NO_DEFAULT_SELECTION";
    pub const AI_UNKNOWN_SELECTION: &str = "AI_UNKNOWN_SELECTION";
    pub const AI_RATE_LIMITED: &str = "AI_RATE_LIMITED";
    pub const AI_EMPTY_COMMAND: &str = "AI_EMPTY_COMMAND";
    pub const AI_DISABLED: &str = "AI_DISABLED";
    pub const AI_NOT_CONFIGURED: &str = "AI_NOT_CONFIGURED";
    pub const AI_CAPABILITY_UNAVAILABLE: &str = "AI_CAPABILITY_UNAVAILABLE";
    pub const AI_AUTH_REQUIRED: &str = "AI_AUTH_REQUIRED";
    pub const AI_REQUEST_CONFLICT: &str = "AI_REQUEST_CONFLICT";
    pub const AI_CONTEXT_STALE: &str = "AI_CONTEXT_STALE";
    pub const AI_SETTINGS_CONFLICT: &str = "AI_SETTINGS_CONFLICT";
}

pub fn ai_error(code: &'static str, message: impl Into<String>) -> MtError {
    MtError::new(ErrorCode::from(code), message.into())
}

pub fn ai_error_with_details(
    code: &'static str,
    message: impl Into<String>,
    details: serde_json::Value,
) -> MtError {
    MtError::with_details(ErrorCode::from(code), message.into(), details)
}

pub fn gateway_unavailable(base_url: &str) -> MtError {
    ai_error_with_details(
        codes::AI_GATEWAY_UNAVAILABLE,
        format!("MaxAiManager não está acessível em {base_url}. Verifique se ele está em execução."),
        json!({ "baseUrl": base_url }),
    )
}

pub fn timeout_error(timeout_ms: u32) -> MtError {
    let secs = (timeout_ms as f64 / 1000.0).ceil() as u32;
    ai_error_with_details(
        codes::AI_TIMEOUT,
        format!("O MaxAiManager não respondeu em {secs} s."),
        json!({ "timeoutMs": timeout_ms }),
    )
}
```

#### `src-tauri/crates/mt-ai/src/config.rs`
```rust
use crate::error::{ai_error, codes};
use mt_core::{MtResult, ProjectId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;
use url::Url;

pub const MIN_TIMEOUT_MS: u32 = 300;
pub const MAX_TIMEOUT_MS: u32 = 120_000;
pub const MAX_RECENT_COMMANDS: u8 = 20;
pub const MAX_RAG_TOP_K: u8 = 20;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiSelection {
    pub harness_id: String,
    pub model_id: String,
    pub reasoning_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisSettings {
    pub enabled: bool,
    pub debounce_ms: u32,
    pub selection: Option<AiSelection>,
    pub ignored_exit_codes: Vec<i32>,
    pub disabled_projects: Vec<ProjectId>,
    pub max_per_minute: u32,
}

impl Default for DiagnosisSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_ms: 800,
            selection: None,
            ignored_exit_codes: vec![130],
            disabled_projects: Vec::new(),
            max_per_minute: 10,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RagSettings {
    pub top_k: u8,
    pub stack_slugs: BTreeMap<String, String>,
    pub last_slug: Option<String>,
}

impl Default for RagSettings {
    fn default() -> Self {
        let mut stack_slugs = BTreeMap::new();
        stack_slugs.insert("laravel".to_string(), "laravel".to_string());
        stack_slugs.insert("vue".to_string(), "vue".to_string());
        stack_slugs.insert("tauri".to_string(), "tauri".to_string());
        stack_slugs.insert("rust".to_string(), "rust".to_string());
        Self {
            top_k: 5,
            stack_slugs,
            last_slug: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextSettings {
    pub recent_commands: u8,
    pub include_output_tail: bool,
    pub output_tail_bytes: u32,
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self {
            recent_commands: 10,
            include_output_tail: false,
            output_tail_bytes: 4096,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TimeoutSettings {
    pub connect_ms: u32,
    pub suggest_ms: u32,
    pub diagnosis_ms: u32,
    pub rag_ms: u32,
    pub structured_ms: u32,
    pub catalog_ms: u32,
}

impl Default for TimeoutSettings {
    fn default() -> Self {
        Self {
            connect_ms: 2_000,
            suggest_ms: 60_000,
            diagnosis_ms: 20_000,
            rag_ms: 10_000,
            structured_ms: 90_000,
            catalog_ms: 5_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub revision: u32,
    pub enabled: bool,
    pub base_url: Option<String>,
    pub allow_remote: bool,
    pub default_selection: Option<AiSelection>,
    pub diagnosis: DiagnosisSettings,
    pub rag: RagSettings,
    pub context: ContextSettings,
    pub timeouts: TimeoutSettings,
    pub save_history: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        let env_url = std::env::var("MAXTERMINAL_AI_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Self {
            revision: 1,
            enabled: true,
            base_url: env_url,
            allow_remote: false,
            default_selection: None,
            diagnosis: DiagnosisSettings::default(),
            rag: RagSettings::default(),
            context: ContextSettings::default(),
            timeouts: TimeoutSettings::default(),
            save_history: true,
        }
    }
}

impl AiSettings {
    /// Valida esquema HTTP/HTTPS, ausência de credenciais embutidas na URL, host loopback (127.0.0.0/8, ::1, localhost)
    /// salvo `allow_remote = true`, e limites de timeouts, contexto e RAG.
    pub fn validate(&self) -> MtResult<()> {
        if let Some(raw_url) = &self.base_url {
            let trimmed = raw_url.trim();
            if trimmed.is_empty() {
                return Err(ai_error(codes::AI_URL_INVALID, "A URL do MaxAiManager não pode ser vazia."));
            }
            let parsed = Url::parse(trimmed).map_err(|_| {
                ai_error(codes::AI_URL_INVALID, "A URL informada para o MaxAiManager é inválida.")
            })?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err(ai_error(
                    codes::AI_URL_INVALID,
                    "A URL do MaxAiManager deve utilizar o protocolo http:// ou https://.",
                ));
            }
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(ai_error(
                    codes::AI_URL_INVALID,
                    "A URL do MaxAiManager não deve conter credenciais (usuário/senha) embutidas.",
                ));
            }
            let host = parsed.host_str().ok_or_else(|| {
                ai_error(codes::AI_URL_INVALID, "A URL do MaxAiManager precisa especificar um host válido.")
            })?;
            if !self.allow_remote && !is_loopback_host(host) {
                return Err(ai_error(
                    codes::AI_URL_NOT_LOOPBACK,
                    "Por segurança, apenas endereços de loopback (127.0.0.1, ::1 ou localhost) são permitidos quando 'Permitir host remoto' está desativado.",
                ));
            }
        }

        for (label, ms) in [
            ("connect_ms", self.timeouts.connect_ms),
            ("suggest_ms", self.timeouts.suggest_ms),
            ("diagnosis_ms", self.timeouts.diagnosis_ms),
            ("rag_ms", self.timeouts.rag_ms),
            ("structured_ms", self.timeouts.structured_ms),
            ("catalog_ms", self.timeouts.catalog_ms),
        ] {
            if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&ms) {
                return Err(ai_error(
                    codes::AI_URL_INVALID,
                    format!("O timeout '{label}' deve estar entre {MIN_TIMEOUT_MS} ms e {MAX_TIMEOUT_MS} ms."),
                ));
            }
        }

        if self.context.recent_commands > MAX_RECENT_COMMANDS {
            return Err(ai_error(
                codes::AI_URL_INVALID,
                format!("O limite máximo de comandos recentes no contexto é {MAX_RECENT_COMMANDS}."),
            ));
        }

        if self.rag.top_k == 0 || self.rag.top_k > MAX_RAG_TOP_K {
            return Err(ai_error(
                codes::AI_URL_INVALID,
                format!("O parâmetro top_k do RAG deve estar entre 1 e {MAX_RAG_TOP_K}."),
            ));
        }

        Ok(())
    }

    /// Sanitiza campos textuais das configurações antes de persistir em JSON.
    pub fn sanitized(mut self) -> Self {
        if let Some(url) = &self.base_url {
            self.base_url = Some(mt_db::sanitizer::sanitize(url.trim()));
        }
        if let Some(sel) = &mut self.default_selection {
            sel.harness_id = mt_db::sanitizer::sanitize(&sel.harness_id);
            sel.model_id = mt_db::sanitizer::sanitize(&sel.model_id);
            sel.reasoning_id = sel.reasoning_id.as_ref().map(|r| mt_db::sanitizer::sanitize(r));
        }
        if let Some(sel) = &mut self.diagnosis.selection {
            sel.harness_id = mt_db::sanitizer::sanitize(&sel.harness_id);
            sel.model_id = mt_db::sanitizer::sanitize(&sel.model_id);
            sel.reasoning_id = sel.reasoning_id.as_ref().map(|r| mt_db::sanitizer::sanitize(r));
        }
        if let Some(slug) = &self.rag.last_slug {
            self.rag.last_slug = Some(mt_db::sanitizer::sanitize(slug));
        }
        self.rag.stack_slugs = self
            .rag
            .stack_slugs
            .into_iter()
            .map(|(k, v)| (mt_db::sanitizer::sanitize(&k), mt_db::sanitizer::sanitize(&v)))
            .collect();
        self
    }

    /// Gera a chave canônica identificadora do gateway para isolamento de caches (`gateway_key`).
    pub fn gateway_key(&self, dialect_id: &str) -> String {
        use sha2::{Digest, Sha256};
        let normalized_url = self
            .base_url
            .as_deref()
            .unwrap_or("unconfigured")
            .trim()
            .trim_end_matches('/')
            .to_ascii_lowercase();
        let mut hasher = Sha256::new();
        hasher.update(normalized_url.as_bytes());
        hasher.update(b"|");
        hasher.update(dialect_id.as_bytes());
        hex::encode(hasher.finalize())
    }
}

pub fn is_loopback_host(host: &str) -> bool {
    let clean = host.trim_start_matches('[').trim_end_matches(']');
    if clean.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(ip) = clean.parse::<IpAddr>() {
        return ip.is_loopback();
    }
    false
}
```

---

### 3.4. Registro de Requisições e Cancelamento (`registry.rs`)

```rust
use crate::error::{ai_error, codes};
use crate::ids::AiRequestId;
use dashmap::DashMap;
use mt_core::{MtResult, PaneId, PtyId, SessionId};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const STATE_RUNNING: u8 = 0;
const STATE_COMPLETED: u8 = 1;
const STATE_FAILED: u8 = 2;
const STATE_CANCELLED: u8 = 3;

const MAX_TOMBSTONES: usize = 256;
const TOMBSTONE_TTL: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalState {
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct RequestMeta {
    pub pane_id: Option<PaneId>,
    pub session_id: Option<SessionId>,
    pub pty_id: Option<PtyId>,
    pub config_revision: u32,
}

struct ActiveEntry {
    meta: RequestMeta,
    cancel: CancellationToken,
    state: Arc<AtomicU8>,
}

/// Registro concorrente de requisições de IA ativas, com suporte a tombstones de cancelamento precoce
/// e guarda RAII (`RequestGuard`) para garantir exatamente um estado terminal por `AiRequestId` (CA-09, CA-33).
pub struct AiRequestRegistry {
    active: Arc<DashMap<AiRequestId, ActiveEntry>>,
    recent_finished: Arc<Mutex<VecDeque<(AiRequestId, Instant)>>>,
    early_cancelled: Arc<Mutex<VecDeque<(AiRequestId, Instant)>>>,
}

impl Default for AiRequestRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AiRequestRegistry {
    pub fn new() -> Self {
        Self {
            active: Arc::new(DashMap::new()),
            recent_finished: Arc::new(Mutex::new(VecDeque::new())),
            early_cancelled: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Registra o início de uma requisição.
    /// - Se o `id` já estiver ativo ou tiver sido concluído recentemente, retorna `AI_REQUEST_CONFLICT`.
    /// - Se um cancelamento chegou antes do `begin` (tombstone), consome o tombstone e retorna `AI_CANCELLED`.
    pub fn begin(&self, id: AiRequestId, meta: RequestMeta) -> MtResult<RequestGuard> {
        let now = Instant::now();
        {
            let mut early = self.early_cancelled.lock();
            early.retain(|(_, ts)| now.duration_since(*ts) < TOMBSTONE_TTL);
            if let Some(pos) = early.iter().position(|(eid, _)| *eid == id) {
                early.remove(pos);
                return Err(ai_error(codes::AI_CANCELLED, "Requisição cancelada antes de iniciar."));
            }
        }
        {
            let mut recent = self.recent_finished.lock();
            recent.retain(|(_, ts)| now.duration_since(*ts) < TOMBSTONE_TTL);
            if recent.iter().any(|(rid, _)| *rid == id) {
                return Err(ai_error(
                    codes::AI_REQUEST_CONFLICT,
                    "Esta solicitação já foi registrada anteriormente.",
                ));
            }
        }

        if self.active.contains_key(&id) {
            return Err(ai_error(
                codes::AI_REQUEST_CONFLICT,
                "Esta solicitação já está em execução.",
            ));
        }

        let cancel = CancellationToken::new();
        let state = Arc::new(AtomicU8::new(STATE_RUNNING));
        self.active.insert(
            id,
            ActiveEntry {
                meta,
                cancel: cancel.clone(),
                state: state.clone(),
            },
        );

        Ok(RequestGuard {
            id,
            cancel,
            state,
            active: self.active.clone(),
            recent_finished: self.recent_finished.clone(),
        })
    }

    /// Cancela uma requisição específica. Se ela ainda não entrou em `begin`, grava um tombstone.
    pub fn cancel(&self, id: AiRequestId) -> bool {
        if let Some(entry) = self.active.get(&id) {
            if entry
                .state
                .compare_exchange(STATE_RUNNING, STATE_CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                entry.cancel.cancel();
                return true;
            }
            return false;
        }

        let now = Instant::now();
        {
            let recent = self.recent_finished.lock();
            if recent.iter().any(|(rid, ts)| *rid == id && now.duration_since(*ts) < TOMBSTONE_TTL) {
                return false;
            }
        }

        let mut early = self.early_cancelled.lock();
        early.retain(|(_, ts)| now.duration_since(*ts) < TOMBSTONE_TTL);
        if !early.iter().any(|(eid, _)| *eid == id) {
            if early.len() >= MAX_TOMBSTONES {
                early.pop_front();
            }
            early.push_back((id, now));
        }
        true
    }

    /// Cancela todas as requisições associadas a um painel (chamado ao fechar painel/aba ou trocar PTY/sessão).
    pub fn cancel_pane(&self, pane: PaneId) -> usize {
        let mut count = 0;
        for item in self.active.iter() {
            if item.value().meta.pane_id == Some(pane)
                && item
                    .value()
                    .state
                    .compare_exchange(STATE_RUNNING, STATE_CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                item.value().cancel.cancel();
                count += 1;
            }
        }
        count
    }

    /// Cancela todas as requisições ativas (chamado ao alterar URL/dialeto ou desabilitar IA).
    pub fn cancel_all(&self) -> usize {
        let mut count = 0;
        for item in self.active.iter() {
            if item
                .value()
                .state
                .compare_exchange(STATE_RUNNING, STATE_CANCELLED, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                item.value().cancel.cancel();
                count += 1;
            }
        }
        count
    }
}

pub struct RequestGuard {
    id: AiRequestId,
    cancel: CancellationToken,
    state: Arc<AtomicU8>,
    active: Arc<DashMap<AiRequestId, ActiveEntry>>,
    recent_finished: Arc<Mutex<VecDeque<(AiRequestId, Instant)>>>,
}

impl RequestGuard {
    pub fn id(&self) -> AiRequestId {
        self.id
    }

    pub fn token(&self) -> &CancellationToken {
        &self.cancel
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.load(Ordering::SeqCst) == STATE_CANCELLED || self.cancel.is_cancelled()
    }

    /// Tenta finalizar atomicamente a requisição como `Completed` ou `Failed`.
    /// Retorna `true` se esta transição venceu, ou `false` se o cancelamento já havia vencido.
    pub fn try_finish(&self, terminal: TerminalState) -> bool {
        let target = match terminal {
            TerminalState::Completed => STATE_COMPLETED,
            TerminalState::Failed => STATE_FAILED,
            TerminalState::Cancelled => STATE_CANCELLED,
        };
        let ok = self
            .state
            .compare_exchange(STATE_RUNNING, target, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if target == STATE_CANCELLED && ok {
            self.cancel.cancel();
        }
        ok
    }
}

impl Drop for RequestGuard {
    fn drop(&mut self) {
        let _ = self
            .state
            .compare_exchange(STATE_RUNNING, STATE_CANCELLED, Ordering::SeqCst, Ordering::SeqCst);
        self.active.remove(&self.id);
        let mut recent = self.recent_finished.lock();
        if recent.len() >= MAX_TOMBSTONES {
            recent.pop_front();
        }
        recent.push_back((self.id, Instant::now()));
    }
}
```

---

### 3.5. Repositório SQLite e Sanitização Dupla (`repo.rs`)

```rust
use crate::config::AiSettings;
use crate::error::{ai_error, codes};
use crate::ids::AiRequestId;
use mt_core::{CommandBlockId, MtResult, PaneId, ProjectId, SessionId, TabId};
use mt_db::Db;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub const HISTORY_MAX_ROWS: usize = 5_000;
pub const HISTORY_PRUNE_BATCH: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AiHistoryKind {
    Suggest,
    Diagnosis,
    Rag,
    Structured,
}

impl AiHistoryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Suggest => "suggest",
            Self::Diagnosis => "diagnosis",
            Self::Rag => "rag",
            Self::Structured => "structured",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CacheClearScope {
    Diagnosis,
    Catalog,
    All,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryFilter {
    pub kind: Option<AiHistoryKind>,
    pub project_id: Option<ProjectId>,
    pub limit: u16,
    pub before_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryItem {
    pub id: AiRequestId,
    pub kind: AiHistoryKind,
    pub trigger: Option<String>,
    pub project_id: Option<ProjectId>,
    pub tab_id: Option<TabId>,
    pub pane_id: Option<PaneId>,
    pub session_id: Option<SessionId>,
    pub block_id: Option<CommandBlockId>,
    pub harness_id: Option<String>,
    pub model_id: Option<String>,
    pub reasoning_id: Option<String>,
    pub rag_slug: Option<String>,
    pub schema_id: Option<String>,
    pub prompt_sanitized: String,
    pub response_sanitized: Option<String>,
    pub risk_level: Option<String>,
    pub status: String,
    pub error_code: Option<String>,
    pub latency_ms: Option<u32>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryPage {
    pub items: Vec<AiHistoryItem>,
    pub next_before_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CachedCatalogRow {
    pub gateway_key: String,
    pub payload_json: String,
    pub etag: Option<String>,
    pub fetched_at: String,
}

#[derive(Clone, Debug)]
pub struct CachedDiagnosisRow {
    pub error_hash: String,
    pub gateway_key: String,
    pub project_id: Option<ProjectId>,
    pub summary: String,
    pub fix_command: Option<String>,
    pub fix_risk: Option<String>,
    pub harness_id: String,
    pub model_id: String,
    pub reasoning_id: Option<String>,
    pub prompt_version: u32,
    pub sanitizer_version: u32,
    pub hits: u32,
    pub created_at: String,
    pub expires_at: String,
}

/// Repositório SQLite de IA com geração monotônica de persistência para evitar repovoamento pós-limpeza (CA-29..CA-31, CA-33).
pub struct AiRepo {
    db: Db,
    history_generation: AtomicU64,
    cache_generation: AtomicU64,
    insert_counter: AtomicU64,
}

impl AiRepo {
    pub fn new(db: Db) -> Self {
        Self {
            db,
            history_generation: AtomicU64::new(1),
            cache_generation: AtomicU64::new(1),
            insert_counter: AtomicU64::new(0),
        }
    }

    pub fn current_history_generation(&self) -> u64 {
        self.history_generation.load(Ordering::SeqCst)
    }

    pub fn current_cache_generation(&self) -> u64 {
        self.cache_generation.load(Ordering::SeqCst)
    }

    pub async fn get_settings(&self) -> MtResult<AiSettings> {
        // Lê singleton id=1; se inexistente, retorna AiSettings::default()
        todo!("Implementado conforme contrato SQLite da Fase 03 em repo.rs")
    }

    /// Atualiza `ai_settings` validando `expected_revision` (concorrência otimista -> `AI_SETTINGS_CONFLICT`).
    pub async fn save_settings(&self, mut next: AiSettings) -> MtResult<AiSettings> {
        next.validate()?;
        next = next.sanitized();
        // Verifica revisão atual no banco; se divergente de next.revision, retorna AI_SETTINGS_CONFLICT.
        // Caso contrário, incrementa next.revision += 1 e faz INSERT OR REPLACE na linha id = 1.
        todo!("Implementado em repo.rs")
    }

    pub async fn get_catalog_cache(&self, gateway_key: &str) -> MtResult<Option<CachedCatalogRow>> {
        todo!("Implementado em repo.rs")
    }

    pub async fn put_catalog_cache(
        &self,
        expected_cache_gen: u64,
        row: &CachedCatalogRow,
    ) -> MtResult<bool> {
        if self.current_cache_generation() != expected_cache_gen {
            return Ok(false);
        }
        todo!("Implementado em repo.rs")
    }

    pub async fn get_diagnosis_cache(
        &self,
        error_hash: &str,
        now_rfc3339: &str,
    ) -> MtResult<Option<CachedDiagnosisRow>> {
        // Filtra `expires_at > :now`; se encontrar, incrementa `hits = hits + 1` e atualiza `last_hit_at = :now` (sem estender expires_at).
        todo!("Implementado em repo.rs")
    }

    pub async fn put_diagnosis_cache(
        &self,
        expected_cache_gen: u64,
        row: &CachedDiagnosisRow,
    ) -> MtResult<bool> {
        // Aplica mt_db::sanitizer::sanitize() em summary e fix_command antes do INSERT.
        todo!("Implementado em repo.rs")
    }

    pub async fn insert_history(
        &self,
        expected_history_gen: u64,
        save_history_enabled: bool,
        mut item: AiHistoryItem,
    ) -> MtResult<bool> {
        if !save_history_enabled || self.current_history_generation() != expected_history_gen {
            return Ok(false);
        }
        item.prompt_sanitized = mt_db::sanitizer::sanitize(&item.prompt_sanitized);
        item.response_sanitized = item
            .response_sanitized
            .as_deref()
            .map(mt_db::sanitizer::sanitize);
        // Insere linha e, a cada HISTORY_PRUNE_BATCH inserções, poda linhas antigas mantendo no máximo HISTORY_MAX_ROWS.
        todo!("Implementado em repo.rs")
    }

    pub async fn list_history(&self, filter: &AiHistoryFilter) -> MtResult<AiHistoryPage> {
        // Paginação keyset: WHERE id < :before_id ORDER BY id DESC LIMIT :limit (máx 200).
        todo!("Implementado em repo.rs")
    }

    pub async fn clear_history(&self, project_id: Option<ProjectId>) -> MtResult<u32> {
        self.history_generation.fetch_add(1, Ordering::SeqCst);
        todo!("Executa DELETE e retorna linhas afetadas")
    }

    pub async fn clear_cache(&self, scope: CacheClearScope) -> MtResult<u32> {
        self.cache_generation.fetch_add(1, Ordering::SeqCst);
        todo!("Executa DELETE em ai_diagnosis_cache e/ou ai_catalog_cache e retorna linhas afetadas")
    }

    pub async fn prune_expired_diagnoses(&self, now_rfc3339: &str) -> MtResult<u32> {
        todo!("DELETE FROM ai_diagnosis_cache WHERE expires_at <= ?1")
    }
}
```

---

### 3.6. Cliente HTTP, Dialeto e Streaming SSE (`client/{mod.rs,dialect.rs,sse.rs,http.rs}`)

#### `src-tauri/crates/mt-ai/src/client/dialect.rs`
```rust
use crate::config::AiSelection;
use crate::error::{ai_error, codes};
use mt_core::MtResult;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum CapabilityState {
    Validated,
    Unavailable { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCapabilities {
    pub catalog: CapabilityState,
    pub generation: CapabilityState,
    pub reasoning: CapabilityState,
    pub json_mode: CapabilityState,
    pub sse: CapabilityState,
    pub rag_search: CapabilityState,
    pub rag_list: CapabilityState,
    pub health: CapabilityState,
}

impl GatewayCapabilities {
    pub fn unconfigured(reason: &str) -> Self {
        let u = || CapabilityState::Unavailable {
            reason: reason.to_string(),
        };
        Self {
            catalog: u(),
            generation: u(),
            reasoning: u(),
            json_mode: u(),
            sse: u(),
            rag_search: u(),
            rag_list: u(),
            health: u(),
        }
    }

    pub fn all_validated() -> Self {
        Self {
            catalog: CapabilityState::Validated,
            generation: CapabilityState::Validated,
            reasoning: CapabilityState::Validated,
            json_mode: CapabilityState::Validated,
            sse: CapabilityState::Validated,
            rag_search: CapabilityState::Validated,
            rag_list: CapabilityState::Validated,
            health: CapabilityState::Validated,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub selection: AiSelection,
    pub messages: Vec<ChatMessage>,
    pub json_mode: bool,
    pub stream: bool,
    pub max_tokens: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct ChatResponse {
    pub content: String,
    pub model: Option<String>,
    pub usage: Option<Usage>,
}

/// Ponto único de tradução entre o MaxTerminal e o protocolo HTTP do gateway MaxAiManager (`R-01`).
pub trait GatewayDialect: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> GatewayCapabilities;
    fn health_path(&self) -> &str;
    fn catalog_path(&self) -> &str;
    fn chat_path(&self) -> &str;
    fn rag_list_path(&self) -> &str;
    fn extra_headers(&self, selection: Option<&AiSelection>) -> Vec<(String, String)>;
    fn build_chat_body(&self, req: &ChatRequest) -> Value;
    fn parse_chat_response(&self, v: Value) -> MtResult<ChatResponse>;
    fn parse_sse_delta(&self, data: &str) -> MtResult<Option<String>>;
    fn is_sse_done(&self, data: &str) -> bool;
}

/// Dialeto compatível OpenAI (`/v1/chat/completions`, `/v1/harnesses`, `/v1/rags/{slug}/search`).
pub struct OpenAiCompatDialect {
    capabilities: GatewayCapabilities,
}

impl OpenAiCompatDialect {
    pub fn new(capabilities: GatewayCapabilities) -> Self {
        Self { capabilities }
    }
}

impl Default for OpenAiCompatDialect {
    fn default() -> Self {
        Self::new(GatewayCapabilities::all_validated())
    }
}

impl GatewayDialect for OpenAiCompatDialect {
    fn id(&self) -> &'static str {
        "openai-compat-v1"
    }

    fn capabilities(&self) -> GatewayCapabilities {
        self.capabilities.clone()
    }

    fn health_path(&self) -> &str {
        "/health"
    }

    fn catalog_path(&self) -> &str {
        "/v1/harnesses"
    }

    fn chat_path(&self) -> &str {
        "/v1/chat/completions"
    }

    fn rag_list_path(&self) -> &str {
        "/v1/rags"
    }

    fn extra_headers(&self, selection: Option<&AiSelection>) -> Vec<(String, String)> {
        match selection {
            Some(sel) => vec![("X-Harness".to_string(), sel.harness_id.clone())],
            None => Vec::new(),
        }
    }

    fn build_chat_body(&self, req: &ChatRequest) -> Value {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                json!({
                    "role": role,
                    "content": mt_db::sanitizer::sanitize(&m.content),
                })
            })
            .collect();

        let mut body = json!({
            "model": format!("{}/{}", req.selection.harness_id, req.selection.model_id),
            "stream": req.stream && matches!(self.capabilities.sse, CapabilityState::Validated),
            "messages": messages,
        });

        if let Some(reasoning) = &req.selection.reasoning_id {
            if matches!(self.capabilities.reasoning, CapabilityState::Validated) {
                body["reasoning_effort"] = Value::String(reasoning.clone());
            }
        }
        if req.json_mode && matches!(self.capabilities.json_mode, CapabilityState::Validated) {
            body["response_format"] = json!({ "type": "json_object" });
        }
        if let Some(max_tokens) = req.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        body
    }

    fn parse_chat_response(&self, v: Value) -> MtResult<ChatResponse> {
        let content = v
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                ai_error(
                    codes::AI_BAD_RESPONSE,
                    "A resposta do MaxAiManager não contém 'choices[0].message.content'.",
                )
            })?;

        let model = v.get("model").and_then(|m| m.as_str()).map(ToOwned::to_owned);
        let usage = v.get("usage").and_then(|u| serde_json::from_value(u.clone()).ok());

        Ok(ChatResponse {
            content: mt_db::sanitizer::sanitize(content),
            model,
            usage,
        })
    }

    fn parse_sse_delta(&self, data: &str) -> MtResult<Option<String>> {
        let trimmed = data.trim();
        if trimmed.is_empty() || self.is_sse_done(trimmed) {
            return Ok(None);
        }
        let v: Value = serde_json::from_str(trimmed).map_err(|_| {
            ai_error(codes::AI_BAD_RESPONSE, "Frame SSE malformado retornado pelo MaxAiManager.")
        })?;
        let delta = v
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
            .map(ToOwned::to_owned);
        Ok(delta)
    }

    fn is_sse_done(&self, data: &str) -> bool {
        data.trim() == "[DONE]"
    }
}
```

#### `src-tauri/crates/mt-ai/src/client/sse.rs`
- Constantes de proteção e coalescência:
  - `pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;` (1 MiB)
  - `pub const MAX_SSE_FRAME_BYTES: usize = 64 * 1024;` (64 KiB)
  - `pub const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(15);`
  - `pub const DELTA_COALESCE_INTERVAL: Duration = Duration::from_millis(32);`
  - `pub const SANITIZER_HOLD_TAIL_BYTES: usize = 128;` (retém os últimos bytes até limite de linha/espaço ou término do stream para impedir que segredos divididos entre chunks escapem antes da sanitização — `CA-36`).
- Se o stream encerrar sem receber `dialect.is_sse_done(data)` (`[DONE]`), retorna `Err(ai_error(codes::AI_BAD_RESPONSE, "Conexão SSE encerrada prematuramente sem marcador de conclusão."))` (`CA-08`).

#### `src-tauri/crates/mt-ai/src/client/http.rs`
```rust
use crate::client::dialect::{
    CapabilityState, ChatRequest, ChatResponse, GatewayCapabilities, GatewayDialect, OpenAiCompatDialect,
};
use crate::config::AiSettings;
use mt_core::MtResult;
use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub const AGENT_HEADER: (&str, &str) = ("X-Agent-Name", "MaxTerminal");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Health,
    Catalog,
    Suggest,
    Diagnosis,
    Rag,
    Structured,
}

#[derive(Clone, Debug)]
pub enum Conditional<T> {
    Fresh { value: T, etag: Option<String> },
    NotModified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum GatewayStatus {
    Online,
    Degraded,
    Offline,
    Disabled,
    Unconfigured,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayHealth {
    pub status: GatewayStatus,
    pub version: Option<String>,
    pub latency_ms: Option<u32>,
    pub base_url: Option<String>,
    pub checked_at: String,
    pub config_revision: u32,
    pub capabilities: GatewayCapabilities,
}

pub struct GatewayClient {
    inner: reqwest::Client,
    snapshot: RwLock<Arc<GatewayConfigSnapshot>>,
    dialect: Arc<dyn GatewayDialect>,
}

impl GatewayClient {
    pub fn new(settings: &AiSettings) -> MtResult<Self>;
    pub fn with_dialect(settings: &AiSettings, dialect: Arc<dyn GatewayDialect>) -> MtResult<Self>;
    pub fn reconfigure(&self, settings: &AiSettings) -> MtResult<()>;
    pub fn gateway_key(&self) -> String;
    pub fn config_revision(&self) -> u32;
    pub fn capabilities(&self) -> GatewayCapabilities;
    pub async fn health(&self) -> MtResult<GatewayHealth>;
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        op: Op,
        etag: Option<&str>,
    ) -> MtResult<Conditional<T>>;
    pub async fn post_json<B: Serialize + Sync, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        op: Op,
        extra_headers: &[(String, String)],
        cancel: &CancellationToken,
    ) -> MtResult<T>;
    pub async fn chat(
        &self,
        req: &ChatRequest,
        op: Op,
        cancel: &CancellationToken,
        on_delta: &mut (dyn FnMut(&str) + Send),
    ) -> MtResult<ChatResponse>;
}
```
- **Regras obrigatórias de `GatewayClient`:**
  1. `reqwest::Client::builder()` configurado com `redirect(reqwest::redirect::Policy::none())` e `default_headers` contendo `X-Agent-Name: MaxTerminal`, `User-Agent: MaxTerminal/0.1.0 (+linux)` e `Accept: application/json, text/event-stream`.
  2. Em `reconfigure(&self, settings)`, valida `settings.validate()?` e substitui atomicamente o `GatewayConfigSnapshot` dentro do `RwLock` para que a próxima requisição use a nova `base_url` e os novos timeouts imediatamente sem reiniciar o aplicativo (`CA-06`).
  3. Retry apenas para `Op::Health` e `Op::Catalog` em erros transitórios de conexão ou status `502`/`503`/`504`: 1 tentativa inicial + até 2 retries com backoff de `200 ms` e `600 ms` (respeitando o deadline total da operação). Nunca fazer retry em `Suggest`, `Diagnosis`, `Rag` ou `Structured`, nem em `401`/`403`/`400`.
  4. `tracing` registra apenas `op`, `status`, `latency_ms` e tamanhos em bytes — jamais prompts ou payloads (`CA-32`).

---

### 3.7. Catálogo de Harnesses (`catalog/{mod.rs,fallback.rs}` + `assets/catalog.fallback.json`)

#### `src-tauri/crates/mt-ai/assets/catalog.fallback.json`
```json
{
  "harnesses": [
    {
      "id": "claude-code",
      "label": "Claude Code",
      "icon": "simple-icons:claude",
      "models": []
    },
    {
      "id": "codex",
      "label": "Codex",
      "icon": "simple-icons:openai",
      "models": []
    },
    {
      "id": "agy",
      "label": "Antigravity (agy)",
      "icon": "simple-icons:google",
      "models": []
    },
    {
      "id": "opencode",
      "label": "OpenCode",
      "icon": "mdi:code-braces",
      "models": []
    }
  ]
}
```

#### `src-tauri/crates/mt-ai/src/catalog/mod.rs`
```rust
use crate::client::dialect::GatewayCapabilities;
use crate::client::http::GatewayClient;
use crate::config::AiSelection;
use crate::repo::AiRepo;
use mt_core::MtResult;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CatalogSource {
    Gateway,
    Cache,
    Fallback,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningLevel {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub rank: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub reasoning_levels: Vec<ReasoningLevel>,
    pub default_reasoning: Option<String>,
    pub context_window: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Harness {
    pub id: String,
    pub label: String,
    pub icon: String,
    #[serde(default)]
    pub models: Vec<AiModel>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HarnessCatalog {
    pub config_revision: u32,
    pub capabilities: GatewayCapabilities,
    pub source: CatalogSource,
    pub fetched_at: Option<String>,
    pub harnesses: Vec<Harness>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSelection {
    pub selection: AiSelection,
    pub harness_label: String,
    pub model_label: String,
    pub reasoning_label: Option<String>,
}

pub struct CatalogService {
    client: Arc<GatewayClient>,
    repo: Arc<AiRepo>,
    mem: RwLock<Option<(String, HarnessCatalog, Instant)>>,
}

impl CatalogService {
    pub const TTL: Duration = Duration::from_secs(6 * 3600);

    pub fn new(client: Arc<GatewayClient>, repo: Arc<AiRepo>) -> Self;
    pub fn invalidate_memory(&self);
    pub async fn get(&self) -> HarnessCatalog;
    pub async fn refresh(&self) -> MtResult<HarnessCatalog>;
    pub fn resolve(&self, catalog: &HarnessCatalog, sel: &AiSelection) -> MtResult<ResolvedSelection>;
    pub fn lowest_reasoning(&self, catalog: &HarnessCatalog, harness_id: &str, model_id: &str) -> Option<String>;
}
```
- **Regras de negócio do `CatalogService` (`CA-11`, `CA-12`):**
  1. Ao mesclar a resposta do gateway com o fallback de identidades, os 4 harnesses obrigatórios (`claude-code`, `codex`, `agy`, `opencode`) estão **sempre** presentes na lista final (mantendo `models: []` caso o gateway não tenha retornado modelos para algum deles); harnesses adicionais enviados pelo gateway aparecem após os 4 canônicos.
  2. Para cada modelo retornado pelo gateway, se `reasoning_levels` vier sem `rank` explícito, os `rank`s são atribuídos na ordem `0, 1, 2...`.
  3. `resolve()` valida que `harness_id` existe, que `model_id` existe dentro daquele harness e:
     - Se `model.reasoning_levels.is_empty()`: aceita apenas `sel.reasoning_id == None` (envia `reasoningId: null`).
     - Se `model.reasoning_levels` não for vazio: exige `sel.reasoning_id == Some(id)` contido em `reasoning_levels` (ou usa `model.default_reasoning` se for válido). Qualquer divergência retorna `Err(ai_error(codes::AI_UNKNOWN_SELECTION, ...))`. Jamais inventa `"default"`, `"low"`, `"medium"` ou `"high"`.
  4. `get()` verifica primeiro o cache em memória para o `gateway_key` atual dentro do `TTL` de 6h (`0 HTTP`). Se expirado ou ausente, consulta `client.get_json("/v1/harnesses", Op::Catalog, cached_etag)`:
     - `Conditional::Fresh` → persiste em `ai_catalog_cache`, atualiza memória e retorna `source = CatalogSource::Gateway`.
     - `Conditional::NotModified` (HTTP 304) → atualiza `fetched_at` no cache e em memória e retorna sem rebaixar os modelos (`CA-12`).
     - Erro de rede/404 → se houver entrada em `ai_catalog_cache` para o mesmo `gateway_key`, retorna com `source = CatalogSource::Cache`; caso contrário, retorna o catálogo de identidades com `source = CatalogSource::Fallback`.
