//! Estado gerenciado do `MaxTerminal` na Fase 01: metadados imutáveis e rate-limiter de logs do frontend.

use mt_core::{app::AppInfo, paths::AppPaths};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Limite máximo de eventos de log do frontend por janela de 1 segundo.
pub const FRONTEND_LOG_MAX_PER_WINDOW: u32 = 200;

/// Duração da janela de rate-limit de logs do frontend.
pub const FRONTEND_LOG_WINDOW: Duration = Duration::from_secs(1);

/// Estado global imutável compartilhado com os comandos IPC de fundação.
#[derive(Clone, Debug)]
pub struct AppState {
    /// Caminhos XDG resolvidos e validados no startup.
    pub paths: AppPaths,
    /// Fotografia imutável de informações da aplicação.
    pub info: AppInfo,
}

impl AppState {
    /// Constrói o `AppState` a partir dos `AppPaths` já resolvidos.
    #[must_use]
    pub fn new(paths: AppPaths) -> Self {
        let build_profile = if cfg!(debug_assertions) {
            mt_core::app::BuildProfile::Debug
        } else {
            mt_core::app::BuildProfile::Release
        };

        let info = AppInfo {
            name: String::from("MaxTerminal"),
            version: String::from(env!("CARGO_PKG_VERSION")),
            build_profile,
            tauri_version: String::from(tauri::VERSION),
            os: String::from(std::env::consts::OS),
            arch: String::from(std::env::consts::ARCH),
            paths: (&paths).into(),
        };

        Self { paths, info }
    }
}

/// Resultado da avaliação do rate-limiter para um evento de log do frontend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimitDecision {
    /// Evento aceito; se `dropped_in_previous_window > 0`, o chamador deve emitir primeiro
    /// um aviso agregado informando quantos eventos foram descartados na janela anterior.
    Allowed {
        /// Quantidade de eventos descartados na janela encerrada imediatamente anterior.
        dropped_in_previous_window: u64,
    },
    /// Evento descartado por exceder a cota de 200 eventos/s na janela atual.
    Dropped,
}

#[derive(Debug)]
struct RateWindow {
    window_start: Instant,
    accepted_in_window: u32,
    dropped_in_window: u64,
}

/// Estado concorrente de controle de taxa para o comando `app_log_frontend`.
#[derive(Debug)]
pub struct FrontendLogState {
    inner: Mutex<RateWindow>,
}

impl Default for FrontendLogState {
    fn default() -> Self {
        Self::new(Instant::now())
    }
}

impl FrontendLogState {
    /// Inicializa o controlador de taxa a partir de um instante monotônico inicial.
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self {
            inner: Mutex::new(RateWindow {
                window_start: now,
                accepted_in_window: 0,
                dropped_in_window: 0,
            }),
        }
    }

    /// Avalia se um novo evento no instante `now` pode ser gravado.
    pub fn check_and_record(&self, now: Instant) -> RateLimitDecision {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };

        if now
            .checked_duration_since(guard.window_start)
            .is_some_and(|d| d >= FRONTEND_LOG_WINDOW)
        {
            let dropped_prev = guard.dropped_in_window;
            guard.window_start = now;
            guard.accepted_in_window = 1;
            guard.dropped_in_window = 0;
            return RateLimitDecision::Allowed {
                dropped_in_previous_window: dropped_prev,
            };
        }

        if guard.accepted_in_window < FRONTEND_LOG_MAX_PER_WINDOW {
            guard.accepted_in_window += 1;
            RateLimitDecision::Allowed {
                dropped_in_previous_window: 0,
            }
        } else {
            guard.dropped_in_window = guard.dropped_in_window.saturating_add(1);
            RateLimitDecision::Dropped
        }
    }
}

/// Serviço gerenciado pelo Tauri contendo a fachada `mt_db::Db` da Fase 03.
#[derive(Clone)]
pub struct DbService {
    inner: Arc<mt_db::Db>,
}

impl DbService {
    /// Constrói um novo `DbService` envolvendo a fachada `Db`.
    #[must_use]
    pub fn new(db: Arc<mt_db::Db>) -> Self {
        Self { inner: db }
    }

    /// Retorna referência compartilhada para a fachada `Db`.
    #[must_use]
    pub fn db(&self) -> &Arc<mt_db::Db> {
        &self.inner
    }
}
