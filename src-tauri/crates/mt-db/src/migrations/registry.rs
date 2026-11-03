//! Registro e validação estrita de migrations embutidas (`migrations/*.sql`) e migrations em código.

use std::sync::LazyLock;

use include_dir::{Dir, include_dir};
use regex::Regex;
use rusqlite::Transaction;
use sha2::{Digest, Sha256};

use crate::error::{DbError, DbResult};

static EMBEDDED_MIGRATIONS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

static FILENAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4})_(\d{2})_([a-z0-9_]+)\.sql$")
        .unwrap_or_else(|e| panic!("Regex de migration inválida: {e}"))
});

/// Assinatura de função para migrations implementadas em código Rust dentro da transação da migration.
pub type CodeMigrationFn = fn(&Transaction<'_>) -> DbResult<()>;

/// Fonte de execução de uma migration registrada.
#[derive(Clone)]
pub enum MigrationSource {
    /// Script SQL estático embutido no binário.
    Sql(&'static str),
    /// Script SQL dinâmico (usado em testes de integração).
    OwnedSql(String),
    /// Função Rust transacional.
    Code(CodeMigrationFn),
}

impl std::fmt::Debug for MigrationSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sql(_) => f.write_str("MigrationSource::Sql(..)"),
            Self::OwnedSql(_) => f.write_str("MigrationSource::OwnedSql(..)"),
            Self::Code(_) => f.write_str("MigrationSource::Code(..)"),
        }
    }
}

/// Definição de uma migration em código Rust.
#[derive(Clone, Debug)]
pub struct CodeMigration {
    /// Número sequencial global da migration (`>= 1`).
    pub version: i64,
    /// Número da fase dona (`1..=99`).
    pub phase: u8,
    /// Nome descritivo em `snake_case`.
    pub name: String,
    /// Checksum SHA-256 de 64 caracteres hexadecimais calculado sobre a semente canônica.
    pub checksum: String,
    /// Função executada na transação `IMMEDIATE`.
    pub run: CodeMigrationFn,
}

impl CodeMigration {
    /// Cria uma nova `CodeMigration` calculando o checksum SHA-256 a partir de `checksum_seed`.
    #[must_use]
    pub fn new(
        version: i64,
        phase: u8,
        name: impl Into<String>,
        checksum_seed: &str,
        run: CodeMigrationFn,
    ) -> Self {
        Self {
            version,
            phase,
            name: name.into(),
            checksum: compute_migration_checksum(checksum_seed),
            run,
        }
    }
}

/// Entrada individual validada no `MigrationRegistry`.
#[derive(Clone, Debug)]
pub struct MigrationEntry {
    /// Versão global sequencial (`1..=N`).
    pub version: i64,
    /// Fase dona da migration (`1..=99`).
    pub phase: u8,
    /// Nome canônico da migration (ex.: `"0001_03_base"`).
    pub name: String,
    /// Checksum SHA-256 (64 caracteres hex minúsculos) do conteúdo normalizado.
    pub checksum: String,
    /// Fonte SQL ou código Rust a executar.
    pub source: MigrationSource,
}

impl MigrationEntry {
    /// Cria uma entrada SQL a partir de nome de arquivo e conteúdo SQL.
    pub fn from_sql(filename: &str, sql: impl Into<String>) -> DbResult<Self> {
        let (version, phase, stem) = parse_migration_filename(filename)?;
        let sql_str = sql.into();
        let checksum = compute_migration_checksum(&sql_str);
        Ok(Self {
            version,
            phase,
            name: stem,
            checksum,
            source: MigrationSource::OwnedSql(sql_str),
        })
    }

    /// Converte uma `CodeMigration` em `MigrationEntry`.
    #[must_use]
    pub fn from_code(code: CodeMigration) -> Self {
        Self {
            version: code.version,
            phase: code.phase,
            name: code.name,
            checksum: code.checksum,
            source: MigrationSource::Code(code.run),
        }
    }
}

/// Calcula o checksum SHA-256 em hexadecimal minúsculo (64 caracteres) normalizando `\r\n` para `\n`.
#[must_use]
pub fn compute_migration_checksum(raw: &str) -> String {
    let normalized = raw.replace("\r\n", "\n");
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let digest = hasher.finalize();
    let mut hex_out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut hex_out, "{byte:02x}");
    }
    hex_out
}

/// Valida o nome de um arquivo de migration contra `^(\d{4})_(\d{2})_([a-z0-9_]+)\.sql$`.
///
/// Retorna `(version, phase, stem_name)`.
pub fn parse_migration_filename(filename: &str) -> DbResult<(i64, u8, String)> {
    let caps = FILENAME_RE
        .captures(filename)
        .ok_or_else(|| DbError::MigrationFailed {
            version: 0,
            name: filename.to_string(),
            reason: String::from(
                "Nome de arquivo de migration fora do padrão NNNN_FF_descricao.sql",
            ),
        })?;

    let version_str = caps.get(1).map_or("", |m| m.as_str());
    let phase_str = caps.get(2).map_or("", |m| m.as_str());
    let desc_str = caps.get(3).map_or("", |m| m.as_str());

    let version = version_str
        .parse::<i64>()
        .map_err(|_| DbError::MigrationFailed {
            version: 0,
            name: filename.to_string(),
            reason: String::from("Versão numérica inválida no nome da migration"),
        })?;

    let phase = phase_str
        .parse::<u8>()
        .map_err(|_| DbError::MigrationFailed {
            version,
            name: filename.to_string(),
            reason: String::from("Fase inválida no nome da migration"),
        })?;

    if version <= 0 || phase == 0 || phase > 99 {
        return Err(DbError::MigrationFailed {
            version,
            name: filename.to_string(),
            reason: String::from("Versão e fase da migration devem ser maiores que zero"),
        });
    }

    let stem = format!("{version_str}_{phase_str}_{desc_str}");
    Ok((version, phase, stem))
}

/// Catálogo ordenado e contíguo (`1..=N`) de migrations conhecidas pela aplicação.
#[derive(Clone, Debug, Default)]
pub struct MigrationRegistry {
    entries: Vec<MigrationEntry>,
}

impl MigrationRegistry {
    /// Carrega todas as migrations SQL embutidas em `src-tauri/crates/mt-db/migrations/`.
    pub fn builtin() -> DbResult<Self> {
        let mut entries = Vec::new();
        for file in EMBEDDED_MIGRATIONS_DIR.files() {
            let file_name = file
                .path()
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| DbError::MigrationFailed {
                    version: 0,
                    name: String::from("<invalid-utf8>"),
                    reason: String::from("Nome de arquivo de migration não é UTF-8 válido"),
                })?;

            let (version, phase, stem) = parse_migration_filename(file_name)?;
            let contents = file
                .contents_utf8()
                .ok_or_else(|| DbError::MigrationFailed {
                    version,
                    name: stem.clone(),
                    reason: String::from("Conteúdo da migration SQL não é UTF-8 válido"),
                })?;

            entries.push(MigrationEntry {
                version,
                phase,
                name: stem,
                checksum: compute_migration_checksum(contents),
                source: MigrationSource::Sql(contents),
            });
        }

        Self::from_entries(entries)
    }

    /// Constrói um `MigrationRegistry` validando ausência de versões duplicadas e contiguidade `1..=N`.
    pub fn from_entries(mut entries: Vec<MigrationEntry>) -> DbResult<Self> {
        entries.sort_by_key(|e| e.version);

        for (idx, entry) in entries.iter().enumerate() {
            if idx > 0 && entries[idx - 1].version == entry.version {
                return Err(DbError::MigrationFailed {
                    version: entry.version,
                    name: entry.name.clone(),
                    reason: format!("Versão de migration duplicada detectada: {}", entry.version),
                });
            }

            let expected_version = i64::try_from(idx + 1).unwrap_or(1);
            if entry.version != expected_version {
                return Err(DbError::MigrationFailed {
                    version: entry.version,
                    name: entry.name.clone(),
                    reason: format!(
                        "Lacuna na sequência de migrations: esperado v{expected_version}, encontrado v{}",
                        entry.version
                    ),
                });
            }
        }

        Ok(Self { entries })
    }

    /// Acrescenta uma `CodeMigration` ao registro, revalidando a sequência contígua.
    pub fn with_code_migration(mut self, migration: CodeMigration) -> DbResult<Self> {
        self.entries.push(MigrationEntry::from_code(migration));
        Self::from_entries(self.entries)
    }

    /// Acrescenta uma `MigrationEntry` ao registro, revalidando a sequência contígua.
    pub fn with_entry(mut self, entry: MigrationEntry) -> DbResult<Self> {
        self.entries.push(entry);
        Self::from_entries(self.entries)
    }

    /// Retorna a fatia ordenada de migrations registradas.
    #[must_use]
    pub fn entries(&self) -> &[MigrationEntry] {
        &self.entries
    }

    /// Retorna a maior versão conhecida no catálogo (`0` se vazio).
    #[must_use]
    pub fn max_version(&self) -> i64 {
        self.entries.last().map_or(0, |e| e.version)
    }
}
