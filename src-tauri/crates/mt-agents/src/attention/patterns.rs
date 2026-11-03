//! Compilação de regras regex de duas âncoras e filtragem de linhas citadas/blocos de código.

use std::path::Path;

use mt_core::{AppPaths, MtResult};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::error;
use crate::ports::AttentionKind;
use crate::registry::AgentKind;

const DEFAULT_PATTERNS_TOML: &str = include_str!("../patterns/default.toml");
const REGEX_SIZE_LIMIT: usize = 256 * 1024;

/// Regra declarativa de detecção heurística em TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternRule {
    /// Identificador único da regra (ex.: `"claude.permission"`).
    pub id: String,
    /// Agente alvo da regra.
    pub agent: AgentKind,
    /// Tipo de atenção (`awaitingApproval` / `awaiting_approval` ou `awaitingInput` / `awaiting_input`).
    #[serde(deserialize_with = "deserialize_attention_kind")]
    pub kind: AttentionKind,
    /// Primeira âncora obrigatória (pergunta/prompt).
    pub regex: String,
    /// Segunda âncora obrigatória no mesmo tail (opções de resposta/confirmação).
    pub confirm: String,
    /// Indica se a regra possui fixture verificado e está habilitada por padrão.
    #[serde(default)]
    pub verified: bool,
}

fn deserialize_attention_kind<'de, D>(deserializer: D) -> Result<AttentionKind, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    match raw.as_str() {
        "awaitingApproval" | "awaiting_approval" => Ok(AttentionKind::AwaitingApproval),
        "awaitingInput" | "awaiting_input" => Ok(AttentionKind::AwaitingInput),
        other => Err(serde::de::Error::custom(format!(
            "AttentionKind inválido: {other}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
struct PatternFile {
    #[serde(default)]
    pattern: Vec<PatternRule>,
}

/// Regra compilada pronta para avaliação no `TailBuffer`.
#[derive(Debug, Clone)]
pub struct CompiledPattern {
    /// Identificador da regra.
    pub id: String,
    /// Agente alvo.
    pub agent: AgentKind,
    /// Natureza da espera.
    pub kind: AttentionKind,
    /// Primeira âncora compilada.
    pub primary: Regex,
    /// Segunda âncora compilada.
    pub confirm: Regex,
    /// Se a regra foi verificada por fixture.
    pub verified: bool,
}

/// Conjunto de padrões compilados para avaliação rápida.
#[derive(Debug, Clone, Default)]
pub struct PatternSet {
    patterns: Vec<CompiledPattern>,
    include_unverified: bool,
}

impl PatternSet {
    /// Carrega os padrões embutidos em `patterns/default.toml`.
    pub fn load_default() -> Self {
        Self::from_toml_str(DEFAULT_PATTERNS_TOML).unwrap_or_default()
    }

    /// Carrega os padrões embutidos e aplica override opcional de `$XDG_CONFIG_HOME/maxterminal/agent-patterns.toml`.
    pub fn load(paths: &AppPaths) -> Self {
        let mut base = Self::load_default();
        let custom_path = paths.config_dir.join("agent-patterns.toml");
        if custom_path.is_file() {
            if let Ok(custom) = Self::from_file(&custom_path) {
                for pat in custom.patterns {
                    if let Some(existing) = base.patterns.iter_mut().find(|p| p.id == pat.id) {
                        *existing = pat;
                    } else {
                        base.patterns.push(pat);
                    }
                }
            }
        }
        base
    }

    /// Compila um conjunto de padrões a partir de uma string TOML.
    pub fn from_toml_str(raw: &str) -> MtResult<Self> {
        let parsed: PatternFile = toml::from_str(raw).map_err(|e| {
            error::settings_invalid(format!("TOML de padrões inválido: {e}"), None, None)
        })?;
        let mut patterns = Vec::with_capacity(parsed.pattern.len());
        for rule in parsed.pattern {
            let primary = RegexBuilder::new(&rule.regex)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .map_err(|e| {
                    error::settings_invalid(
                        format!("Regex inválida em {}: {e}", rule.id),
                        None,
                        None,
                    )
                })?;
            let confirm = RegexBuilder::new(&rule.confirm)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .map_err(|e| {
                    error::settings_invalid(
                        format!("Regex confirm inválida em {}: {e}", rule.id),
                        None,
                        None,
                    )
                })?;
            patterns.push(CompiledPattern {
                id: rule.id,
                agent: rule.agent,
                kind: rule.kind,
                primary,
                confirm,
                verified: rule.verified,
            });
        }
        Ok(Self {
            patterns,
            include_unverified: false,
        })
    }

    /// Carrega padrões de um arquivo TOML.
    pub fn from_file(path: &Path) -> MtResult<Self> {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            error::settings_invalid(format!("Falha ao ler {}: {e}", path.display()), None, None)
        })?;
        Self::from_toml_str(&raw)
    }

    /// Define se padrões com `verified = false` também devem ser avaliados.
    pub fn with_unverified(mut self, include: bool) -> Self {
        self.include_unverified = include;
        self
    }

    /// Avalia o tail de texto (sem ANSI) para um agente específico, filtrando linhas citadas e blocos markdown.
    pub fn match_tail(&self, agent: AgentKind, text: &str) -> Option<(String, AttentionKind)> {
        let filtered = filter_quoted_lines(text);
        if filtered.trim().is_empty() {
            return None;
        }
        for pat in &self.patterns {
            if pat.agent != agent {
                continue;
            }
            if !pat.verified && !self.include_unverified {
                continue;
            }
            if pat.primary.is_match(&filtered) && pat.confirm.is_match(&filtered) {
                return Some((pat.id.clone(), pat.kind));
            }
        }
        None
    }

    /// Lista os padrões carregados.
    pub fn patterns(&self) -> &[CompiledPattern] {
        &self.patterns
    }
}

/// Filtra linhas citadas (`>`, `|`, `+`, `-` de diff) e blocos de código markdown (```)
/// para evitar falsos positivos quando logs/código exibem textos como "Allow?" ou "1. Yes".
pub fn filter_quoted_lines(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_fence = false;

    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Ignora citações markdown (`> texto`), tabelas/pipes ASCII (`| col |`) e linhas de diff (`+`, `-`, `@@`)
        if trimmed.starts_with("> ")
            || trimmed == ">"
            || trimmed.starts_with("| ")
            || trimmed.starts_with("+++")
            || trimmed.starts_with("---")
            || trimmed.starts_with("@@")
            || (trimmed.starts_with('+') && !trimmed.starts_with("++"))
            || (trimmed.starts_with('-') && !trimmed.starts_with("--"))
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}
