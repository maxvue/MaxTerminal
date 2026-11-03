//! Geração de token CSPRNG (256 bits) e validação em tempo constante + filtro de cabeçalho `Origin`.

use http::HeaderMap;
use rand::RngCore;

use crate::error::{BridgeError, BridgeErrorCode};

/// Gera um token aleatório criptograficamente seguro de 32 bytes codificado como 64 caracteres hexadecimais.
#[must_use]
pub fn generate_token_hex() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Compara dois tokens em tempo constante (sem early-return dependente de conteúdo) para mitigar ataques de timing.
#[must_use]
pub fn verify_token_constant_time(expected: &str, provided: &str) -> bool {
    let a = expected.as_bytes();
    let b = provided.as_bytes();
    if a.is_empty() || a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Valida os cabeçalhos HTTP (`Origin` e `Authorization: Bearer <token>`) de uma requisição recebida no loopback.
///
/// Regras:
/// - `Origin`: deve estar ausente (cliente nativo Node/Rust) ou iniciar por `vscode-webview://` ou `tauri://localhost`.
///   Qualquer origem `http://` ou `https://` de navegador é rejeitada imediatamente (anti-CSRF / DNS rebinding).
/// - `Authorization`: deve conter `Bearer <token>` idêntico ao token esperado em tempo constante.
pub fn verify_request_headers(
    headers: &HeaderMap,
    expected_token: &str,
) -> Result<(), BridgeError> {
    if let Some(origin_val) = headers.get(http::header::ORIGIN) {
        let origin_str = origin_val.to_str().map_err(|_| {
            BridgeError::new(
                BridgeErrorCode::Unauthorized,
                "Cabeçalho Origin malformado.",
            )
        })?;
        if !is_allowed_origin(origin_str) {
            return Err(BridgeError::new(
                BridgeErrorCode::Unauthorized,
                "Origem HTTP não autorizada para acessar o servidor loopback do MaxTerminal.",
            ));
        }
    }

    let auth_header = headers
        .get(http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            BridgeError::new(
                BridgeErrorCode::Unauthorized,
                "Cabeçalho Authorization ausente.",
            )
        })?;

    let provided_token = auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))
        .map(str::trim)
        .ok_or_else(|| {
            BridgeError::new(
                BridgeErrorCode::Unauthorized,
                "Formato de Authorization inválido; esperado 'Bearer <token>'.",
            )
        })?;

    if !verify_token_constant_time(expected_token, provided_token) {
        return Err(BridgeError::new(
            BridgeErrorCode::Unauthorized,
            "Token de autenticação da ponte inválido.",
        ));
    }

    Ok(())
}

/// Retorna `true` somente para origens confiáveis de extensões VS Code ou `WebView` Tauri.
#[must_use]
pub fn is_allowed_origin(origin: &str) -> bool {
    let trimmed = origin.trim();
    trimmed.starts_with("vscode-webview://")
        || trimmed == "tauri://localhost"
        || trimmed.starts_with("tauri://localhost/")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use http::HeaderValue;

    #[test]
    fn token_generation_has_64_hex_chars_and_unique_values() {
        let t1 = generate_token_hex();
        let t2 = generate_token_hex();
        assert_eq!(t1.len(), 64);
        assert_ne!(t1, t2);
        assert!(t1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn constant_time_verification_works() {
        let tok = generate_token_hex();
        assert!(verify_token_constant_time(&tok, &tok));
        assert!(!verify_token_constant_time(&tok, "errado"));
        assert!(!verify_token_constant_time("", ""));
    }

    #[test]
    fn origin_and_bearer_validation_blocks_browser_origins() {
        let tok = generate_token_hex();
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {tok}")).unwrap(),
        );
        assert!(verify_request_headers(&headers, &tok).is_ok());

        headers.insert(
            http::header::ORIGIN,
            HeaderValue::from_static("vscode-webview://1234"),
        );
        assert!(verify_request_headers(&headers, &tok).is_ok());

        headers.insert(
            http::header::ORIGIN,
            HeaderValue::from_static("https://evil.example.com"),
        );
        assert!(verify_request_headers(&headers, &tok).is_err());
    }
}
