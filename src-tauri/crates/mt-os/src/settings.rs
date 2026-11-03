//! Configurações persistidas de integração com o sistema operacional (`OsSettings`) e validação pura.

use mt_core::error::MtResult;
use mt_db::sanitizer::inspect_settings_json;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::editor::EditorKind;
use crate::error::{codes, os_error};
use crate::shortcut::{Accelerator, ShortcutBackendPreference};

/// Versão atual suportada do schema de `OsSettings`.
pub const OS_SETTINGS_SCHEMA_VERSION: u32 = 1;

/// Acelerador padrão para o Quake Mode / Quick Toggle.
pub const DEFAULT_QUAKE_ACCELERATOR: &str = "Ctrl+Alt+Backquote";

/// Comportamento ao fechar a janela principal do `MaxTerminal`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    /// Oculta na bandeja quando houver processos ativos; caso contrário, encerra o aplicativo.
    #[default]
    Auto,
    /// Sempre oculta na bandeja do sistema ao clicar no botão fechar.
    AlwaysTray,
    /// Sempre encerra o aplicativo (exibindo confirmação se houver abas ativas).
    AlwaysQuit,
}

/// Modo de apresentação da janela ao acionar o atalho global.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum QuakeMode {
    /// Janela normal do gerenciador de janelas.
    #[default]
    Normal,
    /// Painel suspenso no topo do monitor (Drop-down / Quake).
    DropDown,
}

/// Configurações do atalho global e modo Quake / Drop-down.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct QuakeSettings {
    /// Se o atalho global está habilitado.
    pub enabled: bool,
    /// String canônica do acelerador (ex.: `"Ctrl+Alt+Backquote"`).
    pub accelerator: String,
    /// Modo de janela (`normal` ou `drop_down`).
    pub mode: QuakeMode,
    /// Altura percentual do monitor no modo `drop_down` (`20..=100`).
    pub drop_down_height_pct: u8,
    /// Se a janela permanece sempre no topo no modo `drop_down`.
    pub always_on_top: bool,
    /// Se a janela oculta automaticamente ao perder o foco no modo `drop_down`.
    pub hide_on_blur: bool,
    /// Preferência de backend para registro do atalho global.
    pub preferred_backend: ShortcutBackendPreference,
}

impl Default for QuakeSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            accelerator: String::from(DEFAULT_QUAKE_ACCELERATOR),
            mode: QuakeMode::Normal,
            drop_down_height_pct: 45,
            always_on_top: true,
            hide_on_blur: false,
            preferred_backend: ShortcutBackendPreference::Auto,
        }
    }
}

/// Patch parcial para `QuakeSettings`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuakeSettingsPatch {
    /// Habilita ou desabilita o atalho global.
    #[specta(optional)]
    pub enabled: Option<bool>,
    /// Novo acelerador canônico.
    #[specta(optional)]
    pub accelerator: Option<String>,
    /// Novo modo de apresentação (`normal` ou `drop_down`).
    #[specta(optional)]
    pub mode: Option<QuakeMode>,
    /// Nova altura percentual (`20..=100`).
    #[specta(optional)]
    pub drop_down_height_pct: Option<u8>,
    /// Novo estado de `always_on_top`.
    #[specta(optional)]
    pub always_on_top: Option<bool>,
    /// Novo estado de `hide_on_blur`.
    #[specta(optional)]
    pub hide_on_blur: Option<bool>,
    /// Nova preferência de backend.
    #[specta(optional)]
    pub preferred_backend: Option<ShortcutBackendPreference>,
}

/// Configurações do editor externo acionado ao clicar em links no terminal.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct EditorSettings {
    /// Tipo de editor configurado (`vs_code`, `vs_code_insiders`, `max_code` ou `custom`).
    pub kind: EditorKind,
    /// Vetor de argumentos quando `kind == EditorKind::Custom` (ex.: `["zed", "{file}:{line}:{column}"]`).
    pub custom_argv: Vec<String>,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            kind: EditorKind::VsCode,
            custom_argv: Vec::new(),
        }
    }
}

/// Configurações de notificações nativas do desktop.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct NotificationSettings {
    /// Se as notificações nativas estão habilitadas.
    pub enabled: bool,
    /// Se comandos longos disparam notificação ao terminar.
    pub long_command_enabled: bool,
    /// Limiar mínimo em segundos para considerar um comando longo (`3..=3600`).
    pub long_command_threshold_secs: u32,
    /// Se notificações de comandos longos só disparam quando a janela não está focada.
    pub only_when_unfocused: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            long_command_enabled: true,
            long_command_threshold_secs: 15,
            only_when_unfocused: true,
        }
    }
}

/// Patch parcial para `NotificationSettings`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettingsPatch {
    /// Habilita ou desabilita notificações nativas.
    #[specta(optional)]
    pub enabled: Option<bool>,
    /// Habilita ou desabilita alerta de comandos longos.
    #[specta(optional)]
    pub long_command_enabled: Option<bool>,
    /// Novo limiar em segundos (`3..=3600`).
    #[specta(optional)]
    pub long_command_threshold_secs: Option<u32>,
    /// Novo filtro de foco da janela.
    #[specta(optional)]
    pub only_when_unfocused: Option<bool>,
}

/// Configurações completas de integração com o sistema operacional.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct OsSettings {
    /// Versão do schema (`1`).
    pub schema_version: u32,
    /// Política ao fechar a janela principal.
    pub close_behavior: CloseBehavior,
    /// Configurações do atalho global e modo Quake.
    pub quake: QuakeSettings,
    /// Configurações do editor externo para links.
    pub editor: EditorSettings,
    /// Configurações de notificações nativas.
    pub notifications: NotificationSettings,
    /// Se o `MaxTerminal` inicia automaticamente na sessão do usuário.
    pub autostart: bool,
    /// Se o `MaxTerminal` inicia oculto na bandeja quando acionado via autostart / `--start-hidden`.
    pub start_minimized: bool,
}

impl Default for OsSettings {
    fn default() -> Self {
        Self {
            schema_version: OS_SETTINGS_SCHEMA_VERSION,
            close_behavior: CloseBehavior::Auto,
            quake: QuakeSettings::default(),
            editor: EditorSettings::default(),
            notifications: NotificationSettings::default(),
            autostart: false,
            start_minimized: true,
        }
    }
}

/// Patch parcial vindo da UI para atualização seletiva de `OsSettings`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OsSettingsPatch {
    /// Nova política ao fechar a janela.
    #[specta(optional)]
    pub close_behavior: Option<CloseBehavior>,
    /// Patch parcial das configurações de Quake / atalho global.
    #[specta(optional)]
    pub quake: Option<QuakeSettingsPatch>,
    /// Novas configurações do editor externo.
    #[specta(optional)]
    pub editor: Option<EditorSettings>,
    /// Patch parcial das configurações de notificações.
    #[specta(optional)]
    pub notifications: Option<NotificationSettingsPatch>,
    /// Novo estado de inicialização automática (`autostart`).
    #[specta(optional)]
    pub autostart: Option<bool>,
    /// Novo estado de inicialização minimizada (`start_minimized`).
    #[specta(optional)]
    pub start_minimized: Option<bool>,
}

/// Subconjuntos de runtime que precisam ser reaplicados após um patch de `OsSettings`.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SettingsDiff {
    /// Mudança no atalho global (`enabled`, `accelerator` ou `preferred_backend`).
    pub shortcut: bool,
    /// Mudança no `autostart` ou `start_minimized`.
    pub autostart: bool,
    /// Mudança na geometria ou modo da janela Quake (`mode`, `drop_down_height_pct`, `always_on_top`).
    pub quake_geometry: bool,
    /// Mudança no editor externo configurado.
    pub editor: bool,
}

impl OsSettings {
    /// Aplica um `OsSettingsPatch` produzindo a nova configuração validada e o `SettingsDiff`.
    pub fn apply_patch(&self, patch: OsSettingsPatch) -> MtResult<(Self, SettingsDiff)> {
        let mut next = self.clone();
        let mut diff = SettingsDiff::default();

        if let Some(cb) = patch.close_behavior {
            next.close_behavior = cb;
        }

        if let Some(qp) = patch.quake {
            if let Some(enabled) = qp.enabled
                && enabled != next.quake.enabled
            {
                next.quake.enabled = enabled;
                diff.shortcut = true;
            }
            if let Some(acc) = qp.accelerator {
                let parsed = Accelerator::parse(&acc)?;
                let canonical = parsed.to_canonical_string();
                if canonical != next.quake.accelerator {
                    next.quake.accelerator = canonical;
                    diff.shortcut = true;
                }
            }
            if let Some(mode) = qp.mode
                && mode != next.quake.mode
            {
                next.quake.mode = mode;
                diff.quake_geometry = true;
            }
            if let Some(height) = qp.drop_down_height_pct
                && height != next.quake.drop_down_height_pct
            {
                next.quake.drop_down_height_pct = height;
                diff.quake_geometry = true;
            }
            if let Some(aot) = qp.always_on_top
                && aot != next.quake.always_on_top
            {
                next.quake.always_on_top = aot;
                diff.quake_geometry = true;
            }
            if let Some(hob) = qp.hide_on_blur {
                next.quake.hide_on_blur = hob;
            }
            if let Some(pref) = qp.preferred_backend
                && pref != next.quake.preferred_backend
            {
                next.quake.preferred_backend = pref;
                diff.shortcut = true;
            }
        }

        if let Some(ed) = patch.editor
            && ed != next.editor
        {
            next.editor = ed;
            diff.editor = true;
        }

        if let Some(np) = patch.notifications {
            if let Some(en) = np.enabled {
                next.notifications.enabled = en;
            }
            if let Some(lce) = np.long_command_enabled {
                next.notifications.long_command_enabled = lce;
            }
            if let Some(thresh) = np.long_command_threshold_secs {
                next.notifications.long_command_threshold_secs = thresh;
            }
            if let Some(owu) = np.only_when_unfocused {
                next.notifications.only_when_unfocused = owu;
            }
        }

        if let Some(auto) = patch.autostart
            && auto != next.autostart
        {
            next.autostart = auto;
            diff.autostart = true;
        }

        if let Some(sm) = patch.start_minimized
            && sm != next.start_minimized
        {
            next.start_minimized = sm;
            diff.autostart = true;
        }

        next.validate()?;
        Ok((next, diff))
    }

    /// Valida todas as invariantes de domínio de `OsSettings`.
    pub fn validate(&self) -> MtResult<()> {
        if self.schema_version != OS_SETTINGS_SCHEMA_VERSION {
            return Err(os_error(
                codes::SETTINGS_INVALID,
                format!(
                    "Versão de schema de preferências do sistema não suportada: {}.",
                    self.schema_version
                ),
            ));
        }

        if !(20..=100).contains(&self.quake.drop_down_height_pct) {
            return Err(os_error(
                codes::SETTINGS_INVALID,
                "A altura do modo drop-down deve estar entre 20% e 100%.",
            ));
        }

        let _ = Accelerator::parse(&self.quake.accelerator)?;

        if !(3..=3600).contains(&self.notifications.long_command_threshold_secs) {
            return Err(os_error(
                codes::SETTINGS_INVALID,
                "O limiar de comando longo deve estar entre 3 e 3600 segundos.",
            ));
        }

        if self.editor.kind == EditorKind::Custom {
            validate_custom_argv(&self.editor.custom_argv)?;
        } else if self.editor.custom_argv.len() > 16
            || self.editor.custom_argv.iter().any(|s| s.len() > 512)
        {
            return Err(os_error(
                codes::SETTINGS_INVALID,
                "Os argumentos personalizados do editor excedem o limite máximo de 16 itens de 512 bytes.",
            ));
        }

        let val = serde_json::to_value(self).map_err(|e| {
            os_error(
                codes::SETTINGS_INVALID,
                format!("Falha ao serializar preferências do sistema: {e}"),
            )
        })?;
        if let Err(reason) = inspect_settings_json(&val) {
            return Err(os_error(
                codes::SETTINGS_INVALID,
                format!("Credenciais ou segredos não são permitidos nas preferências: {reason}"),
            ));
        }

        Ok(())
    }

    /// Desserializa `OsSettings` a partir do JSON persistido, rejeitando versões futuras de schema.
    pub fn from_persisted_row(
        row_schema_version: u32,
        raw_json: &str,
    ) -> MtResult<(Self, Map<String, Value>)> {
        if row_schema_version > OS_SETTINGS_SCHEMA_VERSION {
            return Err(os_error(
                codes::SETTINGS_INVALID,
                format!(
                    "As preferências salvas pertencem a uma versão mais recente do aplicativo (v{row_schema_version})."
                ),
            ));
        }

        let parsed: Value = serde_json::from_str(raw_json).map_err(|e| {
            os_error(
                codes::SETTINGS_INVALID,
                format!("JSON de preferências do sistema malformado: {e}"),
            )
        })?;

        let obj = parsed.as_object().cloned().ok_or_else(|| {
            os_error(
                codes::SETTINGS_INVALID,
                "O registro de preferências do sistema deve ser um objeto JSON.",
            )
        })?;

        if let Some(ver) = obj.get("schemaVersion").and_then(Value::as_u64)
            && ver > u64::from(OS_SETTINGS_SCHEMA_VERSION)
        {
            return Err(os_error(
                codes::SETTINGS_INVALID,
                format!(
                    "O JSON de preferências pertence a uma versão mais recente do aplicativo (v{ver})."
                ),
            ));
        }

        let mut settings: Self =
            serde_json::from_value(Value::Object(obj.clone())).map_err(|e| {
                os_error(
                    codes::SETTINGS_INVALID,
                    format!("Valores inválidos nas preferências do sistema: {e}"),
                )
            })?;
        settings.schema_version = OS_SETTINGS_SCHEMA_VERSION;
        settings.validate()?;
        Ok((settings, obj))
    }

    /// Serializa `OsSettings` preservando chaves desconhecidas de nível superior previamente armazenadas.
    pub fn to_merged_json(&self, previous_raw_obj: &Map<String, Value>) -> MtResult<String> {
        let mut base = previous_raw_obj.clone();
        let current = serde_json::to_value(self)?;
        if let Value::Object(map) = current {
            for (k, v) in map {
                base.insert(k, v);
            }
        }
        serde_json::to_string(&Value::Object(base)).map_err(Into::into)
    }
}

/// Valida o vetor de argumentos do editor personalizado (`custom_argv`).
pub fn validate_custom_argv(argv: &[String]) -> MtResult<()> {
    if argv.is_empty() {
        return Err(os_error(
            codes::SETTINGS_INVALID,
            "Informe o executável e os argumentos do editor personalizado.",
        ));
    }
    if argv.len() > 16 {
        return Err(os_error(
            codes::SETTINGS_INVALID,
            "O comando do editor personalizado pode ter no máximo 16 argumentos.",
        ));
    }
    if argv[0].trim().is_empty() || argv[0].starts_with('-') {
        return Err(os_error(
            codes::SETTINGS_INVALID,
            "O primeiro elemento do editor personalizado deve ser o nome ou caminho do executável.",
        ));
    }
    for arg in argv {
        if arg.len() > 512 || arg.contains('\0') {
            return Err(os_error(
                codes::SETTINGS_INVALID,
                "Argumento do editor personalizado inválido ou acima de 512 bytes.",
            ));
        }
    }
    let has_file_placeholder = argv.iter().any(|arg| arg.contains("{file}"));
    if !has_file_placeholder {
        return Err(os_error(
            codes::SETTINGS_INVALID,
            "Os argumentos do editor personalizado devem conter o marcador {file}.",
        ));
    }
    Ok(())
}
