//! Controle de visibilidade da janela e modo Quake / Drop-down (`decide_toggle`, `drop_down_geometry` e `QuakeController`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use mt_core::error::MtResult;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

use crate::environment::{DisplayServer, OsEnvironment};
use crate::settings::{OsSettings, QuakeMode};

/// Decisão pura de alternância de visibilidade da janela (`toggle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ToggleDecision {
    /// Exibe a janela (desminimizando se necessário) e solicita foco.
    ShowAndFocus,
    /// A janela já está visível mas sem foco; apenas traz para frente e foca.
    Focus,
    /// A janela está visível e focada; oculta a janela.
    Hide,
}

/// DTO simplificado retornado pelo comando IPC `os_window_toggle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ToggleDecisionDto {
    /// Janela foi exibida e focada.
    Show,
    /// Janela já visível recebeu foco.
    Focus,
    /// Janela foi ocultada.
    Hide,
}

impl From<ToggleDecision> for ToggleDecisionDto {
    fn from(value: ToggleDecision) -> Self {
        match value {
            ToggleDecision::ShowAndFocus => Self::Show,
            ToggleDecision::Focus => Self::Focus,
            ToggleDecision::Hide => Self::Hide,
        }
    }
}

/// Função pura: oculto ou minimizado → `ShowAndFocus`; visível sem foco → `Focus`; visível com foco → `Hide`.
#[must_use]
pub const fn decide_toggle(visible: bool, focused: bool, minimized: bool) -> ToggleDecision {
    if !visible || minimized {
        ToggleDecision::ShowAndFocus
    } else if !focused {
        ToggleDecision::Focus
    } else {
        ToggleDecision::Hide
    }
}

/// Geometria física calculada para o modo Drop-down (Quake).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DropDownGeometry {
    /// Coordenada física X do canto superior esquerdo.
    pub x: i32,
    /// Coordenada física Y do canto superior esquerdo.
    pub y: i32,
    /// Largura física em pixels.
    pub width: u32,
    /// Altura física em pixels.
    pub height: u32,
}

/// Função pura: calcula a geometria no topo do monitor com largura total e altura percentual (`20..=100`).
#[must_use]
pub fn drop_down_geometry(
    monitor_pos: (i32, i32),
    monitor_size: (u32, u32),
    height_pct: u8,
) -> DropDownGeometry {
    let clamped_pct = u64::from(height_pct.clamp(20, 100));
    let total_w = monitor_size.0.max(1);
    let total_h = u64::from(monitor_size.1.max(1));
    let calc_h = ((total_h * clamped_pct) / 100).max(1);
    let height = u32::try_from(calc_h).unwrap_or(monitor_size.1.max(1));

    DropDownGeometry {
        x: monitor_pos.0,
        y: monitor_pos.1,
        width: total_w,
        height,
    }
}

/// Origem do acionamento de `toggle` da janela.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ToggleSource {
    /// Atalho global via plugin nativo.
    Shortcut,
    /// Atalho global via portal XDG (`GlobalShortcuts`).
    Portal,
    /// Argumento de linha de comando (`maxterminal --toggle`).
    Cli,
    /// Item do menu da bandeja do sistema.
    Tray,
    /// Chamada IPC direta da interface.
    Ipc,
}

/// Estado de visibilidade da janela principal publicado em `os://window-visibility` e `OsRuntimeSnapshot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct WindowVisibilityPayload {
    /// `true` se a janela está visível.
    pub visible: bool,
    /// `true` se a janela detém o foco de entrada.
    pub focused: bool,
    /// `true` se a janela está minimizada.
    pub minimized: bool,
}

/// Abstração de controle da janela nativa implementada pelo adaptador Tauri no crate binário.
pub trait WindowHost: Send + Sync + 'static {
    /// Obtém o estado instantâneo `(visible, focused, minimized)` da janela principal.
    fn snapshot(&self) -> WindowVisibilityPayload;

    /// Retorna `(monitor_pos, monitor_size)` em pixels físicos do monitor ativo/primário.
    fn current_monitor_rect(&self) -> Option<((i32, i32), (u32, u32))>;

    /// Aplica a geometria de Drop-down e `always_on_top` quando suportado (X11).
    fn apply_drop_down_geometry(
        &self,
        geometry: DropDownGeometry,
        always_on_top: bool,
    ) -> MtResult<()>;

    /// Restaura o estado normal de janela (decorações, `always_on_top = false`).
    fn restore_normal_mode(&self) -> MtResult<()>;

    /// Desminimiza, exibe e foca a janela principal.
    fn show_and_focus(&self) -> MtResult<()>;

    /// Apenas desminimiza e foca a janela já visível.
    fn focus_only(&self) -> MtResult<()>;

    /// Oculta a janela (para a bandeja ou atalho) ou minimiza como fallback seguro.
    fn hide_window(&self, can_hide_to_tray_or_shortcut: bool) -> MtResult<()>;

    /// Minimiza explicitamente a janela na barra de tarefas.
    fn minimize_window(&self) -> MtResult<()>;

    /// Notifica mudança de visibilidade da janela para o barramento de eventos (`os://window-visibility`).
    fn on_visibility_changed(&self, payload: WindowVisibilityPayload);
}

/// Controlador de visibilidade e modo Quake / Drop-down da janela principal.
pub struct QuakeController {
    host: Arc<dyn WindowHost>,
    settings: Arc<RwLock<OsSettings>>,
    env: Arc<RwLock<OsEnvironment>>,
    last_shortcut_toggle: Mutex<Option<Instant>>,
    shortcut_usable: Arc<std::sync::atomic::AtomicBool>,
}

impl QuakeController {
    /// Janela de debounce para repetição de tecla em atalhos globais (150 ms).
    pub const SHORTCUT_DEBOUNCE: Duration = Duration::from_millis(150);

    /// Cria um novo `QuakeController`.
    #[must_use]
    pub fn new(
        host: Arc<dyn WindowHost>,
        settings: Arc<RwLock<OsSettings>>,
        env: Arc<RwLock<OsEnvironment>>,
        shortcut_usable: Arc<std::sync::atomic::AtomicBool>,
    ) -> Self {
        Self {
            host,
            settings,
            env,
            last_shortcut_toggle: Mutex::new(None),
            shortcut_usable,
        }
    }

    /// Retorna o snapshot atual de visibilidade da janela.
    #[must_use]
    pub fn visibility(&self) -> WindowVisibilityPayload {
        self.host.snapshot()
    }

    /// Alterna a visibilidade da janela (`ShowAndFocus`, `Focus` ou `Hide`).
    pub fn toggle(&self, source: ToggleSource) -> MtResult<ToggleDecision> {
        if matches!(source, ToggleSource::Shortcut | ToggleSource::Portal) {
            let now = Instant::now();
            let mut last = self.last_shortcut_toggle.lock();
            if let Some(prev) = *last
                && now.duration_since(prev) < Self::SHORTCUT_DEBOUNCE
            {
                let snap = self.host.snapshot();
                return Ok(if snap.visible {
                    ToggleDecision::Focus
                } else {
                    ToggleDecision::Hide
                });
            }
            *last = Some(now);
        }

        let snap = self.host.snapshot();
        let decision = decide_toggle(snap.visible, snap.focused, snap.minimized);
        match decision {
            ToggleDecision::ShowAndFocus => self.show_and_focus()?,
            ToggleDecision::Focus => {
                self.host.focus_only()?;
                self.host.on_visibility_changed(self.host.snapshot());
            }
            ToggleDecision::Hide => self.hide()?,
        }
        Ok(decision)
    }

    /// Exibe e foca a janela aplicando a geometria Drop-down antes de mostrar quando aplicável.
    pub fn show_and_focus(&self) -> MtResult<()> {
        self.reapply_geometry_if_needed()?;
        self.host.show_and_focus()?;
        self.host.on_visibility_changed(self.host.snapshot());
        Ok(())
    }

    /// Oculta a janela (ou minimiza caso não haja bandeja nem atalho global utilizável).
    pub fn hide(&self) -> MtResult<()> {
        let tray_ok = self.env.read().tray_available;
        let shortcut_ok = self
            .shortcut_usable
            .load(std::sync::atomic::Ordering::Acquire);
        self.host.hide_window(tray_ok || shortcut_ok)?;
        self.host.on_visibility_changed(self.host.snapshot());
        Ok(())
    }

    /// Minimiza a janela na barra de tarefas.
    pub fn minimize(&self) -> MtResult<()> {
        self.host.minimize_window()?;
        self.host.on_visibility_changed(self.host.snapshot());
        Ok(())
    }

    /// Reaplica a geometria da janela após mudança nas configurações de Quake.
    pub fn reapply_geometry_if_needed(&self) -> MtResult<()> {
        let quake_cfg = self.settings.read().quake.clone();
        let display_server = self.env.read().display_server;

        if quake_cfg.mode == QuakeMode::DropDown && display_server == DisplayServer::X11 {
            if let Some((pos, size)) = self.host.current_monitor_rect() {
                let geom = drop_down_geometry(pos, size, quake_cfg.drop_down_height_pct);
                self.host
                    .apply_drop_down_geometry(geom, quake_cfg.always_on_top)?;
            }
        } else if quake_cfg.mode == QuakeMode::Normal {
            let _ = self.host.restore_normal_mode();
        }
        Ok(())
    }

    /// Reage à perda de foco da janela (`hide_on_blur` após 120 ms se continuar sem foco no modo `DropDown`).
    pub fn on_blur(&self) {
        let quake_cfg = self.settings.read().quake.clone();
        if !quake_cfg.hide_on_blur || quake_cfg.mode != QuakeMode::DropDown {
            return;
        }
        let host = Arc::clone(&self.host);
        let env = Arc::clone(&self.env);
        let shortcut_usable = Arc::clone(&self.shortcut_usable);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            let snap = host.snapshot();
            if snap.visible && !snap.focused {
                let can_hide = env.read().tray_available
                    || shortcut_usable.load(std::sync::atomic::Ordering::Acquire);
                if host.hide_window(can_hide).is_ok() {
                    host.on_visibility_changed(host.snapshot());
                }
            }
        });
    }
}
