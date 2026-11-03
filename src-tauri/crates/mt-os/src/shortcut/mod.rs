//! Gerenciamento de atalho global (`Accelerator`, `ShortcutManager` e cadeia de backends).

use std::sync::Arc;

use async_trait::async_trait;
use mt_core::error::{MtError, MtResult};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::environment::{Compositor, DisplayServer, OsEnvironment};
use crate::error::{codes, os_error};
use crate::settings::QuakeSettings;

/// Comando CLI canônico para alternar a janela do `MaxTerminal` pelo compositor.
pub const CLI_TOGGLE_COMMAND: &str = "maxterminal --toggle";

/// Tipo de backend de atalho global efetivamente selecionado.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutBackendKind {
    /// Plugin nativo de captura de tecla (X11).
    Plugin,
    /// Portal XDG `org.freedesktop.portal.GlobalShortcuts` (Wayland).
    Portal,
    /// Fallback via comando CLI no compositor (`maxterminal --toggle`).
    #[default]
    CliOnly,
}

/// Preferência do usuário para seleção do backend de atalho global.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutBackendPreference {
    /// Escolha automática conforme o protocolo gráfico (`X11 -> Plugin`, `Wayland -> Portal -> CliOnly`).
    #[default]
    Auto,
    /// Força o uso do backend `Plugin`.
    Plugin,
    /// Força o uso do backend `Portal`.
    Portal,
    /// Desativa captura direta e usa apenas `maxterminal --toggle`.
    CliOnly,
}

/// Status atual do registro de atalho global exposto à UI.
#[derive(Debug, Clone, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    /// Backend em uso (`plugin`, `portal` ou `cli_only`).
    pub backend: ShortcutBackendKind,
    /// Acelerador configurado nas preferências (ex.: `"Ctrl+Alt+Backquote"`).
    pub accelerator: String,
    /// Descrição do gatilho efetivamente concedido pelo portal XDG, quando aplicável.
    pub effective_trigger: Option<String>,
    /// `true` se o atalho está registrado e ativo num backend `plugin` ou `portal`.
    pub registered: bool,
    /// Último erro de registro ou conflito, se houver.
    pub error: Option<MtError>,
    /// Comando CLI de fallback (`"maxterminal --toggle"`).
    pub cli_command: String,
    /// Dica/snippet de configuração para o compositor atual (Hyprland, KDE Plasma, Sway, GNOME).
    pub compositor_hint: Option<String>,
}

impl Default for ShortcutStatus {
    fn default() -> Self {
        Self {
            backend: ShortcutBackendKind::CliOnly,
            accelerator: String::from("Ctrl+Alt+Backquote"),
            effective_trigger: None,
            registered: false,
            error: None,
            cli_command: String::from(CLI_TOGGLE_COMMAND),
            compositor_hint: None,
        }
    }
}

/// Modificadores e tecla de um acelerador validado.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accelerator {
    /// Modificador `Ctrl` presente.
    pub ctrl: bool,
    /// Modificador `Alt` presente.
    pub alt: bool,
    /// Modificador `Shift` presente.
    pub shift: bool,
    /// Modificador `Super` / `Meta` presente.
    pub super_key: bool,
    /// Código canônico da tecla principal (ex.: `"Backquote"`, `"Space"`, `"F12"`, `"KeyT"`).
    pub key: String,
}

impl Accelerator {
    /// Analisa e valida uma string de acelerador (ex.: `"Ctrl+Alt+Backquote"`).
    ///
    /// Requer ao menos 1 modificador (`Ctrl`, `Alt`, `Shift`, `Super`) e exatamente 1 tecla principal,
    /// rejeitando combinações reservadas de terminal (`Ctrl+C`, `Ctrl+D`, `Ctrl+Z`, `Ctrl+L`,
    /// `Ctrl+Shift+C`, `Ctrl+Shift+V`).
    pub fn parse(raw: &str) -> MtResult<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(os_error(
                codes::SHORTCUT_INVALID,
                "O atalho global não pode ser vazio.",
            ));
        }

        let parts: Vec<&str> = trimmed.split('+').collect();
        if parts.len() < 2 {
            return Err(os_error(
                codes::SHORTCUT_INVALID,
                "O atalho global deve incluir pelo menos uma tecla modificadora (Ctrl, Alt, Shift ou Super).",
            ));
        }

        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut super_key = false;
        let mut main_key: Option<String> = None;

        for token in parts {
            let t = token.trim();
            if t.is_empty() {
                return Err(os_error(
                    codes::SHORTCUT_INVALID,
                    format!("Formato de atalho global inválido: '{raw}'."),
                ));
            }
            match t.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "commandorcontrol" | "cmdorctrl" => ctrl = true,
                "alt" | "option" => alt = true,
                "shift" => shift = true,
                "super" | "meta" | "win" | "command" | "cmd" => super_key = true,
                _ => {
                    if main_key.is_some() {
                        return Err(os_error(
                            codes::SHORTCUT_INVALID,
                            "O atalho global deve conter apenas uma tecla principal.",
                        ));
                    }
                    main_key = Some(normalize_key_code(t)?);
                }
            }
        }

        if !(ctrl || alt || shift || super_key) {
            return Err(os_error(
                codes::SHORTCUT_INVALID,
                "O atalho global exige pelo menos um modificador (Ctrl, Alt, Shift ou Super).",
            ));
        }

        let Some(key) = main_key else {
            return Err(os_error(
                codes::SHORTCUT_INVALID,
                "O atalho global precisa de uma tecla principal além dos modificadores.",
            ));
        };

        let acc = Self {
            ctrl,
            alt,
            shift,
            super_key,
            key,
        };

        if acc.is_reserved_terminal_combo() {
            return Err(os_error(
                codes::SHORTCUT_INVALID,
                format!(
                    "O atalho {} é reservado para operações essenciais do terminal.",
                    acc.to_canonical_string()
                ),
            ));
        }

        Ok(acc)
    }

    fn is_reserved_terminal_combo(&self) -> bool {
        if self.ctrl && !self.alt && !self.super_key && !self.shift {
            matches!(self.key.as_str(), "KeyC" | "KeyD" | "KeyZ" | "KeyL")
        } else if self.ctrl && self.shift && !self.alt && !self.super_key {
            matches!(self.key.as_str(), "KeyC" | "KeyV")
        } else {
            false
        }
    }

    /// Formata na representação canônica persistida (ex.: `"Ctrl+Alt+Backquote"`).
    #[must_use]
    pub fn to_canonical_string(&self) -> String {
        let mut tokens = Vec::with_capacity(5);
        if self.ctrl {
            tokens.push("Ctrl");
        }
        if self.alt {
            tokens.push("Alt");
        }
        if self.shift {
            tokens.push("Shift");
        }
        if self.super_key {
            tokens.push("Super");
        }
        tokens.push(self.key.as_str());
        tokens.join("+")
    }

    /// Formata para uso em plugins de atalho global (`tauri-plugin-global-shortcut`).
    #[must_use]
    pub fn to_plugin_string(&self) -> String {
        self.to_canonical_string()
    }

    /// Formata como gatilho XDG Desktop Portal (ex.: `"CTRL+ALT+grave"`).
    #[must_use]
    pub fn to_portal_trigger(&self) -> String {
        let mut tokens = Vec::with_capacity(5);
        if self.ctrl {
            tokens.push("CTRL");
        }
        if self.alt {
            tokens.push("ALT");
        }
        if self.shift {
            tokens.push("SHIFT");
        }
        if self.super_key {
            tokens.push("LOGO");
        }
        let xdg_key = match self.key.as_str() {
            "Backquote" => String::from("grave"),
            "Space" => String::from("space"),
            "Minus" => String::from("minus"),
            "Equal" => String::from("equal"),
            "Tab" => String::from("Tab"),
            "Enter" => String::from("Return"),
            "Backspace" => String::from("BackSpace"),
            other if other.starts_with("Key") && other.len() == 4 => {
                other[3..].to_ascii_lowercase()
            }
            other if other.starts_with("Digit") && other.len() == 6 => other[5..].to_owned(),
            other => other.to_owned(),
        };
        tokens.push(xdg_key.as_str());
        tokens.join("+")
    }
}

fn normalize_key_code(raw: &str) -> MtResult<String> {
    let lower = raw.to_ascii_lowercase();
    let canonical = match lower.as_str() {
        "`" | "backquote" | "grave" | "tilde" => String::from("Backquote"),
        "space" => String::from("Space"),
        "tab" => String::from("Tab"),
        "enter" | "return" => String::from("Enter"),
        "backspace" => String::from("Backspace"),
        "minus" | "-" => String::from("Minus"),
        "equal" | "=" => String::from("Equal"),
        " semicolon" | "semicolon" | ";" => String::from("Semicolon"),
        "quote" | "'" => String::from("Quote"),
        "comma" | "," => String::from("Comma"),
        "period" | "." => String::from("Period"),
        "slash" | "/" => String::from("Slash"),
        "backslash" | "\\" => String::from("Backslash"),
        "bracketleft" | "[" => String::from("BracketLeft"),
        "bracketright" | "]" => String::from("BracketRight"),
        s if s.len() == 1 && s.chars().all(|c| c.is_ascii_alphabetic()) => {
            format!("Key{}", s.to_ascii_uppercase())
        }
        s if s.len() == 1 && s.chars().all(|c| c.is_ascii_digit()) => {
            format!("Digit{s}")
        }
        s if s.starts_with("key")
            && s.len() == 4
            && s[3..].chars().all(|c| c.is_ascii_alphabetic()) =>
        {
            format!("Key{}", s[3..].to_ascii_uppercase())
        }
        s if s.starts_with("digit")
            && s.len() == 6
            && s[5..].chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("Digit{}", &s[5..])
        }
        s if s.starts_with('f') && s.len() >= 2 && s.len() <= 3 => {
            if let Ok(num) = s[1..].parse::<u8>()
                && (1..=24).contains(&num)
            {
                format!("F{num}")
            } else {
                return Err(os_error(
                    codes::SHORTCUT_INVALID,
                    format!("Tecla de função inválida no atalho global: '{raw}'."),
                ));
            }
        }
        _ => {
            return Err(os_error(
                codes::SHORTCUT_INVALID,
                format!("Tecla não reconhecida no atalho global: '{raw}'."),
            ));
        }
    };
    Ok(canonical)
}

/// Gera o snippet de configuração do compositor para o modo `CliOnly` / Wayland.
#[must_use]
pub fn compositor_hint_for(compositor: Compositor, acc: &Accelerator) -> Option<String> {
    let mut hypr_mods = Vec::new();
    if acc.super_key {
        hypr_mods.push("SUPER");
    }
    if acc.ctrl {
        hypr_mods.push("CTRL");
    }
    if acc.alt {
        hypr_mods.push("ALT");
    }
    if acc.shift {
        hypr_mods.push("SHIFT");
    }
    let hypr_key = match acc.key.as_str() {
        "Backquote" => String::from("grave"),
        "Space" => String::from("space"),
        s if s.starts_with("Key") && s.len() == 4 => s[3..].to_ascii_lowercase(),
        s if s.starts_with("Digit") && s.len() == 6 => s[5..].to_owned(),
        other => other.to_owned(),
    };
    match compositor {
        Compositor::Hyprland => Some(format!(
            "bind = {}, {}, exec, {}",
            hypr_mods.join(" "),
            hypr_key,
            CLI_TOGGLE_COMMAND
        )),
        Compositor::KdePlasma => Some(format!(
            "Configurações do Sistema > Atalhos > Adicionar comando > `{CLI_TOGGLE_COMMAND}`"
        )),
        Compositor::Sway => Some(format!(
            "bindsym {} exec {}",
            acc.to_portal_trigger().replace("LOGO", "Mod4"),
            CLI_TOGGLE_COMMAND
        )),
        Compositor::Gnome | Compositor::Other => Some(format!(
            "Configure um atalho personalizado do sistema executando `{CLI_TOGGLE_COMMAND}`"
        )),
    }
}

/// Contrato assíncrono de um backend de captura de atalho global.
#[async_trait]
pub trait GlobalShortcutBackend: Send + Sync {
    /// Identifica o tipo do backend.
    fn kind(&self) -> ShortcutBackendKind;

    /// Registra o acelerador e invoca `on_trigger` quando acionado; retorna `Ok(effective_trigger)`.
    async fn register(
        &self,
        accelerator: &Accelerator,
        on_trigger: Arc<dyn Fn() + Send + Sync>,
    ) -> MtResult<Option<String>>;

    /// Desregistra o atalho global ativo.
    async fn unregister(&self) -> MtResult<()>;
}

/// Backend CLI-only (não captura teclas diretamente; orienta o usuário com o snippet do compositor).
#[derive(Debug, Default)]
pub struct CliShortcutBackend;

#[async_trait]
impl GlobalShortcutBackend for CliShortcutBackend {
    fn kind(&self) -> ShortcutBackendKind {
        ShortcutBackendKind::CliOnly
    }

    async fn register(
        &self,
        _accelerator: &Accelerator,
        _on_trigger: Arc<dyn Fn() + Send + Sync>,
    ) -> MtResult<Option<String>> {
        Ok(None)
    }

    async fn unregister(&self) -> MtResult<()> {
        Ok(())
    }
}

/// Fábrica de backends de atalho injetada no `ShortcutManager`.
pub trait ShortcutBackendFactory: Send + Sync + 'static {
    /// Cria uma instância do backend solicitado, se suportado pelo runtime.
    fn create(&self, kind: ShortcutBackendKind) -> Option<Box<dyn GlobalShortcutBackend>>;
}

/// Fábrica padrão quando não há plugin X11/portal nativo acoplado (fornece `CliShortcutBackend`).
#[derive(Debug, Default)]
pub struct DefaultShortcutBackendFactory;

impl ShortcutBackendFactory for DefaultShortcutBackendFactory {
    fn create(&self, kind: ShortcutBackendKind) -> Option<Box<dyn GlobalShortcutBackend>> {
        match kind {
            ShortcutBackendKind::CliOnly => Some(Box::new(CliShortcutBackend)),
            ShortcutBackendKind::Plugin | ShortcutBackendKind::Portal => None,
        }
    }
}

/// Gerenciador de registro e fallback de atalho global com preservação de estado anterior em caso de conflito (CA-19).
pub struct ShortcutManager {
    factory: Arc<dyn ShortcutBackendFactory>,
    active: tokio::sync::Mutex<Option<(Box<dyn GlobalShortcutBackend>, Accelerator)>>,
    status: RwLock<ShortcutStatus>,
}

impl ShortcutManager {
    /// Cria um novo `ShortcutManager`.
    #[must_use]
    pub fn new(factory: Arc<dyn ShortcutBackendFactory>) -> Self {
        Self {
            factory,
            active: tokio::sync::Mutex::new(None),
            status: RwLock::new(ShortcutStatus::default()),
        }
    }

    /// Retorna o snapshot atual de `ShortcutStatus`.
    #[must_use]
    pub fn status(&self) -> ShortcutStatus {
        self.status.read().clone()
    }

    /// Aplica as configurações de atalho global respeitando a cadeia de backends e preservando o anterior se houver conflito.
    #[allow(clippy::too_many_lines)]
    pub async fn apply(
        &self,
        settings: &QuakeSettings,
        env: &OsEnvironment,
        trigger: Arc<dyn Fn() + Send + Sync>,
    ) -> MtResult<ShortcutStatus> {
        let parsed = Accelerator::parse(&settings.accelerator)?;
        let hint = compositor_hint_for(env.compositor, &parsed);

        let mut guard = self.active.lock().await;

        if !settings.enabled {
            if let Some((prev_backend, _)) = guard.take() {
                let _ = prev_backend.unregister().await;
            }
            let st = ShortcutStatus {
                backend: ShortcutBackendKind::CliOnly,
                accelerator: parsed.to_canonical_string(),
                effective_trigger: None,
                registered: false,
                error: None,
                cli_command: String::from(CLI_TOGGLE_COMMAND),
                compositor_hint: hint,
            };
            *self.status.write() = st.clone();
            return Ok(st);
        }

        let target_kind = resolve_target_backend(settings.preferred_backend, env);

        if target_kind == ShortcutBackendKind::CliOnly {
            if let Some((prev_backend, _)) = guard.take() {
                let _ = prev_backend.unregister().await;
            }
            let portal_missing_err = if settings.preferred_backend
                == ShortcutBackendPreference::Portal
                || (settings.preferred_backend == ShortcutBackendPreference::Auto
                    && matches!(
                        env.display_server,
                        DisplayServer::Wayland | DisplayServer::XWayland
                    )
                    && !env.portal_global_shortcuts)
            {
                Some(os_error(
                    codes::PORTAL_UNAVAILABLE,
                    "Portal de atalhos globais indisponível nesta sessão Wayland; utilize o comando CLI no compositor.",
                ))
            } else {
                None
            };

            let st = ShortcutStatus {
                backend: ShortcutBackendKind::CliOnly,
                accelerator: parsed.to_canonical_string(),
                effective_trigger: None,
                registered: false,
                error: portal_missing_err,
                cli_command: String::from(CLI_TOGGLE_COMMAND),
                compositor_hint: hint,
            };
            *self.status.write() = st.clone();
            return Ok(st);
        }

        let Some(new_backend) = self.factory.create(target_kind) else {
            let st = ShortcutStatus {
                backend: ShortcutBackendKind::CliOnly,
                accelerator: parsed.to_canonical_string(),
                effective_trigger: None,
                registered: false,
                error: Some(os_error(
                    codes::SHORTCUT_UNSUPPORTED,
                    "Backend de atalho global não disponível; usando modo CLI.",
                )),
                cli_command: String::from(CLI_TOGGLE_COMMAND),
                compositor_hint: hint,
            };
            *self.status.write() = st.clone();
            return Ok(st);
        };

        let previous = guard.take();
        if let Some((ref prev_b, _)) = previous {
            let _ = prev_b.unregister().await;
        }

        match new_backend.register(&parsed, Arc::clone(&trigger)).await {
            Ok(effective) => {
                let st = ShortcutStatus {
                    backend: target_kind,
                    accelerator: parsed.to_canonical_string(),
                    effective_trigger: effective,
                    registered: true,
                    error: None,
                    cli_command: String::from(CLI_TOGGLE_COMMAND),
                    compositor_hint: hint,
                };
                *guard = Some((new_backend, parsed));
                *self.status.write() = st.clone();
                Ok(st)
            }
            Err(err) => {
                // CA-19: Em falha de conflito/registro, restaura o atalho anterior se existia
                if let Some((prev_backend, prev_acc)) = previous {
                    if let Ok(eff) = prev_backend.register(&prev_acc, trigger).await {
                        let restored_hint = compositor_hint_for(env.compositor, &prev_acc);
                        let restored = ShortcutStatus {
                            backend: prev_backend.kind(),
                            accelerator: prev_acc.to_canonical_string(),
                            effective_trigger: eff,
                            registered: true,
                            error: Some(err.clone()),
                            cli_command: String::from(CLI_TOGGLE_COMMAND),
                            compositor_hint: restored_hint,
                        };
                        *guard = Some((prev_backend, prev_acc));
                        *self.status.write() = restored;
                    }
                    return Err(err);
                }

                // Startup sem atalho anterior: degrada para CliOnly com o erro anexado
                let fallback = ShortcutStatus {
                    backend: ShortcutBackendKind::CliOnly,
                    accelerator: parsed.to_canonical_string(),
                    effective_trigger: None,
                    registered: false,
                    error: Some(err),
                    cli_command: String::from(CLI_TOGGLE_COMMAND),
                    compositor_hint: hint,
                };
                *self.status.write() = fallback.clone();
                Ok(fallback)
            }
        }
    }

    /// Solicita reconfiguração no portal XDG (`os_shortcut_configure_portal`).
    pub fn configure_portal(&self, env: &OsEnvironment) -> MtResult<ShortcutStatus> {
        if !env.portal_global_shortcuts {
            return Err(os_error(
                codes::PORTAL_UNAVAILABLE,
                "O portal org.freedesktop.portal.GlobalShortcuts não está disponível nesta sessão.",
            ));
        }
        Ok(self.status())
    }
}

const fn resolve_target_backend(
    pref: ShortcutBackendPreference,
    env: &OsEnvironment,
) -> ShortcutBackendKind {
    match pref {
        ShortcutBackendPreference::Plugin => ShortcutBackendKind::Plugin,
        ShortcutBackendPreference::Portal => {
            if env.portal_global_shortcuts {
                ShortcutBackendKind::Portal
            } else {
                ShortcutBackendKind::CliOnly
            }
        }
        ShortcutBackendPreference::CliOnly => ShortcutBackendKind::CliOnly,
        ShortcutBackendPreference::Auto => match env.display_server {
            DisplayServer::X11 => ShortcutBackendKind::Plugin,
            DisplayServer::Wayland | DisplayServer::XWayland => {
                if env.portal_global_shortcuts {
                    ShortcutBackendKind::Portal
                } else {
                    ShortcutBackendKind::CliOnly
                }
            }
            DisplayServer::Unknown => ShortcutBackendKind::CliOnly,
        },
    }
}
