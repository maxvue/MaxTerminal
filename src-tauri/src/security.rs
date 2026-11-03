//! Validação de segurança de fronteira IPC (janela autorizada e origem confiável).

use mt_core::{ErrorCode, MtError};
use mt_db::search::SearchOwner;

/// Código de erro para janela não autorizada.
pub const ERR_UNAUTHORIZED_WINDOW: ErrorCode = ErrorCode("security.unauthorized_window");
/// Código de erro para URL de origem malformada.
pub const ERR_INVALID_ORIGIN: ErrorCode = ErrorCode("security.invalid_origin");
/// Código de erro para origem externa não autorizada.
pub const ERR_UNAUTHORIZED_ORIGIN: ErrorCode = ErrorCode("security.unauthorized_origin");

/// Valida que a chamada IPC provém exclusivamente da janela `"main"` e de uma origem local autorizada.
pub fn ensure_authorized_window<R: tauri::Runtime>(
    webview_window: &tauri::WebviewWindow<R>,
) -> Result<SearchOwner, MtError> {
    let url = webview_window
        .url()
        .map_err(|_| MtError::new(ERR_INVALID_ORIGIN, "Origem da janela inválida."))?;
    validate_window_and_origin(webview_window.label(), url.as_str())
}

/// Função pura e testável de validação de label de janela e URL de origem.
pub fn validate_window_and_origin(
    window_label: &str,
    url_str: &str,
) -> Result<SearchOwner, MtError> {
    if window_label != "main" {
        return Err(MtError::new(
            ERR_UNAUTHORIZED_WINDOW,
            "Esta janela não possui permissão para executar esta operação.",
        ));
    }

    let url = tauri::Url::parse(url_str)
        .map_err(|_| MtError::new(ERR_INVALID_ORIGIN, "Origem da janela inválida."))?;

    let valid_origin = matches!(url.scheme(), "tauri" | "http" | "https")
        && matches!(
            url.host_str(),
            Some("localhost" | "tauri.localhost" | "127.0.0.1") | None
        );

    if !valid_origin {
        return Err(MtError::new(
            ERR_UNAUTHORIZED_ORIGIN,
            "Origem não autorizada para invocar comandos nativos.",
        ));
    }

    Ok(SearchOwner {
        window_label: window_label.to_owned(),
    })
}
