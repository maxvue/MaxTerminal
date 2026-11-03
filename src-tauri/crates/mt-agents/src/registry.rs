//! Registro declarativo dos 4 Agentes CLI (`claude-code`, `codex`, `agy`, `opencode`)
//! e mesclagem segura com `$XDG_CONFIG_HOME/maxterminal/agents.toml`.

use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use mt_core::{AppPaths, MtError, MtResult};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::error;

const CLAUDE_TOML: &str = include_str!("descriptors/claude.toml");
const CODEX_TOML: &str = include_str!("descriptors/codex.toml");
const AGY_TOML: &str = include_str!("descriptors/agy.toml");
const OPENCODE_TOML: &str = include_str!("descriptors/opencode.toml");

/// Identificador canônico dos 4 Agentes CLI suportados pelo MaxTerminal.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type,
)]
pub enum AgentKind {
    /// Claude Code (`claude`).
    #[serde(rename = "claude-code")]
    Claude,
    /// OpenAI Codex (`codex`).
    #[serde(rename = "codex")]
    Codex,
    /// Google Antigravity (`agy`).
    #[serde(rename = "agy")]
    Agy,
    /// OpenCode (`opencode`).
    #[serde(rename = "opencode")]
    OpenCode,
}

impl AgentKind {
    /// Todos os 4 agentes na ordem canônica da interface.
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::Agy, Self::OpenCode];

    /// Retorna o identificador textual canônico (`"claude-code"`, `"codex"`, `"agy"`, `"opencode"`).
    #[must_use]
    pub const fn as_ref_id(&self) -> &'static str {
        match self {
            Self::Claude => "claude-code",
            Self::Codex => "codex",
            Self::Agy => "agy",
            Self::OpenCode => "opencode",
        }
    }

    /// Converte uma string de referência no `AgentKind` correspondente.
    pub fn from_ref_str(s: &str) -> MtResult<Self> {
        match s {
            "claude-code" | "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "agy" => Ok(Self::Agy),
            "opencode" => Ok(Self::OpenCode),
            other => Err(error::unknown_agent(other)),
        }
    }

    /// Retorna o slug curto do agente usado em nomes de branches e worktrees.
    #[must_use]
    pub const fn short(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Agy => "agy",
            Self::OpenCode => "opencode",
        }
    }

    /// Retorna o nome amigável padrão do agente.
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Agy => "Antigravity (agy)",
            Self::OpenCode => "OpenCode",
        }
    }
}

/// Capacidades declarativas estáticas de integração de cada agente CLI.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    /// Suporte a hook estruturado de `statusLine` via JSON (`Claude Code`).
    #[serde(alias = "statusline_hook")]
    pub statusline_hook: bool,
    /// Suporte a hooks nativos de atenção/permissão (`Notification`, `Stop`, `UserPromptSubmit`).
    #[serde(alias = "native_attention_hooks")]
    pub native_attention_hooks: bool,
    /// Emissão de arquivos locais de sessão/transcript JSONL.
    #[serde(alias = "session_files")]
    pub session_files: bool,
    /// Emissão de notificações via sequências OSC 9 / OSC 777 no PTY.
    #[serde(alias = "osc_notifications")]
    pub osc_notifications: bool,
}

/// Descritor completo de um Agente CLI (valores embutidos + overrides opcionais do usuário).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentDescriptor {
    /// Identificador do agente.
    pub kind: AgentKind,
    /// Nome de exibição na interface.
    #[serde(alias = "display_name")]
    pub display_name: String,
    /// Ícone Iconify do agente.
    pub icon: String,
    /// Token semântico de cor (`--max-*`).
    #[serde(alias = "color_token")]
    pub color_token: String,
    /// Nome ou caminho do binário executável.
    pub binary: String,
    /// Nomes de processos associados para correlação de foreground no PTY.
    #[serde(alias = "process_names")]
    pub process_names: Vec<String>,
    /// Argumentos usados para sondar a versão do binário.
    #[serde(alias = "version_args")]
    pub version_args: Vec<String>,
    /// Expressão regular para extrair a versão semântica da saída.
    #[serde(alias = "version_regex")]
    pub version_regex: String,
    /// Argumentos padrão sempre passados ao agente.
    #[serde(alias = "default_args")]
    pub default_args: Vec<String>,
    /// Argumentos extras adicionados pelo usuário.
    #[serde(default, alias = "extra_args")]
    pub extra_args: Vec<String>,
    /// Flag CLI para seleção de modelo (ex.: `"--model"`), quando suportada.
    #[serde(default, alias = "model_flag")]
    pub model_flag: Option<String>,
    /// Variáveis de ambiente adicionais para o processo do agente.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Atalho de teclado padrão para abrir o agente.
    pub shortcut: String,
    /// Capacidades declaradas do agente.
    pub capabilities: AgentCapabilities,
}

/// Estado de suporte de uma funcionalidade na matriz de capacidades do agente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CapabilitySupport {
    /// Funcionalidade disponível e verificada.
    Available,
    /// Funcionalidade não suportada pelo agente ou binário ausente.
    Unsupported,
    /// Funcionalidade sem contrato documentado validado para esta versão.
    Unverified,
    /// Funcionalidade suportada, porém desativada nas configurações do usuário.
    Disabled,
}

/// Evidência e justificativa do estado de suporte de uma funcionalidade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityEvidence {
    /// Nível de suporte.
    pub support: CapabilitySupport,
    /// Fonte técnica da capacidade (ex.: `"statusLine hook"`, `"CLI flag --model"`).
    pub source: Option<String>,
    /// Motivo legível em `pt-BR` quando indisponível, não verificado ou desativado.
    pub reason: Option<String>,
}

/// Relatório consolidado de capacidades verificadas de um agente CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityReport {
    /// Agente avaliado.
    pub agent: AgentKind,
    /// Versão detectada do binário, quando instalado.
    pub binary_version: Option<String>,
    /// Revisão do adaptador interno do MaxTerminal.
    pub adapter_revision: String,
    /// Timestamp UNIX em ms da verificação.
    pub checked_at_ms: i64,
    /// Mapa das 9 chaves canônicas de capacidades.
    pub features: BTreeMap<String, CapabilityEvidence>,
}

/// Override opcional por agente em `$XDG_CONFIG_HOME/maxterminal/agents.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentUserOverride {
    /// Caminho ou nome customizado do binário.
    pub binary: Option<String>,
    /// Argumentos extras adicionados ao lançamento.
    pub extra_args: Option<Vec<String>>,
    /// Variáveis de ambiente mescladas ao descritor.
    pub env: Option<BTreeMap<String, String>>,
    /// Atalho de teclado customizado.
    pub shortcut: Option<String>,
    /// Override opcional da flag de modelo (`Some(None)` desativa, `Some(Some("--model"))` define).
    #[serde(default, deserialize_with = "deserialize_optional_nullable_string")]
    pub model_flag: Option<Option<String>>,
    /// Habilita ou desabilita o agente no lançador.
    pub enabled: Option<bool>,
}

#[allow(clippy::option_option)]
fn deserialize_optional_nullable_string<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val = Option::<String>::deserialize(deserializer)?;
    Ok(Some(val.filter(|s| !s.trim().is_empty())))
}

/// Estrutura raiz de `$XDG_CONFIG_HOME/maxterminal/agents.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserAgentsToml {
    /// Overrides para o Claude Code.
    pub claude: Option<AgentUserOverride>,
    /// Overrides para o Codex.
    pub codex: Option<AgentUserOverride>,
    /// Overrides para o Antigravity (`agy`).
    pub agy: Option<AgentUserOverride>,
    /// Overrides para o OpenCode.
    pub opencode: Option<AgentUserOverride>,
}

/// Registro em memória dos descritores dos 4 Agentes CLI.
pub struct AgentRegistry {
    descriptors: RwLock<Arc<[AgentDescriptor; 4]>>,
    enabled: RwLock<BTreeMap<AgentKind, bool>>,
}

impl AgentRegistry {
    /// Carrega os 4 descritores embutidos e aplica overrides opcionais de `agents.toml`.
    /// Se `agents.toml` for inválido, retorna os descritores embutidos intactos acompanhados do `MtError`.
    #[must_use]
    pub fn load(paths: &AppPaths) -> (Self, Option<MtError>) {
        let builtins = Self::builtin_descriptors();
        let default_enabled = Self::default_enabled_map();

        let registry = Self {
            descriptors: RwLock::new(Arc::new(builtins)),
            enabled: RwLock::new(default_enabled),
        };

        let err = registry.reload(paths).err();
        (registry, err)
    }

    /// Cria um registro apenas com os descritores embutidos (útil para testes).
    #[must_use]
    pub fn with_builtins() -> Self {
        Self {
            descriptors: RwLock::new(Arc::new(Self::builtin_descriptors())),
            enabled: RwLock::new(Self::default_enabled_map()),
        }
    }

    /// Recarrega `$XDG_CONFIG_HOME/maxterminal/agents.toml`.
    /// Em caso de erro de parse ou validação, mantém o estado anterior em memória e retorna `Err`.
    pub fn reload(&self, paths: &AppPaths) -> MtResult<()> {
        let mut descriptors = Self::builtin_descriptors();
        let mut enabled = Self::default_enabled_map();

        let toml_path = paths.config_dir.join("agents.toml");
        if toml_path.exists() {
            let raw = fs::read_to_string(&toml_path).map_err(|e| {
                error::settings_invalid(
                    format!("Falha ao ler {}: {e}", toml_path.display()),
                    None,
                    None,
                )
            })?;

            let user_toml: UserAgentsToml = toml::from_str(&raw).map_err(|e| {
                let (line, col) = e.span().map_or((None, None), |span| {
                    let prefix = &raw[..span.start.min(raw.len())];
                    let line_num = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
                    let last_nl = prefix.rfind('\n').map_or(0, |idx| idx + 1);
                    let col_num = span.start.saturating_sub(last_nl) + 1;
                    (Some(line_num), Some(col_num))
                });
                error::settings_invalid(e.message(), line, col)
            })?;

            Self::apply_override(&mut descriptors[0], &mut enabled, user_toml.claude)?;
            Self::apply_override(&mut descriptors[1], &mut enabled, user_toml.codex)?;
            Self::apply_override(&mut descriptors[2], &mut enabled, user_toml.agy)?;
            Self::apply_override(&mut descriptors[3], &mut enabled, user_toml.opencode)?;
        }

        *self.descriptors.write() = Arc::new(descriptors);
        *self.enabled.write() = enabled;
        Ok(())
    }

    /// Retorna o snapshot atômico dos 4 descritores na ordem canônica.
    #[must_use]
    pub fn descriptors(&self) -> Arc<[AgentDescriptor; 4]> {
        Arc::clone(&self.descriptors.read())
    }

    /// Obtém o descritor de um agente específico.
    #[must_use]
    pub fn get(&self, kind: AgentKind) -> AgentDescriptor {
        let guard = self.descriptors.read();
        match kind {
            AgentKind::Claude => guard[0].clone(),
            AgentKind::Codex => guard[1].clone(),
            AgentKind::Agy => guard[2].clone(),
            AgentKind::OpenCode => guard[3].clone(),
        }
    }

    /// Verifica se o agente está habilitado.
    #[must_use]
    pub fn is_enabled(&self, kind: AgentKind) -> bool {
        self.enabled.read().get(&kind).copied().unwrap_or(true)
    }

    fn builtin_descriptors() -> [AgentDescriptor; 4] {
        let parse = |raw: &str, name: &str| -> AgentDescriptor {
            toml::from_str(raw).unwrap_or_else(|e| {
                panic!("Descritor embutido '{name}' inválido em tempo de compilação: {e}")
            })
        };
        [
            parse(CLAUDE_TOML, "claude.toml"),
            parse(CODEX_TOML, "codex.toml"),
            parse(AGY_TOML, "agy.toml"),
            parse(OPENCODE_TOML, "opencode.toml"),
        ]
    }

    fn default_enabled_map() -> BTreeMap<AgentKind, bool> {
        let mut map = BTreeMap::new();
        for kind in AgentKind::ALL {
            map.insert(kind, true);
        }
        map
    }

    fn apply_override(
        desc: &mut AgentDescriptor,
        enabled_map: &mut BTreeMap<AgentKind, bool>,
        ov: Option<AgentUserOverride>,
    ) -> MtResult<()> {
        let Some(ov) = ov else {
            return Ok(());
        };
        if let Some(binary) = ov.binary {
            let trimmed = binary.trim();
            if trimmed.is_empty() {
                return Err(error::settings_invalid(
                    format!(
                        "O campo 'binary' de '{}' não pode ser vazio.",
                        desc.kind.as_ref_id()
                    ),
                    None,
                    None,
                ));
            }
            desc.binary = trimmed.to_string();
        }
        if let Some(extra_args) = ov.extra_args {
            desc.extra_args = extra_args;
        }
        if let Some(env) = ov.env {
            for (k, v) in env {
                desc.env.insert(k, v);
            }
        }
        if let Some(shortcut) = ov.shortcut {
            let normalized = crate::settings::normalize_and_validate_shortcut(&shortcut)?;
            desc.shortcut = normalized;
        }
        if let Some(model_flag) = ov.model_flag {
            desc.model_flag = model_flag;
        }
        if let Some(en) = ov.enabled {
            enabled_map.insert(desc.kind, en);
        }
        Ok(())
    }
}
