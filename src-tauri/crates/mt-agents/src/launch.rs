//! Planejador de lançamento (`LaunchPlanner`), injeção reversível de configurações (`--settings` / `-c notify`)
//! e serialização segura de linha de comando por shell (`bash`, `zsh`, `fish`).

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mt_core::{AppPaths, MtResult, PaneId, ProjectId, SessionId, TabId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::detect::AgentInstallation;
use crate::error;
use crate::isolation::AgentTabLinkDraft;
use crate::registry::{AgentDescriptor, AgentKind};
use crate::relay::server::RelayToken;
use crate::settings::{AgentsSettings, WorktreeLocationDto};

/// Chave transitória de ambiente usada apenas para correlacionar `open_tab` ao `PendingLaunch`
/// (sempre removida do `SpawnSpec` antes do spawn do PTY).
pub const AGENT_LAUNCH_ENV_KEY: &str = "MAXTERMINAL_AGENT_LAUNCH";

/// Opções específicas de agente associadas a um template de aba (`agent_template_options`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTemplateOptions {
    /// Modelo CLI pré-selecionado para o template, se houver.
    pub model: Option<String>,
    /// Argumentos extras de linha de comando configurados no template.
    pub extra_args: Vec<String>,
    /// Quando `true`, sugere isolamento em Git Worktree por padrão ao usar este template.
    pub isolation_default: bool,
    /// Localização padrão da worktree para este template.
    pub location: WorktreeLocationDto,
}

impl Default for AgentTemplateOptions {
    fn default() -> Self {
        Self {
            model: None,
            extra_args: Vec::new(),
            isolation_default: false,
            location: WorktreeLocationDto::Project,
        }
    }
}

/// Tipo de shell alvo para quoting da linha de comando inicial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    /// GNU Bourne-Again Shell (`bash`).
    Bash,
    /// Z Shell (`zsh`).
    Zsh,
    /// Friendly Interactive Shell (`fish`).
    Fish,
    /// Shell POSIX genérico (`sh`, `dash`, etc.).
    Posix,
}

/// Reserva transitória registrada antes de `WorkspaceManager::open_tab`.
#[derive(Debug, Clone)]
pub struct PendingLaunch {
    /// Identificador da operação de lançamento.
    pub launch_id: Uuid,
    /// Agente solicitado.
    pub agent: AgentKind,
    /// Modelo solicitado na abertura da aba.
    pub model: Option<String>,
    /// Argumentos extras para esta abertura.
    pub extra_args: Vec<String>,
    /// Rascunho de vínculo de worktree, se aplicável.
    pub link: Option<AgentTabLinkDraft>,
}

/// Identidade reservada de uma execução de agente antes do spawn do PTY (`PaneSpawned`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRunBinding {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Identificador do painel.
    pub pane_id: PaneId,
    /// Identificador da sessão SQLite.
    pub session_id: SessionId,
    /// Geração do owner da execução (Fase 07).
    pub generation: i64,
    /// Identificador do projeto.
    pub project_id: ProjectId,
    /// Identificador único desta execução do agente.
    pub run_id: String,
    /// Agente CLI a ser iniciado.
    pub agent: AgentKind,
    /// Diretório de trabalho resolvido.
    pub cwd: PathBuf,
    /// Timestamp UNIX em ms da criação do plano.
    pub started_at_ms: i64,
}

/// Plano completo de lançamento de um Agente CLI dentro do login shell do usuário.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    /// Tipo do agente.
    pub kind: AgentKind,
    /// Identidade reservada da execução.
    pub pending_binding: PendingRunBinding,
    /// Caminho resolvido do executável do agente.
    pub program: PathBuf,
    /// Lista ordenada de argumentos passados ao agente.
    pub args: Vec<String>,
    /// Variáveis de ambiente injetadas no `SpawnSpec` do shell.
    pub env: BTreeMap<String, String>,
    /// Token opaco do relay para esta execução.
    pub token: RelayToken,
    /// Diretório de runtime exclusivo desta execução (`0700`).
    pub runtime_dir: PathBuf,
    /// Modelo efetivamente configurado via flag CLI, quando houver.
    pub configured_model: Option<String>,
}

/// Planejador de lançamento de Agentes CLI.
pub struct LaunchPlanner {
    paths: AppPaths,
    relay_bin: PathBuf,
}

impl LaunchPlanner {
    /// Cria um novo `LaunchPlanner`.
    #[must_use]
    pub fn new(paths: AppPaths, relay_bin: PathBuf) -> Self {
        Self { paths, relay_bin }
    }

    /// Retorna o caminho configurado do binário sidecar `mt-agent-relay`.
    #[must_use]
    pub fn relay_bin(&self) -> &Path {
        &self.relay_bin
    }

    /// Valida e traduz um ID de modelo selecionado para o argumento CLI local do agente.
    /// Retorna `Err(agents.model_unsupported)` se o agente não suportar seleção de modelo por flag
    /// ou se o modelo for incompatível.
    pub fn resolve_cli_model(
        descriptor: &AgentDescriptor,
        requested_model: Option<&str>,
    ) -> MtResult<Option<String>> {
        let Some(raw) = requested_model.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(None);
        };

        if descriptor.model_flag.is_none() {
            return Err(error::model_unsupported(
                descriptor.kind.as_ref_id(),
                raw,
                "Este agente não possui flag documentada de seleção de modelo via CLI.",
            ));
        }

        if raw.len() > 128
            || !raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-'))
        {
            return Err(error::model_unsupported(
                descriptor.kind.as_ref_id(),
                raw,
                "Identificador de modelo contém caracteres inválidos.",
            ));
        }

        let lower = raw.to_ascii_lowercase();
        let stripped = lower
            .strip_prefix("anthropic/")
            .or_else(|| lower.strip_prefix("openai/"))
            .unwrap_or(&lower);

        match descriptor.kind {
            AgentKind::Claude => {
                if stripped.starts_with("gpt-")
                    || stripped.starts_with("o1")
                    || stripped.starts_with("o3")
                    || stripped.starts_with("o4")
                    || stripped.starts_with("codex")
                    || stripped.starts_with("gemini-")
                {
                    return Err(error::model_unsupported(
                        descriptor.kind.as_ref_id(),
                        raw,
                        "Modelo pertence a outro provedor e não é compatível com o Claude Code.",
                    ));
                }
                let normalized = raw
                    .strip_prefix("anthropic/")
                    .or_else(|| raw.strip_prefix("Anthropic/"))
                    .unwrap_or(raw);
                Ok(Some(normalized.to_string()))
            }
            AgentKind::Codex => {
                if stripped.starts_with("claude-")
                    || stripped == "sonnet"
                    || stripped == "opus"
                    || stripped == "haiku"
                    || stripped.starts_with("gemini-")
                {
                    return Err(error::model_unsupported(
                        descriptor.kind.as_ref_id(),
                        raw,
                        "Modelo pertence a outro provedor e não é compatível com o Codex.",
                    ));
                }
                let normalized = raw
                    .strip_prefix("openai/")
                    .or_else(|| raw.strip_prefix("OpenAI/"))
                    .unwrap_or(raw);
                Ok(Some(normalized.to_string()))
            }
            AgentKind::Agy | AgentKind::OpenCode => Err(error::model_unsupported(
                descriptor.kind.as_ref_id(),
                raw,
                "Seleção de modelo via flag CLI não verificada para este agente.",
            )),
        }
    }

    /// Constrói o `LaunchPlan` completo para uma execução de agente.
    #[allow(clippy::too_many_arguments)]
    pub fn build_plan(
        &self,
        descriptor: &AgentDescriptor,
        installation: &AgentInstallation,
        opts: &AgentTemplateOptions,
        pending: Option<&PendingLaunch>,
        binding: PendingRunBinding,
        token: RelayToken,
        relay_sock: &Path,
        settings: &AgentsSettings,
    ) -> MtResult<LaunchPlan> {
        if !installation.installed {
            return Err(error::not_installed(&descriptor.display_name));
        }

        let program = installation
            .path
            .as_deref()
            .map_or_else(|| PathBuf::from(&descriptor.binary), PathBuf::from);

        let requested_model = pending
            .and_then(|p| p.model.as_deref())
            .or(opts.model.as_deref());
        let configured_model = Self::resolve_cli_model(descriptor, requested_model)?;

        let base_runtime = self
            .paths
            .runtime_dir
            .clone()
            .unwrap_or_else(|| self.paths.state_dir.join("run"));
        let run_runtime_dir = base_runtime.join("runs").join(&binding.run_id);
        fs::create_dir_all(&run_runtime_dir).map_err(|e| {
            error::relay_unavailable(format!(
                "Falha ao criar diretório de runtime da execução {}: {e}",
                run_runtime_dir.display()
            ))
        })?;
        let _ = fs::set_permissions(&run_runtime_dir, fs::Permissions::from_mode(0o700));

        let mut user_extra_args: Vec<String> = Vec::new();
        user_extra_args.extend(descriptor.extra_args.iter().cloned());
        user_extra_args.extend(opts.extra_args.iter().cloned());
        if let Some(p) = pending {
            user_extra_args.extend(p.extra_args.iter().cloned());
        }

        let mut args: Vec<String> = descriptor.default_args.clone();
        let inject_enabled = settings
            .inject
            .get(&descriptor.kind)
            .copied()
            .unwrap_or_else(|| matches!(descriptor.kind, AgentKind::Claude | AgentKind::Codex));

        match descriptor.kind {
            AgentKind::Claude => {
                let user_passed_settings = user_extra_args
                    .iter()
                    .any(|a| a == "--settings" || a.starts_with("--settings="));
                if inject_enabled && !user_passed_settings {
                    if let Some(settings_path) =
                        self.prepare_claude_settings_file(&binding.cwd, &run_runtime_dir)?
                    {
                        args.push("--settings".to_string());
                        args.push(settings_path.to_string_lossy().to_string());
                    }
                }
            }
            AgentKind::Codex => {
                let user_passed_notify = user_extra_args
                    .windows(2)
                    .any(|w| w[0] == "-c" && w[1].starts_with("notify="))
                    || user_extra_args.iter().any(|a| a.starts_with("notify="));
                if inject_enabled
                    && !user_passed_notify
                    && let Some(notify_override) = self.prepare_codex_notify_arg()
                {
                    args.push("-c".to_string());
                    args.push(notify_override);
                }
            }
            AgentKind::Agy | AgentKind::OpenCode => {}
        }

        if let (Some(flag), Some(model_val)) = (&descriptor.model_flag, &configured_model) {
            args.push(flag.clone());
            args.push(model_val.clone());
        }

        args.extend(user_extra_args);

        let mut env = descriptor.env.clone();
        env.insert(
            "MAXTERMINAL_AGENT".to_string(),
            descriptor.kind.as_ref_id().to_string(),
        );
        env.insert(
            "MAXTERMINAL_AGENT_TOKEN".to_string(),
            token.as_str().to_string(),
        );
        env.insert(
            "MAXTERMINAL_RELAY_SOCK".to_string(),
            relay_sock.to_string_lossy().to_string(),
        );
        env.insert(
            "MAXTERMINAL_RELAY_BIN".to_string(),
            self.relay_bin.to_string_lossy().to_string(),
        );

        Ok(LaunchPlan {
            kind: descriptor.kind,
            pending_binding: binding,
            program,
            args,
            env,
            token,
            runtime_dir: run_runtime_dir,
            configured_model,
        })
    }

    fn prepare_claude_settings_file(
        &self,
        cwd: &Path,
        run_runtime_dir: &Path,
    ) -> MtResult<Option<PathBuf>> {
        // R-04: verifica política gerenciada ou arquivos existentes com precedência ambígua
        let managed_path = Path::new("/etc/claude-code/managed-settings.json");
        if managed_path.exists() {
            return Ok(None);
        }

        let home_dir = std::env::var_os("MAXCODE_REAL_HOME")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from);

        let mut candidate_files = Vec::new();
        if let Some(home) = home_dir {
            candidate_files.push(home.join(".claude").join("settings.json"));
        }
        candidate_files.push(cwd.join(".claude").join("settings.json"));
        candidate_files.push(cwd.join(".claude").join("settings.local.json"));

        let mut existing_statusline_cmd: Option<String> = None;
        for file_path in candidate_files {
            if !file_path.exists() {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&file_path) else {
                return Ok(None);
            };
            let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
                // Arquivo existente malformado: não injetar para evitar comportamento imprevisível (R-04)
                return Ok(None);
            };
            if let Some(sl) = parsed.get("statusLine") {
                if let Some(cmd) = sl.get("command").and_then(Value::as_str) {
                    if !cmd.trim().is_empty() {
                        existing_statusline_cmd = Some(cmd.to_string());
                    }
                } else if !sl.is_null() {
                    // Formato de statusLine desconhecido; respeita configuração do usuário
                    return Ok(None);
                }
            }
        }

        let relay_quoted = shell_words::quote(&self.relay_bin.to_string_lossy()).into_owned();
        let statusline_cmd = if let Some(orig_cmd) = existing_statusline_cmd {
            let b64 = STANDARD.encode(orig_cmd.as_bytes());
            format!("{relay_quoted} statusline --chain {b64}")
        } else {
            format!("{relay_quoted} statusline")
        };

        let make_hook_entry = |event_name: &str| -> Value {
            json!([
                {
                    "matcher": "",
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("{relay_quoted} hook {event_name}")
                        }
                    ]
                }
            ])
        };

        let settings_json = json!({
            "statusLine": {
                "type": "command",
                "command": statusline_cmd,
                "padding": 0
            },
            "hooks": {
                "Notification": make_hook_entry("Notification"),
                "Stop": make_hook_entry("Stop"),
                "UserPromptSubmit": make_hook_entry("UserPromptSubmit"),
                "PreToolUse": make_hook_entry("PreToolUse")
            }
        });

        let target_file = run_runtime_dir.join("claude-settings.json");
        let serialized = serde_json::to_vec_pretty(&settings_json).map_err(|e| {
            error::relay_unavailable(format!("Falha ao serializar claude-settings.json: {e}"))
        })?;
        fs::write(&target_file, serialized).map_err(|e| {
            error::relay_unavailable(format!("Falha ao gravar {}: {e}", target_file.display()))
        })?;
        let _ = fs::set_permissions(&target_file, fs::Permissions::from_mode(0o600));

        Ok(Some(target_file))
    }

    fn prepare_codex_notify_arg(&self) -> Option<String> {
        let home_dir = std::env::var_os("MAXCODE_REAL_HOME")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from);

        let mut existing_notify: Option<Vec<String>> = None;
        if let Some(home) = home_dir {
            let cfg_path = home.join(".codex").join("config.toml");
            if cfg_path.exists() {
                let raw = fs::read_to_string(&cfg_path).ok()?;
                let parsed: toml::Value = toml::from_str(&raw).ok()?;
                if let Some(notify_val) = parsed.get("notify") {
                    let arr = notify_val.as_array()?;
                    let mut vec_str = Vec::with_capacity(arr.len());
                    for item in arr {
                        vec_str.push(item.as_str()?.to_string());
                    }
                    if !vec_str.is_empty() {
                        existing_notify = Some(vec_str);
                    }
                }
            }
        }

        let relay_str = self.relay_bin.to_string_lossy().to_string();
        let notify_vec = if let Some(orig) = existing_notify {
            let json_orig = serde_json::to_string(&orig).ok()?;
            let b64 = STANDARD.encode(json_orig.as_bytes());
            vec![
                relay_str,
                "codex-notify".to_string(),
                "--chain".to_string(),
                b64,
            ]
        } else {
            vec![relay_str, "codex-notify".to_string()]
        };

        let serialized_arr = serde_json::to_string(&notify_vec).ok()?;
        Some(format!("notify={serialized_arr}"))
    }
}

/// Escapa um argumento individual para o shell `fish` usando aspas simples (onde apenas `\` e `'` são especiais).
#[must_use]
pub fn quote_fish_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '='))
    {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 4);
    out.push('\'');
    for ch in arg.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

/// Escapa um argumento individual para shells POSIX (`bash`, `zsh`, `sh`).
#[must_use]
pub fn quote_posix_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '='))
    {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 4);
    out.push('\'');
    for ch in arg.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Monta a linha de comando inicial para enviar ao login shell após `PromptStart`.
pub fn command_line(
    plan: &LaunchPlan,
    initial_command: Option<&str>,
    shell: ShellKind,
) -> MtResult<String> {
    let prog_str = plan.program.to_string_lossy();
    let quote_fn: fn(&str) -> String = match shell {
        ShellKind::Fish => quote_fish_arg,
        ShellKind::Bash | ShellKind::Zsh | ShellKind::Posix => quote_posix_arg,
    };

    let mut parts = Vec::with_capacity(1 + plan.args.len());
    parts.push(quote_fn(&prog_str));
    for arg in &plan.args {
        parts.push(quote_fn(arg));
    }
    let agent_cmd = parts.join(" ");

    if let Some(init_cmd) = initial_command.map(str::trim).filter(|s| !s.is_empty()) {
        let combined = match shell {
            ShellKind::Fish => format!("begin; {init_cmd}; end; and {agent_cmd}"),
            ShellKind::Bash | ShellKind::Zsh | ShellKind::Posix => {
                format!("{{ {init_cmd}; }} && {agent_cmd}")
            }
        };
        Ok(combined)
    } else {
        Ok(agent_cmd)
    }
}
