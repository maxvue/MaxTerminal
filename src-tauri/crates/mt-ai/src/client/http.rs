//! Cliente HTTP/1.1 + SSE assíncrono para comunicação com o gateway `MaxAiManager`.

use crate::client::dialect::{
    CapabilityState, ChatRequest, ChatResponse, GatewayCapabilities, GatewayDialect,
    OpenAiCompatDialect,
};
use crate::client::sse::{MAX_RESPONSE_BYTES, consume_sse_stream};
use crate::config::{AiSettings, TimeoutSettings};
use crate::error::{ai_error, ai_error_with_details, codes, gateway_unavailable, timeout_error};
use mt_core::MtResult;
use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, ReadBuf};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;
use url::Url;

/// Header canônico obrigatório enviado em todas as requisições ao `MaxAiManager` (`CA-05`).
pub const AGENT_HEADER: (&str, &str) = ("X-Agent-Name", "MaxTerminal");

/// Operação lógica executada pelo `GatewayClient` para seleção de timeout e política de retry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Sondagem de saúde (`/health`).
    Health,
    /// Descoberta de harnesses e modelos (`/v1/harnesses`).
    Catalog,
    /// Sugestão de comando NL→Shell (`/v1/chat/completions`).
    Suggest,
    /// Diagnóstico rápido inline (`/v1/chat/completions`).
    Diagnosis,
    /// Busca ou listagem RAG (`/v1/rags*`).
    Rag,
    /// Geração assistida estruturada (`/v1/chat/completions`).
    Structured,
}

impl Op {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Health => "health",
            Self::Catalog => "catalog",
            Self::Suggest => "suggest",
            Self::Diagnosis => "diagnosis",
            Self::Rag => "rag",
            Self::Structured => "structured",
        }
    }

    const fn is_retryable_read(self) -> bool {
        matches!(self, Self::Health | Self::Catalog)
    }

    const fn timeout_ms(self, t: &TimeoutSettings) -> u32 {
        match self {
            Self::Health => t.connect_ms,
            Self::Catalog => t.catalog_ms,
            Self::Suggest => t.suggest_ms,
            Self::Diagnosis => t.diagnosis_ms,
            Self::Rag => t.rag_ms,
            Self::Structured => t.structured_ms,
        }
    }
}

/// Resposta condicional com suporte a `ETag` / `304 Not Modified`.
#[derive(Clone, Debug)]
pub enum Conditional<T> {
    /// Conteúdo novo (`200 OK`) acompanhado de `ETag` opcional.
    Fresh {
        /// Valor desserializado.
        value: T,
        /// Valor do cabeçalho `ETag` se presente.
        etag: Option<String>,
    },
    /// Conteúdo inalterado (`304 Not Modified`).
    NotModified,
}

/// Status operacional do gateway `MaxAiManager`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum GatewayStatus {
    /// Gateway acessível e saudável.
    Online,
    /// Gateway acessível mas com capacidades parciais.
    Degraded,
    /// Gateway inacessível na URL configurada.
    Offline,
    /// Integração desativada pelo usuário (`enabled = false`).
    Disabled,
    /// URL base ainda não configurada (`base_url = None`).
    Unconfigured,
}

/// Snapshot completo de saúde e capacidades do gateway `MaxAiManager`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayHealth {
    /// Status geral do gateway.
    pub status: GatewayStatus,
    /// Versão reportada pelo gateway.
    pub version: Option<String>,
    /// Latência do último `/health` em milissegundos.
    pub latency_ms: Option<u32>,
    /// URL base configurada.
    pub base_url: Option<String>,
    /// Timestamp ISO-8601 UTC da verificação.
    pub checked_at: String,
    /// Revisão das configurações vigente durante a verificação.
    pub config_revision: u32,
    /// Capacidades validadas do gateway.
    pub capabilities: GatewayCapabilities,
}

#[derive(Clone, Debug)]
struct GatewayConfigSnapshot {
    revision: u32,
    enabled: bool,
    base_url: Option<String>,
    timeouts: TimeoutSettings,
    gateway_key: String,
}

impl GatewayConfigSnapshot {
    fn from_settings(settings: &AiSettings, dialect_id: &str) -> Self {
        Self {
            revision: settings.revision,
            enabled: settings.enabled,
            base_url: settings
                .base_url
                .as_ref()
                .map(|s| s.trim().trim_end_matches('/').to_string()),
            timeouts: settings.timeouts.clone(),
            gateway_key: settings.gateway_key(dialect_id),
        }
    }
}

/// Cliente HTTP/SSE thread-safe para o gateway `MaxAiManager`.
pub struct GatewayClient {
    snapshot: RwLock<Arc<GatewayConfigSnapshot>>,
    dialect: Arc<dyn GatewayDialect>,
}

impl GatewayClient {
    /// Cria um novo `GatewayClient` usando o `OpenAiCompatDialect` padrão.
    pub fn new(settings: &AiSettings) -> MtResult<Self> {
        Self::with_dialect(settings, Arc::new(OpenAiCompatDialect::default()))
    }

    /// Cria um novo `GatewayClient` com um dialeto customizado.
    pub fn with_dialect(settings: &AiSettings, dialect: Arc<dyn GatewayDialect>) -> MtResult<Self> {
        settings.validate()?;
        let snap = GatewayConfigSnapshot::from_settings(settings, dialect.id());
        Ok(Self {
            snapshot: RwLock::new(Arc::new(snap)),
            dialect,
        })
    }

    /// Reconfigura atomicamente `base_url`, `enabled` e `timeouts` sem reiniciar o aplicativo (`CA-06`).
    pub fn reconfigure(&self, settings: &AiSettings) -> MtResult<()> {
        settings.validate()?;
        let next = GatewayConfigSnapshot::from_settings(settings, self.dialect.id());
        *self.snapshot.write() = Arc::new(next);
        Ok(())
    }

    /// Retorna a chave canônica do gateway atual (`gateway_key`).
    #[must_use]
    pub fn gateway_key(&self) -> String {
        self.snapshot.read().gateway_key.clone()
    }

    /// Retorna a revisão de configuração ativa no cliente.
    #[must_use]
    pub fn config_revision(&self) -> u32 {
        self.snapshot.read().revision
    }

    /// Retorna as capacidades do dialeto ativo.
    #[must_use]
    pub fn capabilities(&self) -> GatewayCapabilities {
        self.dialect.capabilities()
    }

    /// Retorna referência ao dialeto ativo.
    #[must_use]
    pub fn dialect(&self) -> &Arc<dyn GatewayDialect> {
        &self.dialect
    }

    /// Executa a verificação de saúde (`/health`) e retorna `GatewayHealth` sem propagar erro de rede.
    pub async fn health(&self) -> MtResult<GatewayHealth> {
        let snap = self.snapshot.read().clone();
        let checked_at = now_iso8601();

        if !snap.enabled {
            return Ok(GatewayHealth {
                status: GatewayStatus::Disabled,
                version: None,
                latency_ms: None,
                base_url: snap.base_url.clone(),
                checked_at,
                config_revision: snap.revision,
                capabilities: GatewayCapabilities::unconfigured(
                    "A integração com o MaxAiManager está desativada.",
                ),
            });
        }

        let Some(base_url) = snap.base_url.clone() else {
            return Ok(GatewayHealth {
                status: GatewayStatus::Unconfigured,
                version: None,
                latency_ms: None,
                base_url: None,
                checked_at,
                config_revision: snap.revision,
                capabilities: GatewayCapabilities::unconfigured(
                    "Configure a URL base do MaxAiManager em Preferências > IA.",
                ),
            });
        };

        let start = Instant::now();
        let cancel = CancellationToken::new();
        let path = self.dialect.health_path();
        match self
            .send_with_retry(&snap, "GET", path, None, &[], Op::Health, &cancel)
            .await
        {
            Ok(resp) if (200..300).contains(&resp.status) => {
                let latency_ms = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
                let parsed: Option<Value> = serde_json::from_slice(&resp.body).ok();
                let version = parsed
                    .as_ref()
                    .and_then(|v| v.get("version"))
                    .and_then(|v| v.as_str())
                    .map(|s| mt_db::sanitizer::sanitize(s).into_owned());
                let caps = self.dialect.capabilities();
                let is_degraded = matches!(caps.generation, CapabilityState::Unavailable { .. });
                Ok(GatewayHealth {
                    status: if is_degraded {
                        GatewayStatus::Degraded
                    } else {
                        GatewayStatus::Online
                    },
                    version,
                    latency_ms: Some(latency_ms),
                    base_url: Some(base_url),
                    checked_at,
                    config_revision: snap.revision,
                    capabilities: caps,
                })
            }
            _ => Ok(GatewayHealth {
                status: GatewayStatus::Offline,
                version: None,
                latency_ms: None,
                base_url: Some(base_url),
                checked_at,
                config_revision: snap.revision,
                capabilities: GatewayCapabilities::unconfigured(
                    "Não foi possível conectar ao MaxAiManager.",
                ),
            }),
        }
    }

    /// Realiza um `GET` JSON com suporte opcional a `If-None-Match` (`ETag`).
    pub async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        op: Op,
        etag: Option<&str>,
    ) -> MtResult<Conditional<T>> {
        let snap = self.require_ready_snapshot()?;
        let mut headers = Vec::new();
        if let Some(tag) = etag {
            headers.push(("If-None-Match".to_string(), tag.to_string()));
        }
        let cancel = CancellationToken::new();
        let resp = self
            .send_with_retry(&snap, "GET", path, None, &headers, op, &cancel)
            .await?;

        if resp.status == 304 {
            return Ok(Conditional::NotModified);
        }

        Self::ensure_2xx(resp.status, &resp.body)?;
        let value: T = serde_json::from_slice(&resp.body).map_err(|e| {
            ai_error(
                codes::AI_BAD_RESPONSE,
                format!("Resposta JSON inválida do MaxAiManager: {e}"),
            )
        })?;
        let new_etag = resp.headers.get("etag").cloned();
        Ok(Conditional::Fresh {
            value,
            etag: new_etag,
        })
    }

    /// Realiza um `POST` JSON sem retry automático.
    pub async fn post_json<B: Serialize + Sync, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        op: Op,
        extra_headers: &[(String, String)],
        cancel: &CancellationToken,
    ) -> MtResult<T> {
        let snap = self.require_ready_snapshot()?;
        let payload = serde_json::to_vec(body).map_err(|e| {
            ai_error(
                codes::AI_BAD_RESPONSE,
                format!("Falha ao serializar requisição JSON: {e}"),
            )
        })?;
        let resp = self
            .send_once(
                &snap,
                "POST",
                path,
                Some(&payload),
                extra_headers,
                op,
                cancel,
            )
            .await?;
        Self::ensure_2xx(resp.status, &resp.body)?;
        serde_json::from_slice(&resp.body).map_err(|e| {
            ai_error(
                codes::AI_BAD_RESPONSE,
                format!("Resposta JSON inválida do MaxAiManager: {e}"),
            )
        })
    }

    /// Executa uma chamada de chat completions com suporte a streaming SSE condicional e fallback JSON.
    pub async fn chat(
        &self,
        req: &ChatRequest,
        op: Op,
        cancel: &CancellationToken,
        on_delta: &mut (dyn FnMut(&str) + Send),
    ) -> MtResult<ChatResponse> {
        let snap = self.require_ready_snapshot()?;
        let body_val = self.dialect.build_chat_body(req);
        let payload = serde_json::to_vec(&body_val).map_err(|e| {
            ai_error(
                codes::AI_BAD_RESPONSE,
                format!("Falha ao serializar corpo do chat: {e}"),
            )
        })?;
        let extra = self.dialect.extra_headers(Some(&req.selection));
        let path = self.dialect.chat_path();
        let timeout_ms = op.timeout_ms(&snap.timeouts);
        let deadline = Duration::from_millis(u64::from(timeout_ms));

        let fut = self.execute_chat_http(&snap, path, &payload, &extra, op, cancel, on_delta);
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                Err(ai_error(codes::AI_CANCELLED, "Operação cancelada pelo usuário."))
            }
            res = tokio::time::timeout(deadline, fut) => {
                match res {
                    Ok(inner) => inner,
                    Err(_) => Err(timeout_error(timeout_ms)),
                }
            }
        }
    }

    fn require_ready_snapshot(&self) -> MtResult<Arc<GatewayConfigSnapshot>> {
        let snap = self.snapshot.read().clone();
        if !snap.enabled {
            return Err(ai_error(
                codes::AI_DISABLED,
                "A integração com o MaxAiManager está desativada nas preferências.",
            ));
        }
        if snap.base_url.is_none() {
            return Err(ai_error(
                codes::AI_NOT_CONFIGURED,
                "Configure a URL base do MaxAiManager em Preferências > IA.",
            ));
        }
        Ok(snap)
    }

    fn ensure_2xx(status: u16, body: &[u8]) -> MtResult<()> {
        if (200..300).contains(&status) {
            return Ok(());
        }
        Err(Self::map_http_status(status, body))
    }

    fn map_http_status(status: u16, body: &[u8]) -> mt_core::MtError {
        let snippet = String::from_utf8_lossy(body);
        let sanitized_snippet = mt_db::sanitizer::sanitize(snippet.trim()).into_owned();
        match status {
            401 | 403 => ai_error_with_details(
                codes::AI_AUTH_REQUIRED,
                "O MaxAiManager recusou a requisição por falta de autorização.",
                json!({ "status": status }),
            ),
            429 => ai_error_with_details(
                codes::AI_RATE_LIMITED,
                "Limite de requisições atingido no MaxAiManager. Aguarde alguns segundos.",
                json!({ "status": status }),
            ),
            _ => ai_error_with_details(
                codes::AI_HTTP_STATUS,
                if sanitized_snippet.is_empty() {
                    format!("O MaxAiManager retornou status HTTP {status}.")
                } else {
                    format!("O MaxAiManager retornou status HTTP {status}: {sanitized_snippet}")
                },
                json!({ "status": status }),
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_with_retry(
        &self,
        snap: &GatewayConfigSnapshot,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        extra_headers: &[(String, String)],
        op: Op,
        cancel: &CancellationToken,
    ) -> MtResult<RawHttpResponse> {
        let backoffs = [Duration::from_millis(200), Duration::from_millis(600)];
        let max_attempts = if op.is_retryable_read() { 3 } else { 1 };
        let mut last_err = None;

        for attempt in 0..max_attempts {
            if attempt > 0 {
                let delay = backoffs[attempt - 1];
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => {
                        return Err(ai_error(codes::AI_CANCELLED, "Operação cancelada pelo usuário."));
                    }
                    () = tokio::time::sleep(delay) => {}
                }
            }

            match self
                .send_once(snap, method, path, body, extra_headers, op, cancel)
                .await
            {
                Ok(resp) => {
                    if op.is_retryable_read()
                        && matches!(resp.status, 502..=504)
                        && attempt + 1 < max_attempts
                    {
                        last_err = Some(Self::map_http_status(resp.status, &resp.body));
                        continue;
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    let retryable_err =
                        err.code == codes::AI_GATEWAY_UNAVAILABLE || err.code == codes::AI_TIMEOUT;
                    if op.is_retryable_read() && retryable_err && attempt + 1 < max_attempts {
                        last_err = Some(err);
                        continue;
                    }
                    return Err(err);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            gateway_unavailable(snap.base_url.as_deref().unwrap_or("unconfigured"))
        }))
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_once(
        &self,
        snap: &GatewayConfigSnapshot,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        extra_headers: &[(String, String)],
        op: Op,
        cancel: &CancellationToken,
    ) -> MtResult<RawHttpResponse> {
        let timeout_ms = op.timeout_ms(&snap.timeouts);
        let deadline = Duration::from_millis(u64::from(timeout_ms));
        let fut = self.send_raw(snap, method, path, body, extra_headers, op);

        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                Err(ai_error(codes::AI_CANCELLED, "Operação cancelada pelo usuário."))
            }
            res = tokio::time::timeout(deadline, fut) => {
                match res {
                    Ok(inner) => inner,
                    Err(_) => Err(timeout_error(timeout_ms)),
                }
            }
        }
    }

    async fn send_raw(
        &self,
        snap: &GatewayConfigSnapshot,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
        extra_headers: &[(String, String)],
        op: Op,
    ) -> MtResult<RawHttpResponse> {
        let base_url = snap.base_url.as_deref().unwrap_or("");
        let target = resolve_url(base_url, path)?;
        let connect_timeout = Duration::from_millis(u64::from(snap.timeouts.connect_ms));
        let start = Instant::now();

        let stream = match tokio::time::timeout(
            connect_timeout,
            TcpStream::connect((target.host.as_str(), target.port)),
        )
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(_)) => return Err(gateway_unavailable(base_url)),
            Err(_) => return Err(timeout_error(snap.timeouts.connect_ms)),
        };

        let _ = stream.set_nodelay(true);
        let (read_half, mut write_half) = stream.into_split();

        let req_bytes = build_http_request(
            method,
            &target.request_path,
            &target.host_header,
            body,
            extra_headers,
        );
        write_half
            .write_all(&req_bytes)
            .await
            .map_err(|_| gateway_unavailable(base_url))?;
        write_half
            .flush()
            .await
            .map_err(|_| gateway_unavailable(base_url))?;

        let mut reader = BufReader::new(read_half);
        let head = read_http_head(&mut reader, base_url).await?;

        if (300..400).contains(&head.status) && head.status != 304 {
            return Err(ai_error_with_details(
                codes::AI_HTTP_STATUS,
                format!(
                    "Redirecionamento HTTP ({}) não permitido por política de segurança.",
                    head.status
                ),
                json!({ "status": head.status }),
            ));
        }

        let resp_body = if head.status == 204 || head.status == 304 {
            Vec::new()
        } else {
            read_http_body(&mut reader, &head.headers).await?
        };

        let latency_ms = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
        tracing::debug!(
            target: "mt_ai::http",
            op = op.as_str(),
            status = head.status,
            latency_ms = latency_ms,
            response_bytes = resp_body.len(),
            "Requisição HTTP concluída"
        );

        Ok(RawHttpResponse {
            status: head.status,
            headers: head.headers,
            body: resp_body,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_chat_http(
        &self,
        snap: &GatewayConfigSnapshot,
        path: &str,
        payload: &[u8],
        extra_headers: &[(String, String)],
        op: Op,
        cancel: &CancellationToken,
        on_delta: &mut (dyn FnMut(&str) + Send),
    ) -> MtResult<ChatResponse> {
        let base_url = snap.base_url.as_deref().unwrap_or("");
        let target = resolve_url(base_url, path)?;
        let connect_timeout = Duration::from_millis(u64::from(snap.timeouts.connect_ms));
        let start = Instant::now();

        let stream = match tokio::time::timeout(
            connect_timeout,
            TcpStream::connect((target.host.as_str(), target.port)),
        )
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(_)) => return Err(gateway_unavailable(base_url)),
            Err(_) => return Err(timeout_error(snap.timeouts.connect_ms)),
        };

        let _ = stream.set_nodelay(true);
        let (read_half, mut write_half) = stream.into_split();

        let req_bytes = build_http_request(
            "POST",
            &target.request_path,
            &target.host_header,
            Some(payload),
            extra_headers,
        );
        write_half
            .write_all(&req_bytes)
            .await
            .map_err(|_| gateway_unavailable(base_url))?;
        write_half
            .flush()
            .await
            .map_err(|_| gateway_unavailable(base_url))?;

        let mut reader = BufReader::new(read_half);
        let head = read_http_head(&mut reader, base_url).await?;

        if !(200..300).contains(&head.status) {
            let err_body = read_http_body(&mut reader, &head.headers)
                .await
                .unwrap_or_default();
            return Err(Self::map_http_status(head.status, &err_body));
        }

        let content_type = head
            .headers
            .get("content-type")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let is_chunked = head
            .headers
            .get("transfer-encoding")
            .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));

        let chat_resp = if content_type.contains("text/event-stream") {
            if is_chunked {
                let chunked_reader = BufReader::new(ChunkedDecoder::new(reader));
                consume_sse_stream(chunked_reader, self.dialect.as_ref(), cancel, on_delta).await?
            } else {
                consume_sse_stream(reader, self.dialect.as_ref(), cancel, on_delta).await?
            }
        } else {
            let body_bytes = read_http_body(&mut reader, &head.headers).await?;
            let json_val: Value = serde_json::from_slice(&body_bytes).map_err(|e| {
                ai_error(
                    codes::AI_BAD_RESPONSE,
                    format!("Resposta JSON malformada do MaxAiManager: {e}"),
                )
            })?;
            let parsed = self.dialect.parse_chat_response(json_val)?;
            if !parsed.content.is_empty() {
                on_delta(&parsed.content);
            }
            parsed
        };

        let latency_ms = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);
        tracing::debug!(
            target: "mt_ai::http",
            op = op.as_str(),
            status = head.status,
            latency_ms = latency_ms,
            content_bytes = chat_resp.content.len(),
            "Chamada chat concluída"
        );

        Ok(chat_resp)
    }
}

struct ResolvedTarget {
    host: String,
    port: u16,
    host_header: String,
    request_path: String,
}

fn resolve_url(base_url: &str, path: &str) -> MtResult<ResolvedTarget> {
    let base = Url::parse(base_url).map_err(|_| {
        ai_error(
            codes::AI_URL_INVALID,
            "A URL base do MaxAiManager é inválida.",
        )
    })?;
    let host = base
        .host_str()
        .ok_or_else(|| {
            ai_error(
                codes::AI_URL_INVALID,
                "Host ausente na URL do MaxAiManager.",
            )
        })?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = base.port_or_known_default().ok_or_else(|| {
        ai_error(
            codes::AI_URL_INVALID,
            "Porta inválida na URL do MaxAiManager.",
        )
    })?;

    let host_header = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };

    let base_path = base.path().trim_end_matches('/');
    let rel = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let request_path = format!("{base_path}{rel}");

    Ok(ResolvedTarget {
        host,
        port,
        host_header,
        request_path,
    })
}

fn build_http_request(
    method: &str,
    request_path: &str,
    host_header: &str,
    body: Option<&[u8]>,
    extra_headers: &[(String, String)],
) -> Vec<u8> {
    let mut head = String::with_capacity(256);
    head.push_str(method);
    head.push(' ');
    head.push_str(request_path);
    head.push_str(" HTTP/1.1\r\n");
    head.push_str("Host: ");
    head.push_str(host_header);
    head.push_str("\r\n");
    head.push_str(AGENT_HEADER.0);
    head.push_str(": ");
    head.push_str(AGENT_HEADER.1);
    head.push_str("\r\n");
    head.push_str("User-Agent: MaxTerminal/0.1.0 (+linux)\r\n");
    head.push_str("Accept: application/json, text/event-stream\r\n");
    head.push_str("Connection: close\r\n");

    for (k, v) in extra_headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }

    if let Some(payload) = body {
        use std::fmt::Write as _;
        head.push_str("Content-Type: application/json\r\n");
        let _ = write!(head, "Content-Length: {}\r\n", payload.len());
    }
    head.push_str("\r\n");

    let mut out = head.into_bytes();
    if let Some(payload) = body {
        out.extend_from_slice(payload);
    }
    out
}

struct HttpHead {
    status: u16,
    headers: HashMap<String, String>,
}

struct RawHttpResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

async fn read_http_head<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    base_url: &str,
) -> MtResult<HttpHead> {
    let mut status_line = String::new();
    let n = reader
        .read_line(&mut status_line)
        .await
        .map_err(|_| gateway_unavailable(base_url))?;
    if n == 0 {
        return Err(gateway_unavailable(base_url));
    }

    let mut parts = status_line.split_whitespace();
    let _http_ver = parts
        .next()
        .ok_or_else(|| ai_error(codes::AI_BAD_RESPONSE, " Linha de status HTTP inválida."))?;
    let status_str = parts
        .next()
        .ok_or_else(|| ai_error(codes::AI_BAD_RESPONSE, "Código de status HTTP ausente."))?;
    let status = status_str
        .parse::<u16>()
        .map_err(|_| ai_error(codes::AI_BAD_RESPONSE, "Código de status HTTP inválido."))?;

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .map_err(|_| ai_error(codes::AI_BAD_RESPONSE, "Cabeçalho HTTP truncado."))?;
        if read == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, val)) = trimmed.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), val.trim().to_string());
        }
    }

    Ok(HttpHead { status, headers })
}

async fn read_http_body<R: AsyncBufReadExt + Unpin>(
    reader: &mut R,
    headers: &HashMap<String, String>,
) -> MtResult<Vec<u8>> {
    if headers
        .get("transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    {
        let mut decoder = ChunkedDecoder::new(reader);
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = decoder.read(&mut buf).await.map_err(|e| {
                ai_error(
                    codes::AI_BAD_RESPONSE,
                    format!("Corpo HTTP chunked inválido: {e}"),
                )
            })?;
            if n == 0 {
                break;
            }
            if out.len() + n > MAX_RESPONSE_BYTES {
                return Err(ai_error(
                    codes::AI_BAD_RESPONSE,
                    "A resposta do MaxAiManager excedeu o limite máximo de 1 MiB.",
                ));
            }
            out.extend_from_slice(&buf[..n]);
        }
        return Ok(out);
    }

    if let Some(len_str) = headers.get("content-length") {
        let len = len_str
            .parse::<usize>()
            .map_err(|_| ai_error(codes::AI_BAD_RESPONSE, "Content-Length inválido."))?;
        if len > MAX_RESPONSE_BYTES {
            return Err(ai_error(
                codes::AI_BAD_RESPONSE,
                "A resposta do MaxAiManager excedeu o limite máximo de 1 MiB.",
            ));
        }
        let mut out = vec![0u8; len];
        reader.read_exact(&mut out).await.map_err(|_| {
            ai_error(
                codes::AI_BAD_RESPONSE,
                "Corpo HTTP truncado antes do Content-Length anunciado.",
            )
        })?;
        return Ok(out);
    }

    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = reader
            .read(&mut buf)
            .await
            .map_err(|_| ai_error(codes::AI_BAD_RESPONSE, "Erro ao ler corpo HTTP."))?;
        if n == 0 {
            break;
        }
        if out.len() + n > MAX_RESPONSE_BYTES {
            return Err(ai_error(
                codes::AI_BAD_RESPONSE,
                "A resposta do MaxAiManager excedeu o limite máximo de 1 MiB.",
            ));
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

/// Decodificador assíncrono de `Transfer-Encoding: chunked` para `AsyncBufRead`.
struct ChunkedDecoder<R> {
    inner: R,
    state: ChunkState,
    line_buf: Vec<u8>,
}

enum ChunkState {
    ReadSize,
    ReadData(usize),
    ReadCrLf,
    Done,
}

impl<R> ChunkedDecoder<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            state: ChunkState::ReadSize,
            line_buf: Vec::with_capacity(32),
        }
    }
}

impl<R: AsyncBufReadExt + Unpin> AsyncRead for ChunkedDecoder<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        loop {
            match this.state {
                ChunkState::Done => return Poll::Ready(Ok(())),
                ChunkState::ReadSize => {
                    let available = match Pin::new(&mut this.inner).poll_fill_buf(cx) {
                        Poll::Ready(Ok(slice)) => slice,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => return Poll::Pending,
                    };
                    if available.is_empty() {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "EOF inesperado lendo tamanho de chunk",
                        )));
                    }
                    if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                        this.line_buf.extend_from_slice(&available[..=pos]);
                        Pin::new(&mut this.inner).consume(pos + 1);
                        let line_str = String::from_utf8_lossy(&this.line_buf);
                        let hex_part = line_str.trim().split(';').next().unwrap_or("").trim();
                        let size = usize::from_str_radix(hex_part, 16).map_err(|_| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "Tamanho de chunk hexadecimal inválido",
                            )
                        })?;
                        this.line_buf.clear();
                        if size == 0 {
                            this.state = ChunkState::Done;
                        } else {
                            this.state = ChunkState::ReadData(size);
                        }
                    } else {
                        let len = available.len();
                        this.line_buf.extend_from_slice(available);
                        Pin::new(&mut this.inner).consume(len);
                    }
                }
                ChunkState::ReadData(remaining) => {
                    let available = match Pin::new(&mut this.inner).poll_fill_buf(cx) {
                        Poll::Ready(Ok(slice)) => slice,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => return Poll::Pending,
                    };
                    if available.is_empty() {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "EOF inesperado dentro de chunk HTTP",
                        )));
                    }
                    let to_copy = available.len().min(remaining).min(buf.remaining());
                    buf.put_slice(&available[..to_copy]);
                    Pin::new(&mut this.inner).consume(to_copy);
                    let next_rem = remaining - to_copy;
                    if next_rem == 0 {
                        this.state = ChunkState::ReadCrLf;
                    } else {
                        this.state = ChunkState::ReadData(next_rem);
                    }
                    return Poll::Ready(Ok(()));
                }
                ChunkState::ReadCrLf => {
                    let available = match Pin::new(&mut this.inner).poll_fill_buf(cx) {
                        Poll::Ready(Ok(slice)) => slice,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => return Poll::Pending,
                    };
                    if available.is_empty() {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "EOF inesperado após dados de chunk",
                        )));
                    }
                    if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                        Pin::new(&mut this.inner).consume(pos + 1);
                        this.state = ChunkState::ReadSize;
                    } else {
                        let len = available.len();
                        Pin::new(&mut this.inner).consume(len);
                    }
                }
            }
        }
    }
}

/// Formata o instante atual em ISO-8601 UTC (`YYYY-MM-DDTHH:MM:SS.mmmZ`).
#[must_use]
pub fn now_iso8601() -> String {
    let ms = mt_db::time::now_unix_ms();
    format_unix_ms_iso8601(ms)
}

/// Formata um timestamp UNIX em milissegundos como string ISO-8601 UTC.
#[must_use]
#[allow(clippy::cast_sign_loss)]
pub fn format_unix_ms_iso8601(unix_ms: i64) -> String {
    let secs = unix_ms.div_euclid(1000);
    let millis = unix_ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem_secs = secs.rem_euclid(86_400);
    let hour = rem_secs / 3600;
    let minute = (rem_secs % 3600) / 60;
    let second = rem_secs % 60;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    #[allow(clippy::cast_possible_wrap)]
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}
