//! Repositório SQLite do subsistema `mt-ai` (`ai_settings`, `ai_catalog_cache`,
//! `ai_diagnosis_cache` e `ai_prompt_history`).

use crate::config::AiSettings;
use crate::error::{ai_error, ai_error_with_details, codes};
use crate::ids::AiRequestId;
use mt_core::ids::{CommandBlockId, PaneId, ProjectId, SessionId, TabId};
use mt_core::{MtError, MtResult};
use mt_db::{Db, DbError};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Limite máximo de linhas mantidas em `ai_prompt_history`.
pub const HISTORY_MAX_ROWS: usize = 5_000;
/// Intervalo de inserções para disparar poda automática do histórico.
pub const HISTORY_PRUNE_BATCH: usize = 100;

/// Categoria de registro no histórico de prompts de IA.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AiHistoryKind {
    /// Sugestão NL→Shell (`@` ou `?`).
    Suggest,
    /// Diagnóstico rápido inline de erro.
    Diagnosis,
    /// Consulta à documentação indexada (RAG).
    Rag,
    /// Geração assistida estruturada (JSON Schema).
    Structured,
}

impl AiHistoryKind {
    /// Retorna a representação textual persistida no SQLite.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Suggest => "suggest",
            Self::Diagnosis => "diagnosis",
            Self::Rag => "rag",
            Self::Structured => "structured",
        }
    }

    /// Converte a string persistida no SQLite em `AiHistoryKind`.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Self {
        match raw {
            "diagnosis" => Self::Diagnosis,
            "rag" => Self::Rag,
            "structured" => Self::Structured,
            _ => Self::Suggest,
        }
    }
}

/// Escopo de limpeza de caches de IA.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CacheClearScope {
    /// Limpa apenas o cache de diagnósticos (`ai_diagnosis_cache`).
    Diagnosis,
    /// Limpa apenas o cache de catálogo (`ai_catalog_cache`).
    Catalog,
    /// Limpa ambos os caches (`ai_diagnosis_cache` e `ai_catalog_cache`).
    All,
}

/// Filtro de listagem paginada do histórico de prompts.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryFilter {
    /// Filtra por tipo de operação.
    pub kind: Option<AiHistoryKind>,
    /// Filtra por projeto.
    pub project_id: Option<ProjectId>,
    /// Quantidade máxima de itens por página (limitada a `1..=200`).
    pub limit: u16,
    /// Cursor keyset (`id < before_id`).
    pub before_id: Option<String>,
}

/// Item individual do histórico de prompts sanitizados.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryItem {
    /// Identificador único (UUID v7).
    pub id: AiRequestId,
    /// Categoria da interação.
    pub kind: AiHistoryKind,
    /// Gatilho acionado (`"at"` ou `"question"` quando aplicável).
    pub trigger: Option<String>,
    /// Projeto associado.
    pub project_id: Option<ProjectId>,
    /// Aba associada.
    pub tab_id: Option<TabId>,
    /// Painel associado.
    pub pane_id: Option<PaneId>,
    /// Sessão de painel associada.
    pub session_id: Option<SessionId>,
    /// Bloco de comando associado (em diagnósticos).
    pub block_id: Option<CommandBlockId>,
    /// Harness utilizado.
    pub harness_id: Option<String>,
    /// Modelo utilizado.
    pub model_id: Option<String>,
    /// Nível de raciocínio utilizado.
    pub reasoning_id: Option<String>,
    /// Slug de documentação consultado (em buscas RAG).
    pub rag_slug: Option<String>,
    /// Identificador do schema JSON (em geração estruturada).
    pub schema_id: Option<String>,
    /// Prompt já sanitizado.
    pub prompt_sanitized: String,
    /// Resposta já sanitizada.
    pub response_sanitized: Option<String>,
    /// Nível de risco avaliado (`"safe"`, `"caution"` ou `"destructive"`).
    pub risk_level: Option<String>,
    /// Status terminal (`"completed"`, `"failed"` ou `"cancelled"`).
    pub status: String,
    /// Código de erro canônico quando `status == "failed"`.
    pub error_code: Option<String>,
    /// Latência total observada em milissegundos.
    pub latency_ms: Option<u32>,
    /// Timestamp ISO-8601 UTC de criação.
    pub created_at: String,
}

/// Página retornada pela consulta de histórico de prompts.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiHistoryPage {
    /// Itens da página atual em ordem decrescente de `id`.
    pub items: Vec<AiHistoryItem>,
    /// Próximo cursor `before_id` quando houver mais itens.
    pub next_before_id: Option<String>,
}

/// Linha persistida na tabela `ai_catalog_cache`.
#[derive(Clone, Debug)]
pub struct CachedCatalogRow {
    /// Chave do gateway (`sha256(base_url|dialect_id)`).
    pub gateway_key: String,
    /// Payload JSON serializado do catálogo.
    pub payload_json: String,
    /// `ETag` HTTP recebido na última busca.
    pub etag: Option<String>,
    /// Timestamp ISO-8601 UTC da coleta.
    pub fetched_at: String,
}

/// Linha persistida na tabela `ai_diagnosis_cache`.
#[derive(Clone, Debug)]
pub struct CachedDiagnosisRow {
    /// Hash SHA-256 canônico da chave de erro.
    pub error_hash: String,
    /// Chave do gateway.
    pub gateway_key: String,
    /// Projeto associado.
    pub project_id: Option<ProjectId>,
    /// Resumo curto sanitizado (`<= 160` caracteres).
    pub summary: String,
    /// Comando corretivo opcional sanitizado.
    pub fix_command: Option<String>,
    /// Nível de risco persistido (`"safe"`, `"caution"` ou `"destructive"`).
    pub fix_risk: Option<String>,
    /// Harness utilizado.
    pub harness_id: String,
    /// Modelo utilizado.
    pub model_id: String,
    /// Nível de raciocínio utilizado.
    pub reasoning_id: Option<String>,
    /// Versão do template de prompt de diagnóstico.
    pub prompt_version: u32,
    /// Versão do catálogo do sanitizador.
    pub sanitizer_version: u32,
    /// Quantidade de reutilizações (cache hits).
    pub hits: u32,
    /// Timestamp ISO-8601 UTC de criação.
    pub created_at: String,
    /// Timestamp ISO-8601 UTC de expiração.
    pub expires_at: String,
}

/// Repositório SQLite de IA com geração monotônica de persistência para evitar repovoamento pós-limpeza (CA-29..CA-31, CA-33).
pub struct AiRepo {
    db: Db,
    history_generation: Arc<AtomicU64>,
    cache_generation: Arc<AtomicU64>,
    insert_counter: Arc<AtomicU64>,
}

impl AiRepo {
    /// Cria uma nova instância do repositório acoplada à fachada `Db`.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self {
            db,
            history_generation: Arc::new(AtomicU64::new(1)),
            cache_generation: Arc::new(AtomicU64::new(1)),
            insert_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Retorna a geração monotônica atual do histórico de prompts.
    #[must_use]
    pub fn current_history_generation(&self) -> u64 {
        self.history_generation.load(Ordering::SeqCst)
    }

    /// Retorna a geração monotônica atual dos caches de IA.
    #[must_use]
    pub fn current_cache_generation(&self) -> u64 {
        self.cache_generation.load(Ordering::SeqCst)
    }

    /// Lê o singleton `id = 1` via `Db::read`; se ausente, retorna `AiSettings::default()`.
    pub async fn get_settings(&self) -> MtResult<AiSettings> {
        let row: Option<String> = self
            .db
            .read(|conn| {
                conn.query_row("SELECT payload FROM ai_settings WHERE id = 1;", [], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(|e| DbError::from_rusqlite(e, "ai_settings_get"))
            })
            .await
            .map_err(MtError::from)?;

        match row {
            Some(json_str) => serde_json::from_str::<AiSettings>(&json_str).map_err(|e| {
                ai_error(
                    codes::AI_BAD_RESPONSE,
                    format!("Falha ao desserializar ai_settings do SQLite: {e}"),
                )
            }),
            None => Ok(AiSettings::default()),
        }
    }

    /// Lê o singleton `id = 1` de forma síncrona no `ReadPool`.
    pub fn get_settings_blocking(&self) -> MtResult<AiSettings> {
        let row: Option<String> = self
            .db
            .read_blocking(|conn| {
                conn.query_row("SELECT payload FROM ai_settings WHERE id = 1;", [], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(|e| DbError::from_rusqlite(e, "ai_settings_get_blocking"))
            })
            .map_err(MtError::from)?;

        match row {
            Some(json_str) => serde_json::from_str::<AiSettings>(&json_str).map_err(|e| {
                ai_error(
                    codes::AI_BAD_RESPONSE,
                    format!("Falha ao desserializar ai_settings do SQLite: {e}"),
                )
            }),
            None => Ok(AiSettings::default()),
        }
    }

    /// Atualiza `ai_settings` validando `revision` (concorrência otimista -> `ai.settings_conflict`).
    pub async fn save_settings(&self, next: AiSettings) -> MtResult<AiSettings> {
        next.validate()?;
        let mut sanitized = next.sanitized();

        let res = self
            .db
            .write(move |tx| {
                let existing: Option<String> = tx
                    .query_row("SELECT payload FROM ai_settings WHERE id = 1;", [], |r| {
                        r.get(0)
                    })
                    .optional()
                    .map_err(|e| DbError::from_rusqlite(e, "ai_settings_select_for_update"))?;

                let current_rev = match existing {
                    Some(raw) => serde_json::from_str::<AiSettings>(&raw).map_or(1, |s| s.revision),
                    None => 1,
                };

                if sanitized.revision != current_rev {
                    return Err(DbError::SettingConflict {
                        key: String::from("ai_settings"),
                        expected_revision: i64::from(sanitized.revision),
                        current_revision: i64::from(current_rev),
                    });
                }

                sanitized.revision = current_rev.saturating_add(1);
                let payload = serde_json::to_string(&sanitized).map_err(|e| {
                    DbError::SettingInvalidValue {
                        key: String::from("ai_settings"),
                        reason: e.to_string(),
                    }
                })?;

                tx.execute(
                    "INSERT INTO ai_settings (id, payload, updated_at)
                     VALUES (1, ?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                     ON CONFLICT(id) DO UPDATE SET
                        payload = excluded.payload,
                        updated_at = excluded.updated_at;",
                    rusqlite::params![payload],
                )
                .map_err(|e| DbError::from_rusqlite(e, "ai_settings_upsert"))?;

                Ok(sanitized)
            })
            .await;

        match res {
            Ok(saved) => Ok(saved),
            Err(DbError::SettingConflict {
                expected_revision,
                current_revision,
                ..
            }) => Err(ai_error_with_details(
                codes::AI_SETTINGS_CONFLICT,
                "As preferências de IA foram modificadas em outra janela ou operação concorrente.",
                json!({
                    "expectedRevision": expected_revision,
                    "currentRevision": current_revision,
                }),
            )),
            Err(other) => Err(MtError::from(other)),
        }
    }

    /// Busca o catálogo em cache para um `gateway_key` específico.
    pub async fn get_catalog_cache(&self, gateway_key: &str) -> MtResult<Option<CachedCatalogRow>> {
        let key = gateway_key.to_string();
        self.db
            .read(move |conn| {
                conn.query_row(
                    "SELECT gateway_key, payload, etag, fetched_at
                     FROM ai_catalog_cache
                     WHERE gateway_key = ?1;",
                    rusqlite::params![key],
                    |r| {
                        Ok(CachedCatalogRow {
                            gateway_key: r.get(0)?,
                            payload_json: r.get(1)?,
                            etag: r.get(2)?,
                            fetched_at: r.get(3)?,
                        })
                    },
                )
                .optional()
                .map_err(|e| DbError::from_rusqlite(e, "ai_catalog_cache_get"))
            })
            .await
            .map_err(MtError::from)
    }

    /// Persiste o catálogo em cache se `expected_cache_gen` coincidir com a geração atual no writer.
    pub async fn put_catalog_cache(
        &self,
        expected_cache_gen: u64,
        row: &CachedCatalogRow,
    ) -> MtResult<bool> {
        let cache_gen = Arc::clone(&self.cache_generation);
        let gateway_key = mt_db::sanitizer::sanitize(&row.gateway_key).into_owned();
        let payload_json = mt_db::sanitizer::sanitize(&row.payload_json).into_owned();
        let etag = row
            .etag
            .as_ref()
            .map(|e| mt_db::sanitizer::sanitize(e).into_owned());
        let fetched_at = mt_db::sanitizer::sanitize(&row.fetched_at).into_owned();

        self.db
            .write(move |tx| {
                if cache_gen.load(Ordering::SeqCst) != expected_cache_gen {
                    return Ok(false);
                }
                tx.execute(
                    "INSERT INTO ai_catalog_cache (gateway_key, payload, etag, fetched_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(gateway_key) DO UPDATE SET
                        payload = excluded.payload,
                        etag = excluded.etag,
                        fetched_at = excluded.fetched_at;",
                    rusqlite::params![gateway_key, payload_json, etag, fetched_at],
                )
                .map_err(|e| DbError::from_rusqlite(e, "ai_catalog_cache_put"))?;
                Ok(true)
            })
            .await
            .map_err(MtError::from)
    }

    /// Consulta o cache de diagnóstico por `error_hash` verificando `expires_at > now_rfc3339`
    /// e incrementando `hits` e `last_hit_at` atomicamente sem prorrogar `expires_at`.
    pub async fn get_diagnosis_cache(
        &self,
        error_hash: &str,
        now_rfc3339: &str,
    ) -> MtResult<Option<CachedDiagnosisRow>> {
        let hash = error_hash.to_string();
        let now = now_rfc3339.to_string();

        self.db
            .write(move |tx| {
                let found: Option<CachedDiagnosisRow> = tx
                    .query_row(
                        "SELECT error_hash, gateway_key, project_id, summary, fix_command,
                                fix_risk, harness_id, model_id, reasoning_id, prompt_version,
                                sanitizer_version, hits, created_at, expires_at
                         FROM ai_diagnosis_cache
                         WHERE error_hash = ?1 AND expires_at > ?2;",
                        rusqlite::params![hash, now],
                        |r| {
                            let proj_str: Option<String> = r.get(2)?;
                            let project_id = proj_str
                                .as_deref()
                                .and_then(|s| s.parse::<ProjectId>().ok());
                            let hits_i64: i64 = r.get(11)?;
                            let pv_i64: i64 = r.get(9)?;
                            let sv_i64: i64 = r.get(10)?;
                            Ok(CachedDiagnosisRow {
                                error_hash: r.get(0)?,
                                gateway_key: r.get(1)?,
                                project_id,
                                summary: r.get(3)?,
                                fix_command: r.get(4)?,
                                fix_risk: r.get(5)?,
                                harness_id: r.get(6)?,
                                model_id: r.get(7)?,
                                reasoning_id: r.get(8)?,
                                prompt_version: u32::try_from(pv_i64).unwrap_or(1),
                                sanitizer_version: u32::try_from(sv_i64).unwrap_or(1),
                                hits: u32::try_from(hits_i64).unwrap_or(0).saturating_add(1),
                                created_at: r.get(12)?,
                                expires_at: r.get(13)?,
                            })
                        },
                    )
                    .optional()
                    .map_err(|e| DbError::from_rusqlite(e, "ai_diagnosis_cache_get"))?;

                if found.is_some() {
                    tx.execute(
                        "UPDATE ai_diagnosis_cache
                         SET hits = hits + 1,
                             last_hit_at = ?2
                         WHERE error_hash = ?1;",
                        rusqlite::params![hash, now],
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "ai_diagnosis_cache_hit"))?;
                }

                Ok(found)
            })
            .await
            .map_err(MtError::from)
    }

    /// Grava ou atualiza uma entrada no cache de diagnósticos se `expected_cache_gen` ainda for válido.
    pub async fn put_diagnosis_cache(
        &self,
        expected_cache_gen: u64,
        row: &CachedDiagnosisRow,
    ) -> MtResult<bool> {
        let cache_gen = Arc::clone(&self.cache_generation);
        let error_hash = row.error_hash.clone();
        let gateway_key = mt_db::sanitizer::sanitize(&row.gateway_key).into_owned();
        let project_id = row.project_id.map(|p| p.to_string());
        let summary = mt_db::sanitizer::sanitize(&row.summary).into_owned();
        let fix_command = row
            .fix_command
            .as_ref()
            .map(|c| mt_db::sanitizer::sanitize(c).into_owned());
        let fix_risk = row.fix_risk.clone();
        let harness_id = mt_db::sanitizer::sanitize(&row.harness_id).into_owned();
        let model_id = mt_db::sanitizer::sanitize(&row.model_id).into_owned();
        let reasoning_id = row
            .reasoning_id
            .as_ref()
            .map(|r| mt_db::sanitizer::sanitize(r).into_owned());
        let prompt_version = i64::from(row.prompt_version);
        let sanitizer_version = i64::from(row.sanitizer_version);
        let hits = i64::from(row.hits);
        let created_at = row.created_at.clone();
        let expires_at = row.expires_at.clone();

        self.db
            .write(move |tx| {
                if cache_gen.load(Ordering::SeqCst) != expected_cache_gen {
                    return Ok(false);
                }
                tx.execute(
                    "INSERT INTO ai_diagnosis_cache (
                        error_hash, gateway_key, project_id, summary, fix_command, fix_risk,
                        harness_id, model_id, reasoning_id, prompt_version, sanitizer_version,
                        hits, created_at, last_hit_at, expires_at
                     ) VALUES (?1, ?2, (SELECT id FROM projects WHERE id = ?3), ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, ?14)
                     ON CONFLICT(error_hash) DO UPDATE SET
                        gateway_key = excluded.gateway_key,
                        project_id = excluded.project_id,
                        summary = excluded.summary,
                        fix_command = excluded.fix_command,
                        fix_risk = excluded.fix_risk,
                        harness_id = excluded.harness_id,
                        model_id = excluded.model_id,
                        reasoning_id = excluded.reasoning_id,
                        prompt_version = excluded.prompt_version,
                        sanitizer_version = excluded.sanitizer_version,
                        created_at = excluded.created_at,
                        expires_at = excluded.expires_at;",
                    rusqlite::params![
                        error_hash,
                        gateway_key,
                        project_id,
                        summary,
                        fix_command,
                        fix_risk,
                        harness_id,
                        model_id,
                        reasoning_id,
                        prompt_version,
                        sanitizer_version,
                        hits,
                        created_at,
                        expires_at,
                    ],
                )
                .map_err(|e| DbError::from_rusqlite(e, "ai_diagnosis_cache_put"))?;
                Ok(true)
            })
            .await
            .map_err(MtError::from)
    }

    /// Insere um item no histórico de prompts se `save_history_enabled` for `true` e `expected_history_gen` coincidir.
    #[allow(clippy::too_many_lines)]
    pub async fn insert_history(
        &self,
        expected_history_gen: u64,
        save_history_enabled: bool,
        item: AiHistoryItem,
    ) -> MtResult<bool> {
        if !save_history_enabled {
            return Ok(false);
        }
        let history_gen = Arc::clone(&self.history_generation);
        let insert_counter = Arc::clone(&self.insert_counter);

        let id_str = item.id.to_string();
        let kind_str = item.kind.as_str().to_string();
        let trigger = item.trigger.clone();
        let project_id = item.project_id.map(|p| p.to_string());
        let tab_id = item.tab_id.map(|t| t.to_string());
        let pane_id = item.pane_id.map(|p| p.to_string());
        let session_id = item.session_id.map(|s| s.to_string());
        let block_id = item.block_id.map(|b| b.to_string());
        let harness_id = item
            .harness_id
            .as_ref()
            .map(|s| mt_db::sanitizer::sanitize(s).into_owned());
        let model_id = item
            .model_id
            .as_ref()
            .map(|s| mt_db::sanitizer::sanitize(s).into_owned());
        let reasoning_id = item
            .reasoning_id
            .as_ref()
            .map(|s| mt_db::sanitizer::sanitize(s).into_owned());
        let rag_slug = item
            .rag_slug
            .as_ref()
            .map(|s| mt_db::sanitizer::sanitize(s).into_owned());
        let schema_id = item
            .schema_id
            .as_ref()
            .map(|s| mt_db::sanitizer::sanitize(s).into_owned());
        let prompt_sanitized = mt_db::sanitizer::sanitize(&item.prompt_sanitized).into_owned();
        let response_sanitized = item
            .response_sanitized
            .as_ref()
            .map(|s| mt_db::sanitizer::sanitize(s).into_owned());
        let risk_level = item.risk_level.clone();
        let status = item.status.clone();
        let error_code = item.error_code.clone();
        let latency_ms = item.latency_ms.map(i64::from);
        let created_at = item.created_at.clone();

        self.db
            .write(move |tx| {
                if history_gen.load(Ordering::SeqCst) != expected_history_gen {
                    return Ok(false);
                }

                tx.execute(
                    "INSERT OR REPLACE INTO ai_prompt_history (
                        id, kind, trigger, project_id, tab_id, pane_id, session_id, block_id,
                        harness_id, model_id, reasoning_id, rag_slug, schema_id,
                        prompt_sanitized, response_sanitized, risk_level, status, error_code,
                        latency_ms, created_at
                     ) VALUES (
                        ?1, ?2, ?3, (SELECT id FROM projects WHERE id = ?4), ?5, ?6, ?7, ?8,
                        ?9, ?10, ?11, ?12, ?13,
                        ?14, ?15, ?16, ?17, ?18,
                        ?19, ?20
                     );",
                    rusqlite::params![
                        id_str,
                        kind_str,
                        trigger,
                        project_id,
                        tab_id,
                        pane_id,
                        session_id,
                        block_id,
                        harness_id,
                        model_id,
                        reasoning_id,
                        rag_slug,
                        schema_id,
                        prompt_sanitized,
                        response_sanitized,
                        risk_level,
                        status,
                        error_code,
                        latency_ms,
                        created_at,
                    ],
                )
                .map_err(|e| DbError::from_rusqlite(e, "ai_history_insert"))?;

                let count = insert_counter.fetch_add(1, Ordering::SeqCst) + 1;
                #[allow(clippy::incompatible_msrv)]
                if count.is_multiple_of(HISTORY_PRUNE_BATCH as u64) {
                    #[allow(clippy::cast_possible_wrap)]
                    let max_rows = HISTORY_MAX_ROWS as i64;
                    tx.execute(
                        "DELETE FROM ai_prompt_history
                         WHERE id NOT IN (
                            SELECT id FROM ai_prompt_history
                            ORDER BY id DESC
                            LIMIT ?1
                         );",
                        rusqlite::params![max_rows],
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "ai_history_prune"))?;
                }

                Ok(true)
            })
            .await
            .map_err(MtError::from)
    }

    /// Lista páginas do histórico de prompts com filtro por `kind`, `project_id` e cursor keyset `before_id`.
    pub async fn list_history(&self, filter: &AiHistoryFilter) -> MtResult<AiHistoryPage> {
        let limit = usize::from(filter.limit.clamp(1, 200));
        let fetch_limit = i64::try_from(limit + 1).unwrap_or(51);
        let kind_str = filter.kind.map(|k| k.as_str().to_string());
        let project_str = filter.project_id.map(|p| p.to_string());
        let before_id = filter.before_id.clone();

        self.db
            .read(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT id, kind, trigger, project_id, tab_id, pane_id, session_id, block_id,
                                harness_id, model_id, reasoning_id, rag_slug, schema_id,
                                prompt_sanitized, response_sanitized, risk_level, status, error_code,
                                latency_ms, created_at
                         FROM ai_prompt_history
                         WHERE (?1 IS NULL OR kind = ?1)
                           AND (?2 IS NULL OR project_id = ?2)
                           AND (?3 IS NULL OR id < ?3)
                         ORDER BY id DESC
                         LIMIT ?4;",
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "ai_history_list_prepare"))?;

                let rows = stmt
                    .query_map(
                        rusqlite::params![kind_str, project_str, before_id, fetch_limit],
                        |r| {
                            let id_raw: String = r.get(0)?;
                            let id = AiRequestId::parse(&id_raw)
                                .unwrap_or_else(|_| AiRequestId(uuid::Uuid::nil()));
                            let kind_raw: String = r.get(1)?;
                            let proj_raw: Option<String> = r.get(3)?;
                            let tab_raw: Option<String> = r.get(4)?;
                            let pane_raw: Option<String> = r.get(5)?;
                            let sess_raw: Option<String> = r.get(6)?;
                            let blk_raw: Option<String> = r.get(7)?;
                            let lat_raw: Option<i64> = r.get(18)?;

                            Ok(AiHistoryItem {
                                id,
                                kind: AiHistoryKind::from_db_str(&kind_raw),
                                trigger: r.get(2)?,
                                project_id: proj_raw
                                    .as_deref()
                                    .and_then(|s| s.parse::<ProjectId>().ok()),
                                tab_id: tab_raw
                                    .as_deref()
                                    .and_then(|s| s.parse::<TabId>().ok()),
                                pane_id: pane_raw
                                    .as_deref()
                                    .and_then(|s| s.parse::<PaneId>().ok()),
                                session_id: sess_raw
                                    .as_deref()
                                    .and_then(|s| s.parse::<SessionId>().ok()),
                                block_id: blk_raw
                                    .as_deref()
                                    .and_then(|s| s.parse::<CommandBlockId>().ok()),
                                harness_id: r.get(8)?,
                                model_id: r.get(9)?,
                                reasoning_id: r.get(10)?,
                                rag_slug: r.get(11)?,
                                schema_id: r.get(12)?,
                                prompt_sanitized: r.get(13)?,
                                response_sanitized: r.get(14)?,
                                risk_level: r.get(15)?,
                                status: r.get(16)?,
                                error_code: r.get(17)?,
                                latency_ms: lat_raw.and_then(|v| u32::try_from(v).ok()),
                                created_at: r.get(19)?,
                            })
                        },
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "ai_history_list_query"))?;

                let mut items = Vec::new();
                for row in rows {
                    items.push(row.map_err(|e| DbError::from_rusqlite(e, "ai_history_list_row"))?);
                }

                let next_before_id = if items.len() > limit {
                    items.truncate(limit);
                    items.last().map(|last| last.id.to_string())
                } else {
                    None
                };

                Ok(AiHistoryPage {
                    items,
                    next_before_id,
                })
            })
            .await
            .map_err(MtError::from)
    }

    /// Limpa o histórico de prompts globalmente ou apenas de um `project_id`, incrementando a geração no writer.
    pub async fn clear_history(&self, project_id: Option<ProjectId>) -> MtResult<u32> {
        let history_gen = Arc::clone(&self.history_generation);
        let proj_str = project_id.map(|p| p.to_string());

        self.db
            .write(move |tx| {
                history_gen.fetch_add(1, Ordering::SeqCst);
                let deleted = match proj_str {
                    Some(pid) => tx
                        .execute(
                            "DELETE FROM ai_prompt_history WHERE project_id = ?1;",
                            rusqlite::params![pid],
                        )
                        .map_err(|e| DbError::from_rusqlite(e, "ai_history_clear_project"))?,
                    None => tx
                        .execute("DELETE FROM ai_prompt_history;", [])
                        .map_err(|e| DbError::from_rusqlite(e, "ai_history_clear_all"))?,
                };
                Ok(u32::try_from(deleted).unwrap_or(u32::MAX))
            })
            .await
            .map_err(MtError::from)
    }

    /// Limpa caches de IA conforme o `CacheClearScope`, incrementando a geração de cache no writer.
    pub async fn clear_cache(&self, scope: CacheClearScope) -> MtResult<u32> {
        let cache_gen = Arc::clone(&self.cache_generation);

        self.db
            .write(move |tx| {
                cache_gen.fetch_add(1, Ordering::SeqCst);
                let mut total = 0usize;
                if matches!(scope, CacheClearScope::Diagnosis | CacheClearScope::All) {
                    total += tx
                        .execute("DELETE FROM ai_diagnosis_cache;", [])
                        .map_err(|e| DbError::from_rusqlite(e, "ai_diagnosis_cache_clear"))?;
                }
                if matches!(scope, CacheClearScope::Catalog | CacheClearScope::All) {
                    total += tx
                        .execute("DELETE FROM ai_catalog_cache;", [])
                        .map_err(|e| DbError::from_rusqlite(e, "ai_catalog_cache_clear"))?;
                }
                Ok(u32::try_from(total).unwrap_or(u32::MAX))
            })
            .await
            .map_err(MtError::from)
    }

    /// Remove entradas expiradas da tabela `ai_diagnosis_cache`.
    pub async fn prune_expired_diagnoses(&self, now_rfc3339: &str) -> MtResult<u32> {
        let now = now_rfc3339.to_string();
        self.db
            .write(move |tx| {
                let deleted = tx
                    .execute(
                        "DELETE FROM ai_diagnosis_cache WHERE expires_at <= ?1;",
                        rusqlite::params![now],
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "ai_diagnosis_cache_prune"))?;
                Ok(u32::try_from(deleted).unwrap_or(u32::MAX))
            })
            .await
            .map_err(MtError::from)
    }
}
