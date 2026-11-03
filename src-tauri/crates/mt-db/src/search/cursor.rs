//! Codificação e decodificação autenticada (HMAC-SHA256 + Base64 URL-safe) de cursores de busca global.

use std::fmt::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::search::error::SearchError;
use crate::search::model::{SearchFilters, SearchSort, SearchSourceKind};

const MAX_CURSOR_TOKEN_BYTES: usize = 2048;
const HMAC_TAG_LEN: usize = 32;

/// Payload estruturado contido em um cursor de paginação por seção.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCursorPayload {
    /// Versão do formato (`1`).
    pub v: u8,
    /// Rótulo da janela dona do snapshot (`SearchOwner.window_label`).
    pub owner: String,
    /// Identificador único do snapshot materializado.
    pub snapshot_id: String,
    /// Fonte paginada por este cursor.
    pub source: SearchSourceKind,
    /// Índice ordinal do próximo item dentro da seção no snapshot.
    pub next_ordinal: usize,
    /// Impressão digital SHA-256 canônica da consulta (`query` + `filters` + `sort`).
    pub query_fingerprint: String,
    /// Timestamp UNIX em milissegundos de expiração do snapshot.
    pub expires_at_ms: i64,
}

/// Assinador HMAC-SHA256 com chave efêmera de 256 bits gerada na inicialização da instância.
#[derive(Debug, Clone)]
pub struct CursorSigner {
    secret_key: [u8; 32],
}

impl Default for CursorSigner {
    fn default() -> Self {
        Self::new_random()
    }
}

impl CursorSigner {
    /// Cria um assinador com chave aleatória de 32 bytes do CSPRNG do sistema.
    #[must_use]
    pub fn new_random() -> Self {
        let mut key = [0u8; 32];
        rand::rng().fill_bytes(&mut key);
        Self { secret_key: key }
    }

    /// Cria um assinador com chave determinística (para testes).
    #[must_use]
    pub const fn with_key(secret_key: [u8; 32]) -> Self {
        Self { secret_key }
    }

    /// Codifica e assina o payload em um token Base64 URL-safe sem padding.
    pub fn encode(&self, payload: &SearchCursorPayload) -> Result<String, SearchError> {
        let json_bytes = serde_json::to_vec(payload).map_err(|_| SearchError::InvalidCursor)?;
        let tag = hmac_sha256(&self.secret_key, &json_bytes);
        let mut combined = Vec::with_capacity(json_bytes.len() + HMAC_TAG_LEN);
        combined.extend_from_slice(&json_bytes);
        combined.extend_from_slice(&tag);
        Ok(URL_SAFE_NO_PAD.encode(combined))
    }

    /// Decodifica um token Base64 URL-safe e valida sua assinatura HMAC em tempo constante.
    pub fn decode(&self, token: &str) -> Result<SearchCursorPayload, SearchError> {
        if token.is_empty() || token.len() > MAX_CURSOR_TOKEN_BYTES {
            return Err(SearchError::InvalidCursor);
        }
        let raw = URL_SAFE_NO_PAD
            .decode(token.as_bytes())
            .map_err(|_| SearchError::InvalidCursor)?;
        if raw.len() <= HMAC_TAG_LEN {
            return Err(SearchError::InvalidCursor);
        }
        let split_at = raw.len() - HMAC_TAG_LEN;
        let (json_bytes, provided_tag) = raw.split_at(split_at);
        let expected_tag = hmac_sha256(&self.secret_key, json_bytes);
        if !constant_time_eq(provided_tag, &expected_tag) {
            return Err(SearchError::InvalidCursor);
        }
        let payload: SearchCursorPayload =
            serde_json::from_slice(json_bytes).map_err(|_| SearchError::InvalidCursor)?;
        if payload.v != 1 {
            return Err(SearchError::InvalidCursor);
        }
        Ok(payload)
    }
}

/// Calcula o fingerprint SHA-256 canônico de `(query, filters, sort)`.
#[must_use]
pub fn compute_query_fingerprint(
    raw_query_trimmed: &str,
    filters: &SearchFilters,
    sort: SearchSort,
) -> String {
    let mut normalized_sources = filters.sources.clone();
    normalized_sources.sort();
    normalized_sources.dedup();

    let canonical = serde_json::json!({
        "q": raw_query_trimmed,
        "projectId": filters.project_id,
        "tabId": filters.tab_id,
        "sessionId": filters.session_id,
        "paneId": filters.pane_id,
        "fromMs": filters.from_ms,
        "untilMs": filters.until_ms,
        "exit": filters.exit,
        "sources": normalized_sources,
        "sort": sort,
    });

    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Implementação RFC 2104 de HMAC-SHA256 sobre `sha2::Sha256`.
fn hmac_sha256(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..32 {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(message);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    let outer_hash = outer.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&outer_hash);
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
