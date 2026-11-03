//! Repositório de leitura e atualização transacional de configurações (`SettingsRepo`).

use std::collections::BTreeMap;
use std::sync::Arc;

use rusqlite::OptionalExtension;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{DbError, DbResult};
use crate::models::{DbEventSink, SettingChangeOrigin, SettingSnapshot, SettingsChanged};
use crate::reader::ReadPool;
use crate::sanitizer::inspect_settings_json;
use crate::settings::registry::{SettingLookup, SettingsRegistry};
use crate::time::now_unix_ms;
use crate::writer::DbWriter;

/// Limite máximo de tamanho serializado de um valor de configuração (64 KiB).
pub const MAX_SETTING_VALUE_BYTES: usize = 65_536;

/// Repositório tipado da tabela `settings` acoplado ao `SettingsRegistry`, `ReadPool` e `DbWriter`.
#[derive(Clone)]
pub struct SettingsRepo {
    registry: Arc<SettingsRegistry>,
    reader: ReadPool,
    writer: Arc<DbWriter>,
    event_sink: Arc<dyn DbEventSink>,
}

impl std::fmt::Debug for SettingsRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsRepo")
            .field("registry", &self.registry)
            .field("reader", &self.reader)
            .field("writer", &self.writer)
            .finish_non_exhaustive()
    }
}

impl SettingsRepo {
    /// Cria uma nova instância de `SettingsRepo`.
    #[must_use]
    pub fn new(
        registry: Arc<SettingsRegistry>,
        reader: ReadPool,
        writer: Arc<DbWriter>,
        event_sink: Arc<dyn DbEventSink>,
    ) -> Self {
        Self {
            registry,
            reader,
            writer,
            event_sink,
        }
    }

    /// Retorna o `SettingSnapshot` atual de uma chave (`revision = 0` se ainda estiver no default).
    pub fn get_snapshot(&self, key: &str) -> DbResult<SettingSnapshot> {
        let default_val = match self.registry.resolve(key) {
            SettingLookup::Registered(desc) => {
                if desc.sensitive {
                    return Err(DbError::SettingSecretRejected {
                        key: key.to_string(),
                        reason: String::from("Configuração sensível não é armazenada no SQLite."),
                    });
                }
                (desc.default_value)()
            }
            SettingLookup::FreeUi => Value::Null,
            SettingLookup::Unknown => {
                return Err(DbError::SettingUnknownKey {
                    key: key.to_string(),
                });
            }
        };

        let key_owned = key.to_string();
        self.reader.with_conn(move |conn| {
            let row: Option<(String, i64, i64)> = conn
                .query_row(
                    "SELECT value_json, revision, updated_at FROM settings WHERE key = ?1;",
                    [&key_owned],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(|e| DbError::from_rusqlite(e, "settings_get_snapshot"))?;

            if let Some((raw_json, revision, updated_at)) = row {
                let parsed: Value =
                    serde_json::from_str(&raw_json).map_err(|e| DbError::Corrupted {
                        path: std::path::PathBuf::new(),
                        detail: format!("JSON inválido na tabela settings para '{key_owned}': {e}"),
                    })?;
                Ok(SettingSnapshot {
                    key: key_owned,
                    value: parsed,
                    revision,
                    updated_at,
                })
            } else {
                Ok(SettingSnapshot {
                    key: key_owned,
                    value: default_val,
                    revision: 0,
                    updated_at: 0,
                })
            }
        })
    }

    /// Obtém o valor JSON efetivo de uma configuração (persistido ou default).
    pub fn get(&self, key: &str) -> DbResult<Value> {
        self.get_snapshot(key).map(|s| s.value)
    }

    /// Obtém o valor deserializado em `T` (retorna `Ok(None)` se o valor atual for `null`).
    pub fn get_typed<T: DeserializeOwned>(&self, key: &str) -> DbResult<Option<T>> {
        let val = self.get(key)?;
        if val.is_null() {
            return Ok(None);
        }
        serde_json::from_value(val)
            .map(Some)
            .map_err(|e| DbError::SettingInvalidValue {
                key: key.to_string(),
                reason: format!("Falha ao converter valor da configuração: {e}"),
            })
    }

    /// Retorna um mapa de configurações (`None` retorna todas as chaves registradas + chaves `ui.*` persistidas).
    pub fn get_many(&self, keys: Option<&[String]>) -> DbResult<BTreeMap<String, Value>> {
        if let Some(requested) = keys {
            let mut map = BTreeMap::new();
            for key in requested {
                let val = self.get(key)?;
                map.insert(key.clone(), val);
            }
            return Ok(map);
        }

        let mut map = BTreeMap::new();
        for desc in self.registry.descriptors() {
            if !desc.sensitive {
                map.insert(desc.key.to_string(), (desc.default_value)());
            }
        }

        self.reader.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT key, value_json FROM settings ORDER BY key ASC;")
                .map_err(|e| DbError::from_rusqlite(e, "settings_get_many_prepare"))?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(|e| DbError::from_rusqlite(e, "settings_get_many_query"))?;

            for row in rows {
                let (k, raw_json) =
                    row.map_err(|e| DbError::from_rusqlite(e, "settings_get_many_row"))?;
                if let Ok(parsed) = serde_json::from_str::<Value>(&raw_json) {
                    map.insert(k, parsed);
                }
            }
            Ok(map)
        })
    }

    /// Atualiza uma configuração validando descritor, segredos, limite de 64 KiB e revisão otimista.
    #[allow(clippy::too_many_lines)]
    pub async fn set(
        &self,
        key: &str,
        value: Value,
        expected_revision: Option<i64>,
        origin: SettingChangeOrigin,
    ) -> DbResult<SettingSnapshot> {
        match self.registry.resolve(key) {
            SettingLookup::Registered(desc) => {
                if desc.sensitive {
                    return Err(DbError::SettingSecretRejected {
                        key: key.to_string(),
                        reason: String::from(
                            "Esta chave é marcada como sensível e não pode ser persistida no SQLite.",
                        ),
                    });
                }
                if desc.internal_only && origin == SettingChangeOrigin::Ui {
                    return Err(DbError::SettingInvalidValue {
                        key: key.to_string(),
                        reason: String::from(
                            "Esta configuração é gerenciada internamente pelo sistema.",
                        ),
                    });
                }
                if let Err(reason) = (desc.validate)(&value) {
                    return Err(DbError::SettingInvalidValue {
                        key: key.to_string(),
                        reason,
                    });
                }
            }
            SettingLookup::FreeUi => {}
            SettingLookup::Unknown => {
                return Err(DbError::SettingUnknownKey {
                    key: key.to_string(),
                });
            }
        }

        if let Err(reason) = inspect_settings_json(&value) {
            return Err(DbError::SettingSecretRejected {
                key: key.to_string(),
                reason,
            });
        }

        let serialized =
            serde_json::to_string(&value).map_err(|e| DbError::SettingInvalidValue {
                key: key.to_string(),
                reason: format!("Falha ao serializar JSON: {e}"),
            })?;

        if serialized.len() > MAX_SETTING_VALUE_BYTES {
            return Err(DbError::SettingInvalidValue {
                key: key.to_string(),
                reason: String::from("O valor excede o limite máximo permitido de 64 KiB."),
            });
        }

        let key_owned = key.to_string();
        let value_for_tx = value.clone();
        let approx_bytes = key_owned.len() + serialized.len() + 64;

        let (snapshot, changed) = self
            .writer
            .execute_tx("settings_set", approx_bytes, move |tx| {
                let existing: Option<(String, i64, i64)> = tx
                    .query_row(
                        "SELECT value_json, revision, updated_at FROM settings WHERE key = ?1;",
                        [&key_owned],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()
                    .map_err(|e| DbError::from_rusqlite(e, "settings_select_current"))?;

                let current_revision = existing.as_ref().map_or(0, |(_, rev, _)| *rev);
                if let Some(expected) = expected_revision
                    && expected != current_revision
                {
                    return Err(DbError::SettingConflict {
                        key: key_owned,
                        expected_revision: expected,
                        current_revision,
                    });
                }

                if let Some((ref cur_json, cur_rev, cur_updated)) = existing
                    && let Ok(cur_val) = serde_json::from_str::<Value>(cur_json)
                    && cur_val == value_for_tx
                {
                    return Ok((
                        SettingSnapshot {
                            key: key_owned,
                            value: cur_val,
                            revision: cur_rev,
                            updated_at: cur_updated,
                        },
                        false,
                    ));
                }

                let new_revision = current_revision + 1;
                let now = now_unix_ms();

                tx.execute(
                    "INSERT INTO settings (key, value_json, updated_at, revision)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(key) DO UPDATE SET
                         value_json = excluded.value_json,
                         updated_at = excluded.updated_at,
                         revision   = excluded.revision;",
                    rusqlite::params![&key_owned, &serialized, now, new_revision],
                )
                .map_err(|e| DbError::from_rusqlite(e, "settings_upsert"))?;

                Ok((
                    SettingSnapshot {
                        key: key_owned,
                        value: value_for_tx,
                        revision: new_revision,
                        updated_at: now,
                    },
                    true,
                ))
            })
            .await?;

        if changed {
            self.event_sink.on_settings_changed(SettingsChanged {
                key: snapshot.key.clone(),
                value: snapshot.value.clone(),
                revision: snapshot.revision,
                origin,
            });
        }

        Ok(snapshot)
    }

    /// Atualiza uma configuração interna do sistema (ex.: `db.maintenance.last_run_at`).
    pub async fn set_internal(&self, key: &str, value: Value) -> DbResult<SettingSnapshot> {
        self.set(key, value, None, SettingChangeOrigin::Sync).await
    }
}
