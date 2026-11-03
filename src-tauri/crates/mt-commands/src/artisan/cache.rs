//! Leitura e escrita do cache `.max-code/commands/artisan.json` (compatível com `MaxCode`)
//! e do sidecar `.max-code/commands/artisan.meta.json`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use mt_core::MtResult;
use mt_db::sanitizer::sanitize;
use serde::{Deserialize, Serialize};

use crate::artisan::fingerprint::Fingerprint;
use crate::artisan::model::{ArtisanCommand, RunnerKind};
use crate::artisan::parser::is_safe_command_token;
use crate::fs_util::{
    atomic_write, create_dirs_safe, ensure_safe_under_root, read_capped, sha256_hex,
};

const MAX_CACHE_BYTES: usize = 8 * 1024 * 1024;
const MAX_META_BYTES: usize = 256 * 1024;
/// Versão atual do schema do sidecar `.max-code/commands/artisan.meta.json`.
pub const CACHE_META_SCHEMA: u32 = 1;

/// Metadados persistidos no sidecar `.max-code/commands/artisan.meta.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheMeta {
    /// Versão do schema (`1`).
    pub schema: u32,
    /// Identificador do gerador (`"maxterminal"`).
    pub generator: String,
    /// Versão do crate gerador.
    pub generator_version: String,
    /// Timestamp Unix em milissegundos da gravação.
    pub generated_at_ms: i64,
    /// Hash SHA-256 dos bytes exatos de `.max-code/commands/artisan.json`.
    pub artisan_json_sha256: String,
    /// Fingerprint dos arquivos do projeto no instante da descoberta.
    pub fingerprint: Fingerprint,
    /// Runner utilizado na descoberta.
    pub runner: RunnerKind,
    /// Versão do Laravel detectada no momento da descoberta.
    pub laravel_version: Option<String>,
    /// Quantidade de comandos gravados no cache.
    pub command_count: u32,
}

/// Resultado da leitura de `.max-code/commands/artisan.json` e seu sidecar no disco.
#[derive(Debug, Clone)]
pub struct LoadedCacheFiles {
    /// Lista de comandos validada e normalizada.
    pub commands: Vec<ArtisanCommand>,
    /// Hash SHA-256 dos bytes brutos de `artisan.json`.
    pub artisan_json_sha256: String,
    /// Sidecar válido quando presente e com `schema == 1`.
    pub meta: Option<CacheMeta>,
}

/// Lê e valida `.max-code/commands/artisan.json` e `.max-code/commands/artisan.meta.json` sob `root`.
/// Retorna `Ok(None)` se `artisan.json` não existir ou estiver corrompido.
pub fn read_cache_files(root: &Path) -> MtResult<Option<LoadedCacheFiles>> {
    let json_rel = Path::new(".max-code/commands/artisan.json");
    let meta_rel = Path::new(".max-code/commands/artisan.meta.json");

    let Ok(json_path) = ensure_safe_under_root(root, json_rel) else {
        return Ok(None);
    };
    let Ok(Some(raw_json)) = read_capped(&json_path, MAX_CACHE_BYTES) else {
        return Ok(None);
    };

    let Ok(raw_commands) = serde_json::from_str::<Vec<ArtisanCommand>>(&raw_json) else {
        return Ok(None);
    };

    let Some(commands) = validate_and_normalize_cached_commands(raw_commands) else {
        return Ok(None);
    };

    let artisan_json_sha256 = sha256_hex(raw_json.as_bytes());

    let meta = ensure_safe_under_root(root, meta_rel)
        .ok()
        .and_then(|p| read_capped(&p, MAX_META_BYTES).ok().flatten())
        .and_then(|s| serde_json::from_str::<CacheMeta>(&s).ok())
        .filter(|m| m.schema == CACHE_META_SCHEMA);

    Ok(Some(LoadedCacheFiles {
        commands,
        artisan_json_sha256,
        meta,
    }))
}

fn validate_and_normalize_cached_commands(
    commands: Vec<ArtisanCommand>,
) -> Option<Vec<ArtisanCommand>> {
    let mut out = Vec::with_capacity(commands.len());
    for mut cmd in commands {
        let name = cmd.name.trim().to_string();
        if !is_safe_command_token(&name) {
            return None;
        }
        cmd.name = name;
        cmd.description = cmd
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| sanitize(s).into_owned());

        for opt in &mut cmd.options {
            let trimmed = opt.name.trim();
            let normalized = if trimmed.starts_with("--") {
                trimmed.to_string()
            } else {
                format!("--{}", trimmed.trim_start_matches('-'))
            };
            let bare = normalized.trim_start_matches('-');
            if !is_safe_command_token(bare) {
                return None;
            }
            opt.name = normalized;
            opt.description = opt
                .description
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| sanitize(s).into_owned());
            opt.shortcut = opt
                .shortcut
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from);
        }

        for arg in &mut cmd.arguments {
            let trimmed = arg.name.trim().to_string();
            if !is_safe_command_token(&trimmed) {
                return None;
            }
            arg.name = trimmed;
            arg.description = arg
                .description
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| sanitize(s).into_owned());
        }

        cmd.aliases.retain(|a| is_safe_command_token(a));
        out.push(cmd);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Some(out)
}

/// Grava `.max-code/commands/artisan.json`, `.max-code/commands/artisan.meta.json` e `.max-code/.gitignore` (sem sobrescrever).
/// Retorna o hash SHA-256 dos bytes gravados em `artisan.json`.
pub fn write_cache_files(
    root: &Path,
    commands: &[ArtisanCommand],
    fingerprint: Fingerprint,
    runner: RunnerKind,
    laravel_version: Option<String>,
    generated_at_ms: i64,
) -> MtResult<String> {
    let commands_dir = create_dirs_safe(root, Path::new(".max-code/commands"))?;
    let maxcode_dir = commands_dir.parent().unwrap_or(&commands_dir).to_path_buf();

    // Cria .max-code/.gitignore com "*\n" apenas se ainda não existir (create_new)
    let gitignore_path = maxcode_dir.join(".gitignore");
    if let Ok(mut f) = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&gitignore_path)
    {
        let _ = f.write_all(b"*\n");
        let _ = f.sync_all();
    }

    let sanitized_commands: Vec<ArtisanCommand> = commands
        .iter()
        .cloned()
        .map(|mut c| {
            c.description = c.description.as_deref().map(|d| sanitize(d).into_owned());
            for o in &mut c.options {
                o.description = o.description.as_deref().map(|d| sanitize(d).into_owned());
            }
            for a in &mut c.arguments {
                a.description = a.description.as_deref().map(|d| sanitize(d).into_owned());
            }
            c
        })
        .collect();

    let mut json_bytes = serde_json::to_vec_pretty(&sanitized_commands).map_err(|e| {
        crate::error::write_failed(format!("Falha ao serializar artisan.json: {e}"))
    })?;
    json_bytes.push(b'\n');

    let artisan_json_sha256 = sha256_hex(&json_bytes);
    let json_path = commands_dir.join("artisan.json");
    atomic_write(&json_path, &json_bytes)
        .map_err(|e| crate::error::write_failed(format!("Falha ao gravar artisan.json: {e}")))?;

    let meta = CacheMeta {
        schema: CACHE_META_SCHEMA,
        generator: "maxterminal".to_string(),
        generator_version: env!("CARGO_PKG_VERSION").to_string(),
        generated_at_ms,
        artisan_json_sha256: artisan_json_sha256.clone(),
        fingerprint,
        runner,
        laravel_version,
        command_count: u32::try_from(sanitized_commands.len()).unwrap_or(u32::MAX),
    };

    let mut meta_bytes = serde_json::to_vec_pretty(&meta).map_err(|e| {
        crate::error::write_failed(format!("Falha ao serializar artisan.meta.json: {e}"))
    })?;
    meta_bytes.push(b'\n');

    let meta_path = commands_dir.join("artisan.meta.json");
    atomic_write(&meta_path, &meta_bytes).map_err(|e| {
        crate::error::write_failed(format!("Falha ao gravar artisan.meta.json: {e}"))
    })?;

    Ok(artisan_json_sha256)
}

/// Remove `.max-code/commands/artisan.meta.json` para forçar revalidação preservando `artisan.json`.
pub fn invalidate_sidecar(root: &Path) {
    if let Ok(meta_path) =
        ensure_safe_under_root(root, Path::new(".max-code/commands/artisan.meta.json"))
    {
        let _ = fs::remove_file(meta_path);
    }
}
