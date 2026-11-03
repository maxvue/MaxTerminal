//! Modelos de configuração e validação de segurança do subsistema `mt-ai`.

use crate::error::{ai_error, codes};
use mt_core::MtResult;
use mt_core::ids::ProjectId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;
use url::Url;

/// Timeout mínimo permitido por operação em milissegundos.
pub const MIN_TIMEOUT_MS: u32 = 300;
/// Timeout máximo permitido por operação em milissegundos.
pub const MAX_TIMEOUT_MS: u32 = 120_000;
/// Quantidade máxima de comandos recentes enviados no contexto.
pub const MAX_RECENT_COMMANDS: u8 = 20;
/// Quantidade máxima de trechos retornados em consultas RAG (`top_k`).
pub const MAX_RAG_TOP_K: u8 = 20;

/// Seleção de Harness, Modelo e Nível de Raciocínio opcional.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiSelection {
    /// Identificador do harness (ex.: `"claude-code"`, `"codex"`, `"agy"`, `"opencode"`).
    pub harness_id: String,
    /// Identificador do modelo dentro do harness.
    pub model_id: String,
    /// Identificador do nível de raciocínio (`None` quando o modelo não expõe níveis configuráveis).
    pub reasoning_id: Option<String>,
}

/// Preferências do Diagnóstico Rápido Inline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisSettings {
    /// Ativa ou desativa a análise automática de blocos com erro.
    pub enabled: bool,
    /// Atraso de debounce por painel antes de disparar o diagnóstico (padrão `800` ms).
    pub debounce_ms: u32,
    /// Seleção dedicada opcional para diagnóstico (quando `None`, usa `default_selection` com menor raciocínio).
    pub selection: Option<AiSelection>,
    /// Códigos de saída que nunca disparam diagnóstico automático (padrão `[130]`).
    pub ignored_exit_codes: Vec<i32>,
    /// Lista de projetos com opt-out de diagnóstico automático.
    pub disabled_projects: Vec<ProjectId>,
    /// Limite máximo de chamadas de diagnóstico ao gateway por minuto (padrão `10`).
    pub max_per_minute: u32,
}

impl Default for DiagnosisSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_ms: 800,
            selection: None,
            ignored_exit_codes: vec![130],
            disabled_projects: Vec::new(),
            max_per_minute: 10,
        }
    }
}

/// Preferências de Consulta RAG.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RagSettings {
    /// Quantidade de resultados solicitados ao endpoint RAG (`1..=20`, padrão `5`).
    pub top_k: u8,
    /// Mapeamento entre stack detectada do projeto e slug RAG correspondente.
    pub stack_slugs: BTreeMap<String, String>,
    /// Último slug selecionado manualmente pelo usuário.
    pub last_slug: Option<String>,
}

impl Default for RagSettings {
    fn default() -> Self {
        let mut stack_slugs = BTreeMap::new();
        stack_slugs.insert("laravel".to_string(), "laravel".to_string());
        stack_slugs.insert("vue".to_string(), "vue".to_string());
        stack_slugs.insert("tauri".to_string(), "tauri".to_string());
        stack_slugs.insert("rust".to_string(), "rust".to_string());
        Self {
            top_k: 5,
            stack_slugs,
            last_slug: None,
        }
    }
}

/// Preferências de coleta de contexto do terminal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextSettings {
    /// Quantidade de comandos recentes enviados no contexto (`0..=20`, padrão `10`).
    pub recent_commands: u8,
    /// Indica se a cauda de saída do último comando deve ser incluída em `@` e `?`.
    pub include_output_tail: bool,
    /// Limite em bytes da cauda de saída incluída (padrão `4096`).
    pub output_tail_bytes: u32,
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self {
            recent_commands: 10,
            include_output_tail: false,
            output_tail_bytes: 4096,
        }
    }
}

/// Timeouts individuais por tipo de operação no gateway em milissegundos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TimeoutSettings {
    /// Timeout de conexão TCP e `/health` (padrão `2000` ms).
    pub connect_ms: u32,
    /// Timeout de sugestão NL→Shell (padrão `60000` ms).
    pub suggest_ms: u32,
    /// Timeout de diagnóstico rápido inline (padrão `20000` ms).
    pub diagnosis_ms: u32,
    /// Timeout de busca RAG (padrão `10000` ms).
    pub rag_ms: u32,
    /// Timeout total de geração estruturada incluindo eventual reparo (padrão `90000` ms).
    pub structured_ms: u32,
    /// Timeout de leitura do catálogo de harnesses (padrão `5000` ms).
    pub catalog_ms: u32,
}

impl Default for TimeoutSettings {
    fn default() -> Self {
        Self {
            connect_ms: 2_000,
            suggest_ms: 60_000,
            diagnosis_ms: 20_000,
            rag_ms: 10_000,
            structured_ms: 90_000,
            catalog_ms: 5_000,
        }
    }
}

/// Configuração completa do subsistema de IA persistida em `ai_settings`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    /// Revisão monotônica para controle de concorrência otimista (`>= 1`).
    pub revision: u32,
    /// Chave geral que habilita ou desabilita a integração com o `MaxAiManager`.
    pub enabled: bool,
    /// URL base do gateway (ex.: `Some("http://127.0.0.1:8787")`).
    pub base_url: Option<String>,
    /// Permite conectar a hosts fora de loopback quando `true`.
    pub allow_remote: bool,
    /// Seleção padrão utilizada pelo gatilho `?`.
    pub default_selection: Option<AiSelection>,
    /// Configurações de diagnóstico rápido inline.
    pub diagnosis: DiagnosisSettings,
    /// Configurações de consulta RAG.
    pub rag: RagSettings,
    /// Configurações de coleta de contexto.
    pub context: ContextSettings,
    /// Configurações de timeouts por operação.
    pub timeouts: TimeoutSettings,
    /// Habilita a gravação do histórico sanitizado de prompts no SQLite.
    pub save_history: bool,
}

impl Default for AiSettings {
    fn default() -> Self {
        let env_url = std::env::var("MAXTERMINAL_AI_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Self {
            revision: 1,
            enabled: true,
            base_url: env_url,
            allow_remote: false,
            default_selection: None,
            diagnosis: DiagnosisSettings::default(),
            rag: RagSettings::default(),
            context: ContextSettings::default(),
            timeouts: TimeoutSettings::default(),
            save_history: true,
        }
    }
}

impl AiSettings {
    /// Valida esquema HTTP/HTTPS, ausência de credenciais embutidas na URL, host loopback (`127.0.0.0/8`, `::1`, `localhost`)
    /// salvo `allow_remote = true`, e limites de timeouts, contexto e RAG.
    pub fn validate(&self) -> MtResult<()> {
        if let Some(raw_url) = &self.base_url {
            let trimmed = raw_url.trim();
            if trimmed.is_empty() {
                return Err(ai_error(
                    codes::AI_URL_INVALID,
                    "A URL do MaxAiManager não pode ser vazia.",
                ));
            }
            let parsed = Url::parse(trimmed).map_err(|_| {
                ai_error(
                    codes::AI_URL_INVALID,
                    "A URL informada para o MaxAiManager é inválida.",
                )
            })?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err(ai_error(
                    codes::AI_URL_INVALID,
                    "A URL do MaxAiManager deve utilizar o protocolo http:// ou https://.",
                ));
            }
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(ai_error(
                    codes::AI_URL_INVALID,
                    "A URL do MaxAiManager não deve conter credenciais (usuário/senha) embutidas.",
                ));
            }
            let host = parsed.host_str().ok_or_else(|| {
                ai_error(
                    codes::AI_URL_INVALID,
                    "A URL do MaxAiManager precisa especificar um host válido.",
                )
            })?;
            if !self.allow_remote && !is_loopback_host(host) {
                return Err(ai_error(
                    codes::AI_URL_NOT_LOOPBACK,
                    "Por segurança, apenas endereços de loopback (127.0.0.1, ::1 ou localhost) são permitidos quando 'Permitir host remoto' está desativado.",
                ));
            }
        }

        for (label, ms) in [
            ("connect_ms", self.timeouts.connect_ms),
            ("suggest_ms", self.timeouts.suggest_ms),
            ("diagnosis_ms", self.timeouts.diagnosis_ms),
            ("rag_ms", self.timeouts.rag_ms),
            ("structured_ms", self.timeouts.structured_ms),
            ("catalog_ms", self.timeouts.catalog_ms),
        ] {
            if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&ms) {
                return Err(ai_error(
                    codes::AI_URL_INVALID,
                    format!(
                        "O timeout '{label}' deve estar entre {MIN_TIMEOUT_MS} ms e {MAX_TIMEOUT_MS} ms."
                    ),
                ));
            }
        }

        if self.context.recent_commands > MAX_RECENT_COMMANDS {
            return Err(ai_error(
                codes::AI_URL_INVALID,
                format!(
                    "O limite máximo de comandos recentes no contexto é {MAX_RECENT_COMMANDS}."
                ),
            ));
        }

        if self.rag.top_k == 0 || self.rag.top_k > MAX_RAG_TOP_K {
            return Err(ai_error(
                codes::AI_URL_INVALID,
                format!("O parâmetro top_k do RAG deve estar entre 1 e {MAX_RAG_TOP_K}."),
            ));
        }

        Ok(())
    }

    /// Sanitiza campos textuais das configurações antes de persistir em JSON.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        if let Some(url) = &self.base_url {
            self.base_url = Some(mt_db::sanitizer::sanitize(url.trim()).into_owned());
        }
        if let Some(sel) = &mut self.default_selection {
            sel.harness_id = mt_db::sanitizer::sanitize(&sel.harness_id).into_owned();
            sel.model_id = mt_db::sanitizer::sanitize(&sel.model_id).into_owned();
            sel.reasoning_id = sel
                .reasoning_id
                .as_ref()
                .map(|r| mt_db::sanitizer::sanitize(r).into_owned());
        }
        if let Some(sel) = &mut self.diagnosis.selection {
            sel.harness_id = mt_db::sanitizer::sanitize(&sel.harness_id).into_owned();
            sel.model_id = mt_db::sanitizer::sanitize(&sel.model_id).into_owned();
            sel.reasoning_id = sel
                .reasoning_id
                .as_ref()
                .map(|r| mt_db::sanitizer::sanitize(r).into_owned());
        }
        if let Some(slug) = &self.rag.last_slug {
            self.rag.last_slug = Some(mt_db::sanitizer::sanitize(slug).into_owned());
        }
        self.rag.stack_slugs = self
            .rag
            .stack_slugs
            .into_iter()
            .map(|(k, v)| {
                (
                    mt_db::sanitizer::sanitize(&k).into_owned(),
                    mt_db::sanitizer::sanitize(&v).into_owned(),
                )
            })
            .collect();
        self
    }

    /// Gera a chave canônica identificadora do gateway para isolamento de caches (`gateway_key`).
    #[must_use]
    pub fn gateway_key(&self, dialect_id: &str) -> String {
        use sha2::{Digest, Sha256};
        let normalized_url = self
            .base_url
            .as_deref()
            .unwrap_or("unconfigured")
            .trim()
            .trim_end_matches('/')
            .to_ascii_lowercase();
        let mut hasher = Sha256::new();
        hasher.update(normalized_url.as_bytes());
        hasher.update(b"|");
        hasher.update(dialect_id.as_bytes());
        hex::encode(hasher.finalize())
    }
}

/// Verifica se o host informado pertence ao loopback (`localhost`, `127.0.0.0/8` ou `::1`).
#[must_use]
pub fn is_loopback_host(host: &str) -> bool {
    let clean = host.trim_start_matches('[').trim_end_matches(']');
    if clean.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Ok(ip) = clean.parse::<IpAddr>() {
        return ip.is_loopback();
    }
    false
}
