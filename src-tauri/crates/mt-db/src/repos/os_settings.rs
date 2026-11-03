//! Repositório persistente para a linha única da tabela `os_settings` (Fase 09).
//!
//! Armazena o JSON versionado das preferências de integração com o sistema operacional
//! sem depender de `mt-os` (evitando ciclo de dependências Cargo).

use rusqlite::OptionalExtension;
use serde_json::Value;

use crate::Db;
use crate::error::{DbError, DbResult};
use crate::sanitizer::inspect_settings_json;
use crate::time::now_unix_ms;

/// Linha persistida na tabela `os_settings` (`id = 1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsSettingsRow {
    /// Versão do schema do objeto JSON persistido (`>= 1`).
    pub schema_version: u32,
    /// Documento JSON serializado (objeto `{...}`).
    pub value_json: String,
    /// Timestamp UNIX em milissegundos da última atualização.
    pub updated_at: i64,
}

/// Repositório de leitura e gravação atômica de `os_settings`.
#[derive(Clone, Debug)]
pub struct OsSettingsRepo {
    db: Db,
}

impl OsSettingsRepo {
    /// Cria uma nova instância do repositório acoplada à fachada `Db`.
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Lê a linha única de `os_settings` de forma assíncrona no `ReadPool`.
    pub async fn get(&self) -> DbResult<OsSettingsRow> {
        self.db.read(get_os_settings_conn).await
    }

    /// Lê a linha única de `os_settings` de forma síncrona no `ReadPool`.
    pub fn get_blocking(&self) -> DbResult<OsSettingsRow> {
        self.db.read_blocking(get_os_settings_conn)
    }

    /// Grava atomicamente `schema_version` e `value_json` na linha `id = 1` da tabela `os_settings`.
    ///
    /// Valida que `value_json` é um objeto JSON válido, que não contém segredos literais
    /// segundo o sanitizador da Fase 03 e que exatamente 1 linha foi atualizada.
    pub async fn save(&self, schema_version: u32, value_json: String) -> DbResult<OsSettingsRow> {
        if schema_version < 1 {
            return Err(DbError::SettingInvalidValue {
                key: String::from("os_settings.schema_version"),
                reason: String::from("A versão de schema de os_settings deve ser >= 1."),
            });
        }

        let parsed: Value =
            serde_json::from_str(&value_json).map_err(|e| DbError::SettingInvalidValue {
                key: String::from("os_settings"),
                reason: format!("JSON inválido para os_settings: {e}"),
            })?;

        if !parsed.is_object() {
            return Err(DbError::SettingInvalidValue {
                key: String::from("os_settings"),
                reason: String::from("O conteúdo de os_settings deve ser um objeto JSON."),
            });
        }

        if let Err(reason) = inspect_settings_json(&parsed) {
            return Err(DbError::SettingSecretRejected {
                key: String::from("os_settings"),
                reason,
            });
        }

        let approx_bytes = value_json.len() + 64;
        self.db
            .write_sized("os_settings_save", approx_bytes, move |tx| {
                let now = now_unix_ms();
                let affected = tx
                    .execute(
                        "UPDATE os_settings
                         SET schema_version = ?1,
                             value_json     = ?2,
                             updated_at     = ?3
                         WHERE id = 1;",
                        rusqlite::params![i64::from(schema_version), &value_json, now],
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "os_settings_update"))?;

                if affected == 0 {
                    tx.execute(
                        "INSERT INTO os_settings (id, schema_version, value_json, updated_at)
                         VALUES (1, ?1, ?2, ?3);",
                        rusqlite::params![i64::from(schema_version), &value_json, now],
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "os_settings_insert"))?;
                }

                Ok(OsSettingsRow {
                    schema_version,
                    value_json,
                    updated_at: now,
                })
            })
            .await
    }
}

fn get_os_settings_conn(conn: &rusqlite::Connection) -> DbResult<OsSettingsRow> {
    let row: Option<(i64, String, i64)> = conn
        .query_row(
            "SELECT schema_version, value_json, updated_at FROM os_settings WHERE id = 1;",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "os_settings_get"))?;

    if let Some((schema_version, value_json, updated_at)) = row {
        let ver = u32::try_from(schema_version).unwrap_or(1).max(1);
        Ok(OsSettingsRow {
            schema_version: ver,
            value_json,
            updated_at,
        })
    } else {
        Ok(OsSettingsRow {
            schema_version: 1,
            value_json: String::from("{}"),
            updated_at: 0,
        })
    }
}
