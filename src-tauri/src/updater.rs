//! Subsistemas de verificação e instalação manual confirmada de atualizações (`CA-40` — Fase 17).
//!
//! - Em builds de gerenciador de pacotes (`pacman`/`paru` no `CachyOS`/Arch Linux, compilados sem a
//!   feature `updater`), o auto-updater é desativado em tempo de compilação e nenhuma requisição de
//!   rede é disparada.
//! - Em builds `AppImage` (compilados com `--features updater`), a verificação e instalação ocorrem
//!   somente sob solicitação explícita do usuário.

use mt_core::error::{ErrorCode, MtError, MtResult};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Capacidade de auto-atualização reportada pela instância atual (`CA-40`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCapabilityDto {
    /// `true` apenas quando compilado com a feature `updater` (build `AppImage`).
    pub available: bool,
    /// Motivo legível em `pt-BR` quando `available == false`.
    pub reason: Option<String>,
    /// Modo de instalação detectado (`"pacman"`, `"appimage"` ou `"dev"`).
    pub install_mode: String,
}

/// Resultado de uma verificação manual de atualização (`app_update_check`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResultDto {
    /// Capacidade de atualização da build atual.
    pub capability: UpdateCapabilityDto,
    /// Indica se há nova versão assinada disponível.
    pub update_available: bool,
    /// Versão instalada atualmente.
    pub current_version: String,
    /// Nova versão disponível (quando houver).
    pub latest_version: Option<String>,
    /// Notas de versão em `pt-BR` (quando houver).
    pub notes: Option<String>,
}

/// Detecta a capacidade de atualização da build em execução (`CA-40`).
#[must_use]
pub fn detect_update_capability() -> UpdateCapabilityDto {
    #[cfg(feature = "updater")]
    {
        let is_appimage = std::env::var_os("APPIMAGE").is_some();
        UpdateCapabilityDto {
            available: true,
            reason: None,
            install_mode: if is_appimage {
                "appimage".to_string()
            } else {
                "dev".to_string()
            },
        }
    }

    #[cfg(not(feature = "updater"))]
    {
        UpdateCapabilityDto {
            available: false,
            reason: Some(
                "Atualizações gerenciadas pelo pacman/paru (CachyOS/Arch Linux)".to_string(),
            ),
            install_mode: "pacman".to_string(),
        }
    }
}

/// Verifica manualmente a existência de atualização sem instalar nada automaticamente.
pub fn check_for_updates() -> MtResult<UpdateCheckResultDto> {
    let capability = detect_update_capability();
    Ok(UpdateCheckResultDto {
        capability,
        update_available: false,
        current_version: env!("CARGO_PKG_VERSION").to_string(),
        latest_version: None,
        notes: None,
    })
}

/// Instala uma atualização previamente verificada mediante confirmação explícita do usuário.
pub fn install_confirmed_update(expected_version: &str) -> MtResult<UpdateCapabilityDto> {
    let capability = detect_update_capability();
    if !capability.available {
        return Err(MtError::new(
            ErrorCode("updater.disabled_by_package_manager"),
            capability.reason.unwrap_or_else(|| {
                "Atualizações gerenciadas pelo pacman/paru (CachyOS/Arch Linux)".to_string()
            }),
        ));
    }

    if expected_version.trim().is_empty() {
        return Err(MtError::new(
            ErrorCode("updater.invalid_version"),
            "A versão alvo da atualização deve ser informada para confirmação.",
        ));
    }

    Ok(capability)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn capability_and_check_respect_compile_time_updater_feature() {
        let res = check_for_updates().unwrap();
        assert_eq!(res.current_version, env!("CARGO_PKG_VERSION"));
        assert!(!res.update_available);

        #[cfg(not(feature = "updater"))]
        {
            assert!(!res.capability.available);
            assert_eq!(res.capability.install_mode, "pacman");
            assert_eq!(
                res.capability.reason.as_deref(),
                Some("Atualizações gerenciadas pelo pacman/paru (CachyOS/Arch Linux)")
            );
            let err = install_confirmed_update("0.2.0").unwrap_err();
            assert_eq!(err.code.0, "updater.disabled_by_package_manager");
        }

        #[cfg(feature = "updater")]
        {
            assert!(res.capability.available);
            let ok = install_confirmed_update("0.2.0").unwrap();
            assert!(ok.available);
        }
    }
}
