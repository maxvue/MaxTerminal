//! Resolvedor seguro de diretório de trabalho (`CwdResolver`), mesclagem de variáveis de ambiente,
//! detecção de dialeto de shell e cálculo de hash canônico de idempotência (`request_hash`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mt_core::MtResult;
use sha2::{Digest, Sha256};

use crate::groups::model::{GroupErrorCode, GroupRunRequest, ShellDialect};
use crate::groups::validate::{
    MAX_MERGED_ENV_BYTES, validate_env_key, validate_relative_cwd_syntax,
};

/// Resolvedor seguro de diretório de trabalho e variáveis de ambiente para execução.
pub struct CwdResolver;

impl CwdResolver {
    /// Resolve o diretório efetivo (`item_cwd` > `group_cwd` > `project_root`), canonicaliza a raiz
    /// e o alvo e verifica se o diretório existe e permanece estritamente dentro da raiz canonicalizada
    /// (impedindo escape via `..` ou symlinks que apontem para fora da raiz do projeto/worktree).
    pub fn resolve_effective_cwd(
        project_root: &Path,
        group_cwd: Option<&str>,
        item_cwd: Option<&str>,
        field_path: &str,
    ) -> MtResult<PathBuf> {
        let canonical_root = project_root.canonicalize().map_err(|_| {
            GroupErrorCode::CwdNotFound.into_error_with_fields(
                "A pasta raiz do projeto não foi encontrada no disco.",
                vec!["projectId".to_string()],
            )
        })?;

        let chosen_rel = match item_cwd.map(str::trim).filter(|s| !s.is_empty()) {
            Some(rel) => Some((rel, field_path)),
            None => group_cwd
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|rel| (rel, "cwd")),
        };

        let Some((rel_str, active_field)) = chosen_rel else {
            return Ok(canonical_root);
        };

        let valid_rel = validate_relative_cwd_syntax(rel_str, active_field)?;
        let candidate = canonical_root.join(&valid_rel);
        if !candidate.exists() {
            return Err(GroupErrorCode::CwdNotFound.into_error_with_fields(
                format!("O diretório relativo '{valid_rel}' não existe no projeto."),
                vec![active_field.to_string()],
            ));
        }

        let canonical_target = candidate.canonicalize().map_err(|_| {
            GroupErrorCode::CwdNotFound.into_error_with_fields(
                format!("Não foi possível resolver o diretório relativo '{valid_rel}'."),
                vec![active_field.to_string()],
            )
        })?;

        if !canonical_target.is_dir() {
            return Err(GroupErrorCode::CwdNotFound.into_error_with_fields(
                format!("O caminho '{valid_rel}' não é um diretório válido."),
                vec![active_field.to_string()],
            ));
        }

        if !canonical_target.starts_with(&canonical_root) {
            return Err(GroupErrorCode::CwdOutsideProject.into_error_with_fields(
                "O diretório de trabalho aponta para fora da pasta raiz do projeto.",
                vec![active_field.to_string()],
            ));
        }

        Ok(canonical_target)
    }

    /// Mescla variáveis de ambiente respeitando a precedência:
    /// `base_env` (herdado/template) → `group_env` → `item_env`, validando o limite de 64 KiB.
    pub fn merge_env(
        base_env: &BTreeMap<String, String>,
        group_env: &BTreeMap<String, String>,
        item_env: &BTreeMap<String, String>,
    ) -> MtResult<BTreeMap<String, String>> {
        let mut merged = BTreeMap::new();
        for (k, v) in base_env {
            if !k.starts_with("MAXTERMINAL_") {
                merged.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in group_env {
            if let Err(msg) = validate_env_key(k) {
                return Err(GroupErrorCode::Validation
                    .into_error_with_fields(msg, vec![format!("env.{k}")]));
            }
            merged.insert(k.clone(), v.clone());
        }
        for (k, v) in item_env {
            if let Err(msg) = validate_env_key(k) {
                return Err(GroupErrorCode::Validation
                    .into_error_with_fields(msg, vec![format!("item.env.{k}")]));
            }
            merged.insert(k.clone(), v.clone());
        }

        let total_bytes: usize = merged.iter().map(|(k, v)| k.len() + v.len() + 2).sum();
        if total_bytes > MAX_MERGED_ENV_BYTES {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                format!(
                    "O tamanho total das variáveis de ambiente mescladas excede o limite de {MAX_MERGED_ENV_BYTES} bytes."
                ),
                vec!["env".to_string()],
            ));
        }

        Ok(merged)
    }
}

/// Detecta o `ShellDialect` a partir do nome do executável do shell (`bash`, `zsh`, `fish`).
/// Retorna `Err(GroupErrorCode::UnsupportedShell)` para qualquer outro executável.
pub fn detect_shell_dialect(shell_path: &Path) -> MtResult<ShellDialect> {
    let file_name = shell_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    match file_name.as_str() {
        "bash" => Ok(ShellDialect::Bash),
        "zsh" => Ok(ShellDialect::Zsh),
        "fish" => Ok(ShellDialect::Fish),
        _ => Err(GroupErrorCode::UnsupportedShell.into_error(format!(
            "O shell '{}' não é suportado para execução de Grupos de Comandos (suportados: bash, zsh, fish).",
            shell_path.display()
        ))),
    }
}

/// Calcula o hash SHA-256 hexadecimal minúsculo (64 caracteres) canônico de um `GroupRunRequest`
/// (incluindo `group_id`, `expected_revision`, `project_id`, `origin_pane_id`, `mode_override` e `template_by_item`)
/// para verificação de idempotência por `request_id`.
#[must_use]
pub fn compute_request_hash(request: &GroupRunRequest) -> String {
    let canonical = serde_json::json!({
        "groupId": request.group_id.to_string(),
        "expectedRevision": request.expected_revision,
        "projectId": request.project_id.to_string(),
        "originPaneId": request.origin_pane_id.as_ref().map(ToString::to_string),
        "modeOverride": request.mode_override.map(|m| m.as_str()),
        "templateByItem": request
            .template_by_item
            .iter()
            .map(|(k, v)| (k.as_str(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
    });
    let serialized = serde_json::to_vec(&canonical).unwrap_or_default();
    let digest = Sha256::digest(&serialized);
    hex::encode(digest)
}
