//! Fonte heurística opt-in de telemetria (`OutputHeuristicSource`, qualidade `Estimated`).

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::registry::AgentKind;
use crate::telemetry::{TelemetryContext, TelemetrySource, TelemetrySourceKind, TelemetryUpdate};

/// Fonte heurística opt-in de telemetria (qualidade sempre marcada como `Estimated`).
pub struct OutputHeuristicSource;

#[async_trait]
impl TelemetrySource for OutputHeuristicSource {
    fn kind(&self) -> TelemetrySourceKind {
        TelemetrySourceKind::OutputHeuristic
    }

    fn supports(&self, _agent: AgentKind) -> bool {
        true
    }

    async fn run(
        &self,
        _ctx: TelemetryContext,
        _sink: mpsc::Sender<TelemetryUpdate>,
        cancel: CancellationToken,
    ) {
        cancel.cancelled().await;
    }
}
