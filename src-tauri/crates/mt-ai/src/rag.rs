//! Consulta à documentação indexada da stack via `/v1/rags/{slug}/search` (`RagService`).

use crate::client::http::{Conditional, GatewayClient, Op};
use crate::config::{MAX_RAG_TOP_K, RagSettings};
use crate::context::truncate_utf8_head;
use crate::error::{ai_error, ai_error_with_details, codes};
use crate::ids::AiRequestId;
use mt_core::MtResult;
use mt_core::ids::PaneId;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cmp::Ordering;
use std::sync::LazyLock;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use url::Url;

/// Limite máximo de caracteres na consulta RAG.
pub const MAX_RAG_QUERY_CHARS: usize = 2000;
/// Limite máximo em bytes do trecho `content` de cada resultado RAG (`2 KiB`).
pub const MAX_RAG_CONTENT_BYTES: usize = 2048;

#[allow(clippy::expect_used)]
static SLUG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9_-]{0,63}$").expect("regex slug válida"));

/// Base de conhecimento RAG disponível no gateway.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RagSlug {
    /// Identificador do slug (ex.: `"laravel"`, `"vue"`, `"tauri"`, `"rust"`).
    pub slug: String,
    /// Título amigável da coleção.
    pub title: Option<String>,
    /// Quantidade de documentos indexados.
    pub documents: Option<u32>,
}

/// Requisição de busca semântica/lexical em `/v1/rags/{slug}/search`.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RagSearchRequest {
    /// Identificador único da requisição.
    pub request_id: AiRequestId,
    /// Slug da documentação alvo.
    pub slug: String,
    /// Consulta em português do Brasil.
    pub query: String,
    /// Quantidade opcional de resultados (`1..=20`).
    pub top_k: Option<u8>,
    /// Painel associado à consulta.
    pub pane_id: Option<PaneId>,
}

/// Trecho individual retornado pela busca RAG.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RagResult {
    /// Identificador opcional do documento/chunk.
    pub id: Option<String>,
    /// Título do trecho ou documento.
    pub title: Option<String>,
    /// Conteúdo textual sanitizado e truncado em 2 KiB.
    pub content: String,
    /// Relevância opcional retornada pelo gateway.
    pub score: Option<f32>,
    /// Caminho ou fonte de origem do documento.
    pub source: Option<String>,
    /// URL externa segura (`http://` ou `https://`).
    pub url: Option<String>,
}

/// Resposta consolidada de `ai_rag_search`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RagSearchResponse {
    /// Slug consultado.
    pub slug: String,
    /// Query executada (sanitizada).
    pub query: String,
    /// Lista de resultados ordenados e sanitizados.
    pub results: Vec<RagResult>,
    /// Latência da busca em milissegundos.
    pub latency_ms: u32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RagSlugsWire {
    Wrapped { rags: Vec<RagSlug> },
    SlugsWrapped { slugs: Vec<RagSlug> },
    List(Vec<RagSlug>),
}

impl RagSlugsWire {
    fn into_vec(self) -> Vec<RagSlug> {
        match self {
            Self::Wrapped { rags } => rags,
            Self::SlugsWrapped { slugs } => slugs,
            Self::List(list) => list,
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RagSearchWire {
    Wrapped { results: Vec<RagResult> },
    ItemsWrapped { items: Vec<RagResult> },
    List(Vec<RagResult>),
}

impl RagSearchWire {
    fn into_vec(self) -> Vec<RagResult> {
        match self {
            Self::Wrapped { results } => results,
            Self::ItemsWrapped { items } => items,
            Self::List(list) => list,
        }
    }
}

/// Serviço de consulta às coleções RAG do `MaxAiManager` (`CA-25`, `CA-26`).
#[derive(Clone, Debug, Default)]
pub struct RagService;

impl RagService {
    /// Valida estritamente o slug contra `^[a-z0-9][a-z0-9_-]{0,63}$` antes de qualquer chamada HTTP (`CA-26`).
    pub fn validate_slug(slug: &str) -> MtResult<()> {
        if !SLUG_RE.is_match(slug) {
            return Err(ai_error_with_details(
                codes::AI_INVALID_SLUG,
                format!(
                    "O slug de documentação '{slug}' é inválido. Use apenas letras minúsculas, números, '-' ou '_'."
                ),
                json!({ "slug": slug }),
            ));
        }
        Ok(())
    }

    /// Resolve o slug padrão preferindo `settings.last_slug` e depois o mapeamento da stack do projeto (`CA-25`).
    #[must_use]
    pub fn default_slug(settings: &RagSettings, stack: Option<&str>) -> Option<String> {
        if let Some(last) = &settings.last_slug
            && Self::validate_slug(last).is_ok()
        {
            return Some(last.clone());
        }
        if let Some(st) = stack {
            let key = st.trim().to_ascii_lowercase();
            if let Some(mapped) = settings.stack_slugs.get(&key)
                && Self::validate_slug(mapped).is_ok()
            {
                return Some(mapped.clone());
            }
            if Self::validate_slug(&key).is_ok() {
                return Some(key);
            }
        }
        None
    }

    /// Lista os slugs disponíveis em `/v1/rags`, retornando os slugs do mapeamento local caso o endpoint não exista (`404`).
    pub async fn list(
        &self,
        client: &GatewayClient,
        settings: &RagSettings,
    ) -> MtResult<Vec<RagSlug>> {
        let path = client.dialect().rag_list_path();
        match client.get_json::<RagSlugsWire>(path, Op::Rag, None).await {
            Ok(Conditional::Fresh { value, .. }) => {
                let mut out = Vec::new();
                for mut item in value.into_vec() {
                    item.slug = mt_db::sanitizer::sanitize(item.slug.trim()).into_owned();
                    if Self::validate_slug(&item.slug).is_ok() {
                        item.title = item
                            .title
                            .as_ref()
                            .map(|t| mt_db::sanitizer::sanitize(t.trim()).into_owned());
                        out.push(item);
                    }
                }
                Ok(out)
            }
            Ok(Conditional::NotModified) => Ok(fallback_slugs_from_settings(settings)),
            Err(err) => {
                if err.code == codes::AI_HTTP_STATUS {
                    return Ok(fallback_slugs_from_settings(settings));
                }
                Err(err)
            }
        }
    }

    /// Executa a busca em `/v1/rags/{slug}/search` validando slug, codificando path segments e sanitizando resultados.
    pub async fn search(
        &self,
        client: &GatewayClient,
        settings: &RagSettings,
        req: &RagSearchRequest,
        cancel: &CancellationToken,
    ) -> MtResult<RagSearchResponse> {
        let slug = req.slug.trim();
        Self::validate_slug(slug)?;

        let query_trimmed = req.query.trim();
        if query_trimmed.is_empty() {
            return Err(ai_error(
                codes::AI_EMPTY_COMMAND,
                "Informe uma consulta para pesquisar na documentação.",
            ));
        }

        let sanitized_query: String = mt_db::sanitizer::sanitize(query_trimmed)
            .chars()
            .take(MAX_RAG_QUERY_CHARS)
            .collect();
        let top_k = req.top_k.unwrap_or(settings.top_k).clamp(1, MAX_RAG_TOP_K);

        // Constrói o caminho codificado via Url::path_segments_mut()
        let mut dummy_url = Url::parse("http://localhost")
            .map_err(|_| ai_error(codes::AI_INVALID_SLUG, "Slug inválido."))?;
        if let Ok(mut segs) = dummy_url.path_segments_mut() {
            segs.push("v1").push("rags").push(slug).push("search");
        }
        let encoded_path = dummy_url.path().to_string();

        let body = json!({
            "query": sanitized_query,
            "top_k": top_k,
        });

        let start = Instant::now();
        let wire: RagSearchWire = match client
            .post_json(&encoded_path, &body, Op::Rag, &[], cancel)
            .await
        {
            Ok(w) => w,
            Err(err) => {
                let is_404 = err
                    .details
                    .as_ref()
                    .and_then(|d| d.get("status"))
                    .and_then(serde_json::Value::as_u64)
                    == Some(404);
                if err.code == codes::AI_HTTP_STATUS && is_404 {
                    return Err(ai_error_with_details(
                        codes::AI_RAG_NOT_FOUND,
                        format!("Base de conhecimento '{slug}' não encontrada."),
                        json!({ "slug": slug }),
                    ));
                }
                return Err(err);
            }
        };

        let latency_ms = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
        let mut results: Vec<RagResult> = wire
            .into_vec()
            .into_iter()
            .take(usize::from(top_k))
            .map(sanitize_rag_result)
            .collect();

        let all_finite_scores = results.iter().all(|r| r.score.is_some_and(f32::is_finite));
        if all_finite_scores {
            results.sort_by(|a, b| {
                let sa = a.score.unwrap_or(0.0);
                let sb = b.score.unwrap_or(0.0);
                sb.partial_cmp(&sa).unwrap_or(Ordering::Equal)
            });
        }

        Ok(RagSearchResponse {
            slug: slug.to_string(),
            query: sanitized_query,
            results,
            latency_ms,
        })
    }
}

fn sanitize_rag_result(mut item: RagResult) -> RagResult {
    item.id = item
        .id
        .as_ref()
        .map(|s| mt_db::sanitizer::sanitize(s.trim()).into_owned());
    item.title = item
        .title
        .as_ref()
        .map(|t| mt_db::sanitizer::sanitize(t.trim()).into_owned())
        .filter(|t| !t.is_empty());
    let sanitized_content = mt_db::sanitizer::sanitize(item.content.trim()).into_owned();
    item.content = truncate_utf8_head(&sanitized_content, MAX_RAG_CONTENT_BYTES);
    item.source = item
        .source
        .as_ref()
        .map(|s| mt_db::sanitizer::sanitize(s.trim()).into_owned())
        .filter(|s| !s.is_empty());
    item.url = item.url.as_deref().and_then(validate_http_https_url);
    item
}

fn validate_http_https_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let parsed = Url::parse(trimmed).ok()?;
    if parsed.scheme() == "http" || parsed.scheme() == "https" {
        Some(mt_db::sanitizer::sanitize(trimmed).into_owned())
    } else {
        None
    }
}

fn fallback_slugs_from_settings(settings: &RagSettings) -> Vec<RagSlug> {
    let mut slugs: Vec<String> = settings.stack_slugs.values().cloned().collect();
    slugs.sort();
    slugs.dedup();
    slugs
        .into_iter()
        .filter(|s| RagService::validate_slug(s).is_ok())
        .map(|slug| RagSlug {
            title: Some(slug.clone()),
            slug,
            documents: None,
        })
        .collect()
}
