//! Configurações de agentes CLI (`AgentsSettings`), normalização de atalhos e validação de colisões.

use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

use mt_core::{ErrorCode, MtError, MtResult};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error;
use crate::registry::{AgentKind, AgentRegistry};

static SHORTCUT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(Ctrl\+)?(Alt\+)?(Shift\+)?(Super\+)?[A-Z0-9]$")
        .unwrap_or_else(|e| panic!("Regex de atalho inválida: {e}"))
});

/// Localização onde as worktrees isoladas de agentes são criadas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeLocationDto {
    /// Subpasta `.worktrees/` dentro do projeto.
    #[default]
    Project,
    /// Subpasta `.max-code-worktrees/` compatível com a extensão MaxCode.
    #[serde(alias = "max_code")]
    MaxCode,
}

impl WorktreeLocationDto {
    /// Retorna a representação textual persistida na coluna SQL `location`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::MaxCode => "max_code",
        }
    }

    /// Converte o valor da coluna SQL `location` no enum correspondente.
    pub fn from_db_str(value: &str) -> MtResult<Self> {
        match value {
            "project" => Ok(Self::Project),
            "max_code" | "maxCode" => Ok(Self::MaxCode),
            _ => Err(MtError::new(
                ErrorCode(error::codes::SETTINGS_INVALID),
                "Localização de worktree inválida no banco de dados.",
            )),
        }
    }

    /// Retorna o nome do subdiretório relativo à raiz do repositório Git.
    #[must_use]
    pub const fn folder_name(self) -> &'static str {
        match self {
            Self::Project => ".worktrees",
            Self::MaxCode => ".max-code-worktrees",
        }
    }
}

/// Preferências padrão de isolamento em Git Worktree para agentes CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct IsolationSettings {
    /// Localização padrão das worktrees criadas.
    pub default_location: WorktreeLocationDto,
    /// Prefixo de branch Git (ex.: `"agent"` gera `agent/claude/<slug>`).
    pub branch_prefix: String,
    /// Quando `true`, o atalho padrão abre o agente já com isolamento marcado.
    pub open_isolated_by_default: bool,
}

impl Default for IsolationSettings {
    fn default() -> Self {
        Self {
            default_location: WorktreeLocationDto::Project,
            branch_prefix: "agent".to_string(),
            open_isolated_by_default: false,
        }
    }
}

/// Comportamento de alerta ao concluir um turno normal do agente.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AttentionLevel {
    /// Envia notificação informativa ao concluir turno quando a aba estiver em segundo plano.
    Notify,
    /// Ignora fim de turno normal (notifica apenas espera real de aprovação/input).
    #[default]
    Ignore,
}

/// Configurações do detector de atenção e notificações de agentes CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct AttentionSettings {
    /// Habilita a detecção de pausa por permissão/input.
    pub enabled: bool,
    /// Ação ao detectar fim de turno normal (`Stop` / `agent-turn-complete`).
    pub on_turn_complete: AttentionLevel,
    /// Janela de silêncio em ms antes de avaliar padrões heurísticos no output do PTY.
    pub silence_ms: u32,
    /// Habilita o envio de notificações nativas do sistema operacional.
    pub notify: bool,
}

impl Default for AttentionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            on_turn_complete: AttentionLevel::Ignore,
            silence_ms: 300,
            notify: true,
        }
    }
}

/// Configurações de coleta e exibição de telemetria (`StatusLine`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct TelemetrySettings {
    /// Habilita a coleta e exibição da telemetria na barra da aba.
    pub enabled: bool,
    /// Tempo em segundos após o qual uma medição sem atualização é marcada como `stale` (padrão: 60 s).
    pub stale_after_s: u32,
    /// Permite ler arquivos locais de sessão/transcript (`.claude/projects`, `.codex/sessions`).
    pub read_session_files: bool,
}

impl Default for TelemetrySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            stale_after_s: 60,
            read_session_files: true,
        }
    }
}

/// Configurações globais de Agentes CLI persistidas em `agents.settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentsSettings {
    /// Habilita os atalhos de teclado dedicados para abrir abas de agentes.
    pub shortcuts_enabled: bool,
    /// Overrides de atalho por agente (`AgentKind -> "Ctrl+Alt+C"`).
    pub shortcuts: BTreeMap<AgentKind, String>,
    /// Configurações de isolamento em worktree.
    pub isolation: IsolationSettings,
    /// Configurações de detecção de atenção.
    pub attention: AttentionSettings,
    /// Configurações de telemetria `StatusLine`.
    pub telemetry: TelemetrySettings,
    /// Habilita a injeção de flags/hooks de integração por agente.
    pub inject: BTreeMap<AgentKind, bool>,
    /// Habilita ou desabilita cada agente individualmente.
    pub enabled: BTreeMap<AgentKind, bool>,
}

impl Default for AgentsSettings {
    fn default() -> Self {
        let mut inject = BTreeMap::new();
        inject.insert(AgentKind::Claude, true);
        inject.insert(AgentKind::Codex, true);
        inject.insert(AgentKind::Agy, false);
        inject.insert(AgentKind::OpenCode, false);
        let mut enabled = BTreeMap::new();
        for kind in AgentKind::ALL {
            enabled.insert(kind, true);
        }
        Self {
            shortcuts_enabled: true,
            shortcuts: BTreeMap::new(),
            isolation: IsolationSettings::default(),
            attention: AttentionSettings::default(),
            telemetry: TelemetrySettings::default(),
            inject,
            enabled,
        }
    }
}

impl AgentsSettings {
    /// Carrega `agents.settings` do banco SQLite (`SettingsStore`), usando `Default` se ausente.
    pub fn load_from_db(db: &mt_db::Db) -> MtResult<Self> {
        let store = db.settings().map_err(MtError::from)?;
        match store.get_snapshot("agents.settings") {
            Ok(snap) => serde_json::from_value(snap.value).map_err(|e| {
                error::settings_invalid(
                    format!("Falha ao desserializar agents.settings: {e}"),
                    None,
                    None,
                )
            }),
            Err(_) => Ok(Self::default()),
        }
    }

    /// Salva `agents.settings` no banco SQLite (`SettingsStore`).
    pub async fn save_to_db(&self, db: &mt_db::Db) -> MtResult<()> {
        let store = db.settings().map_err(MtError::from)?;
        let val = serde_json::to_value(self).map_err(|e| {
            error::settings_invalid(
                format!("Falha ao serializar agents.settings: {e}"),
                None,
                None,
            )
        })?;
        store
            .set(
                "agents.settings",
                val,
                None,
                mt_db::models::SettingChangeOrigin::Ui,
            )
            .await
            .map_err(MtError::from)?;
        Ok(())
    }
}

/// Atalhos globais ou de workspace reservados pelas Fases 04, 07 e 09.
#[must_use]
pub fn default_reserved_shortcuts() -> HashSet<String> {
    [
        "Ctrl+Shift+C",
        "Ctrl+Shift+V",
        "Ctrl+Shift+T",
        "Ctrl+Shift+W",
        "Ctrl+Shift+F",
        "Ctrl+Shift+D",
        "Ctrl+Shift+E",
        "Ctrl+Tab",
        "Ctrl+Shift+Tab",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// Normaliza um atalho para a forma canônica `Ctrl+Alt+Shift+Super+<KEY>` e valida a regex
/// `^(Ctrl\+)?(Alt\+)?(Shift\+)?(Super\+)?[A-Z0-9]$` com pelo menos 1 modificador.
pub fn normalize_and_validate_shortcut(raw: &str) -> MtResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(error::shortcut_invalid(raw));
    }

    let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    if parts.len() < 2 {
        return Err(error::shortcut_invalid(raw));
    }

    let mut has_ctrl = false;
    let mut has_alt = false;
    let mut has_shift = false;
    let mut has_super = false;
    let mut key_part: Option<String> = None;

    for (idx, part) in parts.iter().enumerate() {
        let is_last = idx == parts.len() - 1;
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" if !is_last => {
                if has_ctrl {
                    return Err(error::shortcut_invalid(raw));
                }
                has_ctrl = true;
            }
            "alt" | "option" if !is_last => {
                if has_alt {
                    return Err(error::shortcut_invalid(raw));
                }
                has_alt = true;
            }
            "shift" if !is_last => {
                if has_shift {
                    return Err(error::shortcut_invalid(raw));
                }
                has_shift = true;
            }
            "super" | "meta" | "cmd" | "win" if !is_last => {
                if has_super {
                    return Err(error::shortcut_invalid(raw));
                }
                has_super = true;
            }
            _ if is_last => {
                let upper = part.to_ascii_uppercase();
                if upper.len() == 1 && upper.chars().all(|c| c.is_ascii_alphanumeric()) {
                    key_part = Some(upper);
                } else {
                    return Err(error::shortcut_invalid(raw));
                }
            }
            _ => return Err(error::shortcut_invalid(raw)),
        }
    }

    if !(has_ctrl || has_alt || has_shift || has_super) {
        return Err(error::shortcut_invalid(raw));
    }

    let Some(key) = key_part else {
        return Err(error::shortcut_invalid(raw));
    };

    let mut canonical = String::new();
    if has_ctrl {
        canonical.push_str("Ctrl+");
    }
    if has_alt {
        canonical.push_str("Alt+");
    }
    if has_shift {
        canonical.push_str("Shift+");
    }
    if has_super {
        canonical.push_str("Super+");
    }
    canonical.push_str(&key);

    if !SHORTCUT_RE.is_match(&canonical) {
        return Err(error::shortcut_invalid(raw));
    }

    Ok(canonical)
}

/// Deriva a variante `+Shift` de um atalho normalizado (usada para abrir com isolamento em worktree).
/// Retorna `None` se o atalho já contiver `Shift+`.
#[must_use]
pub fn shift_variant_of(canonical: &str) -> Option<String> {
    if canonical.contains("Shift+") {
        return None;
    }
    let last_plus = canonical.rfind('+')?;
    let (mods_with_plus, key) = canonical.split_at(last_plus + 1);
    if let Some(super_idx) = mods_with_plus.find("Super+") {
        let (before_super, super_and_after) = mods_with_plus.split_at(super_idx);
        Some(format!("{before_super}Shift+{super_and_after}{key}"))
    } else {
        Some(format!("{mods_with_plus}Shift+{key}"))
    }
}

/// Valida `AgentsSettings` contra atalhos inválidos, colisões entre agentes (incluindo variante `+Shift`)
/// e colisões com atalhos reservados das Fases 04/07/09 (`reserved_shortcuts`).
pub fn validate_settings(
    settings: &AgentsSettings,
    reg: &AgentRegistry,
    reserved_shortcuts: &HashSet<String>,
) -> MtResult<AgentsSettings> {
    let mut normalized = settings.clone();

    let branch_prefix = normalized.isolation.branch_prefix.trim().to_string();
    if branch_prefix.is_empty()
        || branch_prefix.len() > 32
        || !branch_prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(error::settings_invalid(
            "O prefixo de branch de isolamento deve conter apenas letras, números, '-' ou '_' (1 a 32 caracteres).",
            None,
            None,
        ));
    }
    normalized.isolation.branch_prefix = branch_prefix;

    if normalized.telemetry.stale_after_s < 5 || normalized.telemetry.stale_after_s > 3600 {
        return Err(error::settings_invalid(
            "O tempo de expiração de telemetria (staleAfterS) deve estar entre 5 e 3600 segundos.",
            None,
            None,
        ));
    }

    if normalized.attention.silence_ms < 50 || normalized.attention.silence_ms > 10_000 {
        return Err(error::settings_invalid(
            "A janela de silêncio (silenceMs) deve estar entre 50 e 10000 ms.",
            None,
            None,
        ));
    }

    let mut canonical_shortcuts = BTreeMap::new();
    for (&kind, raw_sc) in &normalized.shortcuts {
        let canon = normalize_and_validate_shortcut(raw_sc)?;
        canonical_shortcuts.insert(kind, canon);
    }
    normalized.shortcuts = canonical_shortcuts;

    let mut claimed: BTreeMap<String, String> = BTreeMap::new();
    for res in reserved_shortcuts {
        if let Ok(canon_res) = normalize_and_validate_shortcut(res) {
            claimed.insert(canon_res, "sistema".to_string());
        }
    }

    for kind in AgentKind::ALL {
        let desc = reg.get(kind);
        let raw_effective = normalized
            .shortcuts
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| desc.shortcut.clone());
        let effective = normalize_and_validate_shortcut(&raw_effective)?;

        if let Some(owner) = claimed.get(&effective) {
            return Err(error::shortcut_conflict(&effective, owner));
        }
        claimed.insert(effective.clone(), desc.display_name.clone());

        if let Some(shift_var) = shift_variant_of(&effective) {
            if let Some(owner) = claimed.get(&shift_var) {
                return Err(error::shortcut_conflict(&shift_var, owner));
            }
            claimed.insert(
                shift_var,
                format!("{} (isolado em worktree)", desc.display_name),
            );
        }
    }

    for kind in AgentKind::ALL {
        let default_inject = matches!(kind, AgentKind::Claude | AgentKind::Codex);
        normalized.inject.entry(kind).or_insert(default_inject);
    }

    Ok(normalized)
}

/// Configurações do sincronizador `MaxSkills` persistidas em `skills.settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct SkillsSettings {
    /// Diretório-fonte do repositório `MaxSkills` (padrão: `~/GitHub/MaxSkills`).
    pub source_dir: String,
    /// Modo padrão de sincronização (`"symlink"` ou `"copy"`).
    pub default_mode: String,
}

impl Default for SkillsSettings {
    fn default() -> Self {
        Self {
            source_dir: "~/GitHub/MaxSkills".to_string(),
            default_mode: "symlink".to_string(),
        }
    }
}

impl SkillsSettings {
    /// Carrega `skills.settings` do banco SQLite (`SettingsStore`), usando `Default` se ausente.
    pub fn load_from_db(db: &mt_db::Db) -> MtResult<Self> {
        let store = db.settings().map_err(MtError::from)?;
        match store.get_snapshot("skills.settings") {
            Ok(snap) => serde_json::from_value(snap.value).map_err(|e| {
                error::settings_invalid(
                    format!("Falha ao desserializar skills.settings: {e}"),
                    None,
                    None,
                )
            }),
            Err(_) => Ok(Self::default()),
        }
    }

    /// Salva `skills.settings` no banco SQLite (`SettingsStore`).
    pub async fn save_to_db(&self, db: &mt_db::Db) -> MtResult<()> {
        let store = db.settings().map_err(MtError::from)?;
        let val = serde_json::to_value(self).map_err(|e| {
            error::settings_invalid(
                format!("Falha ao serializar skills.settings: {e}"),
                None,
                None,
            )
        })?;
        store
            .set(
                "skills.settings",
                val,
                None,
                mt_db::models::SettingChangeOrigin::Ui,
            )
            .await
            .map_err(MtError::from)?;
        Ok(())
    }
}
