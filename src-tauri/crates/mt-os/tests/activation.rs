//! Testes do `ActivationRouter` e validação de deep links `maxterminal://` (`mt-os`).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mt_core::error::MtResult;
use mt_os::{ActivationEnvelope, ActivationHandler, ActivationRouter, ActivationSource, codes};
use parking_lot::Mutex;
use url::Url;

struct RecordingHandler {
    received: Mutex<Vec<String>>,
}

#[async_trait]
impl ActivationHandler for RecordingHandler {
    fn accepts(&self, uri: &Url) -> bool {
        uri.host_str() == Some("open")
    }

    async fn handle(&self, envelope: &ActivationEnvelope) -> MtResult<()> {
        self.received.lock().push(envelope.uri.as_str().to_owned());
        Ok(())
    }
}

#[tokio::test]
async fn activation_router_queues_dedupes_validates_and_dispatches_on_ready() {
    let router = ActivationRouter::new();
    let handler = Arc::new(RecordingHandler {
        received: Mutex::new(Vec::new()),
    });
    router.register_handler(handler.clone()).await.unwrap();

    let t0 = Instant::now();
    let env1 = ActivationEnvelope::parse(
        "maxterminal://open/project?worktree=wt-1&correlationId=abc_123",
        ActivationSource::Startup,
        1_000,
        t0,
    )
    .unwrap();
    assert_eq!(env1.params.len(), 2);

    // Enfileira durante cold start (antes de set_ready)
    router.enqueue(env1.clone()).await.unwrap();

    // Dedupe de transporte: mesma URL em +200ms é ignorada silenciosamente
    let env_dup = ActivationEnvelope::parse(
        "maxterminal://open/project?worktree=wt-1&correlationId=abc_123",
        ActivationSource::DeepLink,
        1_200,
        t0 + Duration::from_millis(200),
    )
    .unwrap();
    router.enqueue(env_dup).await.unwrap();

    assert!(handler.received.lock().is_empty());

    // Ao ficar pronto, despacha exatamente 1 vez
    router.set_ready().await.unwrap();
    assert_eq!(handler.received.lock().len(), 1);

    // Rota sem handler -> os.activation_unavailable
    let env_unhandled = ActivationEnvelope::parse(
        "maxterminal://unknown-route/action",
        ActivationSource::DeepLink,
        3_000,
        t0 + Duration::from_secs(2),
    )
    .unwrap();
    let err = router.enqueue(env_unhandled).await.unwrap_err();
    assert_eq!(err.code.0, codes::ACTIVATION_UNAVAILABLE);

    // Scheme inválido e escape inválido -> os.activation_invalid
    assert_eq!(
        ActivationEnvelope::parse("https://example.com", ActivationSource::DeepLink, 0, t0)
            .unwrap_err()
            .code
            .0,
        codes::ACTIVATION_INVALID
    );
    assert_eq!(
        ActivationEnvelope::parse(
            "maxterminal://open?bad=%ZZ",
            ActivationSource::DeepLink,
            0,
            t0
        )
        .unwrap_err()
        .code
        .0,
        codes::ACTIVATION_INVALID
    );
}
