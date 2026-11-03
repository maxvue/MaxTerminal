//! Adaptador de abertura segura de portas locais no navegador (`http://127.0.0.1:<port>/`).

use std::sync::Arc;

use mt_ports::{PortUrlOpener, XdgPortUrlOpener};

/// Cria o [`PortUrlOpener`] padrão para Linux (`xdg-open`).
#[must_use]
pub fn default_port_url_opener() -> Arc<dyn PortUrlOpener> {
    Arc::new(XdgPortUrlOpener)
}
