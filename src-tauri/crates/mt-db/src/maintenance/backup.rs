//! Online Backup API (`rusqlite::backup::Backup`), rotação de backups e recuperação explícita de corrupção.

use std::fs::{self, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};

use crate::config::DbConfig;
use crate::connection::{
    ensure_secure_dir, run_quick_check, set_file_mode_0600, shm_path_for, wal_path_for,
};
use crate::error::{DbError, DbResult};
use crate::models::{BackupEntryDto, DbRecovered, DbRecoveryAction, RestoreScheduledDto};
use crate::time::{format_utc_compact, now_unix_ms};

static BACKUP_SEQ: AtomicU64 = AtomicU64::new(1);

/// Limite máximo de backups `pre_migration` mantidos na rotação.
pub const MAX_PRE_MIGRATION_BACKUPS: usize = 3;

/// Categoria semântica de um arquivo de backup SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupKind {
    /// Backup periódico diário (`"daily"`).
    Daily,
    /// Backup solicitado manualmente (`"manual"`).
    Manual,
    /// Backup automático antes de aplicar migrations pendentes (`"pre_migration"`).
    PreMigration {
        /// Versão anterior do schema.
        from_version: i64,
        /// Versão alvo da migration.
        to_version: i64,
    },
    /// Backup/quarentena antes de um reset explícito (`"pre_reset"`).
    PreReset,
    /// Backup automático do banco atual antes de aplicar restauração agendada (`"pre_restore"`).
    PreRestore,
    /// Arquivo corrompido movido para quarentena (`"corrupted"`).
    Corrupted,
}

impl BackupKind {
    /// Retorna o identificador textual canônico exposto em `BackupEntryDto.kind`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Manual => "manual",
            Self::PreMigration { .. } => "pre_migration",
            Self::PreReset => "pre_reset",
            Self::PreRestore => "pre_restore",
            Self::Corrupted => "corrupted",
        }
    }

    fn file_tag(self) -> String {
        match self {
            Self::Daily => String::from("daily"),
            Self::Manual => String::from("manual"),
            Self::PreMigration {
                from_version,
                to_version,
            } => format!("premigration-v{from_version}-to-v{to_version}"),
            Self::PreReset => String::from("pre_reset"),
            Self::PreRestore => String::from("pre_restore"),
            Self::Corrupted => String::from("corrupted"),
        }
    }
}

/// Cria um backup consistente usando a Online Backup API do SQLite (`step(256)` com pausa de `10 ms` e deadline de `60 s`).
///
/// O arquivo é gravado inicialmente como `.tmp` com permissão `0o600`, validado via `PRAGMA quick_check`,
/// sincronizado em disco e renomeado atomicamente antes de executar a rotação de retenção.
#[allow(clippy::too_many_lines)]
pub fn create_backup_blocking(
    source_db_path: &Path,
    backup_dir: &Path,
    kind: BackupKind,
    daily_keep: usize,
) -> DbResult<BackupEntryDto> {
    ensure_secure_dir(backup_dir)?;

    let now_ms = now_unix_ms();
    let ts_compact = format_utc_compact(now_ms);
    let seq = BACKUP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tag = kind.file_tag();
    let file_name = format!("maxterminal-{ts_compact}-{seq:03}-{tag}.db");
    let final_path = backup_dir.join(&file_name);
    let tmp_path = backup_dir.join(format!(".{file_name}.tmp"));

    let _ = fs::remove_file(&tmp_path);

    let _tmp_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp_path)
        .map_err(|e| DbError::BackupFailed {
            reason: format!("Falha ao criar arquivo temporário de backup: {e}"),
        })?;
    set_file_mode_0600(&tmp_path)?;

    let src_flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;
    let src_conn = Connection::open_with_flags(source_db_path, src_flags).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        DbError::BackupFailed {
            reason: format!("Falha ao abrir banco de origem para backup: {e}"),
        }
    })?;
    let _ = src_conn.busy_timeout(Duration::from_secs(5));

    let dst_flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;
    let mut dst_conn = Connection::open_with_flags(&tmp_path, dst_flags).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        DbError::BackupFailed {
            reason: format!("Falha ao abrir arquivo de destino do backup: {e}"),
        }
    })?;

    let deadline = Instant::now() + Duration::from_secs(60);
    {
        let backup = Backup::new(&src_conn, &mut dst_conn).map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            DbError::BackupFailed {
                reason: format!("Falha ao iniciar Online Backup API: {e}"),
            }
        })?;

        loop {
            match backup.step(256) {
                Ok(StepResult::Done) => break,
                Ok(_) => {
                    if Instant::now() >= deadline {
                        drop(backup);
                        let _ = fs::remove_file(&tmp_path);
                        return Err(DbError::BackupFailed {
                            reason: String::from(
                                "Tempo limite de 60s esgotado ao gerar backup online.",
                            ),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => {
                    drop(backup);
                    let _ = fs::remove_file(&tmp_path);
                    return Err(DbError::BackupFailed {
                        reason: format!("Erro durante cópia de páginas do backup: {err}"),
                    });
                }
            }
        }
    }

    if let Err(err) = run_quick_check(&dst_conn, &tmp_path) {
        drop(dst_conn);
        let _ = fs::remove_file(&tmp_path);
        return Err(DbError::BackupFailed {
            reason: format!("O arquivo de backup gerado falhou no quick_check: {err}"),
        });
    }
    drop(dst_conn);

    if let Ok(f) = OpenOptions::new().write(true).open(&tmp_path) {
        let _ = f.sync_all();
    }
    set_file_mode_0600(&tmp_path)?;

    fs::rename(&tmp_path, &final_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        DbError::BackupFailed {
            reason: format!("Falha ao renomear arquivo de backup final: {e}"),
        }
    })?;
    set_file_mode_0600(&final_path)?;

    let size_bytes = fs::metadata(&final_path).map_or(0, |m| i64::try_from(m.len()).unwrap_or(0));

    rotate_backups(backup_dir, daily_keep.max(1), MAX_PRE_MIGRATION_BACKUPS)?;

    Ok(BackupEntryDto {
        file_name,
        path: final_path.to_string_lossy().into_owned(),
        kind: kind.as_str().to_string(),
        created_at: now_ms,
        size_bytes,
    })
}

/// Lista todos os backups `.db` disponíveis em `backup_dir`, ordenados do mais recente para o mais antigo.
pub fn list_backups(backup_dir: &Path) -> DbResult<Vec<BackupEntryDto>> {
    if !backup_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(backup_dir).map_err(|e| DbError::BackupFailed {
        reason: format!("Falha ao listar diretório de backups: {e}"),
    })?;

    let mut list = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            continue;
        }
        let Some(name_str) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name_str.starts_with('.')
            || !std::path::Path::new(name_str)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("db"))
        {
            continue;
        }

        let kind = classify_backup_filename(name_str);
        #[allow(clippy::cast_possible_truncation)]
        let created_at = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_millis() as i64);
        let size_bytes = i64::try_from(meta.len()).unwrap_or(0);

        list.push(BackupEntryDto {
            file_name: name_str.to_string(),
            path: path.to_string_lossy().into_owned(),
            kind: kind.to_string(),
            created_at,
            size_bytes,
        });
    }

    list.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.file_name.cmp(&a.file_name))
    });
    Ok(list)
}

fn classify_backup_filename(file_name: &str) -> &'static str {
    if file_name.contains("-premigration-") || file_name.contains("-pre_migration") {
        "pre_migration"
    } else if file_name.contains("-pre_reset") {
        "pre_reset"
    } else if file_name.contains("-pre_restore") {
        "pre_restore"
    } else if file_name.contains("-corrupted") {
        "corrupted"
    } else if file_name.contains("-manual") {
        "manual"
    } else {
        "daily"
    }
}

fn rotate_backups(backup_dir: &Path, daily_keep: usize, pre_migration_keep: usize) -> DbResult<()> {
    let all = list_backups(backup_dir)?;

    let mut daily_count = 0usize;
    let mut premig_count = 0usize;

    for item in all {
        match item.kind.as_str() {
            "daily" => {
                daily_count += 1;
                if daily_count > daily_keep {
                    let _ = fs::remove_file(&item.path);
                }
            }
            "pre_migration" => {
                premig_count += 1;
                if premig_count > pre_migration_keep {
                    let _ = fs::remove_file(&item.path);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Move os arquivos `.db`, `-wal` e `-shm` atuais para quarentena preservando-os integralmente com modo `0o600`.
pub fn quarantine_corrupted_files(cfg: &DbConfig, tag: &str) -> DbResult<Option<PathBuf>> {
    ensure_secure_dir(&cfg.backup_dir)?;
    ensure_secure_dir(&cfg.quarantine_dir)?;

    let now_ms = now_unix_ms();
    let ts_compact = format_utc_compact(now_ms);
    let seq = BACKUP_SEQ.fetch_add(1, Ordering::Relaxed);
    let base_name = format!("maxterminal-{ts_compact}-{seq:03}-{tag}.db");

    let quarantined_db = cfg.backup_dir.join(&base_name);
    let mut moved_db = None;

    if cfg.db_path.exists() {
        set_file_mode_0600(&cfg.db_path)?;
        fs::rename(&cfg.db_path, &quarantined_db).map_err(|e| DbError::BackupFailed {
            reason: format!("Falha ao mover arquivo .db para quarentena: {e}"),
        })?;
        set_file_mode_0600(&quarantined_db)?;
        moved_db = Some(quarantined_db);
    }

    let wal_src = wal_path_for(&cfg.db_path);
    if wal_src.exists() {
        let wal_dst = cfg.quarantine_dir.join(format!("{base_name}-wal"));
        let _ = fs::rename(&wal_src, &wal_dst);
        let _ = set_file_mode_0600(&wal_dst);
    }

    let shm_src = shm_path_for(&cfg.db_path);
    if shm_src.exists() {
        let shm_dst = cfg.quarantine_dir.join(format!("{base_name}-shm"));
        let _ = fs::rename(&shm_src, &shm_dst);
        let _ = set_file_mode_0600(&shm_dst);
    }

    Ok(moved_db)
}

/// Valida um nome de arquivo de backup contra *path traversal* e garante que existe em `backup_dir`.
fn validate_backup_file_name(backup_dir: &Path, backup_file_name: &str) -> DbResult<PathBuf> {
    let trimmed = backup_file_name.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('.')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("..")
    {
        return Err(DbError::BackupFailed {
            reason: String::from("Nome de arquivo de backup inválido."),
        });
    }

    let rel_path = Path::new(trimmed);
    let mut components = rel_path.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(DbError::BackupFailed {
            reason: String::from("O arquivo de backup deve ser um nome simples em backups/."),
        });
    }

    if !rel_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("db"))
    {
        return Err(DbError::BackupFailed {
            reason: String::from("O arquivo de backup deve ter extensão .db."),
        });
    }

    let candidate = backup_dir.join(trimmed);
    let meta = fs::symlink_metadata(&candidate).map_err(|_| DbError::BackupFailed {
        reason: format!("Arquivo de backup '{trimmed}' não encontrado."),
    })?;

    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(DbError::BackupFailed {
            reason: String::from("O arquivo de backup selecionado não é um arquivo regular."),
        });
    }

    Ok(candidate)
}

/// Executa a ação explícita de recuperação (`RetryOpen`, `RestoreFromBackup`, `ResetDatabase`).
pub fn execute_recovery_action(
    cfg: &DbConfig,
    action: &DbRecoveryAction,
    known_max_version: i64,
) -> DbResult<DbRecovered> {
    match action {
        DbRecoveryAction::RetryOpen => Ok(DbRecovered {
            strategy: String::from("retry_open"),
            quarantined_path: None,
            recovered_at: now_unix_ms(),
        }),
        DbRecoveryAction::RestoreFromBackup { backup_file_name } => {
            let bkp_path = validate_backup_file_name(&cfg.backup_dir, backup_file_name)?;

            // Valida integridade, chaves estrangeiras e versão do backup antes de tocar no banco atual
            {
                let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX
                    | OpenFlags::SQLITE_OPEN_URI;
                let conn = Connection::open_with_flags(&bkp_path, flags).map_err(|e| {
                    DbError::BackupFailed {
                        reason: format!("Não foi possível abrir o backup selecionado: {e}"),
                    }
                })?;

                run_quick_check(&conn, &bkp_path).map_err(|_| DbError::BackupFailed {
                    reason: String::from(
                        "O arquivo de backup selecionado está corrompido (quick_check falhou).",
                    ),
                })?;

                let user_ver: i64 = conn
                    .query_row("PRAGMA user_version;", [], |r| r.get(0))
                    .map_err(|e| DbError::BackupFailed {
                        reason: format!("Falha ao ler versão do backup: {e}"),
                    })?;

                if user_ver > known_max_version {
                    return Err(DbError::SchemaNewerThanApp {
                        db_version: user_ver,
                        app_max_version: known_max_version,
                    });
                }

                let mut fk_stmt = conn.prepare("PRAGMA foreign_key_check;").map_err(|e| {
                    DbError::BackupFailed {
                        reason: format!("Falha ao validar FKs do backup: {e}"),
                    }
                })?;
                let mut rows = fk_stmt.query([]).map_err(|e| DbError::BackupFailed {
                    reason: format!("Falha ao consultar FKs do backup: {e}"),
                })?;
                if rows
                    .next()
                    .map_err(|e| DbError::BackupFailed {
                        reason: e.to_string(),
                    })?
                    .is_some()
                {
                    return Err(DbError::BackupFailed {
                        reason: String::from(
                            "O backup selecionado apresenta violação de chave estrangeira.",
                        ),
                    });
                }
            }

            let quarantined = quarantine_corrupted_files(cfg, "corrupted")?;

            let restore_tmp = cfg.db_path.with_extension("restore.tmp");
            let _ = fs::remove_file(&restore_tmp);
            fs::copy(&bkp_path, &restore_tmp).map_err(|e| DbError::BackupFailed {
                reason: format!("Falha ao copiar arquivo de backup para restauração: {e}"),
            })?;
            set_file_mode_0600(&restore_tmp)?;
            if let Ok(f) = OpenOptions::new().write(true).open(&restore_tmp) {
                let _ = f.sync_all();
            }
            fs::rename(&restore_tmp, &cfg.db_path).map_err(|e| DbError::BackupFailed {
                reason: format!("Falha ao ativar banco restaurado: {e}"),
            })?;
            set_file_mode_0600(&cfg.db_path)?;

            Ok(DbRecovered {
                strategy: String::from("restore_from_backup"),
                quarantined_path: quarantined.map(|p| p.to_string_lossy().into_owned()),
                recovered_at: now_unix_ms(),
            })
        }
        DbRecoveryAction::ResetDatabase => {
            let quarantined = quarantine_corrupted_files(cfg, "corrupted")?;
            Ok(DbRecovered {
                strategy: String::from("reset_database"),
                quarantined_path: quarantined.map(|p| p.to_string_lossy().into_owned()),
                recovered_at: now_unix_ms(),
            })
        }
    }
}

/// Etapa transacional da restauração agendada no boot (usada também para injeção determinística de falhas em testes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreStage {
    /// Etapa 1: Geração do backup `pre_restore` do banco atual.
    PreRestoreBackup,
    /// Etapa 2: Preparação/isolamento do banco atual antes da cópia.
    QuarantineCurrent,
    /// Etapa 3: Cópia do backup candidato para `.restore.tmp` + `fsync`.
    CopyCandidate,
    /// Etapa 4: Substituição atômica (`rename`) de `.restore.tmp` sobre `maxterminal.db`.
    AtomicSwap,
    /// Etapa 5: Execução das migrations pendentes sobre o banco restaurado.
    Migrate,
    /// Etapa 6: Rebuild integral dos índices FTS5 e backfill de projeções.
    RebuildFts,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingRestoreMarker {
    backup_file_name: String,
    scheduled_at_ms: i64,
}

/// Retorna o caminho do marcador `pending-restore.json` associado a um `data_dir` (ou diretório pai de `db_path`).
#[must_use]
pub fn pending_restore_marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join("pending-restore.json")
}

/// Valida integridade física (`quick_check`), referencial (`foreign_key_check`) e versão (`user_version <= known_max_version`)
/// de um arquivo de backup em `backup_dir`.
pub fn validate_candidate_backup(
    backup_dir: &Path,
    backup_file_name: &str,
    known_max_version: i64,
) -> DbResult<PathBuf> {
    let bkp_path = validate_backup_file_name(backup_dir, backup_file_name)?;

    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI;
    let conn =
        Connection::open_with_flags(&bkp_path, flags).map_err(|e| DbError::BackupFailed {
            reason: format!("Não foi possível abrir o backup selecionado: {e}"),
        })?;

    run_quick_check(&conn, &bkp_path).map_err(|_| DbError::BackupFailed {
        reason: String::from(
            "O arquivo de backup selecionado está corrompido (quick_check falhou).",
        ),
    })?;

    let user_ver: i64 = conn
        .query_row("PRAGMA user_version;", [], |r| r.get(0))
        .map_err(|e| DbError::BackupFailed {
            reason: format!("Falha ao ler versão do backup: {e}"),
        })?;

    if user_ver > known_max_version {
        return Err(DbError::SchemaNewerThanApp {
            db_version: user_ver,
            app_max_version: known_max_version,
        });
    }

    let mut fk_stmt =
        conn.prepare("PRAGMA foreign_key_check;")
            .map_err(|e| DbError::BackupFailed {
                reason: format!("Falha ao validar FKs do backup: {e}"),
            })?;
    let mut rows = fk_stmt.query([]).map_err(|e| DbError::BackupFailed {
        reason: format!("Falha ao consultar FKs do backup: {e}"),
    })?;
    if rows
        .next()
        .map_err(|e| DbError::BackupFailed {
            reason: e.to_string(),
        })?
        .is_some()
    {
        return Err(DbError::BackupFailed {
            reason: String::from("O backup selecionado apresenta violação de chave estrangeira."),
        });
    }

    Ok(bkp_path)
}

/// Agenda a restauração de `backup_file_name` para o próximo boot gravando `pending-restore.json` com permissão `0o600`.
pub fn schedule_restore(data_dir: &Path, backup_file_name: &str) -> DbResult<RestoreScheduledDto> {
    let backup_dir = data_dir.join("backups");
    let max_version =
        crate::migrations::MigrationRegistry::builtin().map_or(11, |r| r.max_version());
    schedule_restore_with_dirs(data_dir, &backup_dir, backup_file_name, max_version)
}

/// Agenda a restauração informando explicitamente `data_dir`, `backup_dir` e `known_max_version`.
pub fn schedule_restore_with_dirs(
    data_dir: &Path,
    backup_dir: &Path,
    backup_file_name: &str,
    known_max_version: i64,
) -> DbResult<RestoreScheduledDto> {
    ensure_secure_dir(data_dir)?;
    let _validated = validate_candidate_backup(backup_dir, backup_file_name, known_max_version)?;

    let scheduled_at_ms = now_unix_ms();
    let trimmed = backup_file_name.trim().to_string();
    let marker = PendingRestoreMarker {
        backup_file_name: trimmed.clone(),
        scheduled_at_ms,
    };
    let payload = serde_json::to_vec_pretty(&marker).map_err(|e| DbError::BackupFailed {
        reason: format!("Falha ao serializar pending-restore.json: {e}"),
    })?;

    let final_marker = pending_restore_marker_path(data_dir);
    let tmp_marker = data_dir.join(".pending-restore.json.tmp");
    let _ = fs::remove_file(&tmp_marker);

    {
        use std::io::Write;
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp_marker)
            .map_err(|e| DbError::BackupFailed {
                reason: format!("Falha ao criar marcador temporário de restauração: {e}"),
            })?;
        f.write_all(&payload).map_err(|e| DbError::BackupFailed {
            reason: format!("Falha ao gravar pending-restore.json: {e}"),
        })?;
        let _ = f.sync_all();
    }
    set_file_mode_0600(&tmp_marker)?;

    fs::rename(&tmp_marker, &final_marker).map_err(|e| {
        let _ = fs::remove_file(&tmp_marker);
        DbError::BackupFailed {
            reason: format!("Falha ao ativar pending-restore.json: {e}"),
        }
    })?;
    set_file_mode_0600(&final_marker)?;

    Ok(RestoreScheduledDto {
        backup_file_name: trimmed,
        scheduled_at_ms,
        requires_restart: true,
    })
}

/// Verifica e aplica `pending-restore.json` no boot antes de abrir conexões de escrita/leitura.
pub fn apply_pending_restore_on_boot(cfg: &DbConfig) -> DbResult<Option<PathBuf>> {
    apply_pending_restore_with_injection(cfg, None)
}

/// Variante de [`apply_pending_restore_on_boot`] que permite injetar falha controlada em uma [`RestoreStage`]
/// para validar a preservação integral do banco anterior em testes.
#[allow(clippy::too_many_lines)]
pub fn apply_pending_restore_with_injection(
    cfg: &DbConfig,
    inject_failure_at: Option<RestoreStage>,
) -> DbResult<Option<PathBuf>> {
    let Some(data_dir) = cfg.db_path.parent() else {
        return Ok(None);
    };
    let marker_path = pending_restore_marker_path(data_dir);
    if !marker_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&marker_path).map_err(|e| DbError::BackupFailed {
        reason: format!("Falha ao ler pending-restore.json: {e}"),
    })?;
    let marker: PendingRestoreMarker =
        serde_json::from_str(&raw).map_err(|e| DbError::BackupFailed {
            reason: format!("Conteúdo inválido em pending-restore.json: {e}"),
        })?;

    let registry = crate::migrations::MigrationRegistry::builtin()?;
    let bkp_path = validate_candidate_backup(
        &cfg.backup_dir,
        &marker.backup_file_name,
        registry.max_version(),
    )?;

    // Etapa 1: Backup PreRestore do banco atual (se existir)
    if inject_failure_at == Some(RestoreStage::PreRestoreBackup) {
        return Err(DbError::BackupFailed {
            reason: String::from("Falha injetada na etapa PreRestoreBackup."),
        });
    }

    let pre_restore_bkp = if cfg.db_path.exists() {
        // Garante que qualquer WAL pendente do banco atual seja consolidado na imagem de backup
        Some(create_backup_blocking(
            &cfg.db_path,
            &cfg.backup_dir,
            BackupKind::PreRestore,
            7,
        )?)
    } else {
        None
    };

    // Função auxiliar de rollback que restaura o banco original a partir do backup PreRestore
    let rollback_to_original = |pre: &Option<BackupEntryDto>| {
        let _ = fs::remove_file(wal_path_for(&cfg.db_path));
        let _ = fs::remove_file(shm_path_for(&cfg.db_path));
        if let Some(entry) = pre {
            let rollback_tmp = cfg.db_path.with_extension("rollback.tmp");
            let _ = fs::remove_file(&rollback_tmp);
            if fs::copy(&entry.path, &rollback_tmp).is_ok() {
                let _ = set_file_mode_0600(&rollback_tmp);
                let _ = fs::rename(&rollback_tmp, &cfg.db_path);
                let _ = set_file_mode_0600(&cfg.db_path);
            }
        }
    };

    // Etapa 2: Preparação/isolamento do estado atual
    if inject_failure_at == Some(RestoreStage::QuarantineCurrent) {
        return Err(DbError::BackupFailed {
            reason: String::from("Falha injetada na etapa QuarantineCurrent."),
        });
    }

    // Etapa 3: Cópia do candidato para `.restore.tmp`
    let restore_tmp = cfg.db_path.with_extension("restore.tmp");
    let _ = fs::remove_file(&restore_tmp);
    if inject_failure_at == Some(RestoreStage::CopyCandidate) {
        return Err(DbError::BackupFailed {
            reason: String::from("Falha injetada na etapa CopyCandidate."),
        });
    }

    if let Err(e) = fs::copy(&bkp_path, &restore_tmp) {
        let _ = fs::remove_file(&restore_tmp);
        return Err(DbError::BackupFailed {
            reason: format!("Falha ao copiar arquivo de backup para restauração: {e}"),
        });
    }
    set_file_mode_0600(&restore_tmp)?;
    if let Ok(f) = OpenOptions::new().write(true).open(&restore_tmp) {
        let _ = f.sync_all();
    }

    // Etapa 4: Swap atômico sobre `maxterminal.db`
    if inject_failure_at == Some(RestoreStage::AtomicSwap) {
        let _ = fs::remove_file(&restore_tmp);
        return Err(DbError::BackupFailed {
            reason: String::from("Falha injetada na etapa AtomicSwap."),
        });
    }

    let _ = fs::remove_file(wal_path_for(&cfg.db_path));
    let _ = fs::remove_file(shm_path_for(&cfg.db_path));
    if let Err(e) = fs::rename(&restore_tmp, &cfg.db_path) {
        let _ = fs::remove_file(&restore_tmp);
        rollback_to_original(&pre_restore_bkp);
        return Err(DbError::BackupFailed {
            reason: format!("Falha ao ativar banco restaurado: {e}"),
        });
    }
    let _ = set_file_mode_0600(&cfg.db_path);

    // Etapa 5: Aplicar migrations pendentes até a versão atual do schema
    if inject_failure_at == Some(RestoreStage::Migrate) {
        rollback_to_original(&pre_restore_bkp);
        return Err(DbError::BackupFailed {
            reason: String::from("Falha injetada na etapa Migrate."),
        });
    }

    let step_res: DbResult<()> = (|| {
        let mut opened = crate::connection::open_writer(cfg)?;
        let runner = crate::migrations::MigrationRunner::new(registry);
        runner.run(&mut opened.conn, &cfg.db_path, &cfg.backup_dir, 7)?;

        // Etapa 6: Rebuild FTS5 e backfill de projeções
        if inject_failure_at == Some(RestoreStage::RebuildFts) {
            return Err(DbError::BackupFailed {
                reason: String::from("Falha injetada na etapa RebuildFts."),
            });
        }

        let tx = opened
            .conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| DbError::from_rusqlite(e, "restore_rebuild_tx"))?;
        crate::search::projection::backfill_all_projections(&tx)?;
        let mut fts_reg = crate::fts::FtsRegistry::new();
        let _ = fts_reg.register(crate::fts::COMMAND_BLOCKS_FTS_SPEC);
        let _ = fts_reg.register(crate::repos::SESSION_SCROLLBACK_FTS_SPEC);
        let _ = fts_reg.register(crate::search::projection::SEARCH_METADATA_FTS_SPEC);
        for spec in fts_reg.specs() {
            crate::search::projection::rebuild_search_fts(&tx, &fts_reg, spec.fts_table)?;
        }
        tx.commit()
            .map_err(|e| DbError::from_rusqlite(e, "restore_rebuild_commit"))?;
        let _ = opened
            .conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        Ok(())
    })();

    if let Err(err) = step_res {
        rollback_to_original(&pre_restore_bkp);
        return Err(err);
    }

    let _ = fs::remove_file(&marker_path);
    Ok(Some(bkp_path))
}
