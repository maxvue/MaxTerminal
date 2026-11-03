//! Validação e normalização segura de caminhos de projetos e arquivos recebidos pela ponte.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::{BridgeError, BridgeErrorCode};

/// Limite máximo de comprimento de caminho em bytes (4 096).
pub const MAX_PATH_BYTES: usize = 4096;

/// Valida e canonicaliza um diretório de projeto existente no sistema de arquivos.
///
/// Rejeita caminhos relativos, vazios, com NUL/caracteres de controle ou que não apontem para um diretório existente.
pub fn normalize_existing_project_path(raw: &str) -> Result<PathBuf, BridgeError> {
    validate_raw_path_string(raw)?;
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidTargetPath,
            "O caminho do projeto deve ser absoluto.",
        ));
    }

    let canonical = fs::canonicalize(path).map_err(|_| {
        BridgeError::new(
            BridgeErrorCode::InvalidTargetPath,
            format!("O diretório do projeto não existe ou não está acessível: {raw}"),
        )
    })?;

    if !canonical.is_dir() {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidTargetPath,
            "O caminho informado para o projeto não é um diretório.",
        ));
    }

    Ok(canonical)
}

/// Valida um caminho absoluto de arquivo para abertura no editor (`editor.open_file`).
pub fn validate_file_path_for_open(raw: &str) -> Result<PathBuf, BridgeError> {
    validate_raw_path_string(raw)?;
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidTargetPath,
            "O caminho do arquivo deve ser absoluto.",
        ));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidTargetPath,
            "O caminho do arquivo não pode conter segmentos '..'.",
        ));
    }
    Ok(path.to_path_buf())
}

fn validate_raw_path_string(raw: &str) -> Result<(), BridgeError> {
    if raw.is_empty() || raw.len() > MAX_PATH_BYTES {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidTargetPath,
            "Caminho vazio ou acima do limite de 4096 bytes.",
        ));
    }
    if raw.starts_with('-') {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidTargetPath,
            "Caminho não pode iniciar com '-'.",
        ));
    }
    if raw.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidTargetPath,
            "Caminho contém caracteres de controle inválidos.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn validates_and_canonicalizes_project_dir() {
        let tmp = TempDir::new().unwrap();
        let norm = normalize_existing_project_path(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(norm, fs::canonicalize(tmp.path()).unwrap());

        assert!(normalize_existing_project_path("relative/path").is_err());
        assert!(normalize_existing_project_path("-flag/inject").is_err());
        assert!(normalize_existing_project_path("/path/with\0null").is_err());
    }
}
