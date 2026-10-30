# Implementação 03 — Single Instance, Deep Link, Atalho Global e Quake Mode (`spec.md`)

> Escopo desta sub-implementação: implementar o roteamento de instância única e argumentos de linha de comando (`cli.rs`), a infraestrutura de transporte e fila limitada de deep links `maxterminal://` para a Fase 11 (`activation.rs`), o controlador de Quake Mode / Quick Toggle com cálculo puro de geometria drop-down (`quake.rs`) e o gerenciador de atalhos globais com cadeia de backends X11 Plugin, Wayland Portal (`ashpd`) e fallback CLI (`shortcut/`).

---

## 1. CLI e Instância Única (`crates/mt-os/src/cli.rs`)

### 1.1. Assinaturas e Prioridade de Argumentos

```rust
use parking_lot::Mutex;
use std::collections::VecDeque;
use tauri::{AppHandle, Runtime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction {
    Show,
    Toggle,
    Hide,
    StartMinimized,
    None,
}

/// Whitelist estrita: aceita apenas `--show`, `--toggle`, `--hide`, `--minimized`.
/// Ignora `argv[0]` e quaisquer outros argumentos ou caminhos (logando apenas a contagem em `debug!`, sem ecoar valores).
/// Ordem de precedência: `--show > --toggle > --hide > --minimized`.
pub fn parse_cli_args<I, S>(args: I) -> CliAction
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut has_show = false;
    let mut has_toggle = false;
    let mut has_hide = false;
    let mut has_minimized = false;

    for (idx, raw) in args.into_iter().enumerate() {
        if idx == 0 {
            continue;
        }
        match raw.as_ref() {
            "--show" => has_show = true,
            "--toggle" => has_toggle = true,
            "--hide" => has_hide = true,
            "--minimized" => has_minimized = true,
            _ => {}
        }
    }

    if has_show {
        CliAction::Show
    } else if has_toggle {
        CliAction::Toggle
    } else if has_hide {
        CliAction::Hide
    } else if has_minimized {
        CliAction::StartMinimized
    } else {
        CliAction::None
    }
}

/// Inbox estática/compartilhada criada antes do `tauri::Builder` para receber ativações da 2ª instância
/// que cheguem antes de `OsIntegration::init` concluir no hook `setup`.
#[derive(Default)]
pub struct EarlyCliInbox {
    queue: Mutex<VecDeque<CliAction>>,
}

impl EarlyCliInbox {
    pub const MAX_PENDING: usize = 16;
    pub fn push(&self, action: CliAction);
    pub fn drain(&self) -> Vec<CliAction>;
}

/// Callback registrado em `tauri_plugin_single_instance::init(|app, argv, _cwd| ...)`.
pub fn handle_second_instance<R: Runtime>(app: &AppHandle<R>, argv: Vec<String>);
```

### 1.2. Regras de Startup, Segunda Instância e Handshake `os_ui_ready`

1. **Cold Start (1ª instância)**:
   - A janela principal inicia com `"visible": false` no `tauri.conf.json`.
   - `CliAction::Toggle` no cold start equivale a `Show`.
   - `CliAction::StartMinimized` (ou `settings.autostart && settings.start_minimized` quando acionado via autostart) preserva a janela oculta **somente** se `env.tray_available == true`; caso a bandeja não esteja disponível, degrada para mostrar/minimizar visível na taskbar (CA-13, CA-30, CA-40).
   - O frontend chama `os_ui_ready` após o primeiro render (nunca `os_window_show` incondicional). O backend aplica a intenção inicial de visibilidade no `os_ui_ready`, garantindo que a inicialização minimizada não seja desfeita pelo frontend (CA-40).
   - Um watchdog assíncrono de **3 segundos** exibe a janela automaticamente se a intenção de startup era visível e `os_ui_ready` ainda não tiver sido chamado; se o modo inicial era minimizado na bandeja e saudável, permanece oculto.
2. **Warm Start (2ª instância via D-Bus)**:
   - `tauri-plugin-single-instance` (com feature `deep-link`) é obrigatoriamente o **primeiro** plugin registrado no `tauri::Builder`.
   - O parâmetro `cwd` recebido do segundo processo é **sempre ignorado** (nunca altera o diretório de nenhum terminal).
   - Se `argv` contiver uma URI `maxterminal://`, o plugin de deep-link já a encaminha para `on_open_url`; `handle_second_instance` **não** reenfileira a URI e não dispara ação duplicada.
   - Se `argv` não contiver URI:
     - `CliAction::Toggle` → `QuakeController::toggle(ToggleSource::Cli)` (sem debounce de repetição de tecla; alvo < 150 ms ponta a ponta — CA-18).
     - `CliAction::Show` ou `CliAction::None` → `QuakeController::show_and_focus()`.
     - `CliAction::Hide` → `QuakeController::hide()`.
     - `CliAction::StartMinimized` em instância já aberta é ignorado (não esconde uma janela que já estava em uso).

---

## 2. Roteador de Ativação e Deep Links (`crates/mt-os/src/activation.rs`)

### 2.1. Estruturas e Contratos para a Fase 11

```rust
use crate::error::{codes, os_err, OsResult};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use url::Url;
use uuid::Uuid;

pub const MAX_URI_BYTES: usize = 8 * 1024; // 8 KiB
pub const MAX_QUEUE_LEN: usize = 32;
pub const ENVELOPE_TTL: Duration = Duration::from_secs(30);
pub const TRANSPORT_DEDUPE_WINDOW: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationSource {
    Startup,
    DeepLink,
}

#[derive(Debug, Clone)]
pub struct ActivationEnvelope {
    pub id: Uuid,
    pub uri: Url,
    pub received_at_ms: i64,
    pub received_instant: Instant,
    pub source: ActivationSource,
}

#[async_trait::async_trait]
pub trait ActivationHandler: Send + Sync + 'static {
    fn accepts(&self, uri: &Url) -> bool;
    async fn handle(&self, envelope: &ActivationEnvelope) -> OsResult<()>;
}

pub struct ActivationRouter {
    state: Mutex<RouterState>,
}

impl ActivationRouter {
    pub fn new() -> Self;
    /// Valida e analisa uma string URI bruta em um `ActivationEnvelope`.
    pub fn parse_and_validate(raw: &str, source: ActivationSource, now_ms: i64, now: Instant) -> OsResult<ActivationEnvelope>;
    /// Ponto de extensão para a Fase 11: registra handler antes de `set_ready()`.
    pub async fn register_handler(&self, handler: Arc<dyn ActivationHandler>) -> OsResult<()>;
    /// Enfileira ou despacha imediatamente se `ready == true`.
    pub async fn enqueue(&self, envelope: ActivationEnvelope) -> OsResult<()>;
    /// Marca o roteador como pronto após restauração do workspace (Fase 07) e drena a fila em ordem FIFO.
    pub async fn set_ready(&self) -> OsResult<()>;
}
```

### 2.2. Regras de Validação, Segurança e Deduplicação

1. **Validação Estrita de URI (`parse_and_validate`)**:
   - Comprimento bruto `<= 8192` bytes (`MAX_URI_BYTES`).
   - Rejeita bytes de controle ASCII (`<= 0x1F` ou `0x7F`) e sequências de escape percentual malformadas (`%` não seguido de exatamente 2 dígitos hexadecimais ASCII).
   - Parse via `Url::parse(raw)`:
     - `scheme() == "maxterminal"` (exato).
     - `username() == ""`, `password().is_none()`, `port().is_none()`, `fragment().is_none()`.
   - Qualquer violação retorna `Err(os_err(codes::ACTIVATION_INVALID, "URI de ativação inválida ou malformada."))`.
   - **Privacidade de logs**: nunca logar a query string da URI (pode conter tokens de ponte da Fase 11).
2. **Deduplicação de Transporte e Fila Limitada**:
   - No cold start, `get_current()` e `on_open_url` podem entregar a mesma URI: o `ActivationRouter` mantém o histórico recente `(uri_string, Instant)` e descarta silenciosamente entregas idênticas dentro de `1 s` (`TRANSPORT_DEDUPE_WINDOW`) — garantindo entrega única (CA-36).
   - Enquanto `ready == false`:
     - Purga envelopes expirados (`now.duration_since(env.received_instant) > ENVELOPE_TTL`).
     - Se `queue.len() >= 32` (`MAX_QUEUE_LEN`), retorna `Err(os_err(codes::ACTIVATION_BUSY, "Fila de ativação cheia."))`.
   - Ao processar um envelope (em `set_ready` ou imediatamente quando `ready == true`):
     - Se expirado (> 30 s), descarta e retorna `Err(os_err(codes::ACTIVATION_UNAVAILABLE, "Solicitação de ativação expirou."))`.
     - Busca o primeiro `handler` registrado onde `handler.accepts(&envelope.uri)` é verdadeiro.
     - Se nenhum handler aceitar (cenário padrão na Fase 09 antes da Fase 11 ser ligada), retorna `Err(os_err(codes::ACTIVATION_UNAVAILABLE, "Nenhum manipulador disponível para esta rota de ativação."))`, sem executar comandos nem abrir projetos por conta própria (CA-36).

---

## 3. Quake Mode e Geometria Drop-Down (`crates/mt-os/src/quake.rs`)

### 3.1. Funções Puras (`decide_toggle` e `drop_down_geometry` / `compute_quake_bounds`)

```rust
use crate::environment::{DisplayServer, OsEnvironment};
use crate::error::OsResult;
use crate::settings::{OsSettings, QuakeMode};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Runtime};

pub const SHORTCUT_REPEAT_DEBOUNCE: Duration = Duration::from_millis(150);
pub const BLUR_HIDE_DELAY: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ToggleDecision {
    ShowAndFocus,
    Focus,
    Hide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ToggleDecisionDto {
    Show,
    Focus,
    Hide,
}

impl From<ToggleDecision> for ToggleDecisionDto {
    fn from(d: ToggleDecision) -> Self {
        match d {
            ToggleDecision::ShowAndFocus => Self::Show,
            ToggleDecision::Focus => Self::Focus,
            ToggleDecision::Hide => Self::Hide,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ToggleSource {
    Shortcut,
    Portal,
    Cli,
    Tray,
    Ipc,
}

/// Função pura (8 combinações):
/// - Oculta (`!visible`) ou minimizada (`minimized`) ⇒ `ShowAndFocus`
/// - Visível e não minimizada, mas sem foco (`!focused`) ⇒ `Focus`
/// - Visível, não minimizada e focada ⇒ `Hide`
pub fn decide_toggle(visible: bool, focused: bool, minimized: bool) -> ToggleDecision {
    if !visible || minimized {
        ToggleDecision::ShowAndFocus
    } else if !focused {
        ToggleDecision::Focus
    } else {
        ToggleDecision::Hide
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DropDownGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Calcula a geometria física no topo do monitor (coordenadas físicas já fornecidas pela API do monitor;
/// nunca multiplica scale factor duas vezes; preserva coordenadas X/Y negativas de setups multi-monitor).
pub fn drop_down_geometry(
    monitor_pos: (i32, i32),
    monitor_size: (u32, u32),
    height_pct: u8,
) -> DropDownGeometry {
    let clamped_pct = height_pct.clamp(20, 100) as u64;
    let width = monitor_size.0.max(1);
    let height = ((monitor_size.1 as u64 * clamped_pct) / 100).max(1) as u32;
    DropDownGeometry {
        x: monitor_pos.0,
        y: monitor_pos.1,
        width,
        height,
    }
}

/// Alias semântico para cálculo de bounds do Quake Mode.
#[inline]
pub fn compute_quake_bounds(
    monitor_pos: (i32, i32),
    monitor_size: (u32, u32),
    height_pct: u8,
) -> DropDownGeometry {
    drop_down_geometry(monitor_pos, monitor_size, height_pct)
}
```

### 3.2. `QuakeController` e Regras X11 vs Wayland

```rust
#[derive(Debug, Clone)]
struct SavedNormalWindowState {
    position: Option<(i32, i32)>,
    size: Option<(u32, u32)>,
    decorations: bool,
    always_on_top: bool,
    skip_taskbar: bool,
}

pub struct QuakeController<R: Runtime> {
    app: AppHandle<R>,
    settings: Arc<RwLock<OsSettings>>,
    env: Arc<RwLock<OsEnvironment>>,
    last_shortcut_toggle: Mutex<Option<Instant>>,
    saved_normal: Mutex<Option<SavedNormalWindowState>>,
    modal_interaction_guard: AtomicBool,
}
```

1. **Debounce Seletivo**:
   - `ToggleSource::Shortcut` e `ToggleSource::Portal` aplicam debounce de `150 ms` (`SHORTCUT_REPEAT_DEBOUNCE`) para filtrar auto-repeat de tecla pressionada.
   - `ToggleSource::Cli`, `ToggleSource::Tray` e `ToggleSource::Ipc` **não** sofrem debounce.
2. **Modo `DropDown` vs `Normal`**:
   - Ao entrar em `QuakeMode::DropDown` em sessão `DisplayServer::X11`:
     - Salva posição, tamanho, `is_decorated`, `is_always_on_top` em `saved_normal`.
     - Seleciona o monitor sob o cursor (com fallback para `current_monitor()` e `primary_monitor()`).
     - Calcula `drop_down_geometry(pos, size, height_pct)` e aplica `set_decorations(false)`, `set_always_on_top(settings.quake.always_on_top)`, `set_position(PhysicalPosition)`, `set_size(PhysicalSize)` antes de `show()` e `set_focus()`.
   - Ao sair de `DropDown` para `Normal`: restaura integralmente `saved_normal`.
   - Em `Wayland` / `XWayland`: não falha se o compositor ignorar `set_position`/`set_always_on_top`; aplica em melhor esforço e registra `env.window_positioning = false` (exibido na UI de Preferências — CA-20).
3. **`hide_on_blur` Seguro (CA-20, CA-35)**:
   - Quando a janela perde foco (`on_blur`) e `mode == DropDown && hide_on_blur == true`:
     - Se `modal_interaction_guard` estiver ativo (diálogo nativo de confirmação, seletor do portal XDG ou modal interno de Preferências aberto), **ignora** o blur.
     - Caso contrário, aguarda `120 ms` (`BLUR_HIDE_DELAY`), revalida se a janela ainda está sem foco e sem guard modal ativo, e só então chama `hide()`.
   - Se nem tray nem atalho utilizável estiverem disponíveis, `hide()` degrada para `minimize()` mantendo o app na taskbar (CA-35).

---

## 4. Atalho Global e Backends (`crates/mt-os/src/shortcut/`)

### 4.1. Parser e Formatador de Aceleradores (`shortcut/accelerator.rs`)

```rust
use crate::error::{codes, os_err, OsResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accelerator {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: CanonicalKey,
}

impl Accelerator {
    /// Aceita modificadores `Ctrl`/`Control`, `Alt`, `Shift`, `Super`/`Meta`/`CommandOrControl`
    /// combinados com `+` e exatamente 1 tecla não-modificadora (ex.: `Backquote`, `F12`, `Space`, `KeyA`..`KeyZ`, `Digit0`..`Digit9`).
    /// Rejeita strings vazias, sem modificador, com modificador duplicado ou tecla desconhecida (`os.shortcut_invalid`).
    pub fn parse(input: &str) -> OsResult<Self>;

    /// Formato canônico persistido (ex.: `"Ctrl+Alt+Backquote"`).
    pub fn to_canonical_string(&self) -> String;

    /// Formato aceito por `tauri-plugin-global-shortcut` (ex.: `"Control+Alt+Backquote"`).
    pub fn to_plugin_string(&self) -> String;

    /// Formato XDG Desktop Portal `GlobalShortcuts` (ex.: `"CTRL+ALT+grave"`).
    pub fn to_portal_trigger(&self) -> String;
}
```

### 4.2. Cadeia de Backends (`shortcut/mod.rs`, `plugin_backend.rs`, `portal_backend.rs`, `cli_backend.rs`)

1. **Ordem de Seleção (`ShortcutManager::apply`)**:
   - Se `!settings.enabled`: desregistra o backend ativo, define `registered = false` e mantém apenas dica CLI.
   - Se `preferred_backend == Auto`:
     - `DisplayServer::X11` ⇒ `PluginBackend` (`tauri-plugin-global-shortcut`).
     - `DisplayServer::Wayland` ou `DisplayServer::XWayland` ⇒ se `env.portal_global_shortcuts == true`, tenta `PortalBackend`; se falhar ou indisponível, cai para `CliBackend` (`ShortcutBackendKind::CliOnly`).
2. **Preservação em Edição (Rollback em Conflito — CA-19, CA-40)**:
   - Ao alterar o acelerador em runtime, `ShortcutManager` valida `Accelerator::parse` antes de tocar no backend atual.
   - Se o registro do novo atalho no plugin X11 falhar (ex.: grab negado por conflito com outro app), normaliza o erro para `os.shortcut_conflict`, **re-registra imediatamente o acelerador anterior** e retorna o erro para que `update_settings` aborte a gravação no SQLite.
3. **Portal Wayland (`portal_backend.rs` via `ashpd`)**:
   - Usa `ashpd::desktop::global_shortcuts::GlobalShortcuts`.
   - Fluxo: `create_session()` → `bind_shortcuts(&session, &[NewShortcut::new("quake-toggle", "Alternar janela do MaxTerminal (Quake Mode)").preferred_trigger(Some(acc.to_portal_trigger().as_str()))], None)` → escuta stream `receive_activated()` filtrando `shortcut_id() == "quake-toggle"`.
   - Preserva o `trigger_description` retornado pelo compositor em `ShortcutStatus.effective_trigger` (nunca sobrescreve `settings.quake.accelerator` com o texto do compositor).
   - Mantém a sessão viva com `CancellationToken` para encerramento limpo ou recriação.
4. **Fallback `CliBackend` (`cli_backend.rs`)**:
   - `registered = false`, `backend = ShortcutBackendKind::CliOnly`, `cli_command = "maxterminal --toggle"`.
   - Gera `compositor_hint` específico:
     - `Compositor::Hyprland`: `"bind = CTRL ALT, grave, exec, maxterminal --toggle"`
     - `Compositor::KdePlasma`: `"Configurações do Sistema > Atalhos > Adicionar comando > maxterminal --toggle"`
     - Demais: `"Configure um atalho global no seu ambiente para executar: maxterminal --toggle"`

---

## 5. Mapeamento para Critérios de Aceite

- **CA-02**: Testes de `decide_toggle`, `drop_down_geometry`, `parse_cli_args`, `Accelerator::parse` e `ActivationRouter`.
- **CA-15, CA-20**: Alternância em X11, geometria drop-down sob o cursor e `hide_on_blur` seguro.
- **CA-16, CA-17, CA-19**: Portal `GlobalShortcuts` com `effective_trigger`, fallback `cli_only` com snippet e rollback em `os.shortcut_conflict`/`os.shortcut_invalid`.
- **CA-18**: `maxterminal --toggle` via single-instance alterna janela em < 150 ms sem criar 2º processo.
- **CA-36**: `ActivationRouter` valida `maxterminal://`, deduplica cold/warm start e rejeita rotas sem handler sem executar ações indevidas.
