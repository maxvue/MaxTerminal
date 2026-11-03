//! Roteador de deep links (`maxterminal://...`) e ativação single-instance (`ActivationRouter`).

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mt_core::error::MtResult;
use url::Url;
use uuid::Uuid;

use crate::error::{codes, os_error};

/// Limite máximo de tamanho de uma URI `maxterminal://` (8 KiB).
pub const MAX_ACTIVATION_URI_BYTES: usize = 8192;

/// Capacidade máxima da fila de envelopes pendentes antes de `set_ready()`.
pub const MAX_ACTIVATION_QUEUE_LEN: usize = 32;

/// Tempo de vida máximo de um envelope na fila antes do consumo (30 s).
pub const ACTIVATION_ENVELOPE_TTL: Duration = Duration::from_secs(30);

/// Janela de deduplicação de transporte para a mesma URI (1 s).
pub const ACTIVATION_DEDUPE_WINDOW: Duration = Duration::from_secs(1);

/// Origem de recebimento de uma URI de ativação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationSource {
    /// Recebida nos argumentos de inicialização (cold start).
    Startup,
    /// Recebida via evento de deep link ou segunda instância (warm start).
    DeepLink,
}

/// Envelope validado de uma ativação via `maxterminal://`.
#[derive(Debug, Clone)]
pub struct ActivationEnvelope {
    /// Identificador único do envelope.
    pub id: Uuid,
    /// URI validada (`scheme == "maxterminal"`).
    pub uri: Url,
    /// Timestamp UNIX em milissegundos do recebimento.
    pub received_at_ms: i64,
    /// Instante monotônico do recebimento (usado para TTL e dedupe).
    pub received_instant: Instant,
    /// Origem da ativação (`Startup` ou `DeepLink`).
    pub source: ActivationSource,
    /// Parâmetros de query validados estruturalmente (`<= 16` pares, chaves `[A-Za-z0-9_-]{1,64}`).
    pub params: Vec<(String, String)>,
}

impl ActivationEnvelope {
    /// Analisa e valida uma URI bruta construindo um `ActivationEnvelope`.
    pub fn parse(
        raw_uri: &str,
        source: ActivationSource,
        now_ms: i64,
        now_instant: Instant,
    ) -> MtResult<Self> {
        if raw_uri.is_empty() || raw_uri.len() > MAX_ACTIVATION_URI_BYTES {
            return Err(os_error(
                codes::ACTIVATION_INVALID,
                "URI de ativação vazia ou acima do limite de 8 KiB.",
            ));
        }

        if raw_uri.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return Err(os_error(
                codes::ACTIVATION_INVALID,
                "URI de ativação contém caracteres de controle inválidos.",
            ));
        }

        if !has_valid_percent_escapes(raw_uri) {
            return Err(os_error(
                codes::ACTIVATION_INVALID,
                "URI de ativação contém sequência de escape percentual inválida.",
            ));
        }

        let uri = Url::parse(raw_uri).map_err(|_| {
            os_error(
                codes::ACTIVATION_INVALID,
                "Formato de URI de ativação inválido.",
            )
        })?;

        if uri.scheme() != "maxterminal" {
            return Err(os_error(
                codes::ACTIVATION_INVALID,
                "Esquema de URI não suportado; esperado 'maxterminal://'.",
            ));
        }

        if !uri.username().is_empty()
            || uri.password().is_some()
            || uri.port().is_some()
            || uri.fragment().is_some()
        {
            return Err(os_error(
                codes::ACTIVATION_INVALID,
                "URI de ativação não pode conter credenciais, porta ou fragmento.",
            ));
        }

        let mut params = Vec::new();
        let mut seen_keys = HashSet::new();

        for (k, v) in uri.query_pairs() {
            if params.len() >= 16 {
                return Err(os_error(
                    codes::ACTIVATION_INVALID,
                    "URI de ativação excede o limite de 16 parâmetros de consulta.",
                ));
            }
            let key_str = k.into_owned();
            let val_str = v.into_owned();

            if !is_valid_query_key(&key_str) {
                return Err(os_error(
                    codes::ACTIVATION_INVALID,
                    "Chave de parâmetro de consulta inválida na URI de ativação.",
                ));
            }
            if val_str.len() > 4096 || val_str.bytes().any(|b| b < 0x20 || b == 0x7f) {
                return Err(os_error(
                    codes::ACTIVATION_INVALID,
                    "Valor de parâmetro de consulta inválido ou acima de 4 KiB.",
                ));
            }
            if !seen_keys.insert(key_str.clone()) {
                return Err(os_error(
                    codes::ACTIVATION_INVALID,
                    "Parâmetro de consulta duplicado na URI de ativação.",
                ));
            }
            params.push((key_str, val_str));
        }

        Ok(Self {
            id: Uuid::now_v7(),
            uri,
            received_at_ms: now_ms,
            received_instant: now_instant,
            source,
            params,
        })
    }
}

fn has_valid_percent_escapes(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return false;
            }
            if !bytes[i + 1].is_ascii_hexdigit() || !bytes[i + 2].is_ascii_hexdigit() {
                return false;
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    true
}

fn is_valid_query_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Consumidor de rotas de ativação registrado no `ActivationRouter` (ex.: Fase 11 `mt-bridge`).
#[async_trait]
pub trait ActivationHandler: Send + Sync {
    /// Retorna `true` se este handler processa a rota da URI informada.
    fn accepts(&self, uri: &Url) -> bool;

    /// Processa o envelope validado.
    async fn handle(&self, envelope: &ActivationEnvelope) -> MtResult<()>;
}

#[derive(Default)]
struct RouterState {
    ready: bool,
    queue: VecDeque<ActivationEnvelope>,
    handlers: Vec<Arc<dyn ActivationHandler>>,
    recent_deliveries: VecDeque<(String, Instant)>,
}

/// Roteador assíncrono de deep links com fila limitada, TTL, deduplicação de transporte e despacho serial.
#[derive(Clone, Default)]
pub struct ActivationRouter {
    inner: Arc<tokio::sync::Mutex<RouterState>>,
}

impl ActivationRouter {
    /// Cria um novo `ActivationRouter`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra um novo `ActivationHandler`.
    pub async fn register_handler(&self, handler: Arc<dyn ActivationHandler>) -> MtResult<()> {
        let mut guard = self.inner.lock().await;
        guard.handlers.push(handler);
        Ok(())
    }

    /// Enfileira ou despacha imediatamente um `ActivationEnvelope` validado.
    pub async fn enqueue(&self, envelope: ActivationEnvelope) -> MtResult<()> {
        let mut guard = self.inner.lock().await;
        let now = envelope.received_instant;

        // Limpa histórico de deduplicação expirado (> 1 s)
        guard.recent_deliveries.retain(|(_, ts)| {
            now.checked_duration_since(*ts)
                .is_some_and(|d| d <= ACTIVATION_DEDUPE_WINDOW)
        });

        let raw_url = envelope.uri.as_str().to_owned();
        if guard.recent_deliveries.iter().any(|(u, ts)| {
            u == &raw_url && now.saturating_duration_since(*ts) <= ACTIVATION_DEDUPE_WINDOW
        }) {
            return Ok(());
        }
        guard.recent_deliveries.push_back((raw_url, now));

        if !guard.ready {
            // Remove itens expirados antes de verificar capacidade
            guard.queue.retain(|env| {
                now.checked_duration_since(env.received_instant)
                    .is_some_and(|d| d <= ACTIVATION_ENVELOPE_TTL)
            });

            if guard.queue.len() >= MAX_ACTIVATION_QUEUE_LEN {
                return Err(os_error(
                    codes::ACTIVATION_BUSY,
                    "A fila de ativação do MaxTerminal está saturada; tente novamente em instantes.",
                ));
            }
            guard.queue.push_back(envelope);
            return Ok(());
        }

        dispatch_envelope(&guard.handlers, &envelope, now).await
    }

    /// Marca o roteador como pronto e processa em ordem todos os envelopes pendentes dentro do TTL.
    pub async fn set_ready(&self) -> MtResult<()> {
        let mut guard = self.inner.lock().await;
        guard.ready = true;
        let now = Instant::now();
        let mut first_err: Option<mt_core::error::MtError> = None;

        while let Some(env) = guard.queue.pop_front() {
            let res = dispatch_envelope(&guard.handlers, &env, now).await;
            if let Err(err) = res
                && first_err.is_none()
            {
                first_err = Some(err);
            }
        }

        if let Some(err) = first_err {
            Err(err)
        } else {
            Ok(())
        }
    }
}

async fn dispatch_envelope(
    handlers: &[Arc<dyn ActivationHandler>],
    envelope: &ActivationEnvelope,
    now: Instant,
) -> MtResult<()> {
    if now.saturating_duration_since(envelope.received_instant) > ACTIVATION_ENVELOPE_TTL {
        return Err(os_error(
            codes::ACTIVATION_UNAVAILABLE,
            "A solicitação de ativação expirou antes de poder ser processada.",
        ));
    }

    for handler in handlers {
        if handler.accepts(&envelope.uri) {
            return handler.handle(envelope).await;
        }
    }

    Err(os_error(
        codes::ACTIVATION_UNAVAILABLE,
        "Nenhum consumidor disponível para a rota de ativação solicitada.",
    ))
}
