//! Cálculo de fingerprint determinístico dos arquivos que influenciam o catálogo Artisan.

use std::path::Path;

use mt_core::MtResult;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::error;
use crate::fs_util::file_stamp;

const MAX_SINGLE_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_COMMANDS_DIR_FILES: usize = 5_000;
const MAX_COMMANDS_DIR_DEPTH: usize = 6;

/// Carimbo de um arquivo individual monitorado pelo fingerprint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    /// Timestamp de modificação em nanossegundos desde a Unix Epoch.
    pub mtime_ns: i64,
    /// Tamanho do arquivo em bytes.
    pub size: u64,
    /// Hash SHA-256 hexadecimal do conteúdo do arquivo.
    pub content_sha256: String,
}

/// Carimbo agregado do diretório `app/Console/Commands/**/*.php`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirStamp {
    /// Quantidade total de arquivos `.php` considerados.
    pub file_count: u32,
    /// Hash SHA-256 ordenado dos pares `(caminho_relativo, mtime_ns, size, content_sha256)`.
    pub entries_sha256: String,
}

/// Fingerprint estrutural completo do projeto Laravel para validação do cache de comandos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    /// Carimbo do script `artisan`.
    pub artisan: Option<FileStamp>,
    /// Carimbo do `composer.json`.
    pub composer_json: Option<FileStamp>,
    /// Carimbo do `composer.lock`.
    pub composer_lock: Option<FileStamp>,
    /// Carimbo do `bootstrap/app.php`.
    pub bootstrap_app: Option<FileStamp>,
    /// Carimbo do `routes/console.php`.
    pub console_routes: Option<FileStamp>,
    /// Carimbo recursivo de `app/Console/Commands`.
    pub commands_dir: DirStamp,
}

/// Calcula o [`Fingerprint`] da raiz Laravel `root` (máx. 5 000 arquivos em `app/Console/Commands`, profundidade 6, sem seguir symlinks).
pub fn compute(root: &Path) -> MtResult<Fingerprint> {
    let artisan = file_stamp(&root.join("artisan"), MAX_SINGLE_FILE_BYTES)
        .map_err(|e| error::write_failed(format!("Falha ao ler artisan: {e}")))?;
    let composer_json = file_stamp(&root.join("composer.json"), MAX_SINGLE_FILE_BYTES)
        .map_err(|e| error::write_failed(format!("Falha ao ler composer.json: {e}")))?;
    let composer_lock = file_stamp(&root.join("composer.lock"), MAX_SINGLE_FILE_BYTES)
        .map_err(|e| error::write_failed(format!("Falha ao ler composer.lock: {e}")))?;
    let bootstrap_app = file_stamp(&root.join("bootstrap/app.php"), MAX_SINGLE_FILE_BYTES)
        .map_err(|e| error::write_failed(format!("Falha ao ler bootstrap/app.php: {e}")))?;
    let console_routes = file_stamp(&root.join("routes/console.php"), MAX_SINGLE_FILE_BYTES)
        .map_err(|e| error::write_failed(format!("Falha ao ler routes/console.php: {e}")))?;

    let commands_dir = compute_commands_dir_stamp(root)?;

    Ok(Fingerprint {
        artisan,
        composer_json,
        composer_lock,
        bootstrap_app,
        console_routes,
        commands_dir,
    })
}

fn compute_commands_dir_stamp(root: &Path) -> MtResult<DirStamp> {
    let dir = root.join("app/Console/Commands");
    if !dir.exists() {
        return Ok(DirStamp {
            file_count: 0,
            entries_sha256: crate::fs_util::sha256_hex(b""),
        });
    }

    let mut entries: Vec<(String, FileStamp)> = Vec::new();
    for entry_res in WalkDir::new(&dir)
        .follow_links(false)
        .max_depth(MAX_COMMANDS_DIR_DEPTH)
    {
        let entry = entry_res.map_err(|e| {
            error::write_failed(format!("Falha ao varrer app/Console/Commands: {e}"))
        })?;
        if entry.file_type().is_symlink() {
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("php") {
            continue;
        }
        if entries.len() >= MAX_COMMANDS_DIR_FILES {
            return Err(error::write_failed(format!(
                "O diretório app/Console/Commands excede o limite de {MAX_COMMANDS_DIR_FILES} arquivos para fingerprint."
            )));
        }
        if let Some(stamp) = file_stamp(path, MAX_SINGLE_FILE_BYTES).map_err(|e| {
            error::write_failed(format!("Falha no fingerprint de {}: {e}", path.display()))
        })? {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            entries.push((rel, stamp));
        }
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel, st) in &entries {
        hasher.update(rel.as_bytes());
        hasher.update(b"\0");
        hasher.update(st.mtime_ns.to_le_bytes());
        hasher.update(st.size.to_le_bytes());
        hasher.update(st.content_sha256.as_bytes());
        hasher.update(b"\n");
    }

    Ok(DirStamp {
        file_count: u32::try_from(entries.len()).unwrap_or(u32::MAX),
        entries_sha256: hex::encode(hasher.finalize()),
    })
}
