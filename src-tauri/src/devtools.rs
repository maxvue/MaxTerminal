//! Gerenciamento nativo de DevTools e persistência do posicionamento do Web Inspector.
//!
//! Este módulo gerencia o atalho F12 (para abrir ou trazer à frente o inspetor nativo)
//! e a persistência atômica da preferência de layout (`attached` ou `detached`) em
//! `devtools.json` dentro do diretório de configuração do aplicativo (`AppPaths.config_dir`).

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Nome do arquivo de preferências do DevTools.
pub const DEVTOOLS_CONFIG_FILENAME: &str = "devtools.json";

/// Limite máximo de leitura para o arquivo de configuração (4 KiB).
pub const MAX_CONFIG_FILE_SIZE: u64 = 4096;

/// Versão do schema de configuração suportada.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Posicionamento principal do painel do Web Inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InspectorPlacement {
    /// Acoplado na janela do aplicativo (inferior ou lateral).
    Attached,
    /// Destacado em uma janela separada e independente.
    Detached,
}

/// Lado de acoplamento quando o Web Inspector está acoplado (`Attached`) na janela principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AttachedDockSide {
    /// Acoplado na parte inferior da janela.
    #[default]
    Bottom,
    /// Acoplado na lateral direita da janela.
    Right,
    /// Acoplado na lateral esquerda da janela.
    Left,
}

impl AttachedDockSide {
    /// Retorna o identificador esperado por `InspectorFrontendHost.requestSetDockSide` no WebKit.
    #[must_use]
    pub const fn as_webkit_dock_side(self) -> &'static str {
        match self {
            Self::Bottom => "bottom",
            Self::Right => "right",
            Self::Left => "left",
        }
    }
}

/// Estrutura persistida em disco em `devtools.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevToolsConfig {
    /// Versão do schema da preferência.
    pub version: u32,
    /// Posicionamento selecionado pelo usuário (`attached` ou `detached`).
    pub placement: InspectorPlacement,
    /// Lado do acoplamento quando `placement == Attached` (`bottom`, `right` ou `left`).
    #[serde(default)]
    pub dock_side: AttachedDockSide,
}

/// Erros de I/O e serialização do DevTools.
#[derive(Debug, PartialEq, Eq)]
pub enum DevToolsIoError {
    /// Gravação ou leitura rejeitada por envolver link simbólico.
    SymlinkForbidden,
    /// Arquivo excede o tamanho máximo de 4 KiB.
    FileTooLarge,
    /// Versão do schema desconhecida ou não suportada.
    InvalidVersion(u32),
    /// Formato JSON inválido ou incompatível.
    InvalidFormat,
    /// Falha na serialização do objeto em JSON.
    Serialization,
    /// Erro de I/O do sistema de arquivos com código sanitizado.
    Io(std::io::ErrorKind),
}

/// Lê a configuração completa do DevTools armazenada em disco (`devtools.json`).
///
/// Não segue links simbólicos e rejeita arquivos maiores que 4 KiB.
/// Em caso de arquivo ausente, corrompido ou erro de I/O, retorna `None`
/// com log sanitizado, degradando graciosamente para o comportamento nativo.
#[must_use]
pub fn load_config(config_dir: &Path) -> Option<DevToolsConfig> {
    let file_path = config_dir.join(DEVTOOLS_CONFIG_FILENAME);

    // 1. Verifica metadados sem seguir symlinks
    match std::fs::symlink_metadata(&file_path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                tracing::warn!(
                    target: "maxterminal::devtools",
                    "Arquivo devtools.json é um link simbólico; ignorando"
                );
                return None;
            }
            if meta.len() > MAX_CONFIG_FILE_SIZE {
                tracing::warn!(
                    target: "maxterminal::devtools",
                    file_size = meta.len(),
                    "Arquivo devtools.json excede o limite de tamanho (4 KiB); ignorando"
                );
                return None;
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return None;
        }
        Err(err) => {
            tracing::warn!(
                target: "maxterminal::devtools",
                error_kind = ?err.kind(),
                "Falha ao inspecionar metadados de devtools.json"
            );
            return None;
        }
    }

    // 2. Lê os bytes do arquivo
    let data = match std::fs::read(&file_path) {
        Ok(d) => d,
        Err(err) => {
            tracing::warn!(
                target: "maxterminal::devtools",
                error_kind = ?err.kind(),
                "Falha ao ler conteúdo de devtools.json"
            );
            return None;
        }
    };

    if data.len() as u64 > MAX_CONFIG_FILE_SIZE {
        tracing::warn!(
            target: "maxterminal::devtools",
            "Conteúdo de devtools.json excede 4 KiB"
        );
        return None;
    }

    // 3. Desserializa e valida o schema
    match serde_json::from_slice::<DevToolsConfig>(&data) {
        Ok(cfg) => {
            if cfg.version != CURRENT_SCHEMA_VERSION {
                tracing::warn!(
                    target: "maxterminal::devtools",
                    version = cfg.version,
                    "Versão de schema não suportada em devtools.json"
                );
                return None;
            }
            Some(cfg)
        }
        Err(_) => {
            tracing::warn!(
                target: "maxterminal::devtools",
                "Estrutura JSON inválida ou desconhecida em devtools.json"
            );
            None
        }
    }
}

/// Lê a preferência de posicionamento do DevTools armazenada em disco.
///
/// Não segue links simbólicos e rejeita arquivos maiores que 4 KiB.
/// Em caso de arquivo ausente, corrompido ou erro de I/O, retorna `None`
/// com log sanitizado, degradando graciosamente para o comportamento nativo.
#[must_use]
pub fn load_preference(config_dir: &Path) -> Option<InspectorPlacement> {
    load_config(config_dir).map(|cfg| cfg.placement)
}

/// Grava atomicamente a configuração de posicionamento e lado de acoplamento do DevTools em disco.
///
/// Grava primeiro em um arquivo temporário no mesmo diretório com permissões
/// privadas `0o600` em sistemas Unix, sincroniza os dados e renomeia atomicamente.
/// Recusa-se terminantemente a sobrescrever ou seguir links simbólicos.
///
/// # Errors
///
/// Retorna `DevToolsIoError` em caso de falha de serialização, I/O ou symlink.
pub fn save_config(
    config_dir: &Path,
    placement: InspectorPlacement,
    dock_side: AttachedDockSide,
) -> Result<(), DevToolsIoError> {
    if let Err(e) = std::fs::create_dir_all(config_dir) {
        return Err(DevToolsIoError::Io(e.kind()));
    }

    let file_path = config_dir.join(DEVTOOLS_CONFIG_FILENAME);

    // Se o destino existir e for symlink, recusa a operação
    if let Ok(meta) = std::fs::symlink_metadata(&file_path) {
        if meta.file_type().is_symlink() {
            return Err(DevToolsIoError::SymlinkForbidden);
        }
    }

    let config = DevToolsConfig {
        version: CURRENT_SCHEMA_VERSION,
        placement,
        dock_side,
    };
    let json_bytes =
        serde_json::to_vec_pretty(&config).map_err(|_| DevToolsIoError::Serialization)?;

    let rand_suffix: u64 = rand::random();
    let temp_file_path = config_dir.join(format!(".devtools.json.tmp.{rand_suffix}"));

    let mut open_options = std::fs::OpenOptions::new();
    open_options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open_options.mode(0o600);
    }

    let mut temp_file = match open_options.open(&temp_file_path) {
        Ok(f) => f,
        Err(e) => return Err(DevToolsIoError::Io(e.kind())),
    };

    use std::io::Write;
    if let Err(e) = temp_file.write_all(&json_bytes) {
        let _ = std::fs::remove_file(&temp_file_path);
        return Err(DevToolsIoError::Io(e.kind()));
    }
    if let Err(e) = temp_file.sync_all() {
        let _ = std::fs::remove_file(&temp_file_path);
        return Err(DevToolsIoError::Io(e.kind()));
    }
    drop(temp_file);

    // Validação estrita antes do renomeio
    if let Ok(meta) = std::fs::symlink_metadata(&file_path) {
        if meta.file_type().is_symlink() {
            let _ = std::fs::remove_file(&temp_file_path);
            return Err(DevToolsIoError::SymlinkForbidden);
        }
    }

    if let Err(e) = std::fs::rename(&temp_file_path, &file_path) {
        let _ = std::fs::remove_file(&temp_file_path);
        return Err(DevToolsIoError::Io(e.kind()));
    }

    Ok(())
}

/// Grava atomicamente a preferência de posicionamento do DevTools em disco,
/// preservando o lado de acoplamento salvo anteriormente (ou usando `Bottom` como padrão).
///
/// # Errors
///
/// Retorna `DevToolsIoError` em caso de falha de serialização, I/O ou symlink.
pub fn save_preference(
    config_dir: &Path,
    placement: InspectorPlacement,
) -> Result<(), DevToolsIoError> {
    let existing_side = load_config(config_dir)
        .map(|cfg| cfg.dock_side)
        .unwrap_or_default();
    save_config(config_dir, placement, existing_side)
}

/// Infere o lado em que o Web Inspector está acoplado a partir das alocações GTK
/// da `WebView` principal e da `WebView` interna do inspetor.
#[must_use]
pub fn infer_attached_dock_side(
    main_width: i32,
    main_height: i32,
    inspector_x: i32,
    inspector_y: i32,
    inspector_width: i32,
    inspector_height: i32,
) -> Option<AttachedDockSide> {
    if main_width <= 1 || main_height <= 1 || inspector_width <= 1 || inspector_height <= 1 {
        return None;
    }

    if inspector_x > 0 && inspector_width < main_width {
        return Some(AttachedDockSide::Right);
    }

    if inspector_y > 0 && inspector_height < main_height {
        return Some(AttachedDockSide::Bottom);
    }

    if inspector_width < main_width && inspector_height >= main_height / 2 {
        return Some(AttachedDockSide::Left);
    }

    if inspector_height < main_height && inspector_width >= main_width / 2 {
        return Some(AttachedDockSide::Bottom);
    }

    None
}

/// Ações puras decididas pela máquina de estados do DevTools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateMachineAction {
    /// Abrir ou trazer à frente o inspetor nativo.
    ShowOrBringToFront,
    /// Aplicar posicionamento específico no inspetor nativo.
    ApplyPlacement(InspectorPlacement),
    /// Persistir nova preferência do usuário em disco.
    PersistPreference(InspectorPlacement),
}

/// Decisão de consumo e ação para eventos de tecla.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyHandling {
    /// Consumir o evento no nível GTK para não propagar para widgets filhos ou PTY.
    Consume(Option<StateMachineAction>),
    /// Permitir propagação normal do evento.
    PassThrough,
}

/// Máquina de estados pura para controle do ciclo de vida e posicionamento do DevTools.
#[derive(Debug, Clone)]
pub struct DevToolsStateMachine {
    persisted_preference: Option<InspectorPlacement>,
    persisted_dock_side: AttachedDockSide,
    current_mode: Option<InspectorPlacement>,
    current_dock_side: AttachedDockSide,
    is_open: bool,
    is_restoring: bool,
    pending_dock_side_restore: Option<AttachedDockSide>,
    f12_key_down: bool,
    can_attach: bool,
}

impl DevToolsStateMachine {
    /// Cria uma nova máquina de estados com a preferência inicial e capacidade de acoplamento.
    #[must_use]
    pub fn new(initial_preference: Option<InspectorPlacement>, can_attach: bool) -> Self {
        Self {
            persisted_preference: initial_preference,
            persisted_dock_side: AttachedDockSide::Bottom,
            current_mode: None,
            current_dock_side: AttachedDockSide::Bottom,
            is_open: false,
            is_restoring: false,
            pending_dock_side_restore: None,
            f12_key_down: false,
            can_attach,
        }
    }

    /// Cria uma nova máquina de estados a partir da configuração completa carregada do disco.
    #[must_use]
    pub fn with_config(initial_config: Option<DevToolsConfig>, can_attach: bool) -> Self {
        let (persisted_preference, persisted_dock_side) = match initial_config {
            Some(cfg) => (Some(cfg.placement), cfg.dock_side),
            None => (None, AttachedDockSide::Bottom),
        };
        Self {
            persisted_preference,
            persisted_dock_side,
            current_mode: None,
            current_dock_side: persisted_dock_side,
            is_open: false,
            is_restoring: false,
            pending_dock_side_restore: None,
            f12_key_down: false,
            can_attach,
        }
    }

    /// Retorna a preferência persistida atual.
    #[must_use]
    pub fn persisted_preference(&self) -> Option<InspectorPlacement> {
        self.persisted_preference
    }

    /// Retorna o lado de acoplamento persistido atual (`Bottom`, `Right` ou `Left`).
    #[must_use]
    pub fn persisted_dock_side(&self) -> AttachedDockSide {
        self.persisted_dock_side
    }

    /// Retorna o modo de exibição ativo no momento (`None` se fechado).
    #[must_use]
    pub fn current_mode(&self) -> Option<InspectorPlacement> {
        self.current_mode
    }

    /// Retorna o lado de acoplamento ativo no momento.
    #[must_use]
    pub fn current_dock_side(&self) -> AttachedDockSide {
        self.current_dock_side
    }

    /// Retorna o lado de acoplamento pendente de restauração assíncrona.
    #[must_use]
    pub fn pending_dock_side_restore(&self) -> Option<AttachedDockSide> {
        self.pending_dock_side_restore
    }

    /// Define o lado de acoplamento pendente de restauração assíncrona.
    pub fn set_pending_dock_side_restore(&mut self, side: Option<AttachedDockSide>) {
        self.pending_dock_side_restore = side;
    }

    /// Retorna se o Web Inspector está atualmente aberto.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// Retorna se está ocorrendo restauração programática de layout.
    #[must_use]
    pub fn is_restoring(&self) -> bool {
        self.is_restoring
    }

    /// Define o estado de restauração programática.
    pub fn set_restoring(&mut self, restoring: bool) {
        self.is_restoring = restoring;
    }

    /// Define se o acoplamento é suportado pelo ambiente atual.
    pub fn set_can_attach(&mut self, can_attach: bool) {
        self.can_attach = can_attach;
    }

    /// Trata evento de tecla pressionada (`key-press-event`).
    ///
    /// Consome somente F12 simples (sem Shift, Ctrl, Alt ou Super).
    /// CapsLock e NumLock são tolerados sem interferir.
    /// Suprime repetição automática quando a tecla já estiver pressionada.
    pub fn on_key_press(&mut self, keyval: u32, relevant_modifiers: bool) -> KeyHandling {
        // GDK_KEY_F12 = 0xffc9 (65481)
        const GDK_KEY_F12: u32 = 0xffc9;

        if keyval != GDK_KEY_F12 {
            return KeyHandling::PassThrough;
        }

        // Modificadores ativos (Ctrl, Shift, Alt, Super): deixa passar intacto
        if relevant_modifiers {
            return KeyHandling::PassThrough;
        }

        // F12 simples: suprime repetição enquanto a tecla estiver fisicamente pressionada
        if self.f12_key_down {
            return KeyHandling::Consume(None);
        }

        self.f12_key_down = true;
        KeyHandling::Consume(Some(StateMachineAction::ShowOrBringToFront))
    }

    /// Trata evento de tecla liberada (`key-release-event`).
    pub fn on_key_release(&mut self, keyval: u32) -> KeyHandling {
        const GDK_KEY_F12: u32 = 0xffc9;
        if keyval == GDK_KEY_F12 {
            self.f12_key_down = false;
            return KeyHandling::Consume(None);
        }
        KeyHandling::PassThrough
    }

    /// Trata perda de foco da janela (`focus-out-event`).
    pub fn on_focus_out(&mut self) {
        self.f12_key_down = false;
    }

    /// Acionado no início do ciclo de abertura (por F12 ou menu contextual).
    ///
    /// Se o painel já estiver aberto, não altera a posição nem aplica restauração.
    /// Se estiver abrindo vindo de estado fechado, restaura a preferência salva.
    pub fn on_opening(&mut self) -> Option<StateMachineAction> {
        let was_open = self.is_open;
        self.is_open = true;

        if was_open {
            return None;
        }

        match self.persisted_preference {
            Some(InspectorPlacement::Attached) => {
                if self.can_attach {
                    self.current_mode = Some(InspectorPlacement::Attached);
                    self.current_dock_side = self.persisted_dock_side;
                    if self.persisted_dock_side == AttachedDockSide::Bottom {
                        self.pending_dock_side_restore = None;
                    } else {
                        self.pending_dock_side_restore = Some(self.persisted_dock_side);
                    }
                    Some(StateMachineAction::ApplyPlacement(
                        InspectorPlacement::Attached,
                    ))
                } else {
                    // Fallback nativo se acoplamento indisponível, preservando a preferência
                    self.pending_dock_side_restore = None;
                    None
                }
            }
            Some(InspectorPlacement::Detached) => {
                self.current_mode = Some(InspectorPlacement::Detached);
                self.pending_dock_side_restore = None;
                Some(StateMachineAction::ApplyPlacement(
                    InspectorPlacement::Detached,
                ))
            }
            None => {
                self.pending_dock_side_restore = None;
                None
            }
        }
    }

    /// Acionado quando o sinal `attach` é emitido pelo WebKit.
    pub fn on_signal_attach(&mut self) -> Option<StateMachineAction> {
        self.is_open = true;
        self.current_mode = Some(InspectorPlacement::Attached);

        if self.is_restoring {
            return None;
        }

        if self.persisted_preference != Some(InspectorPlacement::Attached) {
            self.persisted_preference = Some(InspectorPlacement::Attached);
            return Some(StateMachineAction::PersistPreference(
                InspectorPlacement::Attached,
            ));
        }

        None
    }

    /// Acionado quando o layout GTK do inspetor acoplado observa um lado específico (`Bottom`, `Right` ou `Left`).
    ///
    /// Retorna `Some((InspectorPlacement::Attached, side))` quando o lado ou preferência
    /// mudou por ação do usuário e deve ser persistido em disco.
    pub fn on_attached_dock_side_observed(
        &mut self,
        side: AttachedDockSide,
    ) -> Option<(InspectorPlacement, AttachedDockSide)> {
        if !self.is_open || self.current_mode != Some(InspectorPlacement::Attached) {
            return None;
        }

        if self.is_restoring {
            return None;
        }

        if let Some(target_side) = self.pending_dock_side_restore {
            if side == target_side {
                self.pending_dock_side_restore = None;
                self.current_dock_side = side;
            }
            return None;
        }

        self.current_dock_side = side;
        if self.persisted_preference != Some(InspectorPlacement::Attached)
            || self.persisted_dock_side != side
        {
            self.persisted_preference = Some(InspectorPlacement::Attached);
            self.persisted_dock_side = side;
            return Some((InspectorPlacement::Attached, side));
        }

        None
    }

    /// Acionado quando o sinal `detach` é emitido pelo WebKit.
    ///
    /// Importante: `detach` NÃO salva `detached`, pois também ocorre durante
    /// o encerramento do inspetor antes de `closed`.
    pub fn on_signal_detach(&mut self) {
        self.current_mode = Some(InspectorPlacement::Detached);
    }

    /// Acionado quando a janela separada é confirmada pelo sinal `open-window`.
    pub fn on_signal_open_window(&mut self) -> Option<StateMachineAction> {
        self.is_open = true;
        self.current_mode = Some(InspectorPlacement::Detached);
        self.pending_dock_side_restore = None;

        if self.is_restoring {
            return None;
        }

        if self.persisted_preference != Some(InspectorPlacement::Detached) {
            self.persisted_preference = Some(InspectorPlacement::Detached);
            return Some(StateMachineAction::PersistPreference(
                InspectorPlacement::Detached,
            ));
        }

        None
    }

    /// Acionado quando o sinal `closed` é emitido pelo WebKit.
    ///
    /// Preserva rigorosamente a preferência salva; não grava nada em disco.
    pub fn on_signal_closed(&mut self) {
        self.is_open = false;
        self.current_mode = None;
        self.is_restoring = false;
        self.pending_dock_side_restore = None;
    }
}

/// Dispara a gravação assíncrona da configuração do DevTools em background.
#[cfg(all(target_os = "linux", debug_assertions))]
fn persist_config_async(
    config_dir: &Path,
    placement: InspectorPlacement,
    dock_side: AttachedDockSide,
    context: &'static str,
) {
    let cfg_dir = config_dir.to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(err) = save_config(&cfg_dir, placement, dock_side) {
            tracing::warn!(
                target: "maxterminal::devtools",
                ?err,
                context,
                "Falha ao persistir configuração de posicionamento do DevTools"
            );
        }
    });
}

/// Solicita à `WebView` interna do Web Inspector que altere o lado de acoplamento (`bottom`, `right`, `left`).
#[cfg(all(target_os = "linux", debug_assertions))]
fn request_inspector_dock_side(
    inspector_webview: &webkit2gtk::WebViewBase,
    side: AttachedDockSide,
) {
    use glib::prelude::Cast;
    use webkit2gtk::WebViewExt;

    let Some(webview) = inspector_webview.downcast_ref::<webkit2gtk::WebView>() else {
        return;
    };

    let side_str = side.as_webkit_dock_side();
    let script = format!(
        "(function(side) {{\
            function apply() {{\
                if (window.InspectorFrontendHost && typeof window.InspectorFrontendHost.requestSetDockSide === 'function') {{\
                    window.InspectorFrontendHost.requestSetDockSide(side);\
                    return true;\
                }}\
                return false;\
            }}\
            if (!apply()) {{\
                window.addEventListener('DOMContentLoaded', apply, {{ once: true }});\
                window.addEventListener('load', apply, {{ once: true }});\
                setTimeout(apply, 25);\
                setTimeout(apply, 100);\
            }}\
        }})('{side_str}');"
    );

    webview.evaluate_javascript(
        &script,
        None,
        None,
        webkit2gtk::gio::Cancellable::NONE,
        |_| {},
    );
}

/// Registra os interceptadores de teclado (`F12`) em um widget GTK.
#[cfg(all(target_os = "linux", debug_assertions))]
fn bind_f12_handlers<W: gtk::prelude::WidgetExt>(
    widget: &W,
    inspector: &webkit2gtk::WebInspector,
    state: &std::rc::Rc<std::cell::RefCell<DevToolsStateMachine>>,
) {
    use webkit2gtk::WebInspectorExt;

    let state_press = std::rc::Rc::clone(state);
    let inspector_press = inspector.clone();
    widget.connect_key_press_event(move |_widget, event| {
        let keyval = *event.keyval();
        let state_mask = event.state();
        let relevant_modifiers = state_mask.intersects(
            gdk::ModifierType::SHIFT_MASK
                | gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::MOD1_MASK
                | gdk::ModifierType::MOD4_MASK,
        );

        let handling = state_press
            .borrow_mut()
            .on_key_press(keyval, relevant_modifiers);
        match handling {
            KeyHandling::PassThrough => glib::Propagation::Proceed,
            KeyHandling::Consume(None) => glib::Propagation::Stop,
            KeyHandling::Consume(Some(StateMachineAction::ShowOrBringToFront)) => {
                inspector_press.show();
                glib::Propagation::Stop
            }
            KeyHandling::Consume(Some(_)) => glib::Propagation::Stop,
        }
    });

    let state_release = std::rc::Rc::clone(state);
    widget.connect_key_release_event(move |_widget, event| {
        let keyval = *event.keyval();
        let handling = state_release.borrow_mut().on_key_release(keyval);
        match handling {
            KeyHandling::PassThrough => glib::Propagation::Proceed,
            KeyHandling::Consume(_) => glib::Propagation::Stop,
        }
    });

    let state_focus = std::rc::Rc::clone(state);
    widget.connect_focus_out_event(move |_widget, _event| {
        state_focus.borrow_mut().on_focus_out();
        glib::Propagation::Proceed
    });
}

/// Inspeciona as alocações atuais da `WebView` principal e da `WebView` do inspetor
/// para detectar mudanças de lado (`Bottom`, `Right`, `Left`) ou concluir restauração pendente.
#[cfg(all(target_os = "linux", debug_assertions))]
fn sync_attached_dock_side_from_allocations(
    main_webview: &webkit2gtk::WebView,
    inspector: &webkit2gtk::WebInspector,
    state: &std::rc::Rc<std::cell::RefCell<DevToolsStateMachine>>,
    config_dir: &Path,
) {
    use gtk::prelude::WidgetExt;
    use webkit2gtk::WebInspectorExt;

    if !inspector.is_attached() {
        return;
    }

    let Some(inspector_wv) = inspector.web_view() else {
        return;
    };

    let main_alloc = main_webview.allocation();
    let insp_alloc = inspector_wv.allocation();

    let Some(observed_side) = infer_attached_dock_side(
        main_alloc.width(),
        main_alloc.height(),
        insp_alloc.x(),
        insp_alloc.y(),
        insp_alloc.width(),
        insp_alloc.height(),
    ) else {
        return;
    };

    let pending_target = state.borrow().pending_dock_side_restore();
    if let Some(target_side) = pending_target {
        if observed_side != target_side {
            request_inspector_dock_side(&inspector_wv, target_side);
            return;
        }
    }

    let persist_update = state
        .borrow_mut()
        .on_attached_dock_side_observed(observed_side);
    if let Some((placement, dock_side)) = persist_update {
        persist_config_async(config_dir, placement, dock_side, "size-allocate");
    }
}

/// Garante que a `WebView` interna do Web Inspector (e sua janela quando destacada)
/// possua os hooks de atalho `F12`, `size-allocate` e `load-changed`.
#[cfg(all(target_os = "linux", debug_assertions))]
fn ensure_inspector_webview_hooks(
    main_webview: &webkit2gtk::WebView,
    inspector: &webkit2gtk::WebInspector,
    state: &std::rc::Rc<std::cell::RefCell<DevToolsStateMachine>>,
    hooked_inspector_wv_id: &std::rc::Rc<std::cell::RefCell<Option<usize>>>,
    hooked_inspector_toplevel_id: &std::rc::Rc<std::cell::RefCell<Option<usize>>>,
    config_dir: &Path,
) {
    use glib::ObjectType;
    use glib::prelude::Cast;
    use gtk::prelude::WidgetExt;
    use webkit2gtk::WebInspectorExt;
    use webkit2gtk::WebViewExt;

    let Some(inspector_wv) = inspector.web_view() else {
        return;
    };

    let wv_id = inspector_wv.as_ptr() as usize;
    let is_new_wv = hooked_inspector_wv_id.borrow().as_ref().copied() != Some(wv_id);

    if is_new_wv {
        *hooked_inspector_wv_id.borrow_mut() = Some(wv_id);

        bind_f12_handlers(&inspector_wv, inspector, state);

        let main_wv_alloc = main_webview.clone();
        let inspector_alloc = inspector.clone();
        let state_alloc = std::rc::Rc::clone(state);
        let config_dir_alloc = config_dir.to_path_buf();
        inspector_wv.connect_size_allocate(move |_wv, _alloc| {
            sync_attached_dock_side_from_allocations(
                &main_wv_alloc,
                &inspector_alloc,
                &state_alloc,
                &config_dir_alloc,
            );
        });

        if let Some(inspector_webview_full) = inspector_wv.downcast_ref::<webkit2gtk::WebView>() {
            let state_load = std::rc::Rc::clone(state);
            inspector_webview_full.connect_load_changed(move |wv, load_event| {
                if matches!(
                    load_event,
                    webkit2gtk::LoadEvent::Committed | webkit2gtk::LoadEvent::Finished
                ) {
                    let pending = state_load.borrow().pending_dock_side_restore();
                    if let Some(target_side) = pending {
                        request_inspector_dock_side(
                            wv.upcast_ref::<webkit2gtk::WebViewBase>(),
                            target_side,
                        );
                    }
                }
            });
        }
    }

    if let Some(insp_toplevel) = inspector_wv.toplevel() {
        let top_id = insp_toplevel.as_ptr() as usize;
        let main_top_id = main_webview
            .toplevel()
            .map(|t| t.as_ptr() as usize)
            .unwrap_or(0);
        if top_id != main_top_id
            && hooked_inspector_toplevel_id.borrow().as_ref().copied() != Some(top_id)
        {
            *hooked_inspector_toplevel_id.borrow_mut() = Some(top_id);
            bind_f12_handlers(&insp_toplevel, inspector, state);
        }
    }
}

/// Executa a restauração da preferência salva quando o Web Inspector abre vindo do estado fechado.
///
/// Retorna `true` se o inspetor estava fechado e acabou de iniciar o ciclo de abertura.
#[cfg(all(target_os = "linux", debug_assertions))]
fn handle_opening_transition_if_closed(
    main_webview: &webkit2gtk::WebView,
    inspector: &webkit2gtk::WebInspector,
    state: &std::rc::Rc<std::cell::RefCell<DevToolsStateMachine>>,
    hooked_inspector_wv_id: &std::rc::Rc<std::cell::RefCell<Option<usize>>>,
    hooked_inspector_toplevel_id: &std::rc::Rc<std::cell::RefCell<Option<usize>>>,
    config_dir: &Path,
) -> bool {
    use webkit2gtk::WebInspectorExt;

    if state.borrow().is_open() {
        return false;
    }

    let restore_action = state.borrow_mut().on_opening();
    ensure_inspector_webview_hooks(
        main_webview,
        inspector,
        state,
        hooked_inspector_wv_id,
        hooked_inspector_toplevel_id,
        config_dir,
    );

    if let Some(StateMachineAction::ApplyPlacement(placement)) = restore_action {
        match placement {
            InspectorPlacement::Attached => {
                if !inspector.is_attached() {
                    state.borrow_mut().set_restoring(true);
                    inspector.attach();
                    state.borrow_mut().set_restoring(false);
                }

                let target_side = state.borrow().persisted_dock_side();
                if target_side != AttachedDockSide::Bottom {
                    if let Some(inspector_wv) = inspector.web_view() {
                        request_inspector_dock_side(&inspector_wv, target_side);
                    }
                }
            }
            InspectorPlacement::Detached => {
                if inspector.is_attached() {
                    state.borrow_mut().set_restoring(true);
                    inspector.detach();
                    state.borrow_mut().set_restoring(false);
                }
            }
        }
    }

    true
}

/// Configura o adaptador nativo do DevTools para a janela principal em Linux.
#[cfg(all(target_os = "linux", debug_assertions))]
pub fn setup_devtools(window: &tauri::WebviewWindow, config_dir: &Path) {
    use glib::ObjectType;
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use webkit2gtk::SettingsExt;
    use webkit2gtk::WebInspectorExt;
    use webkit2gtk::WebViewExt;

    let config_dir = config_dir.to_path_buf();
    let initial_cfg = load_config(&config_dir);
    let window_for_gtk = window.clone();

    let _ = window.with_webview(move |platform_webview| {
        let webview = platform_webview.inner();
        let gtk_window_opt = window_for_gtk.gtk_window().ok();

        // Garante que `developer-extras` esteja habilitado na WebView para que `inspector.show()` funcione
        if let Some(settings) = WebViewExt::settings(&webview) {
            settings.set_enable_developer_extras(true);
        }

        let Some(inspector) = webview.inspector() else {
            tracing::info!(
                target: "maxterminal::devtools",
                "WebInspector não disponível na WebView atual"
            );
            return;
        };

        let can_attach = inspector.can_attach();
        let state = Rc::new(RefCell::new(DevToolsStateMachine::with_config(
            initial_cfg,
            can_attach,
        )));
        let hooked_inspector_wv_id: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));
        let hooked_inspector_toplevel_id: Rc<RefCell<Option<usize>>> = Rc::new(RefCell::new(None));

        // 1. Conecta interceptação de F12 diretamente na WebView (antes do handler nativo do WebKit)
        // e também na janela GTK de nível superior.
        bind_f12_handlers(&webview, &inspector, &state);

        let mut registered_toplevel_ptr: Option<usize> = None;
        if let Some(ref gtk_win) = gtk_window_opt {
            registered_toplevel_ptr = Some(gtk_win.as_ptr() as usize);
            bind_f12_handlers(gtk_win, &inspector, &state);
        }
        if let Some(toplevel) = webview.toplevel() {
            let top_ptr = toplevel.as_ptr() as usize;
            if registered_toplevel_ptr != Some(top_ptr) {
                bind_f12_handlers(&toplevel, &inspector, &state);
            }
        }

        // Observa `size-allocate` também na WebView principal para sincronizar o lado do dock acoplado
        {
            let main_wv = webview.clone();
            let insp_for_main_alloc = inspector.clone();
            let state_for_main_alloc = Rc::clone(&state);
            let cfg_for_main_alloc = config_dir.clone();
            webview.connect_size_allocate(move |_wv, _alloc| {
                sync_attached_dock_side_from_allocations(
                    &main_wv,
                    &insp_for_main_alloc,
                    &state_for_main_alloc,
                    &cfg_for_main_alloc,
                );
            });
        }

        // 2. Conecta observadores dos sinais do WebInspector
        {
            let main_wv = webview.clone();
            let state_front = Rc::clone(&state);
            let inspector_front = inspector.clone();
            let hooked_wv_front = Rc::clone(&hooked_inspector_wv_id);
            let hooked_top_front = Rc::clone(&hooked_inspector_toplevel_id);
            let cfg_front = config_dir.clone();
            inspector.connect_bring_to_front(move |_insp| {
                let _ = handle_opening_transition_if_closed(
                    &main_wv,
                    &inspector_front,
                    &state_front,
                    &hooked_wv_front,
                    &hooked_top_front,
                    &cfg_front,
                );
                ensure_inspector_webview_hooks(
                    &main_wv,
                    &inspector_front,
                    &state_front,
                    &hooked_wv_front,
                    &hooked_top_front,
                    &cfg_front,
                );
                false
            });
        }

        {
            let main_wv = webview.clone();
            let state_attach = Rc::clone(&state);
            let inspector_attach = inspector.clone();
            let hooked_wv_attach = Rc::clone(&hooked_inspector_wv_id);
            let hooked_top_attach = Rc::clone(&hooked_inspector_toplevel_id);
            let config_dir_attach = config_dir.clone();
            inspector.connect_attach(move |_insp| {
                let opened_from_closed = handle_opening_transition_if_closed(
                    &main_wv,
                    &inspector_attach,
                    &state_attach,
                    &hooked_wv_attach,
                    &hooked_top_attach,
                    &config_dir_attach,
                );
                ensure_inspector_webview_hooks(
                    &main_wv,
                    &inspector_attach,
                    &state_attach,
                    &hooked_wv_attach,
                    &hooked_top_attach,
                    &config_dir_attach,
                );

                // Se foi a abertura inicial e a preferência salva era Detached, `handle_opening_transition_if_closed`
                // já disparou `inspector.detach()`, portanto não deve tratar este `attach` como escolha do usuário.
                if opened_from_closed
                    && state_attach.borrow().persisted_preference()
                        == Some(InspectorPlacement::Detached)
                {
                    return false;
                }

                let action = state_attach.borrow_mut().on_signal_attach();
                if let Some(StateMachineAction::PersistPreference(placement)) = action {
                    let dock_side = state_attach.borrow().persisted_dock_side();
                    persist_config_async(&config_dir_attach, placement, dock_side, "attach");
                }

                let main_wv_idle = main_wv.clone();
                let inspector_idle = inspector_attach.clone();
                let state_idle = Rc::clone(&state_attach);
                let cfg_idle = config_dir_attach.clone();
                glib::idle_add_local_once(move || {
                    sync_attached_dock_side_from_allocations(
                        &main_wv_idle,
                        &inspector_idle,
                        &state_idle,
                        &cfg_idle,
                    );
                });

                false
            });
        }

        {
            let state_detach = Rc::clone(&state);
            inspector.connect_detach(move |_insp| {
                state_detach.borrow_mut().on_signal_detach();
                false
            });
        }

        {
            let main_wv = webview.clone();
            let state_open_win = Rc::clone(&state);
            let inspector_open_win = inspector.clone();
            let hooked_wv_open_win = Rc::clone(&hooked_inspector_wv_id);
            let hooked_top_open_win = Rc::clone(&hooked_inspector_toplevel_id);
            let config_dir_open_win = config_dir.clone();
            inspector.connect_open_window(move |_insp| {
                let opened_from_closed = handle_opening_transition_if_closed(
                    &main_wv,
                    &inspector_open_win,
                    &state_open_win,
                    &hooked_wv_open_win,
                    &hooked_top_open_win,
                    &config_dir_open_win,
                );
                ensure_inspector_webview_hooks(
                    &main_wv,
                    &inspector_open_win,
                    &state_open_win,
                    &hooked_wv_open_win,
                    &hooked_top_open_win,
                    &config_dir_open_win,
                );

                if opened_from_closed
                    && state_open_win.borrow().persisted_preference()
                        == Some(InspectorPlacement::Attached)
                {
                    return false;
                }

                let action = state_open_win.borrow_mut().on_signal_open_window();
                if let Some(StateMachineAction::PersistPreference(placement)) = action {
                    let dock_side = state_open_win.borrow().persisted_dock_side();
                    persist_config_async(&config_dir_open_win, placement, dock_side, "open-window");
                }
                false
            });
        }

        {
            let state_closed = Rc::clone(&state);
            let hooked_wv_closed = Rc::clone(&hooked_inspector_wv_id);
            let hooked_top_closed = Rc::clone(&hooked_inspector_toplevel_id);
            inspector.connect_closed(move |_insp| {
                state_closed.borrow_mut().on_signal_closed();
                *hooked_wv_closed.borrow_mut() = None;
                *hooked_top_closed.borrow_mut() = None;
            });
        }
    });
}

/// Stub no-op para builds release padrão ou plataformas não-Linux.
#[cfg(not(all(target_os = "linux", debug_assertions)))]
pub fn setup_devtools(_window: &tauri::WebviewWindow, _config_dir: &Path) {
    // Preserva compilação limpa sem ativar DevTools em release padrão.
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_serde_roundtrip() {
        let cfg_attached = DevToolsConfig {
            version: 1,
            placement: InspectorPlacement::Attached,
            dock_side: AttachedDockSide::Right,
        };
        let json_att = serde_json::to_string(&cfg_attached).unwrap();
        assert!(json_att.contains("\"placement\":\"attached\""));
        assert!(json_att.contains("\"dock_side\":\"right\""));
        let parsed_att: DevToolsConfig = serde_json::from_str(&json_att).unwrap();
        assert_eq!(parsed_att.placement, InspectorPlacement::Attached);
        assert_eq!(parsed_att.dock_side, AttachedDockSide::Right);

        let cfg_detached = DevToolsConfig {
            version: 1,
            placement: InspectorPlacement::Detached,
            dock_side: AttachedDockSide::Bottom,
        };
        let json_det = serde_json::to_string(&cfg_detached).unwrap();
        assert!(json_det.contains("\"placement\":\"detached\""));
        let parsed_det: DevToolsConfig = serde_json::from_str(&json_det).unwrap();
        assert_eq!(parsed_det.placement, InspectorPlacement::Detached);
        assert_eq!(parsed_det.dock_side, AttachedDockSide::Bottom);

        // Retrocompatibilidade: JSON legado sem dock_side assume Bottom
        let legacy_json = r#"{"version":1,"placement":"attached"}"#;
        let parsed_legacy: DevToolsConfig = serde_json::from_str(legacy_json).unwrap();
        assert_eq!(parsed_legacy.placement, InspectorPlacement::Attached);
        assert_eq!(parsed_legacy.dock_side, AttachedDockSide::Bottom);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path();

        // Inicialmente nenhum arquivo deve existir
        assert_eq!(load_preference(config_dir), None);

        // Salva Attached
        save_preference(config_dir, InspectorPlacement::Attached).unwrap();
        assert_eq!(
            load_preference(config_dir),
            Some(InspectorPlacement::Attached)
        );

        // Salva Detached
        save_preference(config_dir, InspectorPlacement::Detached).unwrap();
        assert_eq!(
            load_preference(config_dir),
            Some(InspectorPlacement::Detached)
        );
    }

    #[test]
    fn test_load_file_missing() {
        let dir = tempdir().unwrap();
        assert_eq!(load_preference(dir.path()), None);
    }

    #[test]
    fn test_load_file_invalid_json() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join(DEVTOOLS_CONFIG_FILENAME);
        std::fs::write(&file_path, b"not valid json").unwrap();
        assert_eq!(load_preference(dir.path()), None);
    }

    #[test]
    fn test_load_file_unknown_version() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join(DEVTOOLS_CONFIG_FILENAME);
        let bad_version = serde_json::json!({
            "version": 99,
            "placement": "attached"
        });
        std::fs::write(&file_path, serde_json::to_vec(&bad_version).unwrap()).unwrap();
        assert_eq!(load_preference(dir.path()), None);
    }

    #[test]
    fn test_load_file_too_large() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join(DEVTOOLS_CONFIG_FILENAME);
        let mut large_data = vec![b' '; 5000];
        large_data.extend_from_slice(b"{}");
        std::fs::write(&file_path, large_data).unwrap();
        assert_eq!(load_preference(dir.path()), None);
    }

    #[test]
    #[cfg(unix)]
    fn test_save_and_load_symlink_forbidden() {
        let dir = tempdir().unwrap();
        let target_file = dir.path().join("real_target.json");
        std::fs::write(&target_file, b"{}").unwrap();

        let symlink_path = dir.path().join(DEVTOOLS_CONFIG_FILENAME);
        std::os::unix::fs::symlink(&target_file, &symlink_path).unwrap();

        // Leitura deve recusar symlink
        assert_eq!(load_preference(dir.path()), None);

        // Gravação deve recusar symlink
        let save_res = save_preference(dir.path(), InspectorPlacement::Attached);
        assert_eq!(save_res, Err(DevToolsIoError::SymlinkForbidden));
    }

    #[test]
    fn test_state_machine_f12_key_press_and_repeat() {
        let mut sm = DevToolsStateMachine::new(None, true);
        const F12: u32 = 0xffc9;

        // Tecla diferente deve passar direto
        assert_eq!(sm.on_key_press(0xffc8, false), KeyHandling::PassThrough);

        // F12 com modificadores (Ctrl/Shift/Alt/Super) deve passar direto
        assert_eq!(sm.on_key_press(F12, true), KeyHandling::PassThrough);

        // F12 simples deve ser consumido e disparar abertura
        assert_eq!(
            sm.on_key_press(F12, false),
            KeyHandling::Consume(Some(StateMachineAction::ShowOrBringToFront))
        );

        // Segundo F12 enquanto pressionado (key repeat) deve ser consumido sem ação
        assert_eq!(sm.on_key_press(F12, false), KeyHandling::Consume(None));

        // Liberação de tecla F12
        assert_eq!(sm.on_key_release(F12), KeyHandling::Consume(None));

        // Novo F12 simples agora dispara nova ação
        assert_eq!(
            sm.on_key_press(F12, false),
            KeyHandling::Consume(Some(StateMachineAction::ShowOrBringToFront))
        );
    }

    #[test]
    fn test_state_machine_focus_out_resets_key_down() {
        let mut sm = DevToolsStateMachine::new(None, true);
        const F12: u32 = 0xffc9;

        assert_eq!(
            sm.on_key_press(F12, false),
            KeyHandling::Consume(Some(StateMachineAction::ShowOrBringToFront))
        );
        // Sem release, mas ocorreu focus out
        sm.on_focus_out();

        // Novo pressionamento deve ser aceito normalmente
        assert_eq!(
            sm.on_key_press(F12, false),
            KeyHandling::Consume(Some(StateMachineAction::ShowOrBringToFront))
        );
    }

    #[test]
    fn test_state_machine_opening_restores_attached() {
        let mut sm = DevToolsStateMachine::new(Some(InspectorPlacement::Attached), true);

        // Primeira abertura vindo de fechado restaura Attached
        assert_eq!(
            sm.on_opening(),
            Some(StateMachineAction::ApplyPlacement(
                InspectorPlacement::Attached
            ))
        );
        assert!(sm.is_open());

        // Se já está aberto, abertura subsequente não reaplica nem move
        assert_eq!(sm.on_opening(), None);
    }

    #[test]
    fn test_state_machine_opening_restores_detached() {
        let mut sm = DevToolsStateMachine::new(Some(InspectorPlacement::Detached), true);

        assert_eq!(
            sm.on_opening(),
            Some(StateMachineAction::ApplyPlacement(
                InspectorPlacement::Detached
            ))
        );
        assert!(sm.is_open());
    }

    #[test]
    fn test_state_machine_opening_fallback_preserves_intention() {
        // can_attach = false
        let mut sm = DevToolsStateMachine::new(Some(InspectorPlacement::Attached), false);

        // Fallback: não consegue acoplar, mas NÃO altera a preferência salva
        assert_eq!(sm.on_opening(), None);
        assert_eq!(
            sm.persisted_preference(),
            Some(InspectorPlacement::Attached)
        );
    }

    #[test]
    fn test_state_machine_restoring_does_not_persist() {
        let mut sm = DevToolsStateMachine::new(None, true);
        sm.set_restoring(true);

        assert_eq!(sm.on_signal_attach(), None);
        sm.on_signal_detach();
        assert_eq!(sm.on_signal_open_window(), None);
        assert_eq!(sm.persisted_preference(), None);
    }

    #[test]
    fn test_state_machine_user_change_persists() {
        let mut sm = DevToolsStateMachine::new(None, true);

        // Usuário acopla
        assert_eq!(
            sm.on_signal_attach(),
            Some(StateMachineAction::PersistPreference(
                InspectorPlacement::Attached
            ))
        );
        assert_eq!(
            sm.persisted_preference(),
            Some(InspectorPlacement::Attached)
        );

        // Sinal detach isolado NÃO persiste nada
        sm.on_signal_detach();
        assert_eq!(
            sm.persisted_preference(),
            Some(InspectorPlacement::Attached)
        );

        // Sinal open-window (janela separada aberta pelo usuário) persiste Detached
        assert_eq!(
            sm.on_signal_open_window(),
            Some(StateMachineAction::PersistPreference(
                InspectorPlacement::Detached
            ))
        );
        assert_eq!(
            sm.persisted_preference(),
            Some(InspectorPlacement::Detached)
        );
    }

    #[test]
    fn test_state_machine_closed_preserves_preference() {
        let mut sm = DevToolsStateMachine::new(Some(InspectorPlacement::Attached), true);
        sm.on_opening();
        assert!(sm.is_open());

        sm.on_signal_closed();
        assert!(!sm.is_open());
        assert_eq!(sm.current_mode(), None);
        // Intenção original preservada!
        assert_eq!(
            sm.persisted_preference(),
            Some(InspectorPlacement::Attached)
        );

        // Cenário com Detached
        let mut sm_det = DevToolsStateMachine::new(Some(InspectorPlacement::Detached), true);
        sm_det.on_opening();
        sm_det.on_signal_closed();
        assert_eq!(
            sm_det.persisted_preference(),
            Some(InspectorPlacement::Detached)
        );
    }

    #[test]
    fn test_save_and_load_config_with_dock_side() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path();

        save_config(
            config_dir,
            InspectorPlacement::Attached,
            AttachedDockSide::Right,
        )
        .unwrap();
        let loaded = load_config(config_dir).unwrap();
        assert_eq!(loaded.placement, InspectorPlacement::Attached);
        assert_eq!(loaded.dock_side, AttachedDockSide::Right);

        save_config(
            config_dir,
            InspectorPlacement::Attached,
            AttachedDockSide::Left,
        )
        .unwrap();
        let loaded_left = load_config(config_dir).unwrap();
        assert_eq!(loaded_left.placement, InspectorPlacement::Attached);
        assert_eq!(loaded_left.dock_side, AttachedDockSide::Left);
    }

    #[test]
    fn test_infer_attached_dock_side() {
        // Painel na parte inferior: webview ocupa largura total, inspetor embaixo (y > 0)
        assert_eq!(
            infer_attached_dock_side(1200, 800, 0, 500, 1200, 300),
            Some(AttachedDockSide::Bottom)
        );

        // Painel na lateral direita: inspetor à direita (x > 0, largura parcial, altura total)
        assert_eq!(
            infer_attached_dock_side(1200, 800, 750, 0, 450, 800),
            Some(AttachedDockSide::Right)
        );

        // Painel na lateral esquerda: inspetor à esquerda (x == 0, largura parcial, altura total)
        assert_eq!(
            infer_attached_dock_side(1200, 800, 0, 0, 450, 800),
            Some(AttachedDockSide::Left)
        );

        // Dimensões degeneradas (widgets ainda não alocados)
        assert_eq!(infer_attached_dock_side(0, 0, 0, 0, 0, 0), None);
    }

    #[test]
    fn test_state_machine_attached_dock_side_preserved_across_close_and_reopen() {
        let mut sm = DevToolsStateMachine::new(None, true);

        // 1. Abre o inspetor pela primeira vez (acoplado embaixo por padrão do WebKit)
        assert_eq!(sm.on_opening(), None);
        assert_eq!(
            sm.on_signal_attach(),
            Some(StateMachineAction::PersistPreference(
                InspectorPlacement::Attached
            ))
        );
        assert_eq!(
            sm.on_attached_dock_side_observed(AttachedDockSide::Bottom),
            None
        );

        // 2. Usuário move o painel para a lateral direita
        assert_eq!(
            sm.on_attached_dock_side_observed(AttachedDockSide::Right),
            Some((InspectorPlacement::Attached, AttachedDockSide::Right))
        );
        assert_eq!(sm.persisted_dock_side(), AttachedDockSide::Right);

        // 3. Usuário fecha o inspetor
        sm.on_signal_closed();
        assert!(!sm.is_open());
        assert_eq!(
            sm.persisted_preference(),
            Some(InspectorPlacement::Attached)
        );
        assert_eq!(sm.persisted_dock_side(), AttachedDockSide::Right);

        // 4. Usuário reabre o inspetor (ex.: F12 ou menu Inspecionar)
        assert_eq!(
            sm.on_opening(),
            Some(StateMachineAction::ApplyPlacement(
                InspectorPlacement::Attached
            ))
        );
        // Enquanto pending_dock_side_restore está ativo, o Bottom inicial transitório do WebKit2GTK é ignorado
        assert_eq!(
            sm.pending_dock_side_restore(),
            Some(AttachedDockSide::Right)
        );
        assert_eq!(
            sm.on_attached_dock_side_observed(AttachedDockSide::Bottom),
            None
        );
        assert_eq!(sm.persisted_dock_side(), AttachedDockSide::Right);

        // Quando o layout Right é efetivado pela restauração, limpa a pendência e mantém Right
        assert_eq!(
            sm.on_attached_dock_side_observed(AttachedDockSide::Right),
            None
        );
        assert_eq!(sm.pending_dock_side_restore(), None);
        assert_eq!(sm.persisted_dock_side(), AttachedDockSide::Right);
    }
}
