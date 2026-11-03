//! Abertura segura de portas locais no navegador padrão (`http://127.0.0.1:<port>/`).

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;

use crate::error::{PortsError, PortsResult};

/// Constrói a URL de loopback estrita para `port`, recusando porta `0`.
pub fn build_safe_loopback_url(port: u16) -> PortsResult<String> {
    if port == 0 {
        return Err(PortsError::OpenFailed {
            port,
            reason: "Porta 0 é inválida para abertura no navegador.".to_string(),
        });
    }
    Ok(format!("http://127.0.0.1:{port}/"))
}

/// Abstração para abertura da URL de loopback no navegador do sistema operacional.
#[async_trait]
pub trait PortUrlOpener: Send + Sync + 'static {
    /// Abre `url` (sempre validada previamente como `http://127.0.0.1:<port>/`) no navegador.
    async fn open_url(&self, port: u16, url: &str) -> PortsResult<()>;
}

/// Implementação Linux (`xdg-open`) sem invocação de shell (`sh -c`).
#[derive(Debug, Default, Clone, Copy)]
pub struct XdgPortUrlOpener;

#[async_trait]
impl PortUrlOpener for XdgPortUrlOpener {
    async fn open_url(&self, port: u16, url: &str) -> PortsResult<()> {
        if !url.starts_with("http://127.0.0.1:") {
            return Err(PortsError::OpenFailed {
                port,
                reason: "Apenas URLs http://127.0.0.1:<port>/ são permitidas.".to_string(),
            });
        }

        let mut cmd = Command::new("xdg-open");
        cmd.arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(false);

        match tokio::time::timeout(Duration::from_secs(5), cmd.status()).await {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(Ok(status)) => Err(PortsError::OpenFailed {
                port,
                reason: format!("xdg-open retornou código {:?}", status.code()),
            }),
            Ok(Err(err)) => Err(PortsError::OpenFailed {
                port,
                reason: err.to_string(),
            }),
            Err(_) => Ok(()),
        }
    }
}
