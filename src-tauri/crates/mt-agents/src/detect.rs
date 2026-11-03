//! Detecção paralela de instalação dos Agentes CLI no login shell e construção do `CapabilityReport`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::future::join_all;
use parking_lot::RwLock;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tokio::sync::OnceCell;

use crate::registry::{
    AgentDescriptor, AgentKind, AgentRegistry, CapabilityEvidence, CapabilityReport,
    CapabilitySupport,
};
use crate::settings::AgentsSettings;

/// Revisão atual dos adaptadores de agentes CLI do MaxTerminal.
pub const ADAPTER_REVISION: &str = "phase13-v1";

/// Estado de instalação detectado para um Agente CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallation {
    /// Identificador do agente.
    pub kind: AgentKind,
    /// Indica se o binário foi encontrado e respondeu com sucesso a `--version`.
    pub installed: bool,
    /// Caminho absoluto resolvido do binário.
    pub path: Option<String>,
    /// Versão semântica extraída da saída de `--version`.
    pub version: Option<String>,
    /// Motivo em `pt-BR` quando `installed == false` ou observação adicional.
    pub reason: Option<String>,
    /// Timestamp UNIX em ms em que a verificação ocorreu.
    pub checked_at_ms: i64,
}

/// Visão completa de um Agente CLI combinando descritor, instalação, status e capacidades.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// Descritor do agente.
    pub descriptor: AgentDescriptor,
    /// Resultado da detecção de instalação.
    pub installation: AgentInstallation,
    /// Indica se o agente está habilitado.
    pub enabled: bool,
    /// Relatório de capacidades verificadas para esta versão e configuração.
    pub capabilities: CapabilityReport,
}

/// Detector assíncrono de instalação e capacidades dos 4 Agentes CLI.
pub struct Detector {
    login_path: OnceCell<Vec<PathBuf>>,
    cache: RwLock<HashMap<AgentKind, AgentInstallation>>,
    path_override: Option<Vec<PathBuf>>,
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}

impl Detector {
    /// Timeout individual para execução de `<binary> --version`.
    pub const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
    /// Timeout para captura do `$PATH` no login shell.
    pub const SHELL_PATH_TIMEOUT: Duration = Duration::from_secs(2);
    /// Diretórios adicionais sondados além do `$PATH` do sistema.
    pub const EXTRA_DIRS: [&'static str; 4] = [
        "~/.local/bin",
        "~/.bun/bin",
        "~/.npm-global/bin",
        "~/.opencode/bin",
    ];

    /// Cria uma nova instância do detector.
    #[must_use]
    pub fn new() -> Self {
        Self {
            login_path: OnceCell::new(),
            cache: RwLock::new(HashMap::new()),
            path_override: None,
        }
    }

    /// Cria um detector com lista de diretórios `PATH` explícita (ideal para testes determinísticos).
    #[must_use]
    pub fn with_search_paths(paths: Vec<PathBuf>) -> Self {
        Self {
            login_path: OnceCell::new(),
            cache: RwLock::new(HashMap::new()),
            path_override: Some(paths),
        }
    }

    /// Resolve e faz cache dos diretórios do `PATH` do login shell combinados com `EXTRA_DIRS`.
    pub async fn login_path(&self) -> &[PathBuf] {
        if let Some(ref ov) = self.path_override {
            return ov.as_slice();
        }
        self.login_path
            .get_or_init(Self::probe_login_shell_path)
            .await
            .as_slice()
    }

    /// Executa a detecção dos 4 agentes em paralelo com timeout individual de 3 s por agente.
    pub async fn detect_all(&self, reg: &AgentRegistry) -> Vec<AgentInstallation> {
        let search_paths = self.login_path().await.to_vec();
        let descriptors = reg.descriptors();

        let futures = descriptors.iter().map(|desc| {
            let desc_clone = desc.clone();
            let paths_clone = search_paths.clone();
            async move { Self::detect_one(&desc_clone, &paths_clone).await }
        });

        let results = join_all(futures).await;
        {
            let mut guard = self.cache.write();
            for inst in &results {
                guard.insert(inst.kind, inst.clone());
            }
        }
        results
    }

    /// Obtém a última instalação em cache para o agente informado.
    #[must_use]
    pub fn cached(&self, kind: AgentKind) -> Option<AgentInstallation> {
        self.cache.read().get(&kind).cloned()
    }

    /// Constrói o `CapabilityReport` com as 9 chaves canônicas (`spec.md` §4 e §7.4).
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn build_capability_report(
        descriptor: &AgentDescriptor,
        installation: &AgentInstallation,
        settings: &AgentsSettings,
    ) -> CapabilityReport {
        let mut features = BTreeMap::new();
        let inject_enabled = settings
            .inject
            .get(&descriptor.kind)
            .copied()
            .unwrap_or_else(|| matches!(descriptor.kind, AgentKind::Claude | AgentKind::Codex));
        let telemetry_enabled = settings.telemetry.enabled;
        let attention_enabled = settings.attention.enabled;

        // 1. launch
        let launch_ev = if installation.installed {
            CapabilityEvidence {
                support: CapabilitySupport::Available,
                source: installation.path.clone(),
                reason: None,
            }
        } else {
            CapabilityEvidence {
                support: CapabilitySupport::Unsupported,
                source: None,
                reason: Some(
                    installation
                        .reason
                        .clone()
                        .unwrap_or_else(|| "Binário não instalado".to_string()),
                ),
            }
        };
        features.insert("launch".to_string(), launch_ev);

        // 2. modelSelection
        let model_sel_ev = match descriptor.kind {
            AgentKind::Claude | AgentKind::Codex => {
                if descriptor.model_flag.is_some() {
                    CapabilityEvidence {
                        support: CapabilitySupport::Available,
                        source: descriptor.model_flag.clone(),
                        reason: None,
                    }
                } else {
                    CapabilityEvidence {
                        support: CapabilitySupport::Disabled,
                        source: None,
                        reason: Some("Flag de modelo desabilitada por override.".to_string()),
                    }
                }
            }
            AgentKind::Agy | AgentKind::OpenCode => CapabilityEvidence {
                support: CapabilitySupport::Unverified,
                source: None,
                reason: Some(
                    "Sem contrato documentado validado para esta versão do CLI".to_string(),
                ),
            },
        };
        features.insert("modelSelection".to_string(), model_sel_ev);

        // 3. modelTelemetry, 4. contextTelemetry, 5. quota5h, 6. quotaWeekly
        match descriptor.kind {
            AgentKind::Claude => {
                let telem_ev = if !telemetry_enabled {
                    CapabilityEvidence {
                        support: CapabilitySupport::Disabled,
                        source: Some("statusLine hook".to_string()),
                        reason: Some("Telemetria desativada nas configurações.".to_string()),
                    }
                } else if !inject_enabled {
                    CapabilityEvidence {
                        support: CapabilitySupport::Disabled,
                        source: Some("statusLine hook".to_string()),
                        reason: Some(
                            "Injeção de configurações desativada para o Claude Code.".to_string(),
                        ),
                    }
                } else {
                    CapabilityEvidence {
                        support: CapabilitySupport::Available,
                        source: Some("statusLine hook".to_string()),
                        reason: None,
                    }
                };
                features.insert("modelTelemetry".to_string(), telem_ev.clone());
                features.insert("contextTelemetry".to_string(), telem_ev.clone());
                features.insert("quota5h".to_string(), telem_ev.clone());
                features.insert("quotaWeekly".to_string(), telem_ev);
            }
            AgentKind::Codex => {
                let model_telem = if !telemetry_enabled {
                    CapabilityEvidence {
                        support: CapabilitySupport::Disabled,
                        source: Some("configured model / rollout session".to_string()),
                        reason: Some("Telemetria desativada nas configurações.".to_string()),
                    }
                } else {
                    CapabilityEvidence {
                        support: CapabilitySupport::Available,
                        source: Some("configured model / rollout session".to_string()),
                        reason: None,
                    }
                };
                let rollout_ev = if !telemetry_enabled || !settings.telemetry.read_session_files {
                    CapabilityEvidence {
                        support: CapabilitySupport::Disabled,
                        source: Some("codex rollout jsonl".to_string()),
                        reason: Some(
                            "Leitura de arquivos de sessão desativada nas configurações."
                                .to_string(),
                        ),
                    }
                } else {
                    CapabilityEvidence {
                        support: CapabilitySupport::Unverified,
                        source: Some("codex rollout jsonl".to_string()),
                        reason: Some(
                            "Aguardando sessão correlacionada por thread-id para validar schema."
                                .to_string(),
                        ),
                    }
                };
                features.insert("modelTelemetry".to_string(), model_telem);
                features.insert("contextTelemetry".to_string(), rollout_ev.clone());
                features.insert("quota5h".to_string(), rollout_ev.clone());
                features.insert("quotaWeekly".to_string(), rollout_ev);
            }
            AgentKind::Agy | AgentKind::OpenCode => {
                let unv = CapabilityEvidence {
                    support: CapabilitySupport::Unverified,
                    source: None,
                    reason: Some(
                        "Sem contrato documentado validado para esta versão do CLI".to_string(),
                    ),
                };
                features.insert("modelTelemetry".to_string(), unv.clone());
                features.insert("contextTelemetry".to_string(), unv.clone());
                features.insert("quota5h".to_string(), unv.clone());
                features.insert("quotaWeekly".to_string(), unv);
            }
        }

        // 7. attention
        let attention_ev = match descriptor.kind {
            AgentKind::Claude => {
                if !attention_enabled {
                    CapabilityEvidence {
                        support: CapabilitySupport::Disabled,
                        source: Some("Notification hook".to_string()),
                        reason: Some(
                            "Detector de atenção desativado nas configurações.".to_string(),
                        ),
                    }
                } else if !inject_enabled {
                    CapabilityEvidence {
                        support: CapabilitySupport::Disabled,
                        source: Some("Notification hook".to_string()),
                        reason: Some("Injeção de hooks desativada para o Claude Code.".to_string()),
                    }
                } else {
                    CapabilityEvidence {
                        support: CapabilitySupport::Available,
                        source: Some("Notification hook".to_string()),
                        reason: None,
                    }
                }
            }
            AgentKind::Codex | AgentKind::Agy | AgentKind::OpenCode => CapabilityEvidence {
                support: if attention_enabled {
                    CapabilitySupport::Unverified
                } else {
                    CapabilitySupport::Disabled
                },
                source: Some("OSC / padrões heurísticos".to_string()),
                reason: Some(
                    "Sem hook nativo de bloqueio por permissão; apenas indício heurístico."
                        .to_string(),
                ),
            },
        };
        features.insert("attention".to_string(), attention_ev);

        // 8. skillsProject e 9. skillsGlobal
        match descriptor.kind {
            AgentKind::Claude => {
                features.insert(
                    "skillsProject".to_string(),
                    CapabilityEvidence {
                        support: CapabilitySupport::Available,
                        source: Some(".claude/skills".to_string()),
                        reason: None,
                    },
                );
                features.insert(
                    "skillsGlobal".to_string(),
                    CapabilityEvidence {
                        support: CapabilitySupport::Available,
                        source: Some("~/.claude/skills".to_string()),
                        reason: None,
                    },
                );
            }
            AgentKind::Codex => {
                features.insert(
                    "skillsProject".to_string(),
                    CapabilityEvidence {
                        support: CapabilitySupport::Available,
                        source: Some(".agents/skills".to_string()),
                        reason: None,
                    },
                );
                features.insert(
                    "skillsGlobal".to_string(),
                    CapabilityEvidence {
                        support: CapabilitySupport::Available,
                        source: Some("~/.agents/skills".to_string()),
                        reason: None,
                    },
                );
            }
            AgentKind::OpenCode => {
                features.insert(
                    "skillsProject".to_string(),
                    CapabilityEvidence {
                        support: CapabilitySupport::Available,
                        source: Some(".opencode/skills".to_string()),
                        reason: None,
                    },
                );
                features.insert(
                    "skillsGlobal".to_string(),
                    CapabilityEvidence {
                        support: CapabilitySupport::Available,
                        source: Some("$XDG_CONFIG_HOME/opencode/skills".to_string()),
                        reason: None,
                    },
                );
            }
            AgentKind::Agy => {
                let agy_skill_ev = CapabilityEvidence {
                    support: CapabilitySupport::Unverified,
                    source: None,
                    reason: Some(
                        "Sem diretório de skills documentado; requer destOverride explícito."
                            .to_string(),
                    ),
                };
                features.insert("skillsProject".to_string(), agy_skill_ev.clone());
                features.insert("skillsGlobal".to_string(), agy_skill_ev);
            }
        }

        CapabilityReport {
            agent: descriptor.kind,
            binary_version: installation.version.clone(),
            adapter_revision: ADAPTER_REVISION.to_string(),
            checked_at_ms: installation.checked_at_ms,
            features,
        }
    }

    /// Lista os `AgentInfo` completos dos 4 agentes na ordem canônica.
    #[must_use]
    pub fn list_agent_info(
        &self,
        reg: &AgentRegistry,
        settings: &AgentsSettings,
    ) -> Vec<AgentInfo> {
        let descriptors = reg.descriptors();
        let now_ms = current_unix_ms();

        descriptors
            .iter()
            .map(|desc| {
                let installation = self.cached(desc.kind).unwrap_or_else(|| AgentInstallation {
                    kind: desc.kind,
                    installed: false,
                    path: None,
                    version: None,
                    reason: Some("Verificação pendente".to_string()),
                    checked_at_ms: now_ms,
                });
                let capabilities = Self::build_capability_report(desc, &installation, settings);
                AgentInfo {
                    descriptor: desc.clone(),
                    installation,
                    enabled: reg.is_enabled(desc.kind),
                    capabilities,
                }
            })
            .collect()
    }

    /// Garante a detecção inicial quando o cache está vazio e retorna os 4 `AgentInfo`.
    pub async fn list(&self, reg: &AgentRegistry, settings: &AgentsSettings) -> Vec<AgentInfo> {
        let is_empty = self.cache.read().is_empty();
        if is_empty {
            let _ = self.detect_all(reg).await;
        }
        self.list_agent_info(reg, settings)
    }

    /// Reexecuta a detecção de todos os agentes ignorando o cache e retorna os 4 `AgentInfo` atualizados.
    pub async fn refresh(&self, reg: &AgentRegistry, settings: &AgentsSettings) -> Vec<AgentInfo> {
        let _ = self.detect_all(reg).await;
        self.list_agent_info(reg, settings)
    }

    async fn probe_login_shell_path() -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        let mut push_unique = |p: PathBuf| {
            if !p.as_os_str().is_empty() && seen.insert(p.clone()) {
                dirs.push(p);
            }
        };

        let shell_bin = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string());

        let mut cmd = Command::new(&shell_bin);
        cmd.args(["-lic", "printf %s \"$PATH\""])
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .env("MAXTERMINAL_PATH_PROBE", "1")
            .kill_on_drop(true);

        if let Ok(Ok(output)) = tokio::time::timeout(Self::SHELL_PATH_TIMEOUT, cmd.output()).await
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for entry in std::env::split_paths(text.trim()) {
                push_unique(entry);
            }
        }

        if let Some(env_path) = std::env::var_os("PATH") {
            for entry in std::env::split_paths(&env_path) {
                push_unique(entry);
            }
        }

        let home_dir = std::env::var_os("MAXCODE_REAL_HOME")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from);

        if let Some(home) = home_dir {
            for extra in Self::EXTRA_DIRS {
                if let Some(rel) = extra.strip_prefix("~/") {
                    push_unique(home.join(rel));
                }
            }
        }

        dirs
    }

    async fn detect_one(desc: &AgentDescriptor, search_paths: &[PathBuf]) -> AgentInstallation {
        let checked_at_ms = current_unix_ms();
        let resolved = resolve_binary_in_paths(&desc.binary, search_paths);

        let Some(bin_path) = resolved else {
            return AgentInstallation {
                kind: desc.kind,
                installed: false,
                path: None,
                version: None,
                reason: Some("Binário não encontrado no PATH".to_string()),
                checked_at_ms,
            };
        };

        let path_str = bin_path.to_string_lossy().to_string();
        let mut cmd = Command::new(&bin_path);
        cmd.args(&desc.version_args)
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        match tokio::time::timeout(Self::VERSION_TIMEOUT, cmd.output()).await {
            Err(_) => AgentInstallation {
                kind: desc.kind,
                installed: false,
                path: Some(path_str),
                version: None,
                reason: Some("Tempo esgotado ao executar --version".to_string()),
                checked_at_ms,
            },
            Ok(Err(io_err)) => AgentInstallation {
                kind: desc.kind,
                installed: false,
                path: Some(path_str),
                version: None,
                reason: Some(format!("Falha ao executar --version: {io_err}")),
                checked_at_ms,
            },
            Ok(Ok(output)) => {
                if !output.status.success() {
                    let code = output.status.code().unwrap_or(-1);
                    return AgentInstallation {
                        kind: desc.kind,
                        installed: false,
                        path: Some(path_str),
                        version: None,
                        reason: Some(format!("Falha ao executar --version (código {code})")),
                        checked_at_ms,
                    };
                }

                let combined = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                let version = Regex::new(&desc.version_regex)
                    .ok()
                    .and_then(|re| re.captures(&combined))
                    .and_then(|caps| caps.get(1).or_else(|| caps.get(0)))
                    .map(|m| m.as_str().to_string());

                AgentInstallation {
                    kind: desc.kind,
                    installed: true,
                    path: Some(path_str),
                    version,
                    reason: None,
                    checked_at_ms,
                }
            }
        }
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Resolve um binário por caminho explícito (quando contém `/`) ou buscando em `search_paths`.
#[must_use]
pub fn resolve_binary_in_paths(binary: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    let trimmed = binary.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains('/') {
        let p = PathBuf::from(trimmed);
        return is_executable_file(&p).then_some(p);
    }
    for dir in search_paths {
        let candidate = dir.join(trimmed);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Retorna o timestamp UNIX atual em milissegundos.
#[must_use]
pub fn current_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}
