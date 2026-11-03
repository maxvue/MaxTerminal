//! Detecção do ambiente gráfico Linux/CachyOS (`DisplayServer`, `Compositor`, D-Bus e editores).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::editor::{EditorAvailability, EditorLauncher};

/// Protocolo do servidor gráfico em uso pelo processo.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayServer {
    /// Sessão X11 nativa.
    X11,
    /// Sessão Wayland nativa.
    Wayland,
    /// Sessão Wayland rodando o aplicativo sob `XWayland` (`GDK_BACKEND=x11`).
    XWayland,
    /// Ambiente gráfico não identificado (ex.: headless/CI).
    Unknown,
}

/// Compositor ou ambiente de desktop identificado.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Compositor {
    /// KDE Plasma (`KWin`).
    KdePlasma,
    /// Hyprland.
    Hyprland,
    /// GNOME (`Mutter`).
    Gnome,
    /// Sway.
    Sway,
    /// Outro compositor ou gerenciador de janelas.
    Other,
}

/// Fotografia das capacidades do ambiente de sistema operacional detectadas em runtime.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OsEnvironment {
    /// Servidor de display (`x11`, `wayland`, `x_wayland` ou `unknown`).
    pub display_server: DisplayServer,
    /// Compositor identificado (`kde_plasma`, `hyprland`, `gnome`, `sway` ou `other`).
    pub compositor: Compositor,
    /// `true` se `org.kde.StatusNotifierWatcher` está ativo e o ícone da bandeja está operacional.
    pub tray_available: bool,
    /// `true` se o portal XDG expõe `org.freedesktop.portal.GlobalShortcuts`.
    pub portal_global_shortcuts: bool,
    /// Nome do servidor de notificações (`org.freedesktop.Notifications.GetServerInformation`).
    pub notification_server: Option<String>,
    /// `true` se o servidor de notificações anuncia a capacidade `"actions"`.
    pub notification_actions: bool,
    /// Disponibilidade de cada editor suportado no sistema.
    pub editors: Vec<EditorAvailability>,
    /// `true` quando o posicionamento absoluto da janela é suportado (`X11`).
    pub window_positioning: bool,
}

impl Default for OsEnvironment {
    fn default() -> Self {
        Self {
            display_server: DisplayServer::Unknown,
            compositor: Compositor::Other,
            tray_available: false,
            portal_global_shortcuts: false,
            notification_server: None,
            notification_actions: false,
            editors: Vec::new(),
            window_positioning: false,
        }
    }
}

/// Função pura: classifica `(DisplayServer, Compositor)` a partir de um leitor de variáveis de ambiente.
pub fn classify_display(env: &impl Fn(&str) -> Option<String>) -> (DisplayServer, Compositor) {
    let wayland_display = env("WAYLAND_DISPLAY")
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let gdk_backend = env("GDK_BACKEND")
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let xdg_session = env("XDG_SESSION_TYPE")
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let x11_display = env("DISPLAY")
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    let display_server = if wayland_display.is_some() || xdg_session == "wayland" {
        if gdk_backend == "x11" || gdk_backend.starts_with("x11,") {
            DisplayServer::XWayland
        } else {
            DisplayServer::Wayland
        }
    } else if xdg_session == "x11" || x11_display.is_some() {
        DisplayServer::X11
    } else {
        DisplayServer::Unknown
    };

    let hyprland_sig = env("HYPRLAND_INSTANCE_SIGNATURE")
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let swaysock = env("SWAYSOCK")
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let current_desktop = env("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_uppercase();
    let desktop_session = env("DESKTOP_SESSION")
        .unwrap_or_default()
        .to_ascii_uppercase();

    let compositor = if hyprland_sig.is_some()
        || current_desktop.contains("HYPRLAND")
        || desktop_session.contains("HYPRLAND")
    {
        Compositor::Hyprland
    } else if swaysock.is_some()
        || current_desktop.contains("SWAY")
        || desktop_session.contains("SWAY")
    {
        Compositor::Sway
    } else if current_desktop.contains("KDE")
        || current_desktop.contains("PLASMA")
        || desktop_session.contains("PLASMA")
        || desktop_session.contains("KDE")
    {
        Compositor::KdePlasma
    } else if current_desktop.contains("GNOME") || desktop_session.contains("GNOME") {
        Compositor::Gnome
    } else {
        Compositor::Other
    };

    (display_server, compositor)
}

/// Realiza a sondagem assíncrona do ambiente gráfico e barramentos D-Bus com timeout de 500 ms por consulta.
pub async fn probe(env: &impl Fn(&str) -> Option<String>) -> OsEnvironment {
    let (display_server, compositor) = classify_display(env);
    let window_positioning = display_server == DisplayServer::X11;

    let has_session_bus = env("DBUS_SESSION_BUS_ADDRESS").is_some_and(|s| !s.trim().is_empty());

    let (tray_watcher, portal_shortcuts, notif_info) = if has_session_bus {
        tokio::join!(
            probe_status_notifier_watcher(),
            probe_portal_global_shortcuts(),
            probe_notification_server()
        )
    } else {
        (false, false, (None, false))
    };

    let editors = EditorLauncher::detect();

    OsEnvironment {
        display_server,
        compositor,
        tray_available: tray_watcher,
        portal_global_shortcuts: portal_shortcuts,
        notification_server: notif_info.0,
        notification_actions: notif_info.1,
        editors,
        window_positioning,
    }
}

async fn run_gdbus_call(args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("gdbus");
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);

    let output = tokio::time::timeout(Duration::from_millis(500), cmd.output())
        .await
        .ok()?
        .ok()?;

    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

async fn probe_status_notifier_watcher() -> bool {
    let out = run_gdbus_call(&[
        "call",
        "--session",
        "--dest",
        "org.freedesktop.DBus",
        "--object-path",
        "/org/freedesktop/DBus",
        "--method",
        "org.freedesktop.DBus.NameHasOwner",
        "org.kde.StatusNotifierWatcher",
    ])
    .await;
    out.is_some_and(|s| s.contains("true"))
}

async fn probe_portal_global_shortcuts() -> bool {
    let out = run_gdbus_call(&[
        "call",
        "--session",
        "--dest",
        "org.freedesktop.portal.Desktop",
        "--object-path",
        "/org/freedesktop/portal/desktop",
        "--method",
        "org.freedesktop.DBus.Properties.Get",
        "org.freedesktop.portal.GlobalShortcuts",
        "version",
    ])
    .await;
    out.is_some()
}

async fn probe_notification_server() -> (Option<String>, bool) {
    let server_out = run_gdbus_call(&[
        "call",
        "--session",
        "--dest",
        "org.freedesktop.Notifications",
        "--object-path",
        "/org/freedesktop/Notifications",
        "--method",
        "org.freedesktop.Notifications.GetServerInformation",
    ])
    .await;

    let server_name = server_out.and_then(|raw| {
        // Ex.: "('Plasma', 'KDE', '6.2.0', '1.2')\n"
        let first_quote = raw.find('\'')?;
        let rest = &raw[first_quote + 1..];
        let end_quote = rest.find('\'')?;
        let name = rest[..end_quote].trim();
        if name.is_empty() {
            None
        } else {
            Some(name.to_owned())
        }
    });

    if server_name.is_none() {
        return (None, false);
    }

    let caps_out = run_gdbus_call(&[
        "call",
        "--session",
        "--dest",
        "org.freedesktop.Notifications",
        "--object-path",
        "/org/freedesktop/Notifications",
        "--method",
        "org.freedesktop.Notifications.GetCapabilities",
    ])
    .await;

    let has_actions =
        caps_out.is_some_and(|s| s.contains("'actions'") || s.contains("\"actions\""));
    (server_name, has_actions)
}
