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

/// Posicionamento do painel do Web Inspector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InspectorPlacement {
    /// Acoplado na janela do aplicativo (por padrão na parte inferior).
    Attached,
    /// Destacado em uma janela separada e independente.
    Detached,
}

/// Estrutura persistida em disco em `devtools.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevToolsConfig {
    /// Versão do schema da preferência.
    pub version: u32,
    /// Posicionamento selecionado pelo usuário.
    pub placement: InspectorPlacement,
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

/// Lê a preferência de posicionamento do DevTools armazenada em disco.
///
/// Não segue links simbólicos e rejeita arquivos maiores que 4 KiB.
/// Em caso de arquivo ausente, corrompido ou erro de I/O, retorna `None`
/// com log sanitizado, degradando graciosamente para o comportamento nativo.
#[must_use]
pub fn load_preference(config_dir: &Path) -> Option<InspectorPlacement> {
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
            Some(cfg.placement)
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

/// Grava atomicamente a preferência de posicionamento do DevTools em disco.
///
/// Grava primeiro em um arquivo temporário no mesmo diretório com permissões
/// privadas `0o600` em sistemas Unix, sincroniza os dados e renomeia atomicamente.
/// Recusa-se terminantemente a sobrescrever ou seguir links simbólicos.
///
/// # Errors
///
/// Retorna `DevToolsIoError` em caso de falha de serialização, I/O ou symlink.
pub fn save_preference(
    config_dir: &Path,
    placement: InspectorPlacement,
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
    current_mode: Option<InspectorPlacement>,
    is_open: bool,
    is_restoring: bool,
    f12_key_down: bool,
    can_attach: bool,
}

impl DevToolsStateMachine {
    /// Cria uma nova máquina de estados com a preferência inicial e capacidade de acoplamento.
    #[must_use]
    pub fn new(initial_preference: Option<InspectorPlacement>, can_attach: bool) -> Self {
        Self {
            persisted_preference: initial_preference,
            current_mode: None,
            is_open: false,
            is_restoring: false,
            f12_key_down: false,
            can_attach,
        }
    }

    /// Retorna a preferência persistida atual.
    #[must_use]
    pub fn persisted_preference(&self) -> Option<InspectorPlacement> {
        self.persisted_preference
    }

    /// Retorna o modo de exibição ativo no momento (`None` se fechado).
    #[must_use]
    pub fn current_mode(&self) -> Option<InspectorPlacement> {
        self.current_mode
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
                    Some(StateMachineAction::ApplyPlacement(
                        InspectorPlacement::Attached,
                    ))
                } else {
                    // Fallback nativo se acoplamento indisponível, preservando a preferência
                    None
                }
            }
            Some(InspectorPlacement::Detached) => Some(StateMachineAction::ApplyPlacement(
                InspectorPlacement::Detached,
            )),
            None => None,
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
    }
}

/// Configura o adaptador nativo do DevTools para a janela principal em Linux.
#[cfg(all(target_os = "linux", debug_assertions))]
pub fn setup_devtools(window: &tauri::WebviewWindow, config_dir: &Path) {
    use gtk::prelude::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use webkit2gtk::WebInspectorExt;
    use webkit2gtk::WebViewExt;

    let config_dir = config_dir.to_path_buf();
    let initial_pref = load_preference(&config_dir);

    let _ = window.with_webview(move |platform_webview| {
        let webview = platform_webview.inner();
        let inspector = match webview.inspector() {
            Some(insp) => insp,
            None => {
                tracing::info!(
                    target: "maxterminal::devtools",
                    "WebInspector não disponível na WebView atual"
                );
                return;
            }
        };

        let can_attach = inspector.can_attach();
        let state = Rc::new(RefCell::new(DevToolsStateMachine::new(
            initial_pref,
            can_attach,
        )));

        // 1. Conecta interceptação de F12 no widget de nível superior (janela GTK)
        if let Some(toplevel) = webview.toplevel() {
            let state_press = Rc::clone(&state);
            let inspector_press = inspector.clone();

            toplevel.connect_key_press_event(move |_widget, event| {
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
                        let restore_action = state_press.borrow_mut().on_opening();
                        inspector_press.show();

                        if let Some(StateMachineAction::ApplyPlacement(placement)) = restore_action
                        {
                            state_press.borrow_mut().set_restoring(true);
                            match placement {
                                InspectorPlacement::Attached => {
                                    inspector_press.attach();
                                }
                                InspectorPlacement::Detached => {
                                    inspector_press.detach();
                                }
                            }
                            state_press.borrow_mut().set_restoring(false);
                        }

                        glib::Propagation::Stop
                    }
                    KeyHandling::Consume(Some(_)) => glib::Propagation::Stop,
                }
            });

            let state_release = Rc::clone(&state);
            toplevel.connect_key_release_event(move |_widget, event| {
                let keyval = *event.keyval();
                let handling = state_release.borrow_mut().on_key_release(keyval);
                match handling {
                    KeyHandling::PassThrough => glib::Propagation::Proceed,
                    KeyHandling::Consume(_) => glib::Propagation::Stop,
                }
            });

            let state_focus = Rc::clone(&state);
            toplevel.connect_focus_out_event(move |_widget, _event| {
                state_focus.borrow_mut().on_focus_out();
                glib::Propagation::Proceed
            });
        }

        // 2. Conecta observadores dos sinais do WebInspector
        let state_front = Rc::clone(&state);
        let inspector_front = inspector.clone();
        inspector.connect_bring_to_front(move |_insp| {
            let restore_action = state_front.borrow_mut().on_opening();
            if let Some(StateMachineAction::ApplyPlacement(placement)) = restore_action {
                state_front.borrow_mut().set_restoring(true);
                match placement {
                    InspectorPlacement::Attached => {
                        inspector_front.attach();
                    }
                    InspectorPlacement::Detached => {
                        inspector_front.detach();
                    }
                }
                state_front.borrow_mut().set_restoring(false);
            }
            false
        });

        let state_attach = Rc::clone(&state);
        let config_dir_attach = config_dir.clone();
        inspector.connect_attach(move |_insp| {
            if let Some(StateMachineAction::PersistPreference(placement)) =
                state_attach.borrow_mut().on_signal_attach()
            {
                let cfg_dir = config_dir_attach.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(err) = save_preference(&cfg_dir, placement) {
                        tracing::warn!(
                            target: "maxterminal::devtools",
                            ?err,
                            "Falha ao persistir preferência devtools em attach"
                        );
                    }
                });
            }
            false
        });

        let state_detach = Rc::clone(&state);
        inspector.connect_detach(move |_insp| {
            state_detach.borrow_mut().on_signal_detach();
            false
        });

        let state_open_win = Rc::clone(&state);
        let config_dir_open_win = config_dir.clone();
        inspector.connect_open_window(move |_insp| {
            if let Some(StateMachineAction::PersistPreference(placement)) =
                state_open_win.borrow_mut().on_signal_open_window()
            {
                let cfg_dir = config_dir_open_win.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    if let Err(err) = save_preference(&cfg_dir, placement) {
                        tracing::warn!(
                            target: "maxterminal::devtools",
                            ?err,
                            "Falha ao persistir preferência devtools em open-window"
                        );
                    }
                });
            }
            false
        });

        let state_closed = Rc::clone(&state);
        inspector.connect_closed(move |_insp| {
            state_closed.borrow_mut().on_signal_closed();
        });
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
        };
        let json_att = serde_json::to_string(&cfg_attached).unwrap();
        assert!(json_att.contains("\"placement\":\"attached\""));
        let parsed_att: DevToolsConfig = serde_json::from_str(&json_att).unwrap();
        assert_eq!(parsed_att.placement, InspectorPlacement::Attached);

        let cfg_detached = DevToolsConfig {
            version: 1,
            placement: InspectorPlacement::Detached,
        };
        let json_det = serde_json::to_string(&cfg_detached).unwrap();
        assert!(json_det.contains("\"placement\":\"detached\""));
        let parsed_det: DevToolsConfig = serde_json::from_str(&json_det).unwrap();
        assert_eq!(parsed_det.placement, InspectorPlacement::Detached);
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
}
