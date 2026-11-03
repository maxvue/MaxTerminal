//! Configuração do banco SQLite local (`DbConfig`) e conjunto de `PRAGMA`s (`PragmaSet`).

use std::path::{Path, PathBuf};

use mt_core::paths::{AppPaths, EnvSource};

use crate::error::{DbError, DbResult};

/// Conjunto de parâmetros ajustáveis dos `PRAGMA`s de escrita e leitura do SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PragmaSet {
    /// Tempo máximo de espera por locks internos do SQLite em milissegundos (`PRAGMA busy_timeout`).
    pub busy_timeout_ms: u32,
    /// Tamanho do cache de páginas por conexão em KiB (`PRAGMA cache_size = -<kib>`).
    pub cache_size_kib: i64,
    /// Limite de mapeamento em memória em bytes (`PRAGMA mmap_size`).
    pub mmap_size_bytes: i64,
    /// Limiar de páginas para autocheckpoint do WAL (`PRAGMA wal_autocheckpoint`).
    pub wal_autocheckpoint_pages: u32,
    /// Limite de tamanho do arquivo `-wal` após checkpoint (`PRAGMA journal_size_limit`).
    pub journal_size_limit_bytes: i64,
}

impl Default for PragmaSet {
    fn default() -> Self {
        Self {
            busy_timeout_ms: 5_000,
            cache_size_kib: 16_384,
            mmap_size_bytes: 268_435_456, // 256 MiB
            wal_autocheckpoint_pages: 1_000,
            journal_size_limit_bytes: 67_108_864, // 64 MiB
        }
    }
}

/// Configuração completa de caminhos, limites de fila, pool de leitura e `PRAGMA`s do `mt-db`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbConfig {
    /// Caminho absoluto do arquivo principal SQLite (`maxterminal.db`).
    pub db_path: PathBuf,
    /// Diretório de armazenamento de backups online e pré-migration (`backups/`).
    pub backup_dir: PathBuf,
    /// Caminho do arquivo de lock exclusivo de instância (`maxterminal.db.lock`).
    pub lock_path: PathBuf,
    /// Diretório de quarentena para arquivos corrompidos ou pré-reset (`quarantine/`).
    pub quarantine_dir: PathBuf,
    /// Quantidade máxima de conexões simultâneas no `ReadPool` (`r2d2`).
    pub reader_pool_size: u32,
    /// Capacidade máxima de jobs pendentes na fila do `DbWriter`.
    pub write_queue_capacity: usize,
    /// Orçamento máximo compartilhado de bytes pendentes (`ByteBudget`).
    pub write_queue_bytes: usize,
    /// Conjunto de `PRAGMA`s aplicados nas conexões.
    pub pragmas: PragmaSet,
    /// Define se o agendador periódico de manutenção deve iniciar automaticamente em background.
    pub enable_background_maintenance: bool,
}

impl DbConfig {
    /// Constrói uma configuração isolada derivada do caminho do arquivo `.db`.
    #[must_use]
    pub fn for_path(db_path: impl Into<PathBuf>) -> Self {
        let resolved_db_path = db_path.into();
        let parent_dir = resolved_db_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        Self {
            backup_dir: parent_dir.join("backups"),
            lock_path: parent_dir.join("maxterminal.db.lock"),
            quarantine_dir: parent_dir.join("quarantine"),
            db_path: resolved_db_path,
            reader_pool_size: 4,
            write_queue_capacity: 4_096,
            write_queue_bytes: 16 * 1024 * 1024, // 16 MiB
            pragmas: PragmaSet::default(),
            enable_background_maintenance: false,
        }
    }

    /// Resolve `DbConfig` a partir de `AppPaths` e de um `EnvSource` (suportando `MAXTERMINAL_DB_PATH`).
    pub fn from_paths(paths: &AppPaths, env: &impl EnvSource) -> DbResult<Self> {
        let target_path = if let Some(raw_override) = env.var("MAXTERMINAL_DB_PATH") {
            let trimmed = raw_override.trim();
            let candidate = PathBuf::from(trimmed);
            if trimmed.is_empty() || !candidate.is_absolute() {
                return Err(DbError::OpenFailed {
                    path: candidate,
                    reason: String::from(
                        "A variável MAXTERMINAL_DB_PATH deve conter um caminho absoluto válido.",
                    ),
                });
            }
            candidate
        } else {
            paths.db_path.clone()
        };

        let mut cfg = Self::for_path(target_path);
        cfg.enable_background_maintenance = true;
        Ok(cfg)
    }

    /// Retorna o caminho do arquivo `.db`.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.db_path
    }
}
