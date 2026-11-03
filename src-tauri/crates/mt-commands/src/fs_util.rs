//! Utilitários seguros de sistema de arquivos: escrita atômica de cache, criação exclusiva (`O_EXCL`),
//! leitura limitada (`read_capped`), cálculo de carimbo (`FileStamp`) e verificação de fronteira sem symlinks.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use mt_core::MtResult;
use rand::Rng;
use sha2::{Digest, Sha256};

use crate::artisan::fingerprint::FileStamp;
use crate::error;

/// Verifica se `candidate` está estritamente dentro de `root` e nenhum componente intermediário é um symlink.
pub fn ensure_safe_under_root(root: &Path, candidate: &Path) -> MtResult<PathBuf> {
    let canonical_root = fs::canonicalize(root).map_err(|e| {
        error::unsafe_path(format!(
            "Raiz do projeto inválida ou inacessível ({}): {e}",
            root.display()
        ))
    })?;

    let rel = if candidate.is_absolute() {
        if let Ok(stripped) = candidate.strip_prefix(&canonical_root) {
            stripped.to_path_buf()
        } else if let Ok(stripped) = candidate.strip_prefix(root) {
            stripped.to_path_buf()
        } else {
            return Err(error::root_forbidden(format!(
                "O caminho '{}' está fora da raiz autorizada '{}'.",
                candidate.display(),
                canonical_root.display()
            )));
        }
    } else {
        candidate.to_path_buf()
    };

    let mut cur = canonical_root.clone();
    for comp in rel.components() {
        match comp {
            Component::Normal(part) => {
                cur.push(part);
                if let Ok(meta) = fs::symlink_metadata(&cur) {
                    if meta.file_type().is_symlink() {
                        return Err(error::unsafe_path(format!(
                            "Symlink não permitido no caminho '{}'.",
                            cur.display()
                        )));
                    }
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(error::unsafe_path(format!(
                    "Componente de caminho inválido em '{}'.",
                    candidate.display()
                )));
            }
        }
    }

    if !cur.starts_with(&canonical_root) {
        return Err(error::root_forbidden(format!(
            "Caminho resolvido '{}' escapa da raiz '{}'.",
            cur.display(),
            canonical_root.display()
        )));
    }

    Ok(cur)
}

/// Verifica se `candidate` (existente ou cujo pai exista) está dentro de `root`.
#[must_use]
pub fn is_inside(root: &Path, candidate: &Path) -> bool {
    ensure_safe_under_root(root, candidate).is_ok()
}

/// Cria recursivamente diretórios sob `root` rejeitando symlinks em cada nível.
pub fn create_dirs_safe(root: &Path, target_dir: &Path) -> MtResult<PathBuf> {
    let resolved = ensure_safe_under_root(root, target_dir)?;
    let canonical_root = fs::canonicalize(root)
        .map_err(|e| error::write_failed(format!("Falha ao resolver raiz: {e}")))?;
    let rel = resolved
        .strip_prefix(&canonical_root)
        .map_err(|_| error::root_forbidden("Diretório fora da raiz autorizada."))?;

    let mut cur = canonical_root;
    for comp in rel.components() {
        if let Component::Normal(name) = comp {
            cur.push(name);
            match fs::symlink_metadata(&cur) {
                Ok(meta) => {
                    if meta.file_type().is_symlink() {
                        return Err(error::unsafe_path(format!(
                            "Symlink detectado em '{}'.",
                            cur.display()
                        )));
                    }
                    if !meta.is_dir() {
                        return Err(error::write_failed(format!(
                            "O caminho '{}' já existe e não é um diretório.",
                            cur.display()
                        )));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    fs::create_dir(&cur).map_err(|ce| {
                        error::write_failed(format!(
                            "Não foi possível criar o diretório '{}': {ce}",
                            cur.display()
                        ))
                    })?;
                }
                Err(e) => {
                    return Err(error::write_failed(format!(
                        "Falha ao inspecionar '{}': {e}",
                        cur.display()
                    )));
                }
            }
        }
    }
    Ok(cur)
}

/// Grava `bytes` atomicamente em `path` usando arquivo temporário no mesmo diretório, `sync_all` e `rename`.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Caminho sem diretório pai para escrita atômica.",
        )
    })?;
    fs::create_dir_all(parent)?;

    let existing_perms = fs::metadata(path).ok().map(|m| m.permissions());
    let pid = std::process::id();
    let rnd: u64 = rand::rng().random();
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("tmp");
    let tmp_path = parent.join(format!(".{file_name}.tmp-{pid}-{rnd:016x}"));

    let write_res = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        if let Some(perms) = existing_perms {
            let _ = file.set_permissions(perms);
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp_path, path)?;
        Ok(())
    })();

    if write_res.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    write_res
}

/// Cria exclusivamente um novo arquivo (`create_new(true)` / `O_EXCL`), sincroniza e retorna seu inode e sha256.
pub fn create_new_file(path: &Path, bytes: &[u8], rel_for_error: &str) -> MtResult<(u64, String)> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                error::file_exists(rel_for_error)
            } else {
                error::write_failed(format!("Falha ao criar '{rel_for_error}': {e}"))
            }
        })?;

    if let Err(e) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error::write_failed(format!(
            "Falha ao gravar '{rel_for_error}': {e}"
        )));
    }

    let meta = file.metadata().map_err(|e| {
        error::write_failed(format!("Falha ao ler metadados de '{rel_for_error}': {e}"))
    })?;
    let ino = meta.ino();
    let hash = sha256_hex(bytes);
    Ok((ino, hash))
}

/// Lê um arquivo de texto até `max_bytes` bytes (sem seguir symlinks no arquivo final).
/// Retorna `Ok(None)` se o arquivo não existir e `Err` se for symlink ou exceder `max_bytes`.
pub fn read_capped(path: &Path, max_bytes: usize) -> io::Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Symlink rejeitado em '{}'", path.display()),
                ));
            }
            if !meta.is_file() {
                return Ok(None);
            }
            let size = usize::try_from(meta.len()).unwrap_or(usize::MAX);
            if size > max_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Arquivo '{}' excede o limite de {max_bytes} bytes.",
                        path.display()
                    ),
                ));
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    }

    let file = File::open(path)?;
    let mut buf = Vec::new();
    let limit = u64::try_from(max_bytes.saturating_add(1)).unwrap_or(u64::MAX);
    file.take(limit).read_to_end(&mut buf)?;
    if buf.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Arquivo '{}' excede o limite de {max_bytes} bytes.",
                path.display()
            ),
        ));
    }
    String::from_utf8(buf)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Calcula o hash SHA-256 em hexadecimal minúsculo para `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Calcula o [`FileStamp`] (mtime em ns, tamanho e SHA-256) de `path`, ou `Ok(None)` se não existir.
pub fn file_stamp(path: &Path, max_bytes: usize) -> io::Result<Option<FileStamp>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Ok(None);
    }
    let size = meta.len();
    if usize::try_from(size).unwrap_or(usize::MAX) > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Arquivo '{}' excede limite de stamp.", path.display()),
        ));
    }
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_nanos()).ok())
        .unwrap_or(0);
    let mut file = File::open(path)?;
    let mut buf = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    file.read_to_end(&mut buf)?;
    Ok(Some(FileStamp {
        mtime_ns,
        size,
        content_sha256: sha256_hex(&buf),
    }))
}
