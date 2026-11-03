//! Taxonomia de erros do domínio `db.*` e conversão para `mt_core::MtError`.

use std::path::PathBuf;

use mt_core::{ErrorCode, MtError};
use serde_json::json;

/// Códigos estáveis do domínio `db` (espelhados em `src/types/errors.ts`).
pub mod codes {
    use mt_core::ErrorCode;

    /// Falha ao abrir ou configurar o banco SQLite.
    pub const OPEN_FAILED: ErrorCode = ErrorCode("db.open_failed");
    /// Outra instância do `MaxTerminal` já detém o lock exclusivo do banco.
    pub const LOCKED_BY_OTHER_INSTANCE: ErrorCode = ErrorCode("db.locked_by_other_instance");
    /// O arquivo do banco SQLite está corrompido.
    pub const CORRUPTED: ErrorCode = ErrorCode("db.corrupted");
    /// Falha ao aplicar uma migration de schema.
    pub const MIGRATION_FAILED: ErrorCode = ErrorCode("db.migration_failed");
    /// Divergência de checksum em uma migration já aplicada.
    pub const MIGRATION_CHECKSUM_MISMATCH: ErrorCode = ErrorCode("db.migration_checksum_mismatch");
    /// O schema do banco é mais recente que a versão desta aplicação.
    pub const SCHEMA_NEWER_THAN_APP: ErrorCode = ErrorCode("db.schema_newer_than_app");
    /// O motor SQLite está temporariamente ocupado.
    pub const BUSY: ErrorCode = ErrorCode("db.busy");
    /// Violação de integridade/restrição no banco de dados.
    pub const CONSTRAINT_VIOLATION: ErrorCode = ErrorCode("db.constraint_violation");
    /// A consulta de busca textual FTS5 é inválida.
    pub const QUERY_INVALID: ErrorCode = ErrorCode("db.query_invalid");
    /// Cursor de paginação inválido ou malformado.
    pub const CURSOR_INVALID: ErrorCode = ErrorCode("db.cursor_invalid");
    /// Chave de configuração desconhecida.
    pub const SETTING_UNKNOWN_KEY: ErrorCode = ErrorCode("db.setting_unknown_key");
    /// Valor inválido para a chave de configuração informada.
    pub const SETTING_INVALID_VALUE: ErrorCode = ErrorCode("db.setting_invalid_value");
    /// Tentativa de gravar credencial ou segredo em texto puro nas configurações.
    pub const SETTING_SECRET_REJECTED: ErrorCode = ErrorCode("db.setting_secret_rejected");
    /// Conflito de revisão otimista ao atualizar configuração.
    pub const SETTING_CONFLICT: ErrorCode = ErrorCode("db.setting_conflict");
    /// Falha ao criar ou restaurar backup do banco de dados.
    pub const BACKUP_FAILED: ErrorCode = ErrorCode("db.backup_failed");
    /// A fila de gravação do banco foi encerrada.
    pub const WRITER_CLOSED: ErrorCode = ErrorCode("db.writer_closed");
    /// O banco está em modo de recuperação e aguarda decisão do usuário.
    pub const RECOVERY_REQUIRED: ErrorCode = ErrorCode("db.recovery_required");
}

/// Erro tipado interno do crate `mt-db`.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// Falha ao abrir o banco em `path`.
    #[error("Não foi possível abrir o banco de dados local: {reason}")]
    OpenFailed {
        /// Caminho do banco.
        path: PathBuf,
        /// Motivo sanitizado.
        reason: String,
    },

    /// Outra instância do `MaxTerminal` já está em execução usando o mesmo diretório de dados.
    #[error("Outra instância do MaxTerminal já está utilizando o banco de dados.")]
    LockedByOtherInstance {
        /// Caminho do arquivo `.lock`.
        lock_path: PathBuf,
    },

    /// Corrupção detectada no banco SQLite.
    #[error("O banco de dados local apresenta inconsistências estruturais ({detail}).")]
    Corrupted {
        /// Caminho do banco.
        path: PathBuf,
        /// Detalhe resumido da corrupção.
        detail: String,
    },

    /// Falha ao executar migration `version` (`name`).
    #[error(
        "Falha ao atualizar o esquema do banco de dados na versão {version} ({name}): {reason}"
    )]
    MigrationFailed {
        /// Versão da migration.
        version: i64,
        /// Nome da migration.
        name: String,
        /// Motivo da falha.
        reason: String,
    },

    /// Divergência de checksum na migration `version`.
    #[error("Divergência de integridade detectada na migração {version} ({name}).")]
    MigrationChecksumMismatch {
        /// Versão da migration.
        version: i64,
        /// Nome da migration.
        name: String,
        /// Checksum registrado no banco.
        expected: String,
        /// Checksum do arquivo embutido no binário.
        found: String,
    },

    /// O banco foi aberto por uma versão mais nova do `MaxTerminal` (`db_version > app_max_version`).
    #[error(
        "O banco de dados pertence a uma versão mais recente do MaxTerminal (esquema v{db_version}, suportado até v{app_max_version})."
    )]
    SchemaNewerThanApp {
        /// Maior versão aplicada no banco.
        db_version: i64,
        /// Maior versão conhecida pela aplicação.
        app_max_version: i64,
    },

    /// Timeout de lock (`SQLITE_BUSY` / `SQLITE_LOCKED`) ou pool de leitura saturado.
    #[error("O banco de dados está temporariamente ocupado. Tente novamente em instantes.")]
    Busy {
        /// Operação em andamento.
        operation: &'static str,
    },

    /// Violação de constraint (`UNIQUE`, `FOREIGN KEY`, `CHECK`, `NOT NULL`).
    #[error("Os dados informados violam uma restrição de integridade ({constraint}).")]
    ConstraintViolation {
        /// Descrição da constraint.
        constraint: String,
    },

    /// Consulta FTS5 inválida ou vazia após higienização.
    #[error("A consulta de busca informada é inválida: {reason}")]
    QueryInvalid {
        /// Explicação do erro.
        reason: String,
    },

    /// Cursor de paginação keyset inválido.
    #[error("O cursor de paginação informado é inválido: {reason}")]
    CursorInvalid {
        /// Explicação do erro.
        reason: String,
    },

    /// Chave de configuração não registrada no `SettingsRegistry`.
    #[error("A chave de configuração '{key}' não é reconhecida.")]
    SettingUnknownKey {
        /// Chave solicitada.
        key: String,
    },

    /// Valor JSON inválido para a chave de configuração.
    #[error("O valor informado para a configuração '{key}' é inválido: {reason}")]
    SettingInvalidValue {
        /// Chave solicitada.
        key: String,
        /// Motivo da invalidação.
        reason: String,
    },

    /// Tentativa de gravar segredo ou campo sensível em `settings`.
    #[error(
        "Por segurança, credenciais e chaves secretas não podem ser salvas em '{key}' ({reason})."
    )]
    SettingSecretRejected {
        /// Chave solicitada.
        key: String,
        /// Motivo da rejeição.
        reason: String,
    },

    /// Conflito de concorrência otimista (`expected_revision != current_revision`).
    #[error(
        "A configuração '{key}' foi modificada por outra operação (revisão esperada {expected_revision}, atual {current_revision})."
    )]
    SettingConflict {
        /// Chave solicitada.
        key: String,
        /// Revisão esperada pelo chamador.
        expected_revision: i64,
        /// Revisão atual persistida.
        current_revision: i64,
    },

    /// Falha ao criar ou restaurar backup.
    #[error("Falha na operação de backup do banco de dados: {reason}")]
    BackupFailed {
        /// Motivo da falha.
        reason: String,
    },

    /// Canal do `DbWriter` encerrado.
    #[error("O serviço de gravação do banco de dados já foi encerrado.")]
    WriterClosed,

    /// O banco entrou em modo degradado (`RecoveryRequired`) e aguarda ação do usuário.
    #[error("O banco de dados requer recuperação antes de prosseguir ({reason}).")]
    RecoveryRequired {
        /// Razão do modo de recuperação.
        reason: String,
    },

    /// Erro genérico do `rusqlite`.
    #[error("Erro interno do motor SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Erro de I/O de arquivo/diretório.
    #[error("Erro de entrada/saída no armazenamento local: {0}")]
    Io(#[from] std::io::Error),
}

/// Alias padrão de resultado para operações internas de `mt-db`.
pub type DbResult<T> = Result<T, DbError>;

impl DbError {
    /// Mapeia um `rusqlite::Error` contextualizando a operação em execução.
    #[must_use]
    pub fn from_rusqlite(err: rusqlite::Error, operation: &'static str) -> Self {
        if let rusqlite::Error::SqliteFailure(ffi_err, ref msg) = err {
            match ffi_err.code {
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => {
                    return Self::Busy { operation };
                }
                rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase => {
                    return Self::Corrupted {
                        path: PathBuf::new(),
                        detail: msg
                            .clone()
                            .unwrap_or_else(|| String::from("arquivo SQLite corrompido")),
                    };
                }
                rusqlite::ErrorCode::ConstraintViolation => {
                    return Self::ConstraintViolation {
                        constraint: msg
                            .clone()
                            .unwrap_or_else(|| String::from("restrição de esquema")),
                    };
                }
                _ => {}
            }
        }
        Self::Sqlite(err)
    }

    /// Retorna o `ErrorCode` correspondente à variante.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::OpenFailed { .. } | Self::Sqlite(_) | Self::Io(_) => codes::OPEN_FAILED,
            Self::LockedByOtherInstance { .. } => codes::LOCKED_BY_OTHER_INSTANCE,
            Self::Corrupted { .. } => codes::CORRUPTED,
            Self::MigrationFailed { .. } => codes::MIGRATION_FAILED,
            Self::MigrationChecksumMismatch { .. } => codes::MIGRATION_CHECKSUM_MISMATCH,
            Self::SchemaNewerThanApp { .. } => codes::SCHEMA_NEWER_THAN_APP,
            Self::Busy { .. } => codes::BUSY,
            Self::ConstraintViolation { .. } => codes::CONSTRAINT_VIOLATION,
            Self::QueryInvalid { .. } => codes::QUERY_INVALID,
            Self::CursorInvalid { .. } => codes::CURSOR_INVALID,
            Self::SettingUnknownKey { .. } => codes::SETTING_UNKNOWN_KEY,
            Self::SettingInvalidValue { .. } => codes::SETTING_INVALID_VALUE,
            Self::SettingSecretRejected { .. } => codes::SETTING_SECRET_REJECTED,
            Self::SettingConflict { .. } => codes::SETTING_CONFLICT,
            Self::BackupFailed { .. } => codes::BACKUP_FAILED,
            Self::WriterClosed => codes::WRITER_CLOSED,
            Self::RecoveryRequired { .. } => codes::RECOVERY_REQUIRED,
        }
    }
}

impl From<DbError> for MtError {
    fn from(err: DbError) -> Self {
        let code = err.code();
        let message = crate::sanitizer::sanitize(&err.to_string()).into_owned();

        let details = match &err {
            DbError::OpenFailed { path, .. } => Some(json!({
                "path": path.display().to_string(),
            })),
            DbError::LockedByOtherInstance { lock_path } => Some(json!({
                "lockPath": lock_path.display().to_string(),
            })),
            DbError::Corrupted { path, detail } => Some(json!({
                "path": path.display().to_string(),
                "detail": crate::sanitizer::sanitize(detail).as_ref(),
            })),
            DbError::MigrationFailed { version, name, .. } => Some(json!({
                "version": version,
                "name": name,
            })),
            DbError::MigrationChecksumMismatch {
                version,
                name,
                expected,
                found,
            } => Some(json!({
                "version": version,
                "name": name,
                "expected": expected,
                "found": found,
            })),
            DbError::SchemaNewerThanApp {
                db_version,
                app_max_version,
            } => Some(json!({
                "dbVersion": db_version,
                "appMaxVersion": app_max_version,
            })),
            DbError::Busy { operation } => Some(json!({
                "operation": operation,
            })),
            DbError::ConstraintViolation { constraint } => Some(json!({
                "constraint": crate::sanitizer::sanitize(constraint).as_ref(),
            })),
            DbError::SettingUnknownKey { key }
            | DbError::SettingInvalidValue { key, .. }
            | DbError::SettingSecretRejected { key, .. } => Some(json!({
                "key": key,
            })),
            DbError::SettingConflict {
                key,
                expected_revision,
                current_revision,
            } => Some(json!({
                "key": key,
                "expectedRevision": expected_revision,
                "currentRevision": current_revision,
            })),
            DbError::RecoveryRequired { reason } => Some(json!({
                "reason": crate::sanitizer::sanitize(reason).as_ref(),
            })),
            _ => None,
        };

        let mut mt_err = MtError::new(code, message).with_source(err);
        if let Some(d) = details {
            mt_err = mt_err.with_details(d);
        }
        mt_err
    }
}
