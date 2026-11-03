//! Executor transacional de migrations (`MigrationRunner`) com validação de checksum,
//! bloqueio de downgrade, `PRAGMA foreign_key_check` e backup automático pré-migration.

use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::{Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::error::{DbError, DbResult};
use crate::maintenance::backup::{BackupKind, create_backup_blocking};
use crate::migrations::registry::{MigrationEntry, MigrationRegistry, MigrationSource};
use crate::time::now_unix_ms;

/// Registro de uma migration aplicada na tabela `_schema_migrations`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppliedMigration {
    /// Versão global sequencial da migration.
    pub version: i64,
    /// Nome canônico da migration.
    pub name: String,
    /// Checksum SHA-256 hexadecimal de 64 caracteres.
    pub checksum: String,
    /// Timestamp UNIX em milissegundos da aplicação.
    pub applied_at: i64,
    /// Duração da execução em milissegundos.
    pub duration_ms: i64,
}

/// Alias público de informações de migration aplicada.
pub type MigrationInfo = AppliedMigration;

/// Resultado consolidado da execução do `MigrationRunner`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    /// Maior versão aplicada ao término da execução.
    pub schema_version: i64,
    /// Migrations que foram efetivamente aplicadas nesta chamada.
    pub applied: Vec<AppliedMigration>,
    /// Caminho do arquivo de backup pré-migration criado, se aplicável.
    pub pre_migration_backup: Option<PathBuf>,
}

/// Executor de migrations sobre a conexão de escrita do banco SQLite.
#[derive(Debug, Clone)]
pub struct MigrationRunner {
    registry: MigrationRegistry,
}

impl MigrationRunner {
    /// Cria um novo runner com o `MigrationRegistry` informado.
    #[must_use]
    pub const fn new(registry: MigrationRegistry) -> Self {
        Self { registry }
    }

    /// Retorna referência ao registro de migrations deste runner.
    #[must_use]
    pub const fn registry(&self) -> &MigrationRegistry {
        &self.registry
    }

    /// Executa a verificação de integridade de versões/checksums e aplica as migrations pendentes.
    ///
    /// Quando o banco já possui versão aplicada (`current_version > 0` ou tabelas de usuário existentes)
    /// e há migrations pendentes, cria automaticamente um backup `BackupKind::PreMigration` em `backup_dir`
    /// antes de aplicar a primeira migration pendente.
    pub fn run(
        &self,
        conn: &mut Connection,
        db_path: &Path,
        backup_dir: &Path,
        daily_keep: usize,
    ) -> DbResult<MigrationOutcome> {
        ensure_bootstrap_table(conn, db_path)?;

        let app_max_version = self.registry.max_version();
        let pragma_user_version: i64 = conn
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .map_err(|e| DbError::from_rusqlite(e, "read_user_version"))?;

        let existing = load_applied_migrations(conn)?;
        let table_max_version = existing.last().map_or(0, |m| m.version);
        let db_version = pragma_user_version.max(table_max_version);

        if db_version > app_max_version {
            return Err(DbError::SchemaNewerThanApp {
                db_version,
                app_max_version,
            });
        }

        // Verifica contiguidade e checksums de todas as migrations já registradas em _schema_migrations
        for (idx, applied) in existing.iter().enumerate() {
            let expected_ver = i64::try_from(idx + 1).unwrap_or(1);
            if applied.version != expected_ver {
                return Err(DbError::Corrupted {
                    path: db_path.to_path_buf(),
                    detail: format!(
                        " Sequência inválida em _schema_migrations: esperado v{expected_ver}, encontrado v{}",
                        applied.version
                    ),
                });
            }

            let registered = self
                .registry
                .entries()
                .iter()
                .find(|e| e.version == applied.version)
                .ok_or_else(|| DbError::SchemaNewerThanApp {
                    db_version: applied.version,
                    app_max_version,
                })?;

            if applied.checksum != registered.checksum {
                return Err(DbError::MigrationChecksumMismatch {
                    version: applied.version,
                    name: registered.name.clone(),
                    expected: applied.checksum.clone(),
                    found: registered.checksum.clone(),
                });
            }
        }

        // Sincroniza user_version se estava em 0 mas _schema_migrations já tinha entradas válidas
        let current_version = table_max_version;
        if pragma_user_version != current_version && pragma_user_version != 0 {
            return Err(DbError::Corrupted {
                path: db_path.to_path_buf(),
                detail: format!(
                    "Divergência entre PRAGMA user_version ({pragma_user_version}) e _schema_migrations ({current_version})"
                ),
            });
        }

        let pending: Vec<&MigrationEntry> = self
            .registry
            .entries()
            .iter()
            .filter(|e| e.version > current_version)
            .collect();

        if pending.is_empty() {
            if pragma_user_version != current_version {
                conn.execute_batch(&format!("PRAGMA user_version = {current_version};"))
                    .map_err(|e| DbError::from_rusqlite(e, "sync_user_version"))?;
            }
            return Ok(MigrationOutcome {
                schema_version: current_version,
                applied: Vec::new(),
                pre_migration_backup: None,
            });
        }

        let target_version = pending.last().map_or(current_version, |e| e.version);

        // Cria backup PreMigration quando o banco já possui migrations anteriores aplicadas (current_version > 0)
        // e o arquivo existe com dados em disco (CA-17).
        let pre_migration_backup =
            if current_version > 0 && std::fs::metadata(db_path).is_ok_and(|m| m.len() > 0) {
                let dto = create_backup_blocking(
                    db_path,
                    backup_dir,
                    BackupKind::PreMigration {
                        from_version: current_version,
                        to_version: target_version,
                    },
                    daily_keep,
                )?;
                Some(PathBuf::from(dto.path))
            } else {
                None
            };

        let mut newly_applied = Vec::with_capacity(pending.len());
        for entry in pending {
            let applied = apply_single_migration(conn, entry)?;
            newly_applied.push(applied);
        }

        conn.execute_batch("PRAGMA optimize;")
            .map_err(|e| DbError::from_rusqlite(e, "pragma_optimize_after_migrations"))?;

        Ok(MigrationOutcome {
            schema_version: target_version,
            applied: newly_applied,
            pre_migration_backup,
        })
    }
}

fn ensure_bootstrap_table(conn: &Connection, db_path: &Path) -> DbResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _schema_migrations (
            version      INTEGER PRIMARY KEY,
            name         TEXT    NOT NULL,
            checksum     TEXT    NOT NULL,
            applied_at   INTEGER NOT NULL,
            duration_ms  INTEGER NOT NULL
        ) STRICT;",
    )
    .map_err(|err| {
        if let rusqlite::Error::SqliteFailure(ffi_err, ref msg) = err
            && matches!(
                ffi_err.code,
                rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
            )
        {
            return DbError::Corrupted {
                path: db_path.to_path_buf(),
                detail: msg
                    .clone()
                    .unwrap_or_else(|| String::from("banco corrompido ao inicializar migrations")),
            };
        }
        DbError::from_rusqlite(err, "ensure_schema_migrations")
    })
}

fn load_applied_migrations(conn: &Connection) -> DbResult<Vec<AppliedMigration>> {
    let mut stmt = conn
        .prepare(
            "SELECT version, name, checksum, applied_at, duration_ms
             FROM _schema_migrations
             ORDER BY version ASC;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "prepare_load_migrations"))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(AppliedMigration {
                version: row.get(0)?,
                name: row.get(1)?,
                checksum: row.get(2)?,
                applied_at: row.get(3)?,
                duration_ms: row.get(4)?,
            })
        })
        .map_err(|e| DbError::from_rusqlite(e, "query_load_migrations"))?;

    let mut list = Vec::new();
    for row in rows {
        list.push(row.map_err(|e| DbError::from_rusqlite(e, "read_migration_row"))?);
    }
    Ok(list)
}

#[allow(clippy::too_many_lines)]
fn apply_single_migration(
    conn: &mut Connection,
    entry: &MigrationEntry,
) -> DbResult<AppliedMigration> {
    let started = Instant::now();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| DbError::MigrationFailed {
            version: entry.version,
            name: entry.name.clone(),
            reason: e.to_string(),
        })?;

    match &entry.source {
        MigrationSource::Sql(sql) => {
            tx.execute_batch(sql)
                .map_err(|e| DbError::MigrationFailed {
                    version: entry.version,
                    name: entry.name.clone(),
                    reason: e.to_string(),
                })?;
        }
        MigrationSource::OwnedSql(sql) => {
            tx.execute_batch(sql)
                .map_err(|e| DbError::MigrationFailed {
                    version: entry.version,
                    name: entry.name.clone(),
                    reason: e.to_string(),
                })?;
        }
        MigrationSource::Code(func) => {
            func(&tx).map_err(|e| DbError::MigrationFailed {
                version: entry.version,
                name: entry.name.clone(),
                reason: e.to_string(),
            })?;
        }
    }

    if entry.name == "0004_07_workspace_templates_tabs_sessions" {
        crate::migrations::code::m07_backfill::run_backfill_tx(&tx).map_err(|e| {
            DbError::MigrationFailed {
                version: entry.version,
                name: entry.name.clone(),
                reason: e.to_string(),
            }
        })?;
    }

    if entry.name == "0011_17_search" {
        crate::search::projection::backfill_all_projections(&tx).map_err(|e| {
            DbError::MigrationFailed {
                version: entry.version,
                name: entry.name.clone(),
                reason: e.to_string(),
            }
        })?;
    }

    // Valida integridade referencial dentro da transação antes do commit (CA-11)
    {
        let mut fk_stmt =
            tx.prepare("PRAGMA foreign_key_check;")
                .map_err(|e| DbError::MigrationFailed {
                    version: entry.version,
                    name: entry.name.clone(),
                    reason: e.to_string(),
                })?;
        let mut fk_rows = fk_stmt.query([]).map_err(|e| DbError::MigrationFailed {
            version: entry.version,
            name: entry.name.clone(),
            reason: e.to_string(),
        })?;
        if let Some(row) = fk_rows.next().map_err(|e| DbError::MigrationFailed {
            version: entry.version,
            name: entry.name.clone(),
            reason: e.to_string(),
        })? {
            let table_name: String = row.get(0).unwrap_or_else(|_| String::from("<unknown>"));
            return Err(DbError::MigrationFailed {
                version: entry.version,
                name: entry.name.clone(),
                reason: format!("Violação de chave estrangeira detectada na tabela '{table_name}'"),
            });
        }
    }

    let applied_at = now_unix_ms();
    let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(0);

    tx.execute(
        "INSERT INTO _schema_migrations (version, name, checksum, applied_at, duration_ms)
         VALUES (?1, ?2, ?3, ?4, ?5);",
        rusqlite::params![
            entry.version,
            entry.name,
            entry.checksum,
            applied_at,
            duration_ms
        ],
    )
    .map_err(|e| DbError::MigrationFailed {
        version: entry.version,
        name: entry.name.clone(),
        reason: e.to_string(),
    })?;

    tx.execute_batch(&format!("PRAGMA user_version = {};", entry.version))
        .map_err(|e| DbError::MigrationFailed {
            version: entry.version,
            name: entry.name.clone(),
            reason: e.to_string(),
        })?;

    tx.commit().map_err(|e| DbError::MigrationFailed {
        version: entry.version,
        name: entry.name.clone(),
        reason: e.to_string(),
    })?;

    Ok(AppliedMigration {
        version: entry.version,
        name: entry.name.clone(),
        checksum: entry.checksum.clone(),
        applied_at,
        duration_ms,
    })
}
