//! Leitura resiliente e memorizada de `config-trabalho.json` da extensão `MaxCode`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use serde::Deserialize;

use crate::fs_util::read_capped;
use crate::ports::MaxCodeConfigLocator;

const MAX_CONFIG_BYTES: usize = 1024 * 1024;

/// Overrides de `comandoBuild` e `comandoDeploy` resolvidos do `MaxCode` para um projeto.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaxCodeProjectOverrides {
    /// Comando customizado de `/build`, quando definido e não vazio.
    pub build_command: Option<String>,
    /// Comando customizado de `/deploy`, quando definido e não vazio.
    pub deploy_command: Option<String>,
    /// Indica se múltiplos arquivos de configuração divergentes causaram ambiguidade.
    pub ambiguous_conflict: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RawSection {
    #[serde(default, rename = "comandoBuild")]
    comando_build: Option<String>,
    #[serde(default, rename = "comandoDeploy")]
    comando_deploy: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawMaxCodeConfig {
    versao: u32,
    #[serde(default)]
    geral: RawSection,
    #[serde(default)]
    projetos: BTreeMap<String, RawSection>,
}

/// Localizador padrão de `config-trabalho.json` nas pastas de perfil do VS Code / VSCodium / Code - OSS.
#[derive(Debug, Default, Clone)]
pub struct SystemMaxCodeConfigLocator;

impl MaxCodeConfigLocator for SystemMaxCodeConfigLocator {
    fn candidate_files(&self) -> Vec<PathBuf> {
        let base_config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("MAXCODE_REAL_HOME")
                    .or_else(|| std::env::var_os("HOME"))
                    .map(|h| PathBuf::from(h).join(".config"))
            });

        let Some(cfg_dir) = base_config else {
            return Vec::new();
        };

        ["Code", "Code - OSS", "VSCodium"]
            .into_iter()
            .map(|editor| {
                cfg_dir
                    .join(editor)
                    .join("User/globalStorage/johnattas.maxcode/config-trabalho.json")
            })
            .collect()
    }
}

/// Leitor com cache por `(path, mtime_ns)` para `config-trabalho.json`.
pub struct MaxCodeConfigReader {
    locator: Arc<dyn MaxCodeConfigLocator>,
    cache: DashMap<PathBuf, (i64, Option<RawMaxCodeConfig>)>,
}

impl MaxCodeConfigReader {
    /// Cria um novo leitor de configuração do `MaxCode`.
    #[must_use]
    pub fn new(locator: Arc<dyn MaxCodeConfigLocator>) -> Self {
        Self {
            locator,
            cache: DashMap::new(),
        }
    }

    /// Resolve os overrides efetivos (`comandoBuild`, `comandoDeploy`) para `canonical_root`.
    #[must_use]
    pub fn resolve_for_root(&self, canonical_root: &Path) -> MaxCodeProjectOverrides {
        let root_key = canonical_root.display().to_string();
        let mut resolved_candidates: Vec<MaxCodeProjectOverrides> = Vec::new();

        for path in self.locator.candidate_files() {
            if let Some(cfg) = self.read_single(&path) {
                let effective = eval_config(&cfg, &root_key);
                if !resolved_candidates.contains(&effective) {
                    resolved_candidates.push(effective);
                }
            }
        }

        match resolved_candidates.len() {
            0 => MaxCodeProjectOverrides::default(),
            1 => resolved_candidates.remove(0),
            _ => MaxCodeProjectOverrides {
                build_command: None,
                deploy_command: None,
                ambiguous_conflict: true,
            },
        }
    }

    fn read_single(&self, path: &Path) -> Option<RawMaxCodeConfig> {
        let meta = fs::symlink_metadata(path).ok()?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return None;
        }
        let mtime_ns = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .and_then(|d| i64::try_from(d.as_nanos()).ok())
            .unwrap_or(0);

        if let Some(cached) = self.cache.get(path) {
            if cached.0 == mtime_ns {
                return cached.1.clone();
            }
        }

        let parsed = read_capped(path, MAX_CONFIG_BYTES)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str::<RawMaxCodeConfig>(&raw).ok())
            .filter(|c| c.versao == 1);

        self.cache
            .insert(path.to_path_buf(), (mtime_ns, parsed.clone()));
        parsed
    }
}

fn eval_config(cfg: &RawMaxCodeConfig, root_key: &str) -> MaxCodeProjectOverrides {
    let proj = cfg.projetos.get(root_key);

    let build_command = merge_field(
        cfg.geral.comando_build.as_deref(),
        proj.and_then(|p| p.comando_build.as_deref()),
    );
    let deploy_command = merge_field(
        cfg.geral.comando_deploy.as_deref(),
        proj.and_then(|p| p.comando_deploy.as_deref()),
    );

    MaxCodeProjectOverrides {
        build_command,
        deploy_command,
        ambiguous_conflict: false,
    }
}

fn merge_field(general: Option<&str>, project: Option<&str>) -> Option<String> {
    match project {
        Some(p) => {
            let trimmed = p.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        None => general
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(String::from),
    }
}
