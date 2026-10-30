# Especificação Técnica — Implementação 01: Domínio, Remotes, Autenticação e Transporte GitHub (`mt-github`)

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `01 - Dominio-Remotes-Auth-e-Transporte-GitHub`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §3 e §7, [`../instructions.md`](../instructions.md) §2.1 e §5.1, [`../spec.md`](../spec.md) §1, §3, §4.1–§4.3 e §6.

---

## 1. Contexto, Objetivo e Escopo

Esta primeira implementação estabelece a fundação do crate Rust `mt-github` dentro do workspace Tauri (`src-tauri/crates/mt-github`), abrangendo:

1. **Modelo de Domínio e Erros (`model.rs`, `error.rs`):** Definição de todos os identificadores fortes (`RepoId`, `AuthScopeId`, `RequestId`), DTOs compatíveis com `specta::Type` (`camelCase` para structs e `snake_case` para enums de estado) e catálogo completo de erros com prefixo `github.*` convertidos para `mt_core::MtError`.
2. **Contratos de Portas e Eventos (`ports.rs`, `events.rs`):** Traits assíncronas (`ProjectPort`, `GitPort`, `WorkspacePort`, `AgentPort`, `MaxCodePort`, `NotificationPort` e `GithubEventSink`) que desacoplam `mt-github` do runtime Tauri e das Fases 06, 07, 08, 09, 11 e 13.
3. **Detecção e Parser de Remotes GitHub (`remote.rs`):** Resolução do remote `origin` (inclusive quando `.git` é um arquivo apontando para worktree) e parser estrito de URLs SSH (`git@github.com:owner/repo.git`, `ssh://git@github.com/owner/repo`) e HTTPS (`https://github.com/owner/repo`), normalizando `host`, `owner` e `name` e rejeitando hosts não-GitHub, path traversal, queries ou fragmentos.
4. **Autenticação Segura e Isolamento de Escopo (`auth.rs`):** Resolução de credenciais via `gh auth token --hostname github.com` (prioritário) com fallback para o keyring nativo do sistema (`keyring = "3"` sobre Secret Service / KWallet no CachyOS), gerenciamento de `AuthScopeId` opaco (UUID v7 rotacionado a cada troca de credencial/conta) e garantia absoluta de que tokens nunca vazam em SQLite, logs (`tracing`), URLs ou `argv`.
5. **Controle de Rate Limit e Clientes de Transporte (`rate_limit.rs`, `client/*`):** Orçamento compartilhado de rate limit primário e secundário (`X-RateLimit-*`, `Retry-After`, backoff exponencial com jitter) e implementação dos dois transportes equivalentes sobre a REST API do GitHub (`2026-03-10`): `GhCliClient` (`gh api --include` com stdin nas escritas e timeout/kill de processo) e `RestClient` (`reqwest` com ETag/`If-None-Match` e paginação `Link rel="next"`), orquestrados pelo `FallbackClient`.

---

## 2. Arquivos a Criar ou Modificar

| Caminho | Ação | Descrição |
|---------|------|-----------|
| `src-tauri/Cargo.toml` | Modificar | Incluir `"crates/mt-github"` em `[workspace.members]` e adicionar `mt-github` em `[workspace.dependencies]` |
| `src-tauri/crates/mt-github/Cargo.toml` | Criar | Manifesto Cargo do crate `mt-github` com dependências de produção e desenvolvimento |
| `src-tauri/crates/mt-github/src/lib.rs` | Criar | Declaração dos módulos públicos e reexports iniciais do crate |
| `src-tauri/crates/mt-github/src/error.rs` | Criar | Enum `GithubError`, códigos `github.*` e conversão `From<GithubError> for MtError` |
| `src-tauri/crates/mt-github/src/model.rs` | Criar | Newtypes (`RepoId`, `AuthScopeId`, `RequestId`) e todos os DTOs de domínio e IPC com `specta::Type` |
| `src-tauri/crates/mt-github/src/ports.rs` | Criar | Traits de portas externas (`ProjectPort`, `GitPort`, `WorkspacePort`, `AgentPort`, `MaxCodePort`, `NotificationPort`) |
| `src-tauri/crates/mt-github/src/events.rs` | Criar | Envelope `GithubEventEnvelope<T>`, payloads dos 8 eventos `github://*` e trait `GithubEventSink` |
| `src-tauri/crates/mt-github/src/remote.rs` | Criar | Parser `parse_github_remote_url` e resolvedor `GithubRemoteResolver` via `GitPort` |
| `src-tauri/crates/mt-github/src/auth.rs` | Criar | Traits `SecretStore` / `TokenProvider`, `KeyringSecretStore` e `DefaultTokenProvider` |
| `src-tauri/crates/mt-github/src/rate_limit.rs` | Criar | `RateLimitTracker`, `RateLimitState`, parser de headers HTTP e cálculo de backoff com jitter |
| `src-tauri/crates/mt-github/src/client/mod.rs` | Criar | Trait `GithubClient`, `ClientKind`, `FetchResult<T>`, `PageLink` e tipos de requisição/resposta |
| `src-tauri/crates/mt-github/src/client/gh_cli.rs` | Criar | `GhCliClient` executando `gh api` com `--include`, `--input -`, timeout de 30s e `kill_on_drop` |
| `src-tauri/crates/mt-github/src/client/rest.rs` | Criar | `RestClient` com `reqwest`, validação de `Link`, ETag e descarte de Pull Requests |
| `src-tauri/crates/mt-github/src/client/fallback.rs` | Criar | `FallbackClient` coordenando `GhCliClient` → `RestClient` com prevenção de replay em mutações |
| `src-tauri/crates/mt-github/tests/remote.rs` | Criar | Testes unitários cobrindo ≥ 12 formatos válidos de URL GitHub e rejeição de URLs inválidas/outros hosts |
| `src-tauri/crates/mt-github/tests/rest_client.rs` | Criar | Testes de integração com `wiremock` (ETag 304, paginação 3 páginas, rate limit 403/429, filtro de PRs) |
| `src-tauri/crates/mt-github/tests/gh_cli.rs` | Criar | Testes com script `gh` simulado em `PATH` temporário, timeout/kill, erro 422 sem fallback e segurança de token |

---

## 3. Assinaturas Exatas e Estruturas Rust

### 3.1. Manifesto `src-tauri/crates/mt-github/Cargo.toml`

```toml
[package]
name = "mt-github"
version = "0.1.0"
edition.workspace = true
rust-version = "1.85"

[dependencies]
mt-core = { workspace = true }
mt-db = { workspace = true }
async-trait = "0.1"
bytes = "1"
chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }
hmac = "0.12"
httpdate = "1.0"
keyring = { version = "3", features = ["linux-native-sync-persistent"] }
pulldown-cmark = { version = "0.12", default-features = false }
rand = "0.8"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json", "gzip"] }
rusqlite = { workspace = true }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
sha2 = "0.10"
specta = { workspace = true }
thiserror = "2.0"
tokio = { version = "1", features = ["process", "sync", "time", "io-util", "rt", "macros"] }
tracing = "0.1"
url = "2.5"
uuid = { version = "1", features = ["v7", "serde"] }

[dev-dependencies]
insta = { version = "1.40", features = ["json"] }
tempfile = "3.13"
wiremock = "0.6"
```

### 3.2. Erros de Domínio (`src-tauri/crates/mt-github/src/error.rs`)

Todos os erros expostos ao frontend possuem código prefixado por `github.` e mensagem em `pt-BR` (sanitizada contra vazamento de tokens):

```rust
use mt_core::{ErrorCode, MtError};
use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GithubErrorKind {
    NotAGithubRepo,
    AuthRequired,
    Forbidden,
    NotFound,
    IssuesDisabled,
    RateLimited,
    Transport,
    Timeout,
    InvalidResponse,
    Validation,
    CatalogInvalid,
    BridgeUnsupported,
    DeliveryUnknown,
    MutationUnknown,
    Conflict,
    StaleRequest,
    WorktreeAmbiguous,
    WorktreeForeign,
    KeyringUnavailable,
}

impl GithubErrorKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::NotAGithubRepo => "github.not_a_github_repo",
            Self::AuthRequired => "github.auth_required",
            Self::Forbidden => "github.forbidden",
            Self::NotFound => "github.not_found",
            Self::IssuesDisabled => "github.issues_disabled",
            Self::RateLimited => "github.rate_limited",
            Self::Transport => "github.transport",
            Self::Timeout => "github.timeout",
            Self::InvalidResponse => "github.invalid_response",
            Self::Validation => "github.validation",
            Self::CatalogInvalid => "github.catalog_invalid",
            Self::BridgeUnsupported => "github.bridge_unsupported",
            Self::DeliveryUnknown => "github.delivery_unknown",
            Self::MutationUnknown => "github.mutation_unknown",
            Self::Conflict => "github.conflict",
            Self::StaleRequest => "github.stale_request",
            Self::WorktreeAmbiguous => "github.worktree_ambiguous",
            Self::WorktreeForeign => "github.worktree_foreign",
            Self::KeyringUnavailable => "github.keyring_unavailable",
        }
    }
}

#[derive(Debug, Error, Clone)]
#[error("{message}")]
pub struct GithubError {
    pub kind: GithubErrorKind,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

impl GithubError {
    pub fn new(kind: GithubErrorKind, message: impl Into<String>) -> Self {
        let sanitized = mt_db::sanitizer::sanitize(&message.into());
        Self {
            kind,
            message: sanitized,
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn rate_limited(reset_at_ms: Option<i64>, retry_at_ms: i64, secondary: bool) -> Self {
        Self::new(
            GithubErrorKind::RateLimited,
            "Limite de requisições da API do GitHub atingido. Aguarde antes de tentar novamente.",
        )
        .with_details(serde_json::json!({
            "resetAtMs": reset_at_ms,
            "retryAtMs": retry_at_ms,
            "secondary": secondary,
        }))
    }
}

impl From<GithubError> for MtError {
    fn from(err: GithubError) -> Self {
        let error = MtError::new(ErrorCode(err.kind.code()), err.message);
        match err.details {
            Some(details) => error.with_details(details),
            None => error,
        }
    }
}
```

### 3.3. Modelo de Domínio (`src-tauri/crates/mt-github/src/model.rs`)

```rust
use mt_core::{ProjectId, TabId, TemplateId};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct RepoId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct AuthScopeId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct RequestId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepoRef {
    pub id: RepoId,
    pub host: String,
    pub owner: String,
    pub name: String,
    pub html_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RepoContext {
    pub project_id: ProjectId,
    pub repo: GithubRepoRef,
    pub auth_scope: AuthScopeId,
    pub generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueState {
    Open,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LabelKind {
    State,
    Closure,
    Category,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LabelDefinition {
    pub name: String,
    pub kind: LabelKind,
    pub color: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LabelInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    pub description: Option<String>,
    pub kind: Option<LabelKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueSummary {
    pub number: u32,
    pub node_id: String,
    pub title: String,
    pub state: IssueState,
    pub author_login: Option<String>,
    pub labels: Vec<LabelInfo>,
    pub comment_count: u32,
    pub updated_at_ms: i64,
    pub html_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueComment {
    pub id: String,
    pub author_login: Option<String>,
    pub body: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub html_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssuePlan {
    pub source_comment_id: Option<String>,
    pub source_updated_at_ms: i64,
    pub markdown: String,
    pub start_line: u32,
    pub end_line: u32,
    pub provisional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueDetail {
    pub summary: IssueSummary,
    pub body: String,
    pub comments: Vec<IssueComment>,
    pub comments_complete: bool,
    pub comments_cursor: Option<String>,
    pub plan: Option<IssuePlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CacheMeta {
    pub revision: u32,
    pub checked_at_ms: Option<i64>,
    pub stale: bool,
    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Cached<T> {
    pub value: T,
    pub meta: CacheMeta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenIssueCount {
    pub count: Option<u32>,
    pub checked_at_ms: Option<i64>,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueListQuery {
    pub state: Option<IssueState>,
    pub labels: Vec<String>,
    pub text: String,
    pub cursor: Option<String>,
    pub limit: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssuePage {
    pub items: Vec<IssueSummary>,
    pub next_cursor: Option<String>,
    pub count: OpenIssueCount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueCreateRequest {
    pub project_id: ProjectId,
    pub request_id: RequestId,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LabelPatchRequest {
    pub project_id: ProjectId,
    pub number: u32,
    pub request_id: RequestId,
    pub expected_updated_at_ms: Option<i64>,
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MutationOutcome {
    Confirmed,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MutationReceipt<T> {
    pub request_id: RequestId,
    pub outcome: MutationOutcome,
    pub value: Option<T>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LabelPatchResult {
    pub issue: IssueDetail,
    pub applied: Vec<String>,
    pub failed: Vec<String>,
    pub outcome: MutationOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AuthSource {
    GhCli,
    Keyring,
    Anonymous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatus {
    pub source: AuthSource,
    pub account_login: Option<String>,
    pub available: bool,
    pub scope: AuthScopeId,
}
```

### 3.4. Parser e Resolução de Remotes (`src-tauri/crates/mt-github/src/remote.rs`)

Regras de normalização e validação:
- Aceita exclusivamente `github.com` (case-insensitive na entrada, normalizado para `"github.com"` em minúsculas).
- Suporta formatos:
  - SSH SCP-like: `git@github.com:owner/repo.git`, `git@github.com:owner/repo`, `custom-user@GITHUB.COM:owner/repo/`
  - SSH URL: `ssh://git@github.com/owner/repo.git`, `ssh://git@github.com:22/owner/repo.git`
  - HTTPS: `https://github.com/owner/repo.git`, `https://github.com/owner/repo`, `https://user@github.com/owner/repo/`, `https://github.com:443/owner/repo.git`
- Rejeita (`Ok(None)`):
  - Outros hosts (`gitlab.com`, `bitbucket.org`, `github.com.evil.tld`, `mygithub.com`, `enterprise.internal`).
  - Caminhos locais (`/home/user/repo`, `file:///path`).
  - Portas não-padrão incompatíveis (ex.: HTTPS com porta `8443` ou SSH com porta HTTP), query strings (`?foo=bar`), fragmentos (`#readme`), mais ou menos de 2 segmentos de caminho (`owner/repo/extra`), caracteres de controle ou path traversal (`..`, `.`).
- `owner` e `name` preservam a grafia original nos campos de exibição, mas o `RepoId` determinístico ou chave de comparação usa lowercase ASCII (`format!("github.com/{}/{}", owner.to_ascii_lowercase(), name.to_ascii_lowercase())`). Remove apenas o sufixo final `/` e `.git` (uma única vez no final do segmento `repo`).

```rust
use std::path::Path;
use crate::error::GithubError;
use crate::model::{GithubRepoRef, RepoId};
use crate::ports::GitPort;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedGithubRemote {
    pub host: String,
    pub owner: String,
    pub name: String,
    pub html_url: String,
}

pub fn parse_github_remote_url(raw_url: &str) -> Option<ParsedGithubRemote>;

pub struct GithubRemoteResolver<G: GitPort> {
    git: G,
}

impl<G: GitPort> GithubRemoteResolver<G> {
    pub fn new(git: G) -> Self;
    pub async fn resolve_project_repo(&self, project_path: &Path) -> Result<Option<GithubRepoRef>, GithubError>;
}
```

### 3.5. Autenticação e Keyring (`src-tauri/crates/mt-github/src/auth.rs`)

```rust
use async_trait::async_trait;
use crate::error::GithubError;
use crate::model::{AuthScopeId, AuthSource, AuthStatus};

/// Segredo em memória protegido contra formatação acidental em logs.
#[derive(Clone)]
pub struct SecretToken(String);

impl SecretToken {
    pub fn new(raw: String) -> Self;
    pub fn expose(&self) -> &str;
}

impl std::fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretToken([REDACTED])")
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedCredentials {
    pub scope: AuthScopeId,
    pub source: AuthSource,
    pub account_login: Option<String>,
    pub token: Option<SecretToken>,
}

#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn get_token(&self) -> Result<Option<SecretToken>, GithubError>;
    async fn set_token(&self, token: &SecretToken) -> Result<(), GithubError>;
    async fn delete_token(&self) -> Result<(), GithubError>;
}

pub struct KeyringSecretStore {
    service_name: String, // "maxterminal"
    account_key: String,  // "github.com:pat"
}

#[async_trait]
pub trait TokenProvider: Send + Sync {
    async fn current_credentials(&self) -> Result<ResolvedCredentials, GithubError>;
    async fn status(&self) -> Result<AuthStatus, GithubError>;
    async fn set_keyring_token(&self, raw_token: String) -> Result<AuthStatus, GithubError>;
    async fn clear_keyring_token(&self) -> Result<AuthStatus, GithubError>;
}
```

Regras de segurança em `auth.rs`:
1. `KeyringSecretStore` opera sobre `keyring::Entry::new("maxterminal", "github.com:pat")` dentro de `tokio::task::spawn_blocking`. Se o Secret Service / KWallet estiver indisponível no ambiente Linux, retorna `GithubErrorKind::KeyringUnavailable` com mensagem explicativa em `pt-BR`; **jamais** faz fallback para arquivo texto ou SQLite.
2. `DefaultTokenProvider` verifica primeiro `gh auth status --hostname github.com` / `gh auth token --hostname github.com` (com `GH_PROMPT_DISABLED=1`). Se disponível e válido, usa `AuthSource::GhCli`. Caso contrário, consulta `SecretStore::get_token()`.
3. Quando um token é salvo via `set_keyring_token`, valida formato básico não-vazio, valida identidade contra `GET /user` (ou registra login quando online), salva no keyring e gera um **novo** `AuthScopeId(Uuid::now_v7().to_string())`, invalidando escopos anteriores.

### 3.6. Rate Limit e Clientes (`rate_limit.rs` e `client/*`)

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use specta::Type;
use crate::auth::ResolvedCredentials;
use crate::error::GithubError;
use crate::model::{GithubRepoRef, IssueComment, IssueDetail, IssueSummary, LabelInfo};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitState {
    pub resource: String,
    pub limit: Option<u32>,
    pub remaining: Option<u32>,
    pub reset_at_ms: Option<i64>,
    pub retry_at_ms: Option<i64>,
    pub secondary: bool,
}

#[derive(Debug, Clone)]
pub enum FetchResult<T> {
    Modified {
        value: T,
        etag: Option<String>,
        next_url: Option<String>,
        rate: RateLimitState,
    },
    NotModified {
        rate: RateLimitState,
    },
}

#[derive(Debug, Clone)]
pub struct RemoteIssueItem {
    pub summary: IssueSummary,
    pub body: Option<String>,
    pub created_at_ms: i64,
}

#[async_trait]
pub trait GithubClient: Send + Sync {
    async fn get_repo(
        &self,
         creds: &ResolvedCredentials,
        owner: &str,
        repo: &str,
    ) -> Result<GithubRepoRef, GithubError>;

    async fn list_issues_page(
        &self,
        creds: &ResolvedCredentials,
        owner: &str,
        repo: &str,
        page_url_or_query: &str,
        etag: Option<&str>,
    ) -> Result<FetchResult<Vec<RemoteIssueItem>>, GithubError>;

    async fn get_issue(
        &self,
        creds: &ResolvedCredentials,
        owner: &str,
        repo: &str,
        number: u32,
        etag: Option<&str>,
    ) -> Result<FetchResult<RemoteIssueItem>, GithubError>;

    async fn list_comments_page(
        &self,
        creds: &ResolvedCredentials,
        owner: &str,
        repo: &str,
        number: u32,
        page_url: Option<&str>,
        etag: Option<&str>,
    ) -> Result<FetchResult<Vec<IssueComment>>, GithubError>;

    async fn list_labels_page(
        &self,
        creds: &ResolvedCredentials,
        owner: &str,
        repo: &str,
        page_url: Option<&str>,
        etag: Option<&str>,
    ) -> Result<FetchResult<Vec<LabelInfo>>, GithubError>;

    async fn create_issue(
        &self,
        creds: &ResolvedCredentials,
        owner: &str,
        repo: &str,
        title: &str,
        body: &str,
        labels: &[String],
    ) -> Result<RemoteIssueItem, GithubError>;

    async fn add_issue_labels(
        &self,
        creds: &ResolvedCredentials,
        owner: &str,
        repo: &str,
        number: u32,
        labels: &[String],
    ) -> Result<Vec<LabelInfo>, GithubError>;

    async fn remove_issue_label(
        &self,
        creds: &ResolvedCredentials,
        owner: &str,
        repo: &str,
        number: u32,
        label_name: &str,
    ) -> Result<(), GithubError>;

    async fn create_label(
        &self,
        creds: &ResolvedCredentials,
        owner: &str,
        repo: &str,
        name: &str,
        color_hex: &str,
        description: Option<&str>,
    ) -> Result<LabelInfo, GithubError>;
}
```

Regras críticas de transporte (`GhCliClient`, `RestClient` e `FallbackClient`):
- **Headers fixos:** `Accept: application/vnd.github+json`, `User-Agent: MaxTerminal`, `X-GitHub-Api-Version: 2026-03-10`.
- **Filtro de Pull Requests:** A rota `/repos/{owner}/{repo}/issues` do GitHub mistura Issues e Pull Requests. Qualquer item JSON que possua a chave `"pull_request"` (não-nula) é imediatamente descartado no parser de `list_issues_page` e rejeitado como `GithubErrorKind::NotFound` em `get_issue`.
- **Segurança de Paginação (`Link`):** Antes de seguir `rel="next"`, valida que a URL usa esquema `https`, host exato `api.github.com` e prefixo `/repos/{owner}/{repo}/` ou `/repositories/`. Detecta ciclos mantendo um `HashSet<String>` de URLs visitadas na operação.
- **Execução segura do `gh`:**
  - Invoca `gh api --hostname github.com --method <METHOD> --include -H "Accept: application/vnd.github+json" -H "X-GitHub-Api-Version: 2026-03-10" <endpoint>`.
  - Em `POST`, passa `--input -` e escreve o JSON serializado no `stdin` do processo filho (evita que `@texto` no corpo da issue seja lido como arquivo local pelo `gh`).
  - Se `creds.source == AuthSource::Keyring`, injeta `GH_TOKEN` apenas no mapa de ambiente do processo filho (`Command::env("GH_TOKEN", token.expose())`) e define `GH_PROMPT_DISABLED=1`.
  - Aplica `kill_on_drop(true)` e `tokio::time::timeout(Duration::from_secs(30), ...)`. Em caso de timeout, executa `child.kill().await` e `child.wait().await` para não deixar processo órfão (Diretriz §4 do `AGENTS.md`).
- **Política do `FallbackClient`:**
  - Para **leituras (`GET`)**: tenta `GhCliClient` se disponível; se o binário `gh` não existir, falhar ao iniciar ou retornar saída corrompida, faz fallback imediato para `RestClient` na mesma identidade.
  - Para **escritas (`POST`/`DELETE`)**: só faz fallback para `RestClient` se a falha no `GhCliClient` ocorreu comprovadamente **antes** de o processo `gh` ser iniciado (`io::ErrorKind::NotFound` ao spawnar o binário). Se o processo `gh` iniciou e sofreu timeout ou queda sem resposta HTTP determinística, retorna `GithubErrorKind::MutationUnknown` — **nunca** repete o `POST` no `RestClient`. Erros semânticos HTTP (`401`, `403`, `404`, `410`, `422`, `429`) nunca disparam fallback.
