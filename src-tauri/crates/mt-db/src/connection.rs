//! Abertura segura de conexões SQLite em modo WAL, permissões Unix (`0700`/`0600`),
//! lock exclusivo de instância (`flock`) e verificação de integridade (`quick_check`).

use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::os::fd::AsFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use rustix::fs::{FlockOperation, flock};
use rustix::io::Errno;

use crate::config::{DbConfig, PragmaSet};
use crate::error::{DbError, DbResult};

/// Versão mínima exigida do SQLite empacotado (`3.51.3` -> `3051003`).
pub const MIN_SQLITE_VERSION_NUMBER: i64 = 3_051_003;

/// Retorna o caminho do arquivo `-wal` associado ao arquivo `.db`.
#[must_use]
pub fn wal_path_for(db_path: &Path) -> PathBuf {
    let mut os = OsString::from(db_path.as_os_str());
    os.push("-wal");
    PathBuf::from(os)
}

/// Retorna o caminho do arquivo `-shm` associado ao arquivo `.db`.
#[must_use]
pub fn shm_path_for(db_path: &Path) -> PathBuf {
    let mut os = OsString::from(db_path.as_os_str());
    os.push("-shm");
    PathBuf::from(os)
}

/// Garante que um diretório exista, não seja um link simbólico e possua permissão `0o700`.
pub fn ensure_secure_dir(dir: &Path) -> DbResult<()> {
    if let Ok(meta) = fs::symlink_metadata(dir) {
        if meta.file_type().is_symlink() {
            return Err(DbError::OpenFailed {
                path: dir.to_path_buf(),
                reason: String::from("O diretório configurado não pode ser um link simbólico."),
            });
        }
        if !meta.is_dir() {
            return Err(DbError::OpenFailed {
                path: dir.to_path_buf(),
                reason: String::from("O caminho configurado para diretório não é uma pasta."),
            });
        }
        let mut perms = meta.permissions();
        if (perms.mode() & 0o777) != 0o700 {
            perms.set_mode(0o700);
            fs::set_permissions(dir, perms).map_err(|e| DbError::OpenFailed {
                path: dir.to_path_buf(),
                reason: format!("Falha ao ajustar permissão 0700 no diretório: {e}"),
            })?;
        }
        return Ok(());
    }

    let mut builder = DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(dir).map_err(|e| DbError::OpenFailed {
        path: dir.to_path_buf(),
        reason: format!("Falha ao criar diretório seguro (0700): {e}"),
    })?;

    fs::set_permissions(dir, Permissions::from_mode(0o700)).map_err(|e| DbError::OpenFailed {
        path: dir.to_path_buf(),
        reason: format!("Falha ao aplicar permissão 0700 no diretório: {e}"),
    })
}

/// Rejeita links simbólicos e aplica permissão `0o600` caso o arquivo exista no disco.
pub fn set_file_mode_0600(path: &Path) -> DbResult<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(DbError::OpenFailed {
                    path: path.to_path_buf(),
                    reason: String::from(
                        "O arquivo do banco de dados não pode ser um link simbólico.",
                    ),
                });
            }
            if meta.is_file() {
                let mut perms = meta.permissions();
                if (perms.mode() & 0o777) != 0o600 {
                    perms.set_mode(0o600);
                    fs::set_permissions(path, perms).map_err(|e| DbError::OpenFailed {
                        path: path.to_path_buf(),
                        reason: format!("Falha ao aplicar permissão 0600 no arquivo: {e}"),
                    })?;
                }
            }
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(DbError::OpenFailed {
            path: path.to_path_buf(),
            reason: format!("Falha ao inspecionar metadados do arquivo: {err}"),
        }),
    }
}

/// Guarda o descritor de arquivo do lock exclusivo (`<data>/maxterminal.db.lock`) adquirido via `flock`.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
    lock_path: PathBuf,
}

impl InstanceLock {
    /// Adquire o lock exclusivo não bloqueante (`LOCK_EX | LOCK_NB`).
    ///
    /// Retorna `Err(DbError::LockedByOtherInstance)` caso outra instância já detenha o lock.
    pub fn acquire(lock_path: &Path) -> DbResult<Self> {
        if let Some(parent) = lock_path.parent() {
            ensure_secure_dir(parent)?;
        }
        set_file_mode_0600(lock_path)?;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(lock_path)
            .map_err(|e| DbError::OpenFailed {
                path: lock_path.to_path_buf(),
                reason: format!("Não foi possível abrir o arquivo de lock: {e}"),
            })?;

        set_file_mode_0600(lock_path)?;

        match flock(file.as_fd(), FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(Self {
                _file: file,
                lock_path: lock_path.to_path_buf(),
            }),
            Err(Errno::WOULDBLOCK) => Err(DbError::LockedByOtherInstance {
                lock_path: lock_path.to_path_buf(),
            }),
            Err(err) => Err(DbError::OpenFailed {
                path: lock_path.to_path_buf(),
                reason: format!("Falha ao adquirir flock exclusivo: {err}"),
            }),
        }
    }

    /// Retorna o caminho do arquivo `.lock` mantido por esta instância.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.lock_path
    }
}

/// Resultado da abertura da conexão de escrita (`open_writer`).
#[derive(Debug)]
pub struct OpenedWriter {
    /// Conexão SQLite configurada para a thread escritora.
    pub conn: Connection,
    /// Indica se o banco já existia e continha bytes antes desta abertura.
    pub existed_non_empty: bool,
}

/// Garante diretórios (`0o700`), valida ausência de symlinks, pré-cria o arquivo `.db` com `0o600`,
/// abre a conexão de escrita e aplica todos os `PRAGMA`s obrigatórios do modo WAL.
pub fn open_writer(cfg: &DbConfig) -> DbResult<OpenedWriter> {
    if let Some(parent) = cfg.db_path.parent() {
        ensure_secure_dir(parent)?;
    }
    ensure_secure_dir(&cfg.backup_dir)?;
    ensure_secure_dir(&cfg.quarantine_dir)?;

    let wal_path = wal_path_for(&cfg.db_path);
    let shm_path = shm_path_for(&cfg.db_path);

    set_file_mode_0600(&cfg.db_path)?;
    set_file_mode_0600(&wal_path)?;
    set_file_mode_0600(&shm_path)?;

    let existed_non_empty = match fs::metadata(&cfg.db_path) {
        Ok(meta) => meta.len() > 0,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let _created = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&cfg.db_path)
                .map_err(|e| DbError::OpenFailed {
                    path: cfg.db_path.clone(),
                    reason: format!("Falha ao criar arquivo inicial do banco (0600): {e}"),
                })?;
            false
        }
        Err(err) => {
            return Err(DbError::OpenFailed {
                path: cfg.db_path.clone(),
                reason: format!("Falha ao verificar arquivo do banco: {err}"),
            });
        }
    };

    set_file_mode_0600(&cfg.db_path)?;

    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;

    let conn = Connection::open_with_flags(&cfg.db_path, flags)
        .map_err(|err| map_open_or_corrupt(&cfg.db_path, &err))?;

    if !existed_non_empty {
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")
            .map_err(|err| map_open_or_corrupt(&cfg.db_path, &err))?;
    }

    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode = WAL;", [], |row| row.get(0))
        .map_err(|err| map_open_or_corrupt(&cfg.db_path, &err))?;

    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(DbError::OpenFailed {
            path: cfg.db_path.clone(),
            reason: format!(
                "Não foi possível ativar PRAGMA journal_mode=WAL (obtido: {journal_mode})"
            ),
        });
    }

    let pragma_sql = format!(
        "PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = {busy_timeout};
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -{cache_kib};
         PRAGMA mmap_size = {mmap_size};
         PRAGMA wal_autocheckpoint = {autocheckpoint};
         PRAGMA journal_size_limit = {journal_limit};
         PRAGMA cell_size_check = ON;
         PRAGMA trusted_schema = ON;",
        busy_timeout = cfg.pragmas.busy_timeout_ms,
        cache_kib = cfg.pragmas.cache_size_kib.abs(),
        mmap_size = cfg.pragmas.mmap_size_bytes,
        autocheckpoint = cfg.pragmas.wal_autocheckpoint_pages,
        journal_limit = cfg.pragmas.journal_size_limit_bytes,
    );

    conn.execute_batch(&pragma_sql)
        .map_err(|err| map_open_or_corrupt(&cfg.db_path, &err))?;

    conn.set_prepared_statement_cache_capacity(64);

    // Se a ativação do modo WAL criou os arquivos -wal e -shm, garante modo 0600 neles.
    set_file_mode_0600(&cfg.db_path)?;
    set_file_mode_0600(&wal_path)?;
    set_file_mode_0600(&shm_path)?;

    Ok(OpenedWriter {
        conn,
        existed_non_empty,
    })
}

/// Aplica os `PRAGMA`s obrigatórios em uma conexão somente leitura.
pub fn apply_reader_pragmas(conn: &Connection, pragmas: &PragmaSet) -> Result<(), rusqlite::Error> {
    let sql = format!(
        "PRAGMA query_only = ON;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = {busy_timeout};
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -{cache_kib};
         PRAGMA mmap_size = {mmap_size};
         PRAGMA trusted_schema = OFF;",
        busy_timeout = pragmas.busy_timeout_ms,
        cache_kib = pragmas.cache_size_kib.abs(),
        mmap_size = pragmas.mmap_size_bytes,
    );
    conn.execute_batch(&sql)?;
    conn.set_prepared_statement_cache_capacity(64);
    Ok(())
}

/// Abre uma conexão individual somente leitura com todos os `PRAGMA`s de leitor aplicados.
pub fn open_reader(path: &Path, pragmas: &PragmaSet) -> DbResult<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;

    let conn =
        Connection::open_with_flags(path, flags).map_err(|err| map_open_or_corrupt(path, &err))?;

    apply_reader_pragmas(&conn, pragmas).map_err(|err| map_open_or_corrupt(path, &err))?;
    Ok(conn)
}

/// Verifica em runtime que o SQLite empacotado tem versão `>= 3.51.3` e suporte a `ENABLE_FTS5`.
///
/// Retorna a string da versão do SQLite (ex.: `"3.51.3"`).
pub fn verify_sqlite_capabilities(conn: &Connection, db_path: &Path) -> DbResult<String> {
    let (version_str, fts5_enabled): (String, i64) = conn
        .query_row(
            "SELECT sqlite_version(), sqlite_compileoption_used('ENABLE_FTS5');",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|err| map_open_or_corrupt(db_path, &err))?;

    let version_num = i64::from(rusqlite::version_number());
    if version_num < MIN_SQLITE_VERSION_NUMBER {
        return Err(DbError::OpenFailed {
            path: db_path.to_path_buf(),
            reason: format!(
                "Versão do SQLite ({version_str}) inferior ao mínimo exigido (3.51.3)."
            ),
        });
    }

    if fts5_enabled != 1 {
        return Err(DbError::OpenFailed {
            path: db_path.to_path_buf(),
            reason: String::from("Extensão FTS5 não está habilitada nesta compilação do SQLite."),
        });
    }

    Ok(version_str)
}

/// Executa `PRAGMA quick_check` e valida que todas as linhas retornadas são `"ok"`.
pub fn run_quick_check(conn: &Connection, db_path: &Path) -> DbResult<()> {
    let mut stmt = conn
        .prepare("PRAGMA quick_check;")
        .map_err(|err| map_corrupt(db_path, &err))?;

    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|err| map_corrupt(db_path, &err))?;

    let mut count = 0usize;
    for res in rows {
        let line = res.map_err(|err| map_corrupt(db_path, &err))?;
        count += 1;
        if !line.eq_ignore_ascii_case("ok") {
            return Err(DbError::Corrupted {
                path: db_path.to_path_buf(),
                detail: line,
            });
        }
    }

    if count == 0 {
        return Err(DbError::Corrupted {
            path: db_path.to_path_buf(),
            detail: String::from("PRAGMA quick_check não retornou resultados"),
        });
    }

    Ok(())
}

fn map_open_or_corrupt(path: &Path, err: &rusqlite::Error) -> DbError {
    if let rusqlite::Error::SqliteFailure(ffi_err, msg) = err
        && matches!(
            ffi_err.code,
            rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
        )
    {
        return DbError::Corrupted {
            path: path.to_path_buf(),
            detail: msg
                .clone()
                .unwrap_or_else(|| String::from("arquivo SQLite inválido ou corrompido")),
        };
    }
    DbError::OpenFailed {
        path: path.to_path_buf(),
        reason: err.to_string(),
    }
}

fn map_corrupt(path: &Path, err: &rusqlite::Error) -> DbError {
    DbError::Corrupted {
        path: path.to_path_buf(),
        detail: err.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn pragmas_applied() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = DbConfig::for_path(dir.path().join("maxterminal.db"));
        let opened = open_writer(&cfg).unwrap();
        let conn = opened.conn;

        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_lowercase(), "wal");

        let auto_vacuum: i64 = conn
            .query_row("PRAGMA auto_vacuum;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(auto_vacuum, 2); // INCREMENTAL

        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);

        let busy_timeout: i64 = conn
            .query_row("PRAGMA busy_timeout;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(busy_timeout, 5_000);

        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(synchronous, 1); // NORMAL

        // Cria uma tabela mínima para que os arquivos -wal/-shm existam e o leitor possa abrir
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY) STRICT;")
            .unwrap();

        let reader = open_reader(&cfg.db_path, &cfg.pragmas).unwrap();
        let query_only: i64 = reader
            .query_row("PRAGMA query_only;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(query_only, 1);
    }

    #[test]
    fn permissions_0600() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().join("data");
        let cfg = DbConfig::for_path(data_dir.join("maxterminal.db"));
        let _lock = InstanceLock::acquire(&cfg.lock_path).unwrap();
        let opened = open_writer(&cfg).unwrap();
        opened
            .conn
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY) STRICT; INSERT INTO t VALUES (1);",
            )
            .unwrap();

        let wal = wal_path_for(&cfg.db_path);
        let shm = shm_path_for(&cfg.db_path);
        set_file_mode_0600(&wal).unwrap();
        set_file_mode_0600(&shm).unwrap();

        let dir_mode = fs::metadata(&data_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);

        let db_mode = fs::metadata(&cfg.db_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(db_mode, 0o600);

        if wal.exists() {
            let wal_mode = fs::metadata(&wal).unwrap().permissions().mode() & 0o777;
            assert_eq!(wal_mode, 0o600);
        }
        if shm.exists() {
            let shm_mode = fs::metadata(&shm).unwrap().permissions().mode() & 0o777;
            assert_eq!(shm_mode, 0o600);
        }
    }

    #[test]
    fn instance_lock_prevents_second_writer() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join("maxterminal.db.lock");
        let _first = InstanceLock::acquire(&lock_path).unwrap();

        let second = InstanceLock::acquire(&lock_path);
        assert!(matches!(second, Err(DbError::LockedByOtherInstance { .. })));
    }
}
