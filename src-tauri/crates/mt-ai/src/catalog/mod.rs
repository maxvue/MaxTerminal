//! Catálogo hierárquico **Harness → Modelo → Nível de Raciocínio** (`CatalogService`).

pub mod fallback;

use crate::catalog::fallback::canonical_fallback_harnesses;
use crate::client::dialect::GatewayCapabilities;
use crate::client::http::{Conditional, GatewayClient, Op, now_iso8601};
use crate::config::AiSelection;
use crate::error::{ai_error, codes};
use crate::repo::{AiRepo, CachedCatalogRow};
use mt_core::MtResult;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Origem do catálogo retornado ao frontend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CatalogSource {
    /// Obtido diretamente do gateway (`200 OK` ou `304 Not Modified`).
    Gateway,
    /// Obtido do cache persistente SQLite (`ai_catalog_cache`).
    Cache,
    /// Fallback estático somente com as identidades dos 4 harnesses canônicos (`models: []`).
    Fallback,
}

/// Nível de raciocínio suportado por um modelo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningLevel {
    /// Identificador técnico (ex.: `"low"`, `"medium"`, `"high"`).
    pub id: String,
    /// Rótulo amigável exibido na interface.
    pub label: String,
    /// Ordem de complexidade/custo (`0` = mais rápido/econômico).
    #[serde(default)]
    pub rank: u8,
}

/// Modelo de IA associado a um harness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    /// Identificador do modelo.
    pub id: String,
    /// Rótulo amigável do modelo.
    pub label: String,
    /// Lista de níveis de raciocínio suportados (vazia quando não configurável).
    #[serde(default)]
    pub reasoning_levels: Vec<ReasoningLevel>,
    /// Nível de raciocínio padrão sugerido pelo modelo.
    pub default_reasoning: Option<String>,
    /// Tamanho da janela de contexto em tokens.
    pub context_window: Option<u32>,
}

/// Harness de IA (ex.: `Claude Code`, `Codex`, `Antigravity (agy)`, `OpenCode`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Harness {
    /// Identificador canônico do harness.
    pub id: String,
    /// Nome de exibição do harness.
    pub label: String,
    /// Ícone Iconify do harness.
    pub icon: String,
    /// Modelos disponíveis para este harness.
    #[serde(default)]
    pub models: Vec<AiModel>,
}

/// Catálogo completo de harnesses, modelos e capacidades do gateway.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HarnessCatalog {
    /// Revisão das configurações vigente ao montar o catálogo.
    pub config_revision: u32,
    /// Capacidades validadas do gateway.
    pub capabilities: GatewayCapabilities,
    /// Fonte dos dados do catálogo.
    pub source: CatalogSource,
    /// Timestamp ISO-8601 UTC da coleta (`None` no fallback estático).
    pub fetched_at: Option<String>,
    /// Lista de harnesses (sempre iniciando pelos 4 canônicos).
    pub harnesses: Vec<Harness>,
}

/// Seleção validada contra o catálogo com rótulos prontos para exibição.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedSelection {
    /// Seleção técnica normalizada.
    pub selection: AiSelection,
    /// Nome do harness.
    pub harness_label: String,
    /// Nome do modelo.
    pub model_label: String,
    /// Nome do nível de raciocínio (quando aplicável).
    pub reasoning_label: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum GatewayCatalogWire {
    Wrapped { harnesses: Vec<Harness> },
    List(Vec<Harness>),
}

impl GatewayCatalogWire {
    fn into_harnesses(self) -> Vec<Harness> {
        match self {
            Self::Wrapped { harnesses } | Self::List(harnesses) => harnesses,
        }
    }
}

/// Serviço gerenciador do catálogo de harnesses com cache em memória (TTL 6h),
/// cache SQLite por `gateway_key`, revalidação `ETag` e fallback de identidades (`CA-11`, `CA-12`).
pub struct CatalogService {
    client: Arc<GatewayClient>,
    repo: Arc<AiRepo>,
    mem: RwLock<Option<(String, HarnessCatalog, Instant)>>,
}

impl CatalogService {
    /// Tempo de vida do cache em memória antes de revalidar com o gateway (`6 horas`).
    pub const TTL: Duration = Duration::from_secs(6 * 3600);

    /// Cria uma nova instância do `CatalogService`.
    #[must_use]
    pub fn new(client: Arc<GatewayClient>, repo: Arc<AiRepo>) -> Self {
        Self {
            client,
            repo,
            mem: RwLock::new(None),
        }
    }

    /// Invalida imediatamente o cache em memória.
    pub fn invalidate_memory(&self) {
        *self.mem.write() = None;
    }

    /// Retorna o catálogo atual sem falhar: consulta memória (dentro do TTL de 6h),
    /// depois tenta o gateway, recorrendo ao cache SQLite ou ao fallback de identidades.
    pub async fn get(&self) -> HarnessCatalog {
        let gw_key = self.client.gateway_key();
        if let Some((cached_key, cat, ts)) = self.mem.read().clone()
            && cached_key == gw_key
            && ts.elapsed() < Self::TTL
        {
            return cat;
        }

        match self.fetch_from_gateway_or_cache(false).await {
            Ok(cat) => cat,
            Err(_) => self.fallback_catalog(),
        }
    }

    /// Força a atualização do catálogo consultando o gateway (com `If-None-Match` se houver `ETag`).
    pub async fn refresh(&self) -> MtResult<HarnessCatalog> {
        self.invalidate_memory();
        self.fetch_from_gateway_or_cache(true).await
    }

    /// Retorna uma cópia síncrona do catálogo em memória ou o fallback de identidades (sem I/O).
    #[must_use]
    pub fn snapshot_or_fallback(&self) -> HarnessCatalog {
        let gw_key = self.client.gateway_key();
        if let Some((cached_key, cat, _)) = self.mem.read().clone()
            && cached_key == gw_key
        {
            return cat;
        }
        self.fallback_catalog()
    }

    /// Valida uma `AiSelection` contra o `HarnessCatalog`, sem jamais inventar modelos ou níveis de raciocínio (`CA-11`).
    pub fn resolve(
        &self,
        catalog: &HarnessCatalog,
        sel: &AiSelection,
    ) -> MtResult<ResolvedSelection> {
        let harness = catalog
            .harnesses
            .iter()
            .find(|h| h.id == sel.harness_id)
            .ok_or_else(|| {
                ai_error(
                    codes::AI_UNKNOWN_SELECTION,
                    format!(
                        "O harness '{}' não foi encontrado no catálogo do MaxAiManager.",
                        sel.harness_id
                    ),
                )
            })?;

        let model = harness
            .models
            .iter()
            .find(|m| m.id == sel.model_id)
            .ok_or_else(|| {
                ai_error(
                    codes::AI_UNKNOWN_SELECTION,
                    format!(
                        "O modelo '{}' não está disponível para o harness '{}'.",
                        sel.model_id, harness.label
                    ),
                )
            })?;

        if model.reasoning_levels.is_empty() {
            if sel.reasoning_id.is_some() {
                return Err(ai_error(
                    codes::AI_UNKNOWN_SELECTION,
                    format!(
                        "O modelo '{}' não aceita nível de raciocínio configurável.",
                        model.label
                    ),
                ));
            }
            return Ok(ResolvedSelection {
                selection: AiSelection {
                    harness_id: harness.id.clone(),
                    model_id: model.id.clone(),
                    reasoning_id: None,
                },
                harness_label: harness.label.clone(),
                model_label: model.label.clone(),
                reasoning_label: None,
            });
        }

        let effective_reasoning_id = sel
            .reasoning_id
            .as_deref()
            .or(model.default_reasoning.as_deref())
            .ok_or_else(|| {
                ai_error(
                    codes::AI_UNKNOWN_SELECTION,
                    format!(
                        "Selecione um nível de raciocínio válido para o modelo '{}'.",
                        model.label
                    ),
                )
            })?;

        let level = model
            .reasoning_levels
            .iter()
            .find(|r| r.id == effective_reasoning_id)
            .ok_or_else(|| {
                ai_error(
                    codes::AI_UNKNOWN_SELECTION,
                    format!(
                        "O nível de raciocínio '{}' não é suportado pelo modelo '{}'.",
                        effective_reasoning_id, model.label
                    ),
                )
            })?;

        Ok(ResolvedSelection {
            selection: AiSelection {
                harness_id: harness.id.clone(),
                model_id: model.id.clone(),
                reasoning_id: Some(level.id.clone()),
            },
            harness_label: harness.label.clone(),
            model_label: model.label.clone(),
            reasoning_label: Some(level.label.clone()),
        })
    }

    /// Retorna o identificador do nível de raciocínio de menor `rank` para um modelo (usado como padrão econômico em diagnósticos).
    #[must_use]
    pub fn lowest_reasoning(
        &self,
        catalog: &HarnessCatalog,
        harness_id: &str,
        model_id: &str,
    ) -> Option<String> {
        let harness = catalog.harnesses.iter().find(|h| h.id == harness_id)?;
        let model = harness.models.iter().find(|m| m.id == model_id)?;
        model
            .reasoning_levels
            .iter()
            .min_by_key(|r| r.rank)
            .map(|r| r.id.clone())
    }

    fn fallback_catalog(&self) -> HarnessCatalog {
        HarnessCatalog {
            config_revision: self.client.config_revision(),
            capabilities: self.client.capabilities(),
            source: CatalogSource::Fallback,
            fetched_at: None,
            harnesses: canonical_fallback_harnesses(),
        }
    }

    async fn fetch_from_gateway_or_cache(&self, strict_refresh: bool) -> MtResult<HarnessCatalog> {
        let gw_key = self.client.gateway_key();
        let cache_gen = self.repo.current_cache_generation();
        let cached_row = self.repo.get_catalog_cache(&gw_key).await.ok().flatten();
        let cached_etag = cached_row.as_ref().and_then(|r| r.etag.as_deref());
        let path = self.client.dialect().catalog_path();

        match self
            .client
            .get_json::<GatewayCatalogWire>(path, Op::Catalog, cached_etag)
            .await
        {
            Ok(Conditional::Fresh { value, etag }) => {
                let merged = merge_with_canonical(value.into_harnesses());
                let now = now_iso8601();
                if let Ok(payload_json) = serde_json::to_string(&merged) {
                    let _ = self
                        .repo
                        .put_catalog_cache(
                            cache_gen,
                            &CachedCatalogRow {
                                gateway_key: gw_key.clone(),
                                payload_json,
                                etag,
                                fetched_at: now.clone(),
                            },
                        )
                        .await;
                }
                let catalog = HarnessCatalog {
                    config_revision: self.client.config_revision(),
                    capabilities: self.client.capabilities(),
                    source: CatalogSource::Gateway,
                    fetched_at: Some(now),
                    harnesses: merged,
                };
                *self.mem.write() = Some((gw_key, catalog.clone(), Instant::now()));
                Ok(catalog)
            }
            Ok(Conditional::NotModified) => {
                if let Some(row) = cached_row
                    && let Ok(parsed) = serde_json::from_str::<Vec<Harness>>(&row.payload_json)
                {
                    let now = now_iso8601();
                    let _ = self
                        .repo
                        .put_catalog_cache(
                            cache_gen,
                            &CachedCatalogRow {
                                gateway_key: gw_key.clone(),
                                payload_json: row.payload_json,
                                etag: row.etag,
                                fetched_at: now.clone(),
                            },
                        )
                        .await;
                    let catalog = HarnessCatalog {
                        config_revision: self.client.config_revision(),
                        capabilities: self.client.capabilities(),
                        source: CatalogSource::Gateway,
                        fetched_at: Some(now),
                        harnesses: merge_with_canonical(parsed),
                    };
                    *self.mem.write() = Some((gw_key, catalog.clone(), Instant::now()));
                    return Ok(catalog);
                }
                Ok(self.fallback_catalog())
            }
            Err(err) => {
                if let Some(row) = cached_row
                    && let Ok(parsed) = serde_json::from_str::<Vec<Harness>>(&row.payload_json)
                {
                    let catalog = HarnessCatalog {
                        config_revision: self.client.config_revision(),
                        capabilities: self.client.capabilities(),
                        source: CatalogSource::Cache,
                        fetched_at: Some(row.fetched_at),
                        harnesses: merge_with_canonical(parsed),
                    };
                    *self.mem.write() = Some((gw_key, catalog.clone(), Instant::now()));
                    return Ok(catalog);
                }
                if strict_refresh
                    && err.code != codes::AI_HTTP_STATUS
                    && err.code != codes::AI_DISABLED
                    && err.code != codes::AI_NOT_CONFIGURED
                {
                    let fb = self.fallback_catalog();
                    *self.mem.write() = Some((gw_key, fb.clone(), Instant::now()));
                    return Ok(fb);
                }
                let fb = self.fallback_catalog();
                *self.mem.write() = Some((gw_key, fb.clone(), Instant::now()));
                Ok(fb)
            }
        }
    }
}

fn normalize_harness(mut h: Harness) -> Harness {
    h.id = mt_db::sanitizer::sanitize(h.id.trim()).into_owned();
    h.label = mt_db::sanitizer::sanitize(h.label.trim()).into_owned();
    h.icon = mt_db::sanitizer::sanitize(h.icon.trim()).into_owned();
    for m in &mut h.models {
        m.id = mt_db::sanitizer::sanitize(m.id.trim()).into_owned();
        m.label = mt_db::sanitizer::sanitize(m.label.trim()).into_owned();
        let all_zero = m.reasoning_levels.iter().all(|r| r.rank == 0);
        for (idx, r) in m.reasoning_levels.iter_mut().enumerate() {
            r.id = mt_db::sanitizer::sanitize(r.id.trim()).into_owned();
            r.label = mt_db::sanitizer::sanitize(r.label.trim()).into_owned();
            if all_zero {
                r.rank = u8::try_from(idx).unwrap_or(u8::MAX);
            }
        }
    }
    h
}

fn merge_with_canonical(remote: Vec<Harness>) -> Vec<Harness> {
    let mut normalized_remote: Vec<Harness> = remote.into_iter().map(normalize_harness).collect();
    let canonical = canonical_fallback_harnesses();
    let mut seen_ids = HashSet::new();
    let mut out = Vec::with_capacity(canonical.len() + normalized_remote.len());

    for mut base in canonical {
        if let Some(pos) = normalized_remote.iter().position(|r| r.id == base.id) {
            let found = normalized_remote.remove(pos);
            if !found.label.is_empty() {
                base.label = found.label;
            }
            if !found.icon.is_empty() {
                base.icon = found.icon;
            }
            base.models = found.models;
        }
        seen_ids.insert(base.id.clone());
        out.push(base);
    }

    for extra in normalized_remote {
        if !extra.id.is_empty() && seen_ids.insert(extra.id.clone()) {
            out.push(extra);
        }
    }

    out
}
