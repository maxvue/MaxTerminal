//! Modelos e validações puras de Templates de Abas e configurações de lançamento (Fase 07).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{MtError, MtResult, codes};
use crate::ids::{ProjectId, TemplateId};

/// Comprimento máximo em caracteres do nome do template.
pub const MAX_TEMPLATE_NAME_CHARS: usize = 64;
/// Comprimento máximo em caracteres do ícone Iconify do template.
pub const MAX_TEMPLATE_ICON_CHARS: usize = 64;
/// Tamanho máximo em bytes UTF-8 do comando inicial.
pub const MAX_INITIAL_COMMAND_BYTES: usize = 4096;
/// Número máximo de variáveis de ambiente por template/override.
pub const MAX_ENV_VARS: usize = 64;
/// Comprimento máximo da chave/source de variável de ambiente.
pub const MAX_ENV_KEY_LEN: usize = 128;
/// Tamanho máximo em bytes UTF-8 do valor individual de uma variável de ambiente (8 KiB).
pub const MAX_ENV_VALUE_BYTES: usize = 8 * 1024;
/// Tamanho máximo acumulado em bytes UTF-8 do mapa de variáveis de ambiente (64 KiB).
pub const MAX_ENV_TOTAL_BYTES: usize = 64 * 1024;

/// Tipo de ambiente de execução do template (`shell` ou `agent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentKind {
    /// Shell interativo padrão do sistema/usuário.
    #[default]
    Shell,
    /// Agente CLI dedicado (ex.: `claude`, `codex`, `agy`, `opencode`).
    Agent,
}

impl EnvironmentKind {
    /// Representação textual persistida em `tab_templates.environment_kind`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Agent => "agent",
        }
    }

    /// Reconstrói [`EnvironmentKind`] a partir da string persistida no SQLite.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "shell" => Some(Self::Shell),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }
}

/// Configuração de ambiente de execução do template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateEnvironment {
    /// Família de ambiente (`shell` ou `agent`).
    pub kind: EnvironmentKind,
    /// Identificador do agente CLI quando `kind == Agent` (`None` para `Shell`).
    pub agent_id: Option<String>,
}

impl Default for TemplateEnvironment {
    fn default() -> Self {
        Self {
            kind: EnvironmentKind::Shell,
            agent_id: None,
        }
    }
}

/// Valor de variável de ambiente configurada no template ou override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnvValue {
    /// Valor literal não-sensível definido diretamente no template.
    #[serde(rename_all = "camelCase")]
    Literal {
        /// Conteúdo literal da variável.
        value: String,
    },
    /// Referência ao nome de uma variável herdada do ambiente permitido do processo.
    #[serde(rename_all = "camelCase")]
    Inherited {
        /// Nome da variável de origem no ambiente do processo.
        source: String,
    },
}

/// Configuração persistida e compartilhada de um Template de Aba.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TemplateConfig {
    /// Nome legível do template (`1..=64` caracteres).
    pub name: String,
    /// Ícone Iconify (`prefixo:nome`, até 64 caracteres).
    pub icon: String,
    /// Pasta personalizada opcional (relativa ao projeto, `~` ou absoluta).
    pub cwd: Option<String>,
    /// Comando inicial opcional executado automaticamente no painel primário.
    pub initial_command: Option<String>,
    /// Se `true`, abre automaticamente uma aba de workspace ao iniciar o `MaxTerminal`.
    pub add_to_workspace: bool,
    /// Se `true`, classifica comandos iniciados nesta aba como execução contínua.
    pub continuous: bool,
    /// Tipo de ambiente (`shell` ou `agent`).
    pub environment: TemplateEnvironment,
    /// Variáveis de ambiente adicionais.
    pub env_vars: BTreeMap<String, EnvValue>,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            name: String::from("Terminal"),
            icon: String::from("mdi:console"),
            cwd: None,
            initial_command: None,
            add_to_workspace: false,
            continuous: false,
            environment: TemplateEnvironment::default(),
            env_vars: BTreeMap::new(),
        }
    }
}

/// Payload de entrada para criação ou atualização de um Template de Aba.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TabTemplateInput {
    /// Nome do template (`1..=64` caracteres após trim).
    pub name: String,
    /// Ícone Iconify (`prefixo:nome`).
    pub icon: String,
    /// Pasta personalizada opcional.
    pub cwd: Option<String>,
    /// Comando inicial opcional (`<= 4096` bytes UTF-8).
    pub initial_command: Option<String>,
    /// Se marcado, abre automaticamente ao iniciar o `MaxTerminal`.
    pub add_to_workspace: bool,
    /// Se marcado, classifica como execução contínua.
    pub continuous: bool,
    /// Ambiente (`shell` ou `agent`).
    pub environment: TemplateEnvironment,
    /// Mapa de variáveis de ambiente (`<= 64` entradas).
    pub env_vars: BTreeMap<String, EnvValue>,
    /// Se `true`, promove este template a padrão do projeto na mesma transação.
    pub make_default: bool,
}

/// Representação completa de um Template de Aba persistido.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TabTemplate {
    /// Identificador único do template (UUID v7).
    pub id: TemplateId,
    /// Identificador do projeto proprietário.
    pub project_id: ProjectId,
    /// Configuração funcional do template.
    pub config: TemplateConfig,
    /// Indica se é o template padrão do projeto.
    pub is_default: bool,
    /// Posição de ordenação (`0..N-1`).
    pub position: i64,
    /// Revisão monotônica otimista (`>= 1`).
    pub revision: i64,
    /// Timestamp UNIX em ms de criação.
    pub created_at: i64,
    /// Timestamp UNIX em ms da última atualização.
    pub updated_at: i64,
}

/// Política de substituição do comando inicial ao abrir uma aba com overrides.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandOverride {
    /// Mantém o comando inicial do template.
    #[default]
    Inherit,
    /// Suprime qualquer comando inicial do template.
    Clear,
    /// Substitui pelo comando informado.
    #[serde(rename_all = "camelCase")]
    Replace {
        /// Novo comando inicial a executar.
        command: String,
    },
}

/// Sobrescritas pontuais ao abrir uma aba (`tab_open`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TabOpenOverrides {
    /// Título personalizado opcional para a aba.
    pub title: Option<String>,
    /// Diretório de trabalho personalizado opcional.
    pub cwd: Option<String>,
    /// Política de override para o comando inicial.
    pub initial_command: CommandOverride,
    /// Variáveis de ambiente adicionais mescladas sobre as do template.
    pub extra_env: BTreeMap<String, EnvValue>,
    /// Sobrescrita opcional da flag de execução contínua.
    pub continuous: Option<bool>,
    /// Se `true`, faz fallback para a pasta raiz do projeto caso o `cwd` configurado não exista.
    pub cwd_fallback_to_project: bool,
}

/// Pedido tipado para abertura de uma nova aba no projeto (`tab_open`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TabOpenRequest {
    /// Identificador de idempotência da intenção (UUID v7 textual).
    pub request_id: String,
    /// Projeto onde a aba será aberta.
    pub project_id: ProjectId,
    /// Template base opcional (`None` seleciona o template padrão do projeto na transação).
    pub template_id: Option<TemplateId>,
    /// Overrides opcionais para este lançamento.
    pub overrides: Option<TabOpenOverrides>,
    /// Indica se a aba deve receber foco ao abrir.
    pub focus: bool,
}

/// Snapshot imutável e sanitizado da configuração de lançamento salva na aba (`tabs.launch_json`).
///
/// Nunca contém valores secretos resolvidos de `Inherited`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchSnapshot {
    /// Template de origem no momento da abertura, se houver.
    pub template_id: Option<TemplateId>,
    /// Título validado da aba.
    pub title: String,
    /// Ícone validado da aba.
    pub icon: String,
    /// Diretório de trabalho resolvido e validado no momento da abertura.
    pub resolved_cwd: String,
    /// Expressão original do `cwd` configurado (se houver).
    pub configured_cwd: Option<String>,
    /// Comando inicial validado (se houver).
    pub initial_command: Option<String>,
    /// Flag de execução contínua efetiva.
    pub continuous: bool,
    /// Ambiente de execução (`shell` ou `agent`).
    pub environment: TemplateEnvironment,
    /// Variáveis de ambiente declaradas (sem expandir `Inherited`).
    pub env_vars: BTreeMap<String, EnvValue>,
}

/// Verifica se um identificador de ícone Iconify é válido (`prefixo:nome`, até 64 chars).
#[must_use]
pub fn is_valid_iconify_id(icon: &str) -> bool {
    let trimmed = icon.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_TEMPLATE_ICON_CHARS {
        return false;
    }
    let Some((prefix, name)) = trimmed.split_once(':') else {
        return false;
    };
    if prefix.is_empty() || name.is_empty() || name.contains(':') {
        return false;
    }
    let valid_part = |s: &str| {
        s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && s.chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    };
    valid_part(prefix) && valid_part(name)
}

/// Verifica se o identificador de agente CLI obedece `^[a-z0-9][a-z0-9._-]{0,63}$`.
#[must_use]
pub fn is_valid_agent_slug(slug: &str) -> bool {
    if slug.is_empty() || slug.len() > 64 {
        return false;
    }
    let mut chars = slug.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

/// Verifica se o nome de uma variável de ambiente obedece `^[A-Za-z_][A-Za-z0-9_]*$`.
#[must_use]
pub fn is_valid_env_ident(ident: &str) -> bool {
    if ident.is_empty() || ident.len() > MAX_ENV_KEY_LEN {
        return false;
    }
    let mut chars = ident.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Retorna `true` se a chave de variável de ambiente usar prefixos reservados internos (`MAXTERMINAL_` ou `MT_`).
#[must_use]
pub fn is_forbidden_env_prefix(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.starts_with("MAXTERMINAL_") || upper.starts_with("MT_")
}

/// Retorna `true` se o nome da variável de ambiente indicar credencial/segredo que não pode ter valor `Literal`.
#[must_use]
pub fn is_likely_secret_env_key(key: &str) -> bool {
    const SECRET_TOKENS: &[&str] = &[
        "PASSWORD",
        "PASSWD",
        "SECRET",
        "PRIVATE_KEY",
        "ACCESS_KEY",
        "API_KEY",
        "APIKEY",
        "APP_KEY",
        "AUTH_TOKEN",
        "BEARER",
        "CLIENT_SECRET",
        "COOKIE",
        "CREDENTIALS",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GEMINI_API_KEY",
        "JWT_SECRET",
        "SESSION_TOKEN",
        "REFRESH_TOKEN",
    ];
    let upper = key.to_ascii_uppercase();
    if SECRET_TOKENS.iter().any(|tok| upper.contains(tok)) {
        return true;
    }
    upper.ends_with("_TOKEN") || upper.ends_with("_KEY") || upper == "TOKEN"
}

/// Valida texto de comando inicial quanto a tamanho (`<= 4096` bytes) e ausência de NUL.
pub fn validate_command_text(cmd: &str) -> MtResult<Option<String>> {
    if cmd.contains('\0') {
        return Err(MtError::new(
            codes::TEMPLATE_INVALID,
            "O comando inicial não pode conter bytes nulos.",
        ));
    }
    if cmd.len() > MAX_INITIAL_COMMAND_BYTES {
        return Err(MtError::new(
            codes::TEMPLATE_INVALID,
            "O comando inicial excede o limite máximo de 4096 bytes.",
        ));
    }
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed.to_string()))
}

/// Valida um mapa de variáveis de ambiente quanto a chaves, tamanhos e prefixos reservados.
pub fn validate_env_map(env_vars: &BTreeMap<String, EnvValue>) -> MtResult<()> {
    if env_vars.len() > MAX_ENV_VARS {
        return Err(MtError::new(
            codes::TEMPLATE_INVALID,
            format!("O template suporta no máximo {MAX_ENV_VARS} variáveis de ambiente."),
        ));
    }

    let mut total_bytes = 0usize;
    for (key, val) in env_vars {
        if !is_valid_env_ident(key) {
            return Err(MtError::new(
                codes::TEMPLATE_INVALID,
                format!("Nome de variável de ambiente inválido: '{key}'."),
            ));
        }
        if is_forbidden_env_prefix(key) {
            return Err(MtError::new(
                codes::TEMPLATE_INVALID,
                format!(
                    "O prefixo da variável '{key}' é reservado para uso interno do MaxTerminal."
                ),
            ));
        }

        total_bytes = total_bytes.saturating_add(key.len());
        match val {
            EnvValue::Literal { value } => {
                if value.contains('\0') {
                    return Err(MtError::new(
                        codes::TEMPLATE_INVALID,
                        format!("O valor da variável '{key}' não pode conter bytes nulos."),
                    ));
                }
                if value.len() > MAX_ENV_VALUE_BYTES {
                    return Err(MtError::new(
                        codes::TEMPLATE_INVALID,
                        format!("O valor da variável '{key}' excede o limite de 8 KiB."),
                    ));
                }
                if is_likely_secret_env_key(key) && !value.trim().is_empty() {
                    return Err(MtError::new(
                        codes::TEMPLATE_SECRET_LITERAL,
                        format!(
                            "A variável sensível '{key}' não pode ser salva com valor literal; use variável herdada (Inherited)."
                        ),
                    ));
                }
                total_bytes = total_bytes.saturating_add(value.len());
            }
            EnvValue::Inherited { source } => {
                if !is_valid_env_ident(source) {
                    return Err(MtError::new(
                        codes::TEMPLATE_INVALID,
                        format!(
                            "A variável de origem '{source}' para '{key}' possui identificador inválido."
                        ),
                    ));
                }
                total_bytes = total_bytes.saturating_add(source.len());
            }
        }

        if total_bytes > MAX_ENV_TOTAL_BYTES {
            return Err(MtError::new(
                codes::TEMPLATE_INVALID,
                "O conjunto de variáveis de ambiente excede o limite máximo de 64 KiB.",
            ));
        }
    }

    Ok(())
}

/// Valida e normaliza um [`TabTemplateInput`], retornando o [`TemplateConfig`] higienizado.
pub fn validate_template_input(input: &TabTemplateInput) -> MtResult<TemplateConfig> {
    let trimmed_name = input.name.trim();
    let name_chars = trimmed_name.chars().count();
    if name_chars == 0 || name_chars > MAX_TEMPLATE_NAME_CHARS {
        return Err(MtError::new(
            codes::TEMPLATE_INVALID,
            "O nome do template deve conter entre 1 e 64 caracteres.",
        ));
    }
    if trimmed_name.chars().any(char::is_control) {
        return Err(MtError::new(
            codes::TEMPLATE_INVALID,
            "O nome do template não pode conter caracteres de controle.",
        ));
    }

    let trimmed_icon = input.icon.trim();
    if !is_valid_iconify_id(trimmed_icon) {
        return Err(MtError::new(
            codes::TEMPLATE_INVALID,
            "O ícone do template deve seguir o padrão Iconify 'prefixo:nome' (até 64 caracteres).",
        ));
    }

    let clean_cwd = match &input.cwd {
        Some(raw) => {
            if raw.contains('\0') || raw.chars().any(char::is_control) {
                return Err(MtError::new(
                    codes::TEMPLATE_INVALID,
                    "A pasta personalizada contém caracteres inválidos.",
                ));
            }
            let t = raw.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        None => None,
    };

    let validated_command = match &input.initial_command {
        Some(raw) => validate_command_text(raw)?,
        None => None,
    };

    let clean_env_kind = match input.environment.kind {
        EnvironmentKind::Shell => {
            if input
                .environment
                .agent_id
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
            {
                return Err(MtError::new(
                    codes::TEMPLATE_INVALID,
                    "Templates do tipo Shell não devem definir identificador de agente.",
                ));
            }
            TemplateEnvironment {
                kind: EnvironmentKind::Shell,
                agent_id: None,
            }
        }
        EnvironmentKind::Agent => {
            let slug = input.environment.agent_id.as_deref().map_or("", str::trim);
            if !is_valid_agent_slug(slug) {
                return Err(MtError::new(
                    codes::TEMPLATE_INVALID,
                    "Identificador de agente CLI inválido (esperado slug [a-z0-9._-]{1,64}).",
                ));
            }
            TemplateEnvironment {
                kind: EnvironmentKind::Agent,
                agent_id: Some(slug.to_string()),
            }
        }
    };

    validate_env_map(&input.env_vars)?;

    Ok(TemplateConfig {
        name: trimmed_name.to_string(),
        icon: trimmed_icon.to_string(),
        cwd: clean_cwd,
        initial_command: validated_command,
        add_to_workspace: input.add_to_workspace,
        continuous: input.continuous,
        environment: clean_env_kind,
        env_vars: input.env_vars.clone(),
    })
}
