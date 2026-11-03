//! Cliente HTTP REST v3 (`api.github.com` ou mock local) com suporte a `ETag`, paginação `Link` e rate limit.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::auth::TokenStore;
use crate::client::gh_cli::{parse_issue_comment_json, parse_rest_or_gh_issue_json};
use crate::client::{RemoteFetchOutcome, RemoteIssueDetailBundle, RemoteIssueRecord};
use crate::error::{GithubError, GithubErrorKind, GithubResult};
use crate::labels::to_repo_label;
use crate::model::{
    GithubRepoIdentity, GithubTransportKind, IssueComment, IssueStateFilter, RepoLabel,
};
use crate::rate_limit::RateLimitTracker;

const DEFAULT_API_BASE: &str = "https://api.github.com";
const GITHUB_API_VERSION: &str = "2022-11-28";

/// Cliente REST v3 do GitHub com cabeçalhos canônicos, ETag e contabilidade de cota.
#[derive(Clone)]
pub struct RestClient {
    http: Client,
    base_url: String,
    token_store: TokenStore,
    rate_limit: RateLimitTracker,
}

impl RestClient {
    /// Cria um novo `RestClient` apontando para `https://api.github.com`.
    pub fn new(token_store: TokenStore, rate_limit: RateLimitTracker) -> GithubResult<Self> {
        Self::with_base_url(DEFAULT_API_BASE, token_store, rate_limit)
    }

    /// Cria um `RestClient` com URL base customizada (usado em testes de integração com mock server local).
    pub fn with_base_url(
        base_url: impl Into<String>,
        token_store: TokenStore,
        rate_limit: RateLimitTracker,
    ) -> GithubResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| {
                GithubError::new(
                    GithubErrorKind::Internal,
                    "Falha ao construir cliente HTTP REST do GitHub.",
                )
                .with_details(e.to_string())
            })?;

        Ok(Self {
            http,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token_store,
            rate_limit,
        })
    }

    /// Retorna referência ao `RateLimitTracker`.
    #[must_use]
    pub const fn rate_limit(&self) -> &RateLimitTracker {
        &self.rate_limit
    }

    /// Retorna referência ao `TokenStore`.
    #[must_use]
    pub const fn token_store(&self) -> &TokenStore {
        &self.token_store
    }

    /// Valida o token configurado consultando `GET /user` e retorna o login autenticado.
    pub async fn validate_authenticated_user(&self) -> GithubResult<String> {
        let resp = self.send_request("GET", "/user", None, None).await?;
        let val: Value = serde_json::from_slice(&resp.body).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "Resposta JSON inválida em GET /user.",
            )
            .with_details(e.to_string())
        })?;
        let login = val
            .get("login")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::AuthInvalid,
                    "Resposta de GET /user não contém o campo 'login'.",
                )
            })?
            .to_string();
        self.token_store.set_validated_login(Some(login.clone()));
        Ok(login)
    }

    /// Lista issues paginadas via REST (`GET /repos/{owner}/{repo}/issues`), suportando `If-None-Match`.
    pub async fn fetch_issues(
        &self,
        repo: &GithubRepoIdentity,
        state: IssueStateFilter,
        per_page: u32,
        max_pages: u32,
        cached_etag: Option<&str>,
    ) -> GithubResult<RemoteFetchOutcome<Vec<RemoteIssueRecord>>> {
        let state_str = match state {
            IssueStateFilter::Open => "open",
            IssueStateFilter::Closed => "closed",
            IssueStateFilter::All => "all",
        };
        let clamped_per_page = per_page.clamp(1, 100);
        let clamped_pages = max_pages.clamp(1, 10);

        let first_path = format!(
            "/repos/{}/{}/issues?state={state_str}&per_page={clamped_per_page}&sort=updated&direction=desc",
            repo.owner, repo.repo
        );

        let first_resp = self
            .send_request("GET", &first_path, cached_etag, None)
            .await?;

        if first_resp.status == 304 {
            return Ok(RemoteFetchOutcome::NotModified {
                transport: GithubTransportKind::Rest,
                rate_limit: self.rate_limit.snapshot(),
            });
        }

        let first_etag = first_resp.headers.get("etag").cloned();
        let mut all_issues = Vec::new();
        Self::append_issues_from_body(&first_resp.body, &mut all_issues)?;

        let mut next_link = first_resp
            .headers
            .get("link")
            .and_then(|l| extract_next_link(l));
        let mut page_count = 1;

        while let Some(next_url) = next_link.take() {
            if page_count >= clamped_pages {
                break;
            }
            page_count += 1;
            let resp = self.send_request("GET", &next_url, None, None).await?;
            Self::append_issues_from_body(&resp.body, &mut all_issues)?;
            next_link = resp.headers.get("link").and_then(|l| extract_next_link(l));
        }

        Ok(RemoteFetchOutcome::Fresh {
            data: all_issues,
            etag: first_etag,
            transport: GithubTransportKind::Rest,
            rate_limit: self.rate_limit.snapshot(),
        })
    }

    fn append_issues_from_body(body: &[u8], out: &mut Vec<RemoteIssueRecord>) -> GithubResult<()> {
        let items: Vec<Value> = serde_json::from_slice(body).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "Resposta JSON inválida ao listar issues via REST.",
            )
            .with_details(e.to_string())
        })?;
        for item in items {
            // Pull Requests retornados no endpoint /issues possuem a chave `pull_request`
            if item.get("pull_request").is_some() {
                continue;
            }
            if let Some(parsed) = parse_rest_or_gh_issue_json(&item) {
                out.push(parsed);
            }
        }
        Ok(())
    }

    /// Busca detalhe e comentários paginados de uma issue via REST.
    pub async fn fetch_issue_detail(
        &self,
        repo: &GithubRepoIdentity,
        issue_number: u64,
    ) -> GithubResult<RemoteIssueDetailBundle> {
        let issue_path = format!("/repos/{}/{}/issues/{issue_number}", repo.owner, repo.repo);
        let issue_resp = self.send_request("GET", &issue_path, None, None).await?;
        let issue_val: Value = serde_json::from_slice(&issue_resp.body).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "JSON inválido ao obter detalhe da issue via REST.",
            )
            .with_details(e.to_string())
        })?;
        let issue = parse_rest_or_gh_issue_json(&issue_val).ok_or_else(|| {
            GithubError::new(
                GithubErrorKind::NotFound,
                format!("Issue #{issue_number} não encontrada."),
            )
        })?;

        let mut comments: Vec<IssueComment> = Vec::new();
        let mut next_url: Option<String> = Some(format!(
            "/repos/{}/{}/issues/{issue_number}/comments?per_page=100",
            repo.owner, repo.repo
        ));
        let mut page_count = 0_u32;
        let max_comment_pages = 5_u32;

        while let Some(url_or_path) = next_url.take() {
            if page_count >= max_comment_pages {
                break;
            }
            page_count += 1;
            let resp = self.send_request("GET", &url_or_path, None, None).await?;
            let page_items: Vec<Value> = serde_json::from_slice(&resp.body).map_err(|e| {
                GithubError::new(
                    GithubErrorKind::Internal,
                    "JSON inválido ao listar comentários via REST.",
                )
                .with_details(e.to_string())
            })?;
            for c in &page_items {
                if let Some(parsed) = parse_issue_comment_json(c, issue_number) {
                    comments.push(parsed);
                }
            }
            next_url = resp.headers.get("link").and_then(|l| extract_next_link(l));
        }

        let comments_complete =
            next_url.is_none() && (comments.len() as u32) >= issue.comments_count;

        Ok(RemoteIssueDetailBundle {
            issue,
            comments,
            comments_complete,
            transport: GithubTransportKind::Rest,
        })
    }

    /// Lista labels do repositório via REST (`GET /repos/{owner}/{repo}/labels`).
    pub async fn fetch_repo_labels(
        &self,
        repo: &GithubRepoIdentity,
        cached_etag: Option<&str>,
    ) -> GithubResult<RemoteFetchOutcome<Vec<RepoLabel>>> {
        let path = format!("/repos/{}/{}/labels?per_page=100", repo.owner, repo.repo);
        let resp = self.send_request("GET", &path, cached_etag, None).await?;
        if resp.status == 304 {
            return Ok(RemoteFetchOutcome::NotModified {
                transport: GithubTransportKind::Rest,
                rate_limit: self.rate_limit.snapshot(),
            });
        }

        let raw: Vec<RestLabelWire> = serde_json::from_slice(&resp.body).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "JSON inválido ao listar labels via REST.",
            )
            .with_details(e.to_string())
        })?;
        let labels = raw
            .into_iter()
            .map(|l| to_repo_label(&l.name, l.color.as_deref(), l.description.as_deref()))
            .collect();

        Ok(RemoteFetchOutcome::Fresh {
            data: labels,
            etag: resp.headers.get("etag").cloned(),
            transport: GithubTransportKind::Rest,
            rate_limit: self.rate_limit.snapshot(),
        })
    }

    /// Cria uma nova label no repositório (`POST /repos/{owner}/{repo}/labels`).
    pub async fn create_repo_label(
        &self,
        repo: &GithubRepoIdentity,
        name: &str,
        color_hex: &str,
        description: &str,
    ) -> GithubResult<RepoLabel> {
        let path = format!("/repos/{}/{}/labels", repo.owner, repo.repo);
        let clean_color = color_hex.trim_start_matches('#');
        let payload = json!({
            "name": name,
            "color": clean_color,
            "description": description,
        });
        let resp = self
            .send_request("POST", &path, None, Some(&payload))
            .await?;
        let parsed: Option<RestLabelWire> = serde_json::from_slice(&resp.body).ok();
        Ok(match parsed {
            Some(w) => to_repo_label(&w.name, w.color.as_deref(), w.description.as_deref()),
            None => to_repo_label(name, Some(clean_color), Some(description)),
        })
    }

    /// Cria uma nova issue (`POST /repos/{owner}/{repo}/issues`).
    pub async fn create_issue(
        &self,
        repo: &GithubRepoIdentity,
        title: &str,
        body: &str,
        labels: &[String],
        assignees: &[String],
    ) -> GithubResult<RemoteIssueRecord> {
        let path = format!("/repos/{}/{}/issues", repo.owner, repo.repo);
        let payload = json!({
            "title": title,
            "body": body,
            "labels": labels,
            "assignees": assignees,
        });
        let resp = self
            .send_request("POST", &path, None, Some(&payload))
            .await?;
        let val: Value = serde_json::from_slice(&resp.body).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "JSON inválido ao criar issue via REST.",
            )
            .with_details(e.to_string())
        })?;
        parse_rest_or_gh_issue_json(&val).ok_or_else(|| {
            GithubError::new(
                GithubErrorKind::Internal,
                "Resposta de criação de issue via REST está incompleta.",
            )
        })
    }

    /// Substitui atomicamente as labels de uma issue (`PUT /repos/{owner}/{repo}/issues/{number}/labels`).
    pub async fn replace_issue_labels(
        &self,
        repo: &GithubRepoIdentity,
        issue_number: u64,
        labels: &[String],
    ) -> GithubResult<Vec<RepoLabel>> {
        let path = format!(
            "/repos/{}/{}/issues/{issue_number}/labels",
            repo.owner, repo.repo
        );
        let payload = json!({ "labels": labels });
        let resp = self
            .send_request("PUT", &path, None, Some(&payload))
            .await?;
        let raw: Vec<RestLabelWire> = serde_json::from_slice(&resp.body).unwrap_or_else(|_| {
            labels
                .iter()
                .map(|n| RestLabelWire {
                    name: n.clone(),
                    color: None,
                    description: None,
                })
                .collect()
        });
        Ok(raw
            .into_iter()
            .map(|l| to_repo_label(&l.name, l.color.as_deref(), l.description.as_deref()))
            .collect())
    }

    async fn send_request(
        &self,
        method: &str,
        path_or_url: &str,
        etag: Option<&str>,
        json_body: Option<&Value>,
    ) -> GithubResult<RawRestResponse> {
        let now_ms = current_epoch_ms();
        if let Some(until_ms) = self.rate_limit.active_cooldown_until(now_ms) {
            return Err(GithubError::new(
                GithubErrorKind::RateLimited,
                "Limite de requisições da API do GitHub atingido. Aguardando janela de reset.",
            )
            .with_retry_after(Some(until_ms)));
        }

        let target_url = self.resolve_validated_url(path_or_url)?;
        let req_method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| {
            GithubError::new(GithubErrorKind::Internal, "Método HTTP inválido.")
                .with_details(e.to_string())
        })?;

        let mut builder = self
            .http
            .request(req_method, target_url.as_str())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
            .header("User-Agent", "MaxTerminal/0.1 (+https://maxvue.dev)");

        if let Some(secret) = self.token_store.resolve_secret() {
            builder = builder.header("Authorization", format!("Bearer {secret}"));
        }
        if let Some(tag) = etag {
            if !tag.trim().is_empty() {
                builder = builder.header("If-None-Match", tag);
            }
        }
        if let Some(body) = json_body {
            builder = builder.json(body);
        }

        let resp = builder.send().await.map_err(|e| {
            GithubError::new(
                GithubErrorKind::NetworkUnavailable,
                "Falha de comunicação com a API do GitHub.",
            )
            .with_details(e.to_string())
        })?;

        let status = resp.status().as_u16();
        let mut headers = HashMap::new();
        for (k, v) in resp.headers() {
            if let Ok(val_str) = v.to_str() {
                headers.insert(k.as_str().to_ascii_lowercase(), val_str.to_string());
            }
        }

        let updated_rl = self
            .rate_limit
            .update_from_headers(&headers, status, current_epoch_ms());

        let body = resp.bytes().await.map_err(|e| {
            GithubError::new(
                GithubErrorKind::NetworkUnavailable,
                "Falha ao ler corpo da resposta da API do GitHub.",
            )
            .with_details(e.to_string())
        })?;

        if status == 304 || (200..300).contains(&status) {
            return Ok(RawRestResponse {
                status,
                headers,
                body: body.to_vec(),
            });
        }

        let retry_after = updated_rl.and_then(|r| r.retry_after_until_ms);
        let body_text = String::from_utf8_lossy(&body);
        let kind = match status {
            401 => GithubErrorKind::AuthInvalid,
            403 if retry_after.is_some()
                || body_text.to_ascii_lowercase().contains("rate limit") =>
            {
                GithubErrorKind::RateLimited
            }
            403 => GithubErrorKind::PermissionDenied,
            404 => GithubErrorKind::NotFound,
            422 => GithubErrorKind::ValidationFailed,
            429 => GithubErrorKind::RateLimited,
            _ => GithubErrorKind::NetworkUnavailable,
        };

        let message = match kind {
            GithubErrorKind::AuthInvalid => "Token do GitHub inválido ou expirado (HTTP 401).",
            GithubErrorKind::RateLimited => {
                "Limite de taxa da API do GitHub atingido (HTTP 403/429)."
            }
            GithubErrorKind::PermissionDenied => {
                "Permissão negada no repositório do GitHub (HTTP 403)."
            }
            GithubErrorKind::NotFound => "Recurso não encontrado no GitHub (HTTP 404).",
            GithubErrorKind::ValidationFailed => {
                "Dados recusados pela validação do GitHub (HTTP 422)."
            }
            _ => "Erro retornado pela API REST do GitHub.",
        };

        Err(GithubError::new(kind, message)
            .with_details(body_text.into_owned())
            .with_retry_after(retry_after))
    }

    fn resolve_validated_url(&self, path_or_url: &str) -> GithubResult<Url> {
        let base = Url::parse(&self.base_url).map_err(|e| {
            GithubError::new(GithubErrorKind::Internal, "URL base da API inválida.")
                .with_details(e.to_string())
        })?;

        if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
            let parsed = Url::parse(path_or_url).map_err(|e| {
                GithubError::new(GithubErrorKind::Internal, "URL de paginação inválida.")
                    .with_details(e.to_string())
            })?;
            // Anti-SSRF: o host e a porta da URL de paginação devem coincidir exatamente com base_url
            if parsed.host_str() != base.host_str() || parsed.port() != base.port() {
                return Err(GithubError::new(
                    GithubErrorKind::PermissionDenied,
                    "URL de paginação bloqueada: host diferente do endpoint configurado.",
                ));
            }
            return Ok(parsed);
        }

        let normalized_path = if path_or_url.starts_with('/') {
            path_or_url.to_string()
        } else {
            format!("/{path_or_url}")
        };
        Url::parse(&format!("{}{normalized_path}", self.base_url)).map_err(|e| {
            GithubError::new(GithubErrorKind::Internal, "Caminho REST inválido.")
                .with_details(e.to_string())
        })
    }
}

struct RawRestResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct RestLabelWire {
    name: String,
    color: Option<String>,
    description: Option<String>,
}

/// Extrai a URL `rel="next"` de um cabeçalho HTTP `Link` do GitHub.
#[must_use]
pub fn extract_next_link(link_header: &str) -> Option<String> {
    for part in link_header.split(',') {
        let segments: Vec<&str> = part.split(';').map(str::trim).collect();
        if segments.len() < 2 {
            continue;
        }
        let is_next = segments[1..]
            .iter()
            .any(|attr| *attr == "rel=\"next\"" || *attr == "rel=next");
        if is_next {
            let url_part = segments[0].trim();
            if url_part.starts_with('<') && url_part.ends_with('>') && url_part.len() > 2 {
                return Some(url_part[1..url_part.len() - 1].to_string());
            }
        }
    }
    None
}

fn current_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}
