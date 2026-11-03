//! Adaptadores de integração do `mt-bridge` no binário Tauri (`TauriBridgeHost`, `DbBridgeSettingsStore`,
//! `BridgeActivationHandler` e `BridgeEditorOpener`).

pub mod deep_link;
pub mod editor;
pub mod host;

pub use deep_link::BridgeActivationHandler;
pub use editor::BridgeEditorOpener;
pub use host::{DbBridgeSettingsStore, TauriBridgeHost};
