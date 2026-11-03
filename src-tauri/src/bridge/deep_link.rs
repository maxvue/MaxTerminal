//! Handler de ativação `maxterminal://...` registrado no `ActivationRouter` do `mt-os`.

use std::sync::Arc;

use async_trait::async_trait;
use mt_bridge::{BridgeService, Url};
use mt_core::error::{MtError, MtResult};
use mt_os::{ActivationEnvelope, ActivationHandler};

/// Consumidor das rotas `maxterminal://open-project`, `maxterminal://open-terminal` e `maxterminal://focus`.
pub struct BridgeActivationHandler {
    bridge: Arc<BridgeService>,
}

impl BridgeActivationHandler {
    /// Cria um novo `BridgeActivationHandler` acoplado ao `BridgeService`.
    #[must_use]
    pub fn new(bridge: Arc<BridgeService>) -> Arc<Self> {
        Arc::new(Self { bridge })
    }
}

#[async_trait]
impl ActivationHandler for BridgeActivationHandler {
    fn accepts(&self, uri: &Url) -> bool {
        uri.scheme() == "maxterminal"
    }

    async fn handle(&self, envelope: &ActivationEnvelope) -> MtResult<()> {
        self.bridge
            .handle_inbound_uri(envelope.uri.as_str())
            .await
            .map_err(MtError::from)
    }
}
