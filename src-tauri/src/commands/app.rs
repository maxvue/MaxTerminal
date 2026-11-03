//! Comandos IPC de fundação: metadados da aplicação (`app_info`) e recepção de eventos de log (`app_log_frontend`).

use std::time::Instant;

use mt_core::{
    MtResult,
    app::{AppInfo, FrontendLogEvent},
};

use crate::state::{AppState, FrontendLogState, RateLimitDecision};

/// Retorna a fotografia imutável de metadados e caminhos resolvidos do `MaxTerminal`.
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn app_info(state: tauri::State<'_, AppState>) -> MtResult<AppInfo> {
    Ok(state.info.clone())
}

/// Registra um evento enumerado do frontend no log estruturado, respeitando o rate-limit de 200 eventos/s.
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn app_log_frontend(
    event: FrontendLogEvent,
    state: tauri::State<'_, FrontendLogState>,
) -> MtResult<()> {
    record_frontend_event_with_now(event, &state, Instant::now());
    Ok(())
}

/// Função interna testável com relógio injetado (`Instant`) para validação determinística do rate-limiter.
pub fn record_frontend_event_with_now(
    event: FrontendLogEvent,
    state: &FrontendLogState,
    now: Instant,
) {
    match state.check_and_record(now) {
        RateLimitDecision::Allowed {
            dropped_in_previous_window,
        } => {
            if dropped_in_previous_window > 0 {
                tracing::warn!(
                    target: "frontend",
                    event = "frontend_log_rate_limited",
                    dropped = dropped_in_previous_window,
                    "Eventos de log do frontend descartados por limite de taxa"
                );
            }
            tracing::error!(
                target: "frontend",
                event = event.as_str(),
                "{}",
                event.static_message()
            );
        }
        RateLimitDecision::Dropped => {}
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub mod tests {
    //! Testes unitários do comando de log e metadados.
    use super::*;
    use crate::logging::{FoundationRedactor, RedactedJsonFormatter, build_env_filter};
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tracing_subscriber::{Layer, layer::SubscriberExt};

    #[derive(Clone, Default)]
    struct BufferWriter {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for BufferWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer
                .lock()
                .map_err(|_| io::Error::other("lock envenenado"))?
                .write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufferWriter {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn vue_error_generates_log_with_target_frontend_and_level_error() {
        let redactor = Arc::new(FoundationRedactor);
        let writer = BufferWriter::default();
        let buffer = Arc::clone(&writer.buffer);

        let layer = tracing_subscriber::fmt::layer()
            .event_format(RedactedJsonFormatter::new(redactor))
            .with_writer(writer)
            .with_filter(build_env_filter(Some("debug")));

        let subscriber = tracing_subscriber::registry().with(layer);

        let t0 = Instant::now();
        let state = FrontendLogState::new(t0);

        tracing::subscriber::with_default(subscriber, || {
            record_frontend_event_with_now(FrontendLogEvent::VueError, &state, t0);
        });

        let output = String::from_utf8(buffer.lock().unwrap().clone())
            .unwrap_or_else(|e| panic!("Saída UTF-8 inválida: {e}"));
        let json: serde_json::Value = serde_json::from_str(output.trim())
            .unwrap_or_else(|e| panic!("JSON inválido: {e} na saída {output}"));

        assert_eq!(json["level"], "ERROR");
        assert_eq!(json["target"], "frontend");
        assert_eq!(json["event"], "vue_error");
        assert_eq!(
            json["message"],
            "Erro capturado no runtime de componentes Vue"
        );
    }

    #[test]
    fn invalid_enum_fails_deserialization() {
        let invalid = "\"malicious_injection\"";
        let res = serde_json::from_str::<FrontendLogEvent>(invalid);
        assert!(res.is_err());
    }

    #[test]
    fn rate_limiter_accepts_200_drops_201_and_warns_in_next_window() {
        let redactor = Arc::new(FoundationRedactor);
        let writer = BufferWriter::default();
        let buffer = Arc::clone(&writer.buffer);

        let layer = tracing_subscriber::fmt::layer()
            .event_format(RedactedJsonFormatter::new(redactor))
            .with_writer(writer)
            .with_filter(build_env_filter(Some("warn")));

        let subscriber = tracing_subscriber::registry().with(layer);

        let t0 = Instant::now();
        let state = FrontendLogState::new(t0);

        tracing::subscriber::with_default(subscriber, || {
            // Emite 200 eventos na primeira janela (todos aceitos)
            for _ in 0..200 {
                record_frontend_event_with_now(FrontendLogEvent::WindowError, &state, t0);
            }

            // O 201º evento no mesmo instante t0 deve ser descartado
            record_frontend_event_with_now(FrontendLogEvent::WindowError, &state, t0);

            // Avança o tempo em 1.1s para nova janela
            let t1 = t0 + Duration::from_millis(1100);
            record_frontend_event_with_now(FrontendLogEvent::UnhandledRejection, &state, t1);
        });

        let output = String::from_utf8(buffer.lock().unwrap().clone())
            .unwrap_or_else(|e| panic!("Saída UTF-8 inválida: {e}"));
        let lines: Vec<&str> = output.lines().collect();

        // 200 erros no primeiro segundo + 1 warning de dropped na nova janela + 1 erro no segundo tempo
        assert_eq!(lines.len(), 202);

        // A penúltima linha (primeira da nova janela) deve ser o aviso de rate limit
        let warn_json: serde_json::Value = serde_json::from_str(lines[200])
            .unwrap_or_else(|e| panic!("JSON inválido para warning: {e}"));
        assert_eq!(warn_json["level"], "WARN");
        assert_eq!(warn_json["target"], "frontend");
        assert_eq!(warn_json["event"], "frontend_log_rate_limited");
        assert_eq!(warn_json["dropped"], 1);

        // A última linha deve ser o evento unhandled_rejection aceito na nova janela
        let event_json: serde_json::Value = serde_json::from_str(lines[201])
            .unwrap_or_else(|e| panic!("JSON inválido para evento: {e}"));
        assert_eq!(event_json["level"], "ERROR");
        assert_eq!(event_json["event"], "unhandled_rejection");
    }
}
