//! Metadados estáticos da aplicação e catálogo fechado de eventos de log do frontend.

use serde::{Deserialize, Serialize};

use crate::paths::AppPathsDto;

/// Perfil de compilação do binário Rust.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum BuildProfile {
    /// Compilação de desenvolvimento (`debug_assertions` ativo).
    Debug,
    /// Compilação otimizada de produção.
    Release,
}

/// Fotografia imutável de metadados do `MaxTerminal` retornada por `app_info`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Nome oficial da aplicação (`"MaxTerminal"`).
    pub name: String,
    /// Versão semântica do pacote (`CARGO_PKG_VERSION`).
    pub version: String,
    /// Perfil de compilação (`debug` ou `release`).
    pub build_profile: BuildProfile,
    /// Versão do runtime Tauri em uso.
    pub tauri_version: String,
    /// Sistema operacional (`std::env::consts::OS`).
    pub os: String,
    /// Arquitetura da CPU (`std::env::consts::ARCH`).
    pub arch: String,
    /// Diretórios XDG resolvidos no boot.
    pub paths: AppPathsDto,
}

/// Catálogo fechado de eventos de erro reportáveis pelo frontend antes do sanitizador da Fase 03.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FrontendLogEvent {
    /// Erro capturado pelo `app.config.errorHandler` do Vue.
    VueError,
    /// Erro global capturado no listener `window.addEventListener('error', ...)`.
    WindowError,
    /// Promise rejeitada sem tratamento capturada em `unhandledrejection`.
    UnhandledRejection,
    /// Falha ao executar o bootstrap inicial da store `useAppStore`.
    BootstrapFailed,
}

impl FrontendLogEvent {
    /// Identificador estático `snake_case` do evento para gravação estruturada no log.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VueError => "vue_error",
            Self::WindowError => "window_error",
            Self::UnhandledRejection => "unhandled_rejection",
            Self::BootstrapFailed => "bootstrap_failed",
        }
    }

    /// Mensagem estática em pt-BR associada ao evento (sem interpolação de payload externo).
    #[must_use]
    pub const fn static_message(self) -> &'static str {
        match self {
            Self::VueError => "Erro capturado no runtime de componentes Vue",
            Self::WindowError => "Erro global não tratado capturado na janela",
            Self::UnhandledRejection => "Rejeição de Promise não tratada no frontend",
            Self::BootstrapFailed => "Falha durante o bootstrap inicial do frontend",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_log_event_serde_snake_case() {
        let events = [
            (FrontendLogEvent::VueError, "\"vue_error\"", "vue_error"),
            (
                FrontendLogEvent::WindowError,
                "\"window_error\"",
                "window_error",
            ),
            (
                FrontendLogEvent::UnhandledRejection,
                "\"unhandled_rejection\"",
                "unhandled_rejection",
            ),
            (
                FrontendLogEvent::BootstrapFailed,
                "\"bootstrap_failed\"",
                "bootstrap_failed",
            ),
        ];

        for (event, expected_json, expected_str) in events {
            assert_eq!(event.as_str(), expected_str);
            assert_ne!(event.static_message(), "");

            let json = serde_json::to_string(&event)
                .unwrap_or_else(|e| panic!("Erro ao serializar {expected_str}: {e}"));
            assert_eq!(json, expected_json);

            let deserialized: FrontendLogEvent = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("Erro ao desserializar {expected_str}: {e}"));
            assert_eq!(deserialized, event);
        }

        let invalid_json = "\"unknown_event\"";
        assert!(serde_json::from_str::<FrontendLogEvent>(invalid_json).is_err());
    }

    #[test]
    fn build_profile_and_app_info_serde() {
        let debug_json = serde_json::to_string(&BuildProfile::Debug)
            .unwrap_or_else(|e| panic!("Erro ao serializar BuildProfile: {e}"));
        assert_eq!(debug_json, "\"debug\"");

        let release_json = serde_json::to_string(&BuildProfile::Release)
            .unwrap_or_else(|e| panic!("Erro ao serializar BuildProfile: {e}"));
        assert_eq!(release_json, "\"release\"");

        let paths = AppPathsDto {
            config_dir: "/tmp/config".to_string(),
            data_dir: "/tmp/data".to_string(),
            state_dir: "/tmp/state".to_string(),
            cache_dir: "/tmp/cache".to_string(),
            log_dir: "/tmp/state/logs".to_string(),
            db_path: "/tmp/data/maxterminal.db".to_string(),
        };

        let app_info = AppInfo {
            name: "MaxTerminal".to_string(),
            version: "0.1.0".to_string(),
            build_profile: BuildProfile::Debug,
            tauri_version: "2.0.0".to_string(),
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            paths,
        };

        let json = serde_json::to_string(&app_info)
            .unwrap_or_else(|e| panic!("Erro ao serializar AppInfo: {e}"));
        assert!(json.contains("\"buildProfile\":\"debug\""));
        assert!(json.contains("\"name\":\"MaxTerminal\""));

        let deserialized: AppInfo = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("Erro ao desserializar AppInfo: {e}"));
        assert_eq!(deserialized, app_info);
    }
}
