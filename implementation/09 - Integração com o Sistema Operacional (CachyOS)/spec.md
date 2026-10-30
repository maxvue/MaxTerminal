# Fase 09 — Integração com o Sistema Operacional (CachyOS) (`spec.md`)

> Escopo e critérios de aceite: [`./instructions.md`](./instructions.md). Contratos compartilhados: [`../README.md`](../README.md) §2.
> Esta fase consome apenas contratos das Fases 01–07 e expõe pontos de extensão para as Fases 11, 13 e 16.

---

## 1. Visão Geral da Arquitetura

```
┌──────────────────────────── crate binário `maxterminal` ────────────────────────────┐
│ lib.rs: plugins (single-instance 1º) → setup(OsIntegration::init) → on_window_event │
│ commands/os.rs: 17 comandos IPC `os_*`                                              │
│ adapters/os.rs: impl TabStatusProvider / TabCwdProvider / ShutdownHooks             │
│                 (pontes finas para mt-shell (05), mt-pty (02), mt-db (03), tabs (07))│
└───────────────┬───────────────────────────────────────────────────────────────────┘
                │ Arc<OsIntegration>  (tauri::State)
┌───────────────▼───────────────── crate `mt-os` ───────────────────────────────────┐
│ settings ─ environment ─ status ─ attention                                        │
│ tray (TrayController) ◄── StatusCounters + AttentionRegistry (debounce 250 ms)     │
│ lifecycle (decide_close, ShutdownCoordinator) ── quake (decide_toggle, geometria)  │
│ shortcut::{PluginBackend, PortalBackend, CliBackend} ── cli (single-instance)      │
│ links (LinkResolver) ── editor (EditorLauncher + EditorOpener registry)            │
│ notify (Notifier + RateLimiter) ── long_command (hook sobre fim de comando)        │
│ autostart (AutostartSync)                                                          │
│ activation (ActivationRouter: transporte de deep links, handlers registrados)      │
└────────────────────────────────────────────────────────────────────────────────────┘
```

Princípios:
- `mt-os` **não depende** diretamente de `mt-shell`/`mt-pty`; depende de `mt-core`, `mt-db` (repo de settings e
  `sanitizer`) e de **traits próprias** implementadas por adaptadores no binário. Isso isola o crate de renomeações nas
  Fases 02/05/07 e permite testes com mocks.
- Toda decisão de comportamento (fechar, alternar, resolver link, montar argv, rate limit) é uma **função pura**
  testável sem Tauri. Os módulos com efeitos (tray, janela, D-Bus) são cascas finas.
- Nenhuma operação bloqueante roda na thread principal: D-Bus (portal, notificações, watcher) em `tokio`/`zbus` assíncrono;
  ações de notificação usam stream compartilhado cancelável, com limites explícitos.

### 1.1. Dependências do `mt-os` (`Cargo.toml`)
```toml
[dependencies]
mt-core = { path = "../mt-core" }
mt-db = { path = "../mt-db" }
tauri = { workspace = true, features = ["tray-icon", "image-png"] }
tauri-plugin-global-shortcut = "2"
tauri-plugin-notification = "2"
tauri-plugin-autostart = "2"
tauri-plugin-dialog = "2"
tauri-plugin-opener = "2"
ashpd = { version = "0.11", default-features = false, features = ["tokio"] }   # portal GlobalShortcuts
notify-rust = { version = "4", default-features = false, features = ["z", "images"] } # zbus puro
zbus = { version = "5", default-features = false, features = ["tokio"] }
tokio = { workspace = true, features = ["sync", "time", "fs", "rt", "signal", "process"] }
async-trait = "0.1"
futures = "0.3"
url = "2"
serde = { workspace = true }
serde_json = { workspace = true }
specta = { workspace = true }
tracing = { workspace = true }
percent-encoding = "2"
which = "7"
parking_lot = "0.12"
lru = "0.12"
```
`tauri-plugin-single-instance = { version = "2", features = ["deep-link"] }` e `tauri-plugin-deep-link = "2"`
são dependências do binário; single-instance precisa ser o primeiro plugin. Versões acima são referências de famílias,
não um lockfile: resolver versões compatíveis com o toolchain da Fase 01, fixar `Cargo.lock` e validar MSRV/features.

### 1.2. Adaptadores e direção das dependências

Dependências diretas de fase: **05 e 07**; bases transitivas 01–04/06. `src-tauri/src/adapters/os.rs` monta `OsDeps`:

| Fonte existente | Adaptação desta fase |
|-----------------|---------------------|
| 05 `TabStateRegistry::list()` → `TabStateSnapshot` | Projetar `badge_bucket` para contadores; inspecionar `panes` para atividade, preservando `revision` |
| 05 `ShellService::subscribe()` → `CommandEvent::Finished(CommandFinished)` | Converter para `CommandFinishedInfo`, preservar `Option<i32>`, `block_id`, `pane_id`, `end_reason`; tratar broadcast lag sem inventar conclusão |
| 05 `CommandEvent::CwdChanged` / `shell://cwd-changed` | Atualizar cwd e geração por painel/PTY, nunca cwd global por aba |
| 07 `WorkspaceObserver::{on_tab_closed,on_pane_spawned,on_pane_exited}` | Limpar atenção/cache, invalidar gerações, manter índice aba → painel → PTY |
| 07 `WorkspaceManager::shutdown()` e supervisão 02 | Adaptar para `ShutdownHooks`, arquivando com `app_exit` e aguardando filhos antes da saída |
| 07 `useTabsStore.focus(projectId, tabId, paneId?)` | Foco contextual após hidratação; regra `seen` permanece nas Fases 05/07 |

Não usar eventos frontend como autoridade de vida/morte. `mt-db` armazena JSON/row sem importar `mt-os`; os tipos
de settings e sua validação pertencem a `mt-os`. `mt-bridge` (11) e `mt-agents` (13) registram consumidores no binário,
sem importações desses crates por `mt-os`. Todos os exemplos de traits abaixo são contratos a implementar, não APIs
de terceiros presumidas; adaptar nomes divergentes só na borda, sem redefinir os estados da Fase 05.

---

## 2. Módulos Rust (`crates/mt-os/src/`)

### 2.1. `error.rs` — códigos de erro do domínio
Seguindo o contrato da Fase 01 (constantes por domínio, sem novas variantes em `mt-core`):
```rust
pub mod codes {
    pub const SETTINGS_INVALID: &str = "os.settings_invalid";
    pub const SHORTCUT_INVALID: &str = "os.shortcut_invalid";
    pub const SHORTCUT_CONFLICT: &str = "os.shortcut_conflict";
    pub const SHORTCUT_UNSUPPORTED: &str = "os.shortcut_unsupported";
    pub const PORTAL_UNAVAILABLE: &str = "os.portal_unavailable";
    pub const TRAY_UNAVAILABLE: &str = "os.tray_unavailable";
    pub const LINK_NOT_FOUND: &str = "os.link_not_found";
    pub const LINK_REJECTED: &str = "os.link_rejected";       // não-arquivo, tamanho, fora de limites
    pub const EDITOR_NOT_FOUND: &str = "os.editor_not_found";
    pub const EDITOR_LAUNCH_FAILED: &str = "os.editor_launch_failed";
    pub const NOTIFY_FAILED: &str = "os.notify_failed";
    pub const AUTOSTART_FAILED: &str = "os.autostart_failed";
    pub const SHUTDOWN_TIMEOUT: &str = "os.shutdown_timeout";
    pub const TAB_NOT_FOUND: &str = "os.tab_not_found";
    pub const LINK_CONTEXT_STALE: &str = "os.link_context_stale";
    pub const ACTIVATION_BUSY: &str = "os.activation_busy";
    pub const ACTIVATION_UNAVAILABLE: &str = "os.activation_unavailable";
    pub const ACTIVATION_INVALID: &str = "os.activation_invalid";
    pub const NOT_READY: &str = "os.not_ready";
}
```
Mensagens sempre em pt-BR, ex.: `MtError::new(codes::SHORTCUT_CONFLICT, "O atalho Ctrl+Alt+` já está em uso por outro aplicativo.")`.

### 2.2. `settings.rs`
```rust
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct OsSettings {
    pub schema_version: u32,              // 1
    pub close_behavior: CloseBehavior,    // padrão: Auto
    pub quake: QuakeSettings,
    pub editor: EditorSettings,
    pub notifications: NotificationSettings,
    pub autostart: bool,                  // padrão: false
    pub start_minimized: bool,            // padrão: true (só tem efeito com autostart / --start-hidden)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior { Auto, AlwaysTray, AlwaysQuit }

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct QuakeSettings {
    pub enabled: bool,                    // padrão: true
    pub accelerator: String,              // padrão: "Ctrl+Alt+Backquote"
    pub mode: QuakeMode,                  // padrão: Normal
    pub drop_down_height_pct: u8,         // 20..=100, padrão 45
    pub always_on_top: bool,              // padrão: true (só drop-down)
    pub hide_on_blur: bool,               // padrão: false
    pub preferred_backend: ShortcutBackendPreference, // Auto | Plugin | Portal | CliOnly
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuakeMode { Normal, DropDown }

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct EditorSettings {
    pub kind: EditorKind,                 // padrão: VsCode
    pub custom_argv: Vec<String>,         // ex.: ["zed", "{file}:{line}:{column}"]
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct NotificationSettings {
    pub enabled: bool,                    // padrão: true
    pub long_command_enabled: bool,       // padrão: true
    pub long_command_threshold_secs: u32, // 3..=3600, padrão 15
    pub only_when_unfocused: bool,        // padrão: true
}

/// Patch parcial vindo da UI (todos os campos opcionais, aplicados com merge profundo).
#[derive(Debug, Clone, Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct OsSettingsPatch {
    pub close_behavior: Option<CloseBehavior>,
    pub quake: Option<QuakeSettingsPatch>,
    pub editor: Option<EditorSettings>,
    pub notifications: Option<NotificationSettingsPatch>,
    pub autostart: Option<bool>,
    pub start_minimized: Option<bool>,
}

impl OsSettings {
    pub fn apply_patch(&self, patch: OsSettingsPatch) -> MtResult<(OsSettings, SettingsDiff)>;
    pub fn validate(&self) -> MtResult<()>; // faixas numéricas, acelerador parseável, custom_argv não vazio com {file}
}

/// Quais subsistemas precisam ser reaplicados após uma alteração.
#[derive(Debug, Default, Clone, Copy)]
pub struct SettingsDiff { pub shortcut: bool, pub autostart: bool, pub quake_geometry: bool, pub editor: bool }
```
`#[serde(default)]` garante que JSON antigo sem campos novos desserialize (compatibilidade para fases futuras).

### 2.3. `environment.rs`
```rust
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OsEnvironment {
    pub display_server: DisplayServer,
    pub compositor: Compositor,
    pub tray_available: bool,                    // org.kde.StatusNotifierWatcher tem dono no D-Bus
    pub portal_global_shortcuts: bool,           // interface org.freedesktop.portal.GlobalShortcuts presente
    pub notification_server: Option<String>,     // GetServerInformation().name (ex.: "Plasma", "mako")
    pub notification_actions: bool,              // GetCapabilities() contém "actions"
    pub editors: Vec<EditorAvailability>,
    pub window_positioning: bool,                // false em Wayland
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayServer { X11, Wayland, XWayland, Unknown }

#[derive(Debug, Clone, Copy, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Compositor { KdePlasma, Hyprland, Gnome, Sway, Other }

/// Função pura: classifica a partir de um mapa de variáveis (testável).
pub fn classify_display(env: &impl Fn(&str) -> Option<String>) -> (DisplayServer, Compositor);
/// Consultas D-Bus assíncronas (timeout de 500 ms cada, resultado em cache até `refresh()`).
pub async fn probe(env: &impl Fn(&str) -> Option<String>) -> OsEnvironment;
```
Regras de `classify_display`: `WAYLAND_DISPLAY` definido ⇒ `Wayland`, exceto se `GDK_BACKEND=x11` ⇒ `XWayland`;
`XDG_SESSION_TYPE=x11` sem `WAYLAND_DISPLAY` ⇒ `X11`. Compositor por `HYPRLAND_INSTANCE_SIGNATURE` ⇒ `Hyprland`,
`XDG_CURRENT_DESKTOP` contém `KDE` ⇒ `KdePlasma`, `GNOME` ⇒ `Gnome`, `SWAYSOCK` ⇒ `Sway`.

### 2.4. `status.rs` e `attention.rs` — agregação de estados
```rust
/// Implementado no binário com dados da Fase 05 (TabStatus + seen) e Fase 07 (TabId → ProjectId).
pub trait TabStatusProvider: Send + Sync + 'static {
    fn snapshot(&self) -> Vec<TabStatusEntry>;
    fn project_of(&self, tab_id: &TabId) -> Option<ProjectId>;
}

#[derive(Debug, Clone)]
pub struct TabStatusEntry { pub tab_id: TabId, pub status: TabStatus, pub seen: bool, pub title: String }

#[derive(Debug, Clone, Copy, Default, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatusCounters {
    pub running: u32, pub continuous: u32,
    pub finished_unseen: u32, pub error_unseen: u32,
    pub idle: u32, pub awaiting_input: u32,
}
impl StatusCounters {
    pub fn aggregate(entries: &[TabStatusEntry], attention: &AttentionRegistry) -> Self; // puro
    pub fn has_active(&self) -> bool { self.running + self.continuous > 0 } // só apresentação, não política de fechar
    pub fn needs_attention(&self) -> bool { self.error_unseen + self.awaiting_input > 0 }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason { AwaitingInput, AwaitingApproval }

/// Ponto de extensão para a Fase 13: o detector de agente chama set/clear.
#[derive(Default)]
pub struct AttentionRegistry { inner: parking_lot::RwLock<HashMap<TabId, AttentionReason>> }
impl AttentionRegistry {
    pub fn set(&self, tab: TabId, reason: AttentionReason) -> bool;  // true se mudou
    pub fn clear(&self, tab: &TabId) -> bool;
    pub fn count(&self) -> u32;
    pub fn get(&self, tab: &TabId) -> Option<AttentionReason>;
}
```
`TabStatus` é o enum definido na Fase 05 (re-exportado por ela); `mt-os` recebe-o via `mt-core` ou por re-export do
adaptador — se a Fase 05 o mantiver em `mt-shell`, o adaptador converte para `mt_os::status::TabStatusKind` espelho
(`Running | Continuous | Finished | Error | Idle`).

O adaptador preenche `TabStatusEntry.status` com **`badge_bucket`**, evitando que `finished/error` já vistos sumam do
total de abas ociosas. Cada aba entra em exatamente um dos cinco buckets; atenção é um contador transversal.
Adicionar `TabStatusProvider::activity() -> ActivitySnapshot` separado dos contadores:

```rust
pub struct ActivitySnapshot {
    pub active_panes: u32,
    pub pending_spawns: u32,
    pub supervised_busy: bool,
    pub incomplete: bool,
}
```

`has_active` do fechamento é verdadeiro se qualquer painel vivo está `Running/Continuous`, existe spawn pendente,
processo ocupado conhecido pela Fase 02 ou snapshot incompleto. Shell ocioso sozinho não é atividade. `Error` agregado
com servidor em outro split e agente vivo aguardando intervenção não permitem saída automática indevida. Snapshot
é obtido no Rust no instante da decisão; sem cache debounce do tray. Após confirmar saída, bloquear novos spawns.
`AttentionRegistry::clear` ocorre ao resolver a intervenção ou remover a aba; apenas focar/`seen` não significa que o
agente foi atendido. Se necessário, manter supressão visual por revisão separada da razão funcional de atenção.

### 2.5. `tray.rs`
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TrayVisualState { Idle, Active, Attention }

impl TrayVisualState {
    pub fn from_counters(c: &StatusCounters) -> Self {
        if c.needs_attention() { Self::Attention } else if c.has_active() { Self::Active } else { Self::Idle }
    }
}

pub struct TrayController<R: Runtime> {
    tray: TrayIcon<R>,
    items: TrayMenuItems<R>,           // handles de MenuItem para set_text sem reconstruir o menu
    last: parking_lot::Mutex<(StatusCounters, TrayVisualState, bool /*window_visible*/)>,
    refresh_tx: tokio::sync::mpsc::Sender<()>,
}

impl<R: Runtime> TrayController<R> {
    pub fn init(app: &AppHandle<R>, os: Weak<OsIntegration<R>>) -> MtResult<Self>;
    /// Agenda atualização (coalescida em 250 ms). Chamado em `tab://state-changed`, attention e visibilidade.
    pub fn request_refresh(&self);
    fn apply(&self, counters: StatusCounters, visible: bool) -> MtResult<()>; // set_icon só se o estado mudou
}
```
Menu (ids estáveis `tray.*`):
| id | Texto (pt-BR) | Tipo |
|----|---------------|------|
| `tray.toggle` | "Mostrar MaxTerminal" / "Ocultar MaxTerminal" | item |
| — | separador | |
| `tray.count.running` | "● Executando: N" | desabilitado |
| `tray.count.continuous` | "● Execução contínua: N" | desabilitado |
| `tray.count.finished` | "● Finalizado: N" | desabilitado |
| `tray.count.error` | "● Com erro: N" | desabilitado |
| `tray.count.awaiting` | "● Aguardando intervenção: N" | desabilitado, inclusive quando N = 0 (`set_enabled` não oculta item) |
| — | separador | |
| `tray.preferences` | "Preferências…" | item → mostra janela + emite `os://open-preferences` |
| `tray.quit` | "Sair" | item → `ShutdownCoordinator::request_quit(QuitSource::Tray)` |

Observações Linux: `libayatana-appindicator` não emite clique esquerdo; `set_tooltip` é ignorado — usa-se
`set_title(Some("N ativas"))` quando suportado (Plasma exibe o rótulo). Ícones embutidos: `include_bytes!("../icons/tray/*.png")`.

`tray_available` exige watcher **e** ícone registrado com sucesso; observar `NameOwnerChanged` no D-Bus. Se o watcher
desaparecer enquanto oculto, executar recuperação `unminimize + show`, reabilitar entrada na taskbar e emitir ambiente
atualizado. Se minimizar não for suportado/confirmado pelo compositor, deixar visível e informar indisponibilidade.
Atualizações Tauri/GTK de menu/janela são despachadas para a thread apropriada; consultas D-Bus ficam fora dela.

### 2.6. `lifecycle.rs`
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision { HideToTray, Minimize, Quit, ConfirmQuit }

#[derive(Debug, Clone, Copy)]
pub struct CloseContext { pub behavior: CloseBehavior, pub has_active: bool, pub tray_available: bool, pub force_quit: bool }

/// Função pura — tabela-verdade testada exaustivamente (CA-02).
pub fn decide_close(ctx: CloseContext) -> CloseDecision {
    use CloseBehavior::*;
    if ctx.force_quit { return CloseDecision::Quit; }
    let keep_alive = match ctx.behavior { AlwaysTray => true, AlwaysQuit => false, Auto => ctx.has_active };
    match (keep_alive, ctx.tray_available, ctx.behavior, ctx.has_active) {
        (true, true, ..) => CloseDecision::HideToTray,
        (true, false, ..) => CloseDecision::Minimize,
        (false, _, AlwaysQuit, true) => CloseDecision::ConfirmQuit,
        (false, ..) => CloseDecision::Quit,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum QuitSource { Window, Tray, Ipc, Signal, Cli }

/// Implementado no binário: Fase 02 (PTYs) e Fase 03 (WAL).
#[async_trait::async_trait]
pub trait ShutdownHooks: Send + Sync + 'static {
    async fn freeze_spawns(&self) -> MtResult<()>;
    async fn shutdown_workspace(&self) -> MtResult<()>; // 07: PTYs, recorder, sessões app_exit
    async fn shutdown_ptys(&self, grace: Duration) -> ShutdownReport;
    async fn flush_db(&self) -> MtResult<()>; // drena writer/captura e tenta checkpoint, sem depender dele para durabilidade
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownReport { pub terminated: u32, pub killed: u32, pub survivors: Vec<u32> /* PIDs */ }

pub struct ShutdownCoordinator<R: Runtime> { app: AppHandle<R>, hooks: Arc<dyn ShutdownHooks>, phase: Mutex<ShutdownPhase>, exit_authorized: AtomicBool }
pub enum ShutdownPhase { Idle, Confirming, Stopping, Draining, Failed, ReadyToExit }
impl<R: Runtime> ShutdownCoordinator<R> {
    /// Pergunta (diálogo nativo) se houver abas ativas e a origem não for `Signal`.
    pub async fn request_quit(&self, source: QuitSource) -> MtResult<QuitOutcome>;
    /// Encerra sem nova confirmação; só autoriza saída após relatório sem sobreviventes.
    pub async fn shutdown(&self, source: QuitSource) -> MtResult<ShutdownReport>;
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum QuitOutcome { Quitting, Cancelled }
```
Diálogo: `app.dialog().message("Existem N abas em execução. Sair encerrará todos os processos (inclusive servidores).")
.title("Sair do MaxTerminal?").kind(MessageDialogKind::Warning).buttons(OkCancelCustom("Sair e encerrar tudo", "Cancelar"))`.

Integração no binário:
- `on_window_event(CloseRequested { api })` ⇒ `api.prevent_close()` e aplica `decide_close` (`HideToTray` → `window.hide()`;
  `Minimize` → `window.minimize()`; `ConfirmQuit`/`Quit` → `request_quit(Window)`).
- `RunEvent::ExitRequested { api, .. }` ⇒ `api.prevent_exit()` enquanto `exit_authorized == false`, mesmo com
  `code: Some(_)`; rotear solicitação ao coordenador. Só o coordenador marca `ReadyToExit` e chama `app.exit(0)`.
- Sinais `SIGTERM`/`SIGINT`/`SIGHUP` via `tokio::signal::unix` ⇒ `shutdown(Signal)` sem diálogo.

Sequência única: `Idle → Confirming → Stopping → Draining → ReadyToExit`. A confirmação é assíncrona, usa contagem
de **abas/painéis**, não chama essa contagem de PIDs. Cancelar volta a `Idle`. Requisições concorrentes compartilham
o resultado em andamento, inclusive durante o diálogo; sinal avança para parada sem esperar resposta interativa.
Após `freeze_spawns`, `shutdown_workspace` da 07 encerra PTYs via 02, aguarda callbacks/taps de saída, drena recorder
e arquiva sessões `app_exit`. `shutdown_ptys` reconcilia possíveis PTYs remanescentes fora do workspace, sem reiniciar
os prazos de uma parada já em curso. Só depois drenar ShellService/DbWriter e fazer checkpoint WAL limitado.
O hook `RunEvent::Exit` da 07 torna-se apenas verificação final idempotente: já é tarde para iniciar cleanup assíncrono.

Hiding não chama nenhum desses hooks: PTYs, leitores, supervisão `/proc`, captura e writers permanecem ativos.
Atualizações de layout e `Channel` respeitam as regras de backpressure da Fase 02 mesmo com webview oculta.
SIGTERM/INT/HUP são pedidos gerenciáveis, não garantia contra SIGKILL, queda de energia ou processo preso no kernel.
Se `survivors` não esvaziar após escalada da 02, `Failed` mantém supervisor vivo, mostra janela/erro e permite repetir
limpeza; **não** executar `app.exit(0)` abandonando filhos. Não sinalizar PIDs reutilizados: usar identidade PID/start-time
da 02. Falha no checkpoint por leitor ocupado é reportada, preservando WAL válido; nunca apagar WAL para forçar saída.
O `force_quit` da decisão pura representa somente aprovação interna já obtida, não um parâmetro público de bypass.

### 2.7. `quake.rs`
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleDecision { ShowAndFocus, Focus, Hide }

/// Puro: oculto/minimizado → mostrar; visível sem foco → focar; visível com foco → ocultar.
pub fn decide_toggle(visible: bool, focused: bool, minimized: bool) -> ToggleDecision;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropDownGeometry { pub x: i32, pub y: i32, pub width: u32, pub height: u32 }

/// Puro: monitor (posição/tamanho físicos, scale) → geometria topo/largura total/altura %.
pub fn drop_down_geometry(monitor_pos: (i32, i32), monitor_size: (u32, u32), height_pct: u8) -> DropDownGeometry;

pub struct QuakeController<R: Runtime> { app: AppHandle<R>, settings: Arc<RwLock<OsSettings>>, env: Arc<RwLock<OsEnvironment>>, last_toggle: Mutex<Instant> }
impl<R: Runtime> QuakeController<R> {
    pub fn toggle(&self, source: ToggleSource) -> MtResult<ToggleDecision>; // debounce 150 ms (repetição de tecla)
    pub fn show_and_focus(&self) -> MtResult<()>;  // unminimize + show + set_focus; drop-down: posiciona antes de show
    pub fn hide(&self) -> MtResult<()>;
    pub fn on_blur(&self);                          // hide_on_blur && mode == DropDown ⇒ hide após 120 ms se ainda sem foco
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ToggleSource { Shortcut, Portal, Cli, Tray, Ipc }
```
Drop-down: escolher monitor sob o cursor pelas APIs disponíveis na versão Tauri fixada (X11), com fallback ao monitor
atual/primário. Coordenadas físicas inclusive negativas; não multiplicar escala duas vezes. Salvar geometria,
decorações, taskbar e always-on-top originais ao entrar; restaurá-los ao sair do modo. `set_skip_taskbar(true)` só
quando houver rota de retorno comprovada. Em Wayland posição/foco/topmost podem ser recusados ou ignorados: não
declarar sucesso geométrico pelo retorno de `show`; retornar estado degradado e permitir janela normal. Tokens de
ativação do portal só são usados se o backend realmente os expuser; não presumir API Tauri de token ou mover workspace.
Ajuda do compositor indica identidade/app-id real e versão testada; não fornecer sintaxe `windowrulev2` universal.
Suspender `hide_on_blur` enquanto houver modal/portal/menu em interação. Ao ocultar sem tray nem atalho utilizável,
preferir minimizar, mantendo taskbar; se falhar, manter visível. Debounce só em repetição do atalho, não em `--show`.

### 2.8. `shortcut/` — atalho global com cadeia de backends
```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutBackendKind { Plugin, Portal, CliOnly }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutBackendPreference { #[default] Auto, Plugin, Portal, CliOnly }

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutStatus {
    pub backend: ShortcutBackendKind,
    pub accelerator: String,          // conforme configurado
    pub effective_trigger: Option<String>, // portal: descrição retornada pelo compositor
    pub registered: bool,
    pub error: Option<MtError>,
    pub cli_command: String,          // "maxterminal --toggle"
    pub compositor_hint: Option<String>, // snippet pronto (Hyprland/KDE)
}

#[async_trait::async_trait]
pub trait GlobalShortcutBackend: Send + Sync {
    fn kind(&self) -> ShortcutBackendKind;
    async fn register(&self, accelerator: &Accelerator, on_trigger: Arc<dyn Fn() + Send + Sync>) -> MtResult<Option<String>>;
    async fn unregister(&self) -> MtResult<()>;
}

pub struct ShortcutManager { active: tokio::sync::Mutex<Option<Box<dyn GlobalShortcutBackend>>>, status: RwLock<ShortcutStatus> }
impl ShortcutManager {
    /// Ordem: preferência explícita, senão Auto: X11 ⇒ Plugin; Wayland ⇒ Portal (se disponível) ⇒ CliOnly.
    /// Em falha mantém o backend/atalho anterior e retorna o erro (CA-19).
    pub async fn apply(&self, settings: &QuakeSettings, env: &OsEnvironment, trigger: Arc<dyn Fn() + Send + Sync>) -> ShortcutStatus;
    pub fn status(&self) -> ShortcutStatus;
}
```
- `accelerator.rs`: `Accelerator::parse(&str) -> MtResult<Accelerator>` (modificadores `Ctrl|Alt|Shift|Super` + tecla
  `KeyboardEvent.code` normalizada); `to_plugin_string()` (formato do `tauri-plugin-global-shortcut`) e
  `to_portal_trigger()` (formato XDG `CTRL+ALT+grave`).
- `plugin_backend.rs`: `app.global_shortcut().on_shortcut(sc, |_, _, ev| if ev.state == Pressed { trigger() })`;
  normalizar erro de grab para `os.shortcut_conflict` quando essa for a causa; não presumir variante `AlreadyRegistered`
  para conflitos externos. `is_registered` consulta o registro do app, não detecta todos os atalhos do desktop.
- `portal_backend.rs`: fluxo conceitual `CreateSession → BindShortcuts → Activated`, usando os tipos de sessão,
  requests e respostas do `ashpd` fixado. Identificador estável `quake-toggle`, descrição pt-BR e preferred trigger
  são sugestões; compositor pode conceder outro acelerador. Aguardar resposta/concessão, manter sessão e stream vivos,
  filtrar session handle e shortcut id; renovar após perda do portal. Negação/cancelamento não abre diálogos em loop.
  `os_shortcut_configure_portal` só oferece `ConfigureShortcuts` se versão/interface do portal e cliente suportarem;
  caso contrário, mostra ajuda de configuração do sistema. Não chamar método inexistente no `ashpd` instalado.
- `cli_backend.rs`: não registra nada; status `registered = false`, `compositor_hint` por compositor:
  - Hyprland: `bind = SUPER, grave, exec, maxterminal --toggle`
  - KDE: "Configurações do Sistema > Atalhos > Adicionar comando > `maxterminal --toggle`"

`XWayland` em sessão Wayland usa a política Wayland: um grab X11 não basta. Em startup, falha → `CliOnly`; durante
edição, preservar backend anterior até confirmar o novo quando possível. Operações serializadas; se for necessário
desregistrar primeiro, registrar compensação e expor falha de rollback como estado degradado. `quake.enabled=false`
libera recurso do plugin/portal sem desligar CLI/tray. Nunca persistir o acelerador solicitado como se fosse o trigger
efetivo concedido. O estado do backend é observado após suspend/resume e reinício de D-Bus, com retry limitado.

### 2.9. `cli.rs` — single-instance
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction { Toggle, Show, Hide, StartHidden, None }

/// Whitelist estrita; qualquer outro argumento é ignorado e logado em `debug` (sem eco do valor).
pub fn parse_cli_args<I: IntoIterator<Item = S>, S: AsRef<str>>(args: I) -> CliAction;

/// Callback de `tauri_plugin_single_instance::init(|app, argv, cwd| ...)`.
pub fn handle_second_instance<R: Runtime>(app: &AppHandle<R>, argv: Vec<String>);
```
Primeira instância: ignorar argv[0], interpretar flags sem executar caminhos/comandos. Prioridade explícita:
`--show > --toggle > --hide > --start-hidden`; `--toggle` no cold start equivale a mostrar. `--start-hidden` só oculta se
tray comprovada, senão mostra. Segunda instância sem ação nem URI ⇒ `Show`; `--start-hidden` não esconde instância já
aberta. `cwd` recebido do segundo processo não modifica cwd de nenhum terminal.
Janela inicialmente `visible: false`; após primeiro render a UI chama **`os_ui_ready`**, não `os_window_show`.
O Rust aplica intenção de startup preservada. Watchdog de 3 s mostra janela se startup visível ficou sem handshake;
modo minimizado saudável permanece oculto. Callback de segunda instância pode ocorrer antes de `OsIntegration`
estar gerenciada: usar inbox criada antes do builder, limitada e drenada após setup, sem `state()` que cause panic.

### 2.9.1. `activation.rs` — deep links como infraestrutura da Fase 09

Configuração estática no binário:
```json
{ "plugins": { "deep-link": { "desktop": { "schemes": ["maxterminal"] } } } }
```

Registrar `tauri_plugin_single_instance::init(...)` com feature `deep-link` **antes** de
`tauri_plugin_deep_link::init()`. No Linux o segundo processo recebe URI no argv e o plugin encaminha à instância
existente. Consumir URL inicial (`DeepLinkExt`, `get_current`) e novas aberturas (`on_open_url`) em um router Rust;
assinar antes de consultar snapshot e deduplicar a sobreposição. Callback single-instance trata somente flags e
fallback sem URI; **não** reenfileira a mesma URI encaminhada automaticamente pelo plugin.

```rust
pub struct ActivationEnvelope {
    pub id: uuid::Uuid,
    pub uri: url::Url,
    pub received_at_ms: i64,
    pub received_instant: std::time::Instant,
    pub source: ActivationSource,
    pub params: Vec<(String, String)>, // query params validados estruturalmente e repassados ao handler (worktree, correlationId...)
}
pub enum ActivationSource { Startup, DeepLink }
#[async_trait::async_trait]
pub trait ActivationHandler: Send + Sync {
    fn accepts(&self, uri: &url::Url) -> bool;
    async fn handle(&self, envelope: &ActivationEnvelope) -> MtResult<()>;
}
pub struct ActivationRouter { /* inbox limitada, handlers, prontidão e dedupe */ }
impl ActivationRouter {
    pub async fn register_handler(&self, handler: Arc<dyn ActivationHandler>) -> MtResult<()>;
    pub async fn enqueue(&self, envelope: ActivationEnvelope) -> MtResult<()>;
    pub async fn set_ready(&self) -> MtResult<()>;
}
```

Usar UUID do workspace (dependência `uuid` compartilhada), envelope somente Rust: não publicar URI completa em
eventos/logs nem persistir queries que possam conter credenciais. Aceitar scheme exato `maxterminal`, UTF-8/URL válido,
≤ 8 KiB, sem NUL/controles, usuário/senha/porta/fragmento; rejeitar escapes percentuais inválidos. Query params são extraídos
para `ActivationEnvelope.params` (≤ 16 pares, chave `[A-Za-z0-9_-]{1,64}`, valor ≤ 4096 bytes, sem repetição) e **repassados**
ao handler — nunca rejeitados por serem chaves de negócio (`worktree`, `correlationId`). Esta fase mantém o **único**
listener `on_open_url`/`get_current`; consumidores só registram `ActivationHandler`. Fila ≤ 32 envelopes,
TTL 30 s, processamento serial. Dedupe de transporte (mesma URL ≤ 1 s) elimina duplicação de entrega; dedupe de negócio
por request id/autenticação pertence à Fase 11. Saturação retorna `os.activation_busy`; expiração/rota sem handler →
`os.activation_unavailable`; conteúdo inválido → `os.activation_invalid`.

Registrar handlers no composition root antes de `set_ready`; prontidão exige setup concluído, workspace restaurado
e consumidor pronto. Sem Fase 11, handlers fake em testes comprovam cold/warm start; em produção desta fase URLs
de negócio resultam em indisponibilidade legível, sem abrir projeto, executar comando ou confiar no remetente.
A Fase 11 implementa parser/autorização das próprias rotas e registra `ActivationHandler`; `mt-os` não conhece
`mt-bridge`. URI externa é entrada não autenticada mesmo quando encaminhada via single-instance.

O scheme `vscode` é do editor; o app registra apenas `maxterminal`. Em desenvolvimento, não sobrescrever associação
MIME do aplicativo instalado automaticamente; permitir `register_all()` explicitamente no perfil de teste. Pacote
instalado usa `.desktop` com identidade estável e handler `x-scheme-handler/maxterminal`, validado na Fase 17.
AppImage movido exige reconciliação da associação com o executável atual; erro de registro não interrompe PTYs.

### 2.10. `links.rs` — resolução de links
```rust
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkCandidate { pub key: String, pub path: String, pub line: Option<u32>, pub column: Option<u32> }

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLink { pub key: String, pub absolute_path: String, pub line: Option<u32>, pub column: Option<u32> }

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkContext { pub tab_id: TabId, pub pane_id: PaneId, pub pty_id: PtyId, pub cwd_epoch: u32 }

/// Implementado no binário: cwd do painel via Fase 05 (OSC 7/rastreamento) com fallback `/proc/<pid_shell>/cwd` (Fase 02).
pub trait TabCwdProvider: Send + Sync + 'static {
    fn cwd(&self, context: &LinkContext) -> MtResult<Option<PathBuf>>;
}

pub struct LinkResolver { cwd: Arc<dyn TabCwdProvider>, home: PathBuf }
impl LinkResolver {
    pub const MAX_BATCH: usize = 64;
    pub const MAX_PATH_LEN: usize = 4096;
    /// Retorna apenas candidatos que existem e são arquivos regulares (após seguir symlinks).
    pub async fn resolve(&self, context: &LinkContext, candidates: Vec<LinkCandidate>) -> MtResult<Vec<ResolvedLink>>;
    /// Puro: junta cwd/home/path sem tocar disco (`~` ⇒ home; absoluto mantém; relativo ⇒ cwd.join).
    pub fn join(cwd: &Path, home: &Path, raw: &str) -> Option<PathBuf>;
}
```
Regras: `line`/`column` = 0 ou > 10⁷ ⇒ candidato rejeitado, sem transformar overflow em trecho parcialmente válido;
`column` sem `line` é inválido. `tokio::fs::metadata` em paralelo (`buffer_unordered(16)`/semáforo, não `join_all`
ilimitado); canonicalizar antes de verificar se destino é arquivo regular. Links para fora do cwd são
**permitidos** (logs de compiladores citam `/usr/include/...`), pois o efeito é apenas abrir no editor.

O backend verifica vínculo aba/painel/PTY e geração antes e depois do I/O; mudança retorna `os.link_context_stale`.
Geração de cwd é mantida no Rust a partir da 05/07, exposta no snapshot e `os://link-context-changed` (por painel);
não confiar no cwd enviado pelo frontend. Ao clicar, revalidar contexto e arquivo: erro por arquivo removido, aba
fechada ou PTY trocado é esperado. Isso reduz corrida, sem alegar abertura atômica em editor externo (TOCTOU existe).
Não expandir variáveis, globs nem `~outro_usuario`; apenas `~/` do usuário atual. Recusar NUL/controles e URLs, que
pertencem ao provider web da 04. Histórico sem PTY usa cwd persistido do bloco por adaptador específico, sem inventar
PTY vivo; a primeira entrega cobre links dos terminais vivos.

### 2.11. `editor.rs`
```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EditorKind { #[default] VsCode, VsCodeInsiders, MaxCode, Custom }

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditorAvailability { pub kind: EditorKind, pub available: bool, pub binary: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorInvocation { Exec { program: PathBuf, args: Vec<String> }, OpenUri(String) }

/// Puro (CA-02): VsCode ⇒ `code --goto <abs>:<l>:<c>` (l/c padrão 1); Insiders ⇒ `code-insiders --goto …`;
/// MaxCode ⇒ `vscode://file/<abs percent-encoded, '/' preservado>:<l>:<c>`; Custom ⇒ substitui `{file}`, `{line}`,
/// `{column}` dentro de cada elemento de `custom_argv` (nunca concatena elementos, nunca usa shell).
pub fn build_editor_invocation(settings: &EditorSettings, link: &ResolvedLink) -> MtResult<EditorInvocation>;

/// Ponto de extensão: Fase 11 registra um opener que envia ao MaxCode via ponte loopback (prioridade sobre o URI).
#[async_trait::async_trait]
pub trait EditorOpener: Send + Sync {
    fn handles(&self, kind: EditorKind) -> bool;
    async fn open(&self, link: &ResolvedLink) -> MtResult<()>;
}

pub struct EditorLauncher { bridge_opener: RwLock<Option<Arc<dyn EditorOpener>>>, openers: RwLock<Vec<Arc<dyn EditorOpener>>>, settings: Arc<RwLock<OsSettings>> }
impl EditorLauncher {
    pub fn register_opener(&self, opener: Arc<dyn EditorOpener>); // inserido no início (maior prioridade)
    pub fn set_bridge_opener(&self, opener: Arc<dyn EditorOpener>); // slot dedicado, substituição idempotente, prioridade sobre openers/fallback
    pub async fn open(&self, link: &ResolvedLink) -> MtResult<()>;
    pub fn detect() -> Vec<EditorAvailability>; // `which::which("code")`, `code-insiders`; MaxCode = `code` + `xdg-open`
}
```
Execução `Exec`: argv vetorizado via `tokio::process::Command`, stdin/stdout/stderr nulos; task supervisiona e faz reap
do launcher, sem aguardar fechamento do editor para retornar à UI. Não tratar editor externo como PTY da aplicação.
Execução `OpenUri`: `OpenerExt` e `app.opener().open_url(uri, None::<&str>)`, respeitando scheme/host gerados pelo
backend. URI canônica de exemplo: `vscode://file/home/usuario/projeto/src/main.ts:42:1` (sem barra duplicada após
`file`); percent-encode UTF-8, espaços, `%`, `#`, `?` e `:` do caminho, preservando `/`, depois anexar linha/coluna.
Esse URI abre **arquivo no VS Code**; não comprova que a extensão MaxCode está instalada. Ponte/ações da extensão
`vscode://johnattas.maxcode/...` pertencem à Fase 11. `detect()` informa separadamente binário e handler MIME;
não desabilitar URI funcional apenas porque `code` não está no PATH. Falha da ponte pode usar fallback explícito
do opener, sem reenviar comando de negócio. Limites de argv são aplicados ao template e ao resultado expandido.

### 2.12. `notify.rs` — notificações nativas
```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind { CommandFinished, CommandFailed, AgentAwaitingInput, Info }

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotifyRequest { pub kind: NotificationKind, pub title: String, pub body: String, pub tab_id: Option<TabId> }

#[derive(Debug, Clone, Copy, Serialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotifyOutcome { Shown, Replaced, Suppressed, Disabled, Failed }

pub struct Notifier<R: Runtime> {
    app: AppHandle<R>,
    limiter: Mutex<RateLimiter>,
    active: Mutex<HashMap<(Option<TabId>, NotificationKind), u32 /* replaces_id D-Bus */>>,
    settings: Arc<RwLock<OsSettings>>,
    env: Arc<RwLock<OsEnvironment>>,
}

impl<R: Runtime> Notifier<R> {
    /// API canônica consumida pela própria Fase 09 (long_command) e pela Fase 13.
    pub async fn notify_user(&self, kind: NotificationKind, title: &str, body: &str, tab_id: Option<TabId>) -> NotifyOutcome;
}

/// Puro: dedupe por (tab_id, kind, hash(title+body)) em janela de 10 s; teto global de 5 notificações / 30 s
/// (AgentAwaitingInput também respeita o teto; mantém atenção na UI quando suprimido).
pub struct RateLimiter { /* VecDeque<Instant>, HashMap<u64, Instant> */ }
impl RateLimiter { pub fn check(&mut self, key: u64, kind: NotificationKind, now: Instant) -> LimitDecision; }
pub enum LimitDecision { Allow, Replace(u64), Suppress }
```
Fluxo de `notify_user`: `enabled`? → sanitiza `title`/`body` com `mt_db::sanitizer::sanitize()` → trunca (título 80,
corpo 200 caracteres) → `RateLimiter` → `notify_rust::Notification::new().appname("MaxTerminal").icon("maxterminal")
.summary(..).body(..).urgency(kind.urgency()).hint(Hint::Category(kind.category())).action("default", "Abrir aba")
 .id(replaces_id)` → envio pelo adaptador assíncrono disponível na versão fixada.
Urgência: `CommandFailed`/`AgentAwaitingInput` ⇒ `Critical` só se a aba não estiver visível; demais `Normal`.
Sem suporte a ações, ainda enviar pelo D-Bus em texto simples e marcar `clickSupported=false`; fallback pelo plugin
de notificação só quando esse adaptador falhar e o plugin puder entregar. Servidor ausente não vira sucesso fictício.

`focus_tab(tab_id)`: `QuakeController::show_and_focus()` e emissão de `os://focus-tab` com `{ tabId, projectId, source: 'notification' }`.

Implementação preferida de ações: conexão `zbus` compartilhada com streams `ActionInvoked`/`NotificationClosed`,
mapa limitado a 32 notificações, TTL 5 min, remoção em fechamento/expiração, cancellation token no shutdown. Se
`notify-rust` exigir `wait_for_action`, não criar uma task bloqueante ilimitada por mensagem nem supor que timeout
em `JoinHandle` cancela a chamada; usar adaptador D-Bus cancelável para as ações. Preservar handles/ids de substituição
somente enquanto válidos e ignorar cliques atrasados em abas fechadas (apenas mostrar app). `NotifyRequest` pode
receber `pane_id: Option<PaneId>` aditivo; validar vínculo antes do foco. Notificação negada pelo sistema ou DND
não garante exibição visual: `Shown` significa envio aceito, não confirmação de leitura. Mapear permissão do plugin
quando aplicável, sem pedir autorização repetidamente. Rate limiter inclui todos os kinds e a atenção continua viva.

### 2.13. `long_command.rs`
Assina no binário `ShellService::subscribe()`/`CommandEvent::Finished` da 05 e projeta
`CommandFinishedInfo { block_id, tab_id, pane_id, command, exit_code: Option<i32>, duration_ms, end_reason }`:
```rust
pub fn should_notify(info: &CommandFinishedInfo, s: &NotificationSettings, window_focused: bool, tab_visible: bool) -> Option<NotificationKind>;
// None se !enabled || !long_command_enabled || duration < threshold || (only_when_unfocused && window_focused)
// None em shutdown, MissingEnd, ModeSwitched ou exit_code desconhecido; não inventar sucesso/falha.
// Some(CommandFailed) se Some(código != 0), Some(CommandFinished) se Some(0).
```
Título: "Comando concluído" / "Comando falhou"; corpo: `"{comando_sanitizado} — {duração formatada} (exit {código})"`.

### 2.14. `autostart.rs`
```rust
pub struct AutostartSync;
impl AutostartSync {
    /// `tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--start-hidden"]))` no binário.
    pub fn apply<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> MtResult<AutostartStatus>;
    pub fn status<R: Runtime>(app: &AppHandle<R>) -> MtResult<AutostartStatus>;
}
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AutostartStatus { pub enabled: bool, pub desktop_file: Option<String> }
```
No startup, se `settings.autostart != status.enabled` (usuário apagou o `.desktop` manualmente), o arquivo prevalece e
o setting é corrigido (fonte da verdade = sistema).

`start_minimized` precisa ter efeito real: argumentos do launcher refletem o setting. Como `init` configura args
no startup, alternar essa preferência deve reconciliar a entrada autostart por API suportada ou adaptador `.desktop`
próprio, com escrita atômica e escaping Desktop Entry (nunca shell). Não manter `--start-hidden` fixo ignorando o setting.
Falha de leitura do sistema não equivale a `enabled=false`; preservar setting e expor erro. Respeitar
`$XDG_CONFIG_HOME/autostart` (fallback `~/.config/autostart`) e caminho estável do executável instalado/AppImage.

### 2.15. `lib.rs` — fachada
```rust
pub struct OsDeps {
    pub status: Arc<dyn TabStatusProvider>,
    pub cwd: Arc<dyn TabCwdProvider>,
    pub shutdown: Arc<dyn ShutdownHooks>,
    pub settings_repo: Arc<mt_db::repos::os_settings::OsSettingsRepo>,
}

pub struct OsIntegration<R: Runtime> {
    pub settings: Arc<RwLock<OsSettings>>,
    pub env: Arc<RwLock<OsEnvironment>>,
    pub attention: Arc<AttentionRegistry>,
    pub tray: Option<TrayController<R>>,      // None se tray indisponível
    pub quake: QuakeController<R>,
    pub shortcuts: ShortcutManager,
    pub links: LinkResolver,
    pub editor: EditorLauncher,
    pub notifier: Notifier<R>,
    pub shutdown: ShutdownCoordinator<R>,
    pub activations: ActivationRouter,
    deps: OsDeps,
}

impl<R: Runtime> OsIntegration<R> {
    pub async fn init(app: &AppHandle<R>, deps: OsDeps) -> MtResult<Arc<Self>>;
    pub async fn update_settings(&self, patch: OsSettingsPatch) -> MtResult<OsSettings>;
    pub fn on_tab_state_changed(&self);                // → tray.request_refresh()
    pub fn on_window_focus(&self, focused: bool);      // → quake.on_blur, emite os://window-visibility
    pub fn close_requested(&self, force_quit: bool) -> CloseDecision;
}
```

`update_settings` serializa patches: validar/mesclar em memória → preparar mudanças externas → aplicar → persistir
via writer 03 → publicar snapshot/evento. Não manter transação SQLite aberta enquanto espera portal. Cada efeito
tem compensação (atalho/autostart/geometria); falha de DB compensa e preserva settings anteriores. Se compensação
falhar, emitir estado runtime degradado com erro e reconciliar; nunca emitir sucesso de settings não aplicados.
`schema_version` superior à suportada é erro explícito, sem sobrescrever JSON futuro. Campos desconhecidos compatíveis
são preservados na mesclagem; defaults só preenchem campos ausentes, não valores malformados.

---

## 3. Comandos Tauri IPC (`src-tauri/src/commands/os.rs`)

Todos retornam `Result<T, MtError>` e são exportados via `tauri-specta` para `src/types/bindings.ts`.

| # | Comando | Args | Retorno | Erros |
|---|---------|------|---------|-------|
| 1 | `os_settings_get` | — | `OsSettings` | `db.*` |
| 2 | `os_settings_update` | `patch: OsSettingsPatch` | `OsSettings` (aplicado) | `os.settings_invalid`, `os.shortcut_invalid`, `os.shortcut_conflict`, `os.autostart_failed` |
| 3 | `os_environment_info` | `refresh: bool` | `OsEnvironment` | — (falhas D-Bus viram `false`) |
| 4 | `os_shortcut_status` | — | `ShortcutStatus` | — |
| 5 | `os_shortcut_configure_portal` | — | `ShortcutStatus` | `os.portal_unavailable` |
| 6 | `os_window_toggle` | `source: ToggleSource` | `ToggleDecisionDto` (`'show' \| 'focus' \| 'hide'`) | `os.*` de janela |
| 7 | `os_window_show` | — | `()` | — |
| 8 | `os_window_hide` | — | `()` | — |
| 9 | `os_quit_request` | — | `QuitOutcome` | `os.shutdown_timeout` (mantém supervisão) |
| 10 | `os_link_resolve` | `context: LinkContext, candidates: LinkCandidate[]` (≤ 64) | `ResolvedLink[]` | `os.tab_not_found`, `os.link_rejected`, `os.link_context_stale` |
| 11 | `os_link_open` | `context: LinkContext, candidate: LinkCandidate` | `()` | `os.link_not_found`, `os.link_context_stale`, `os.editor_not_found`, `os.editor_launch_failed` |
| 12 | `os_notify` | `request: NotifyRequest` | `NotifyOutcome` | `os.notify_failed` |
| 13 | `os_attention_set` | `tabId: TabId, reason: AttentionReason \| null` | `StatusCounters` | `os.tab_not_found` |
| 14 | `os_get_autostart` | — | `AutostartStatus` (leitura do estado real do `.desktop`) | `os.autostart_failed` |
| 15 | `os_runtime_snapshot` | — | `OsRuntimeSnapshot` | `os.not_ready` |
| 16 | `os_ui_ready` | — | `OsRuntimeSnapshot` | `os.not_ready` |
| 17 | `os_set_autostart` | `enabled: bool` | `AutostartStatus` | `os.autostart_failed`, `os.settings_invalid` |

`os_link_open` **re-resolve** o candidato no backend (não confia em caminho absoluto vindo do frontend).

`os_get_autostart`/`os_set_autostart` são a **única** superfície de autostart do produto (a Fase 17 apenas empacota e valida).
`os_set_autostart(enabled)` equivale a `os_settings_update({ autostart: enabled })` (mesma serialização, compensação e
persistência) e devolve o `AutostartStatus` real após aplicar. O `.desktop` gerado usa a flag `--start-hidden`.

`OsRuntimeSnapshot` contém `{ revision, window: WindowVisibilityPayload, environment, shortcutStatus, counters,
shutdownPhase, linkContexts: LinkContext[], pendingFocus: FocusTabPayload | null }`; tipos definidos em Rust e gerados.
Eventos de estado carregam `revision` monotônica compatível com o snapshot. Inicialização da store: registrar listeners,
bufferizar eventos → snapshot → aplicar apenas revisões posteriores. `os_ui_ready` é idempotente e aplica intenção
inicial de visibilidade no backend. `pendingFocus` preserva seleção recebida antes da UI; consumir após hidratar 07,
validar aba ainda existente e só então usar foco. Nenhum `force` público, nem permissão de saída direta do processo.

### 3.1. Eventos emitidos

Todos os eventos `os://*` abaixo usam envelope gerado `OsEvent<T> { revision: u32, payload: T }`; a coluna Payload
descreve **T**, não o envelope. Revisão é global e monotônica na instância (reiniciada no boot, snapshot substitui
estado anterior); ao atingir limite deve renovar epoch de runtime em vez de wrap silencioso. Wrappers preservam
o envelope até a store aplicar ordenação. Eventos de ação/foco não são descartados por revisão global de outro
domínio: manter revisão por domínio e processar intenções por id/revisão próprio.

| Evento | Payload (TS) | Quando |
|--------|--------------|--------|
| `os://focus-tab` | `{ tabId: TabId; paneId: PaneId \| null; projectId: ProjectId \| null; source: 'notification' \| 'tray' \| 'cli' }` | clique em notificação; Fase 13 reutiliza |
| `os://window-visibility` | `{ visible: boolean; focused: boolean; minimized: boolean }` | show/hide/focus/blur (coalescido 50 ms) |
| `os://settings-changed` | `OsSettings` | após `os_settings_update` bem-sucedido |
| `os://shortcut-status` | `ShortcutStatus` | após (re)registro ou erro de backend |
| `os://counters` | `StatusCounters` | junto com o refresh do tray (para badge global/título) |
| `os://open-preferences` | `{ section: 'system' }` | item "Preferências…" da bandeja |
| `os://shutdown-started` | `{ source: QuitSource }` | início do encerramento (frontend exibe overlay "Encerrando processos…") |
| `os://shutdown-failed` | `{ error: MtError; phase: 'failed' }` | limpeza incompleta; remove overlay bloqueante, permite repetir |
| `os://environment-changed` | `OsEnvironment` | watcher/portal/servidor muda |
| `os://link-context-changed` | `LinkContext` | cwd/PTY do painel muda; fechamento invalida contexto no snapshot |

### 3.2. Eventos consumidos
| Evento/contrato | Fase | Uso |
|-----------------|------|-----|
| `tab://state-changed` | 05 | refresh de contadores/ícone |
| fim de comando (`CommandFinishedInfo`) | 05 | `long_command::should_notify` |
| ativação de aba (frontend, `useTabsStore`/`useProjectsStore`) | 06/07 | handler de `os://focus-tab` |

### 3.3. Capabilities (`src-tauri/capabilities/os.json`)
```json
{
    "identifier": "os",
    "description": "Comandos de integração com o sistema para a interface local",
    "windows": ["main"],
    "platforms": ["linux"],
    "permissions": [
        "core:event:allow-listen",
        "core:event:allow-unlisten",
        "allow-os-integration"
    ]
}
```
`allow-os-integration` é permissão **própria**, definida em `permissions/os.toml`, com `commands.allow` enumerando
exatamente os 17 nomes da §3. Usar `tauri_build::AppManifest::new().commands(&[...])` em `build.rs`, preservando todos
os comandos anteriores, para submeter comandos próprios à ACL; apenas registrá-los em `invoke_handler` não restringe
acesso por janela. Validar schema gerado pelo build. Não habilitar origens remotas nem curingas de webview.

O frontend chama somente `os_*`; APIs Rust não são autorizadas por capabilities JS. Portanto **não** conceder
`opener:allow-open-url`, `global-shortcut:allow-register`, `autostart:*`, `notification:*`, `dialog:*`, `deep-link:*`
ou `core:tray:*` apenas porque o Rust usa esses plugins. Single-instance não tem API JS/permissão própria. Caso uma
chamada direta JS seja introduzida futuramente, conceder apenas sua permissão e scope específico após justificar o uso.
Capabilities de uma mesma janela se somam: revisar também grants existentes da Fase 01 para não reabrir caminho de
bypass. Wrapper Rust valida inputs, origem/label local e scopes de URI; ACL não protege código Rust inseguro.

---

## 4. Banco de Dados (SQLite)

Migration `crates/mt-db/migrations/0005_09_os_settings.sql` (numeração contígua canônica D-01: `0005`, após `0004_07_workspace_templates_tabs_sessions`):
```sql
-- Fase 09: configurações de integração com o SO (linha única, JSON versionado).
CREATE TABLE IF NOT EXISTS os_settings (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version INTEGER NOT NULL DEFAULT 1 CHECK (schema_version >= 1),
    value_json     TEXT    NOT NULL CHECK (json_valid(value_json) AND json_type(value_json) = 'object'),
    updated_at     INTEGER NOT NULL            -- epoch ms
) STRICT;

INSERT OR IGNORE INTO os_settings (id, schema_version, value_json, updated_at)
VALUES (1, 1, '{}', CAST(strftime('%s','now') AS INTEGER) * 1000);
```
- Sem índices adicionais (acesso por PK de linha única).
- `'{}'` + `#[serde(default)]` ⇒ defaults definidos em Rust (fonte única).
- Repositório `OsSettingsRepo` lê/grava `OsSettingsRow { schema_version, value_json, updated_at }`, sem conhecer
  `OsSettings` de `mt-os`. Conversão/validação/merge ficam na fachada; evita ciclo Cargo `mt-os → mt-db → mt-os`.
  `save` usa writer 03 e `UPDATE ... WHERE id = 1` transacional; versão da coluna coincide com `schemaVersion` do JSON
  materializado. Verificar uma linha afetada. Migration default `'{}'` usa versão da coluna até primeira gravação.
- Não criar banco/config-store paralelo. Configuração de editor é persistida literalmente para preservar argv;
  rejeitar credenciais/tokens como configuração de launcher nesta fase e orientar uso de credencial externa.
  Logs, auditoria, erros e notificações derivados do setting sempre passam pelo sanitizador. URI de ativação, texto
  de notificação e snapshot de processos são efêmeros e não entram em `os_settings`.

---

## 5. Arquitetura Frontend

### 5.1. `src/ipc/os.ts`
Wrappers finos sobre `commands.*` de `bindings.ts` e `listen` tipado:
```ts
export const osIpc = {
    getSettings: () => unwrap(commands.osSettingsGet()),
    updateSettings: (patch: OsSettingsPatch) => unwrap(commands.osSettingsUpdate(patch)),
    environment: (refresh = false) => unwrap(commands.osEnvironmentInfo(refresh)),
    shortcutStatus: () => unwrap(commands.osShortcutStatus()),
    configurePortal: () => unwrap(commands.osShortcutConfigurePortal()),
    toggleWindow: (source: ToggleSource = 'ipc') => unwrap(commands.osWindowToggle(source)),
    showWindow: () => unwrap(commands.osWindowShow()),
    hideWindow: () => unwrap(commands.osWindowHide()),
    requestQuit: () => unwrap(commands.osQuitRequest()),
    resolveLinks: (context: LinkContext, c: LinkCandidate[]) => unwrap(commands.osLinkResolve(context, c)),
    openLink: (context: LinkContext, c: LinkCandidate) => unwrap(commands.osLinkOpen(context, c)),
    notify: (r: NotifyRequest) => unwrap(commands.osNotify(r)),
    setAttention: (tabId: TabId, reason: AttentionReason | null) => unwrap(commands.osAttentionSet(tabId, reason)),
    getAutostart: () => unwrap(commands.osGetAutostart()),
    setAutostart: (enabled: boolean) => unwrap(commands.osSetAutostart(enabled)),
    runtimeSnapshot: () => unwrap(commands.osRuntimeSnapshot()),
    uiReady: () => unwrap(commands.osUiReady())
};
export function onFocusTab(cb: (e: OsEvent<FocusTabPayload>) => void): Promise<UnlistenFn>;
export function onWindowVisibility(cb: (e: OsEvent<WindowVisibilityPayload>) => void): Promise<UnlistenFn>;
export function onSettingsChanged(cb: (e: OsEvent<OsSettings>) => void): Promise<UnlistenFn>;
export function onShortcutStatus(cb: (e: OsEvent<ShortcutStatus>) => void): Promise<UnlistenFn>;
export function onCounters(cb: (e: OsEvent<StatusCounters>) => void): Promise<UnlistenFn>;
export function onOpenPreferences(cb: (e: OsEvent<OpenPreferencesPayload>) => void): Promise<UnlistenFn>;
export function onShutdownStarted(cb: (e: OsEvent<ShutdownStartedPayload>) => void): Promise<UnlistenFn>;
```
Payloads de eventos (`FocusTabPayload`, `WindowVisibilityPayload`) também são gerados via `tauri-specta` (`#[derive(Event)]`).

### 5.2. `useWindowStore` (`src/stores/window.ts`)
```ts
interface WindowState {
    visible: boolean;
    focused: boolean;
    minimized: boolean;
    environment: OsEnvironment | null;
    shortcutStatus: ShortcutStatus | null;
    counters: StatusCounters;
    shuttingDown: boolean;
    preferencesOpen: boolean;
    preferencesSection: string | null;
}
// getters
isWayland: (s) => s.environment?.displayServer === 'wayland';
trayAvailable: (s) => s.environment?.trayAvailable ?? false;
shortcutHealthy: (s) => s.shortcutStatus?.registered === true || s.shortcutStatus?.backend === 'cli_only';
needsAttention: (s) => s.counters.errorUnseen + s.counters.awaitingInput > 0;
// actions
init(): Promise<void>;                     // carrega environment + shortcutStatus, registra listeners (idempotente)
refreshEnvironment(): Promise<void>;       // osIpc.environment(true)
toggle(): Promise<void>; hide(): Promise<void>; quit(): Promise<QuitOutcome>;
openPreferences(section?: string): void; closePreferences(): void;
dispose(): void;                           // remove listeners (testes/HMR)
```
Não usa cache persistente do `max-pinia` (estado efêmero de runtime).
`init` usa handshake/snapshot da §3, guarda revisão e fase de shutdown. `dispose` remove listeners mesmo se seu
registro resolver depois do unmount/HMR; não muda política de janela nem encerra PTYs. Snapshot de counters continua
disponível sem tray criada. `cli_only` é fallback informado, não `registered=true` nem comprovação de atalho global.

### 5.3. `useOsSettingsStore` (`src/stores/osSettings.ts`)
```ts
interface OsSettingsState { settings: OsSettings | null; saving: boolean; error: MtError | null; autostart: AutostartStatus | null }
// getters
editorKind: (s) => s.settings?.editor.kind ?? 'vs_code';
quake: (s) => s.settings?.quake ?? null;
// actions
load(): Promise<void>;
update(patch: OsSettingsPatch): Promise<boolean>;   // otimista com rollback em erro; debounce 400 ms para campos numéricos
setAccelerator(acc: string): Promise<boolean>;      // atalho de conveniência; erro preserva valor anterior
```
Sincroniza com `os://settings-changed` (fonte da verdade = backend). Erros exibidos via `Toast.show({ title, severity: 'error' })`
do `@maxvue/max-components-ui` com `MtError.message` (pt-BR).

### 5.4. Composables
- **`composables/editorLinks/parseLinkCandidates.ts`** — puro:
  ```ts
  export interface ParsedLink { key: string; path: string; line: number | null; column: number | null; startIndex: number; endIndex: number }
  export function parseLinkCandidates(lineText: string): ParsedLink[];
  ```
   Scanner linear por tokens delimitados; reconhecer aspas para caminhos com espaços, Unicode com classes `\p{L}`/
   `\p{N}` (`u`), absolutos, `./`, `../`, `~/` e nomes com extensão. Extrair sufixo `:linha[:coluna]` da direita;
   números longos/overflow rejeitam candidato inteiro. Delimitação não remove pontuação válida dentro de caminho
   entre aspas. Excluir **todo** token URL, não só trecho que sucede `://`; versões/horários não geram links.
   Máximo 200 candidatos/linha e 16 KiB de texto por análise; chave inclui posição da ocorrência, para paths repetidos
   na mesma linha. Caminho ambíguo com espaços sem aspas não é adivinhado.
- **`composables/editorLinks/linkCache.ts`** — LRU (512 entradas, TTL 5 s) por
   `tabId|paneId|ptyId|cwdEpoch|path|line|column` → `ResolvedLink | null`.
   `cwdEpoch` vem do backend (`os://link-context-changed`), com invalidação em troca/saída de PTY e remoção do painel.
- **`composables/useEditorLinks.ts`**:
  ```ts
   export function useEditorLinks(terminal: Terminal, context: Ref<LinkContext>): { dispose: () => void };
  ```
  Registra `terminal.registerLinkProvider({ provideLinks(y, cb) })` via ponto de extensão da Fase 04; para a linha `y`
  (considerando linhas com wrap via `buffer.getLine(y).isWrapped`), parseia, consulta cache, agrupa faltantes em lotes
  de 64 e chama `osIpc.resolveLinks`; devolve `ILink[]` com `decorations: { underline: true, pointerCursor: true }` e
   `activate: (ev) => osIpc.openLink(context, candidate)` (respeitando o modificador `Ctrl` se a Fase 04 adotar essa política).
  Debounce de 60 ms por linha; `AbortController` lógico descarta respostas de hovers obsoletos.
- **`composables/useOsFocusTab.ts`** — montado uma vez no `App.vue`: ouve `os://focus-tab` e chama as ações de
   ativação de projeto/aba das stores das Fases 06/07 (`projectsStore.select(projectId)`, `tabsStore.focus(projectId, tabId, paneId?)`);
  a marcação `seen` continua responsabilidade da Fase 05/07 ao exibir a aba.
- **`composables/useShortcutRecorder.ts`**:
  ```ts
  export function useShortcutRecorder(): { recording: Ref<boolean>; preview: Ref<string>; start(): void; cancel(): void; result: Ref<string | null> };
  ```
  Captura `keydown` (usa `KeyboardEvent.code`), exige ≥ 1 modificador + 1 tecla não-modificadora, `Escape` cancela,
   produz string no formato de `Accelerator::parse` (ex.: `Ctrl+Alt+Backquote`).

Ranges xterm são coordenadas de **células**, não índices UTF-16: `provideLinks(y)` recebe linha 1-based, enquanto
`buffer.getLine` é 0-based. Unir somente sequência de linhas realmente wrapped, limitar varredura e mapear células
com largura 0/1/2, emojis/combining marks para início/fim 1-based de `ILink`. Cancelamento de invoke é lógico: geração
de request impede que resultado antigo sublinhe outro conteúdo. Capturar contexto da resolução no clique, sem trocar
silenciosamente pelo painel ativo atual; backend recusa se obsoleto. Descartar provider/cache/listeners ao dispose.
Atualizações de settings são serializadas e o rollback otimista não sobrescreve patch posterior; em falha recarregar
snapshot reconciliado. `useOsFocusTab` espera hidratação 07 e ignora aba removida; não chama `open` para recriá-la.

### 5.5. Componentes
| Componente | Props | Emits | Descrição |
|------------|-------|-------|-----------|
| `preferences/PreferencesDialog.vue` | `open: boolean; section?: string` | `update:open`, `update:section` | `MaxModal` com navegação lateral; seções vindas de `registry.ts` (`registerPreferencesSection({ id, label, icon, component, order })`) — ponto de extensão para Fases 10+ |
| `preferences/system/PreferencesSystemSection.vue` | — | — | Compõe os blocos abaixo; usa `useOsSettingsStore` e `useWindowStore` |
| `preferences/system/CloseBehaviorSelector.vue` | `modelValue: CloseBehavior; trayAvailable: boolean` | `update:modelValue` | Rádio: "Automático (ocultar se houver processos ativos)", "Sempre ocultar na bandeja", "Sempre sair"; aviso se `!trayAvailable` |
| `preferences/system/ShortcutRecorder.vue` | `modelValue: string; status: ShortcutStatus \| null; disabled?: boolean` | `update:modelValue` | Campo "Gravar atalho", preview, botão "Restaurar padrão" |
| `preferences/system/WaylandShortcutHelp.vue` | `environment: OsEnvironment; status: ShortcutStatus` | `configure-portal` | Explica backend ativo; snippet copiável (`compositorHint`); botão "Configurar no sistema" (portal) |
| `preferences/system/EditorSelector.vue` | `modelValue: EditorSettings; editors: EditorAvailability[]` | `update:modelValue` | Select de editor (indisponíveis desabilitados com tooltip), editor de `customArgv` com validação de `{file}` |
| `os/ShortcutStatusBadge.vue` | `status: ShortcutStatus \| null` | — | Badge `--max-success-500` (registrado), `--max-info-500` (CLI), `--max-danger-500` (erro) com `MaxIcon` |

Seções do Preferências > Sistema: **Janela e bandeja** (fechar, iniciar minimizado), **Atalho global** (habilitar,
gravador, backend preferido, modo drop-down com altura/always-on-top/ocultar ao perder foco), **Editor**, **Notificações**
(habilitar, comando longo, limiar em segundos, apenas sem foco, botão "Enviar notificação de teste" → `osIpc.notify({ kind: 'info', … })`),
**Inicialização** (iniciar com o sistema). Todas as tags de abertura em linha única; SCSS scoped com tokens `--max-*`.

---

## 6. Fluxos de Dados

### 6.1. Fechar a janela
```mermaid
sequenceDiagram
    participant U as Usuário
    participant W as Janela (Tauri)
    participant OS as OsIntegration
    participant P as TabStatusProvider (F05)
    participant SC as ShutdownCoordinator
    participant H as ShutdownHooks (F02/F03)
    U->>W: clicar X
    W->>OS: CloseRequested (prevent_close)
    OS->>P: snapshot()
    OS->>OS: decide_close(ctx)
    alt HideToTray
        OS->>W: hide(); emit os://window-visibility
    else Minimize
        OS->>W: minimize()
    else Quit / ConfirmQuit
    OS->>SC: request_quit(Window)
        SC-->>U: diálogo nativo (se ativo)
        SC->>H: freeze_spawns → shutdown_workspace → shutdown_ptys → flush_db
        SC->>W: app.exit(0) somente sem sobreviventes
    end
```

### 6.2. Quake toggle (Wayland com portal)
```
Compositor (KDE/Hyprland) ──Activated("quake-toggle")──► PortalBackend stream
   └─► QuakeController::toggle(Portal) ─► decide_toggle(visible, focused, minimized)
         ├─ ShowAndFocus: [drop-down: geometria] → show → set_focus → emit os://window-visibility
         ├─ Focus: unminimize → set_focus
         └─ Hide: hide
Fallback: `maxterminal --toggle` ─► 2ª instância ─D-Bus single-instance─► handle_second_instance ─► mesmo toggle(Cli)
```

### 6.3. Link clicável
```
xterm hover linha y ─► parseLinkCandidates ─► linkCache (hit?) ──não──► os_link_resolve(context, lote≤64)
   backend: TabCwdProvider.cwd(context) ─► LinkResolver.join ─► canonicalize ─► metadata (arquivo regular?)
   ◄── ResolvedLink[] ─► cache ─► ILink (sublinhado)
clique ─► os_link_open(context, candidate) ─► re-resolve ─► EditorLauncher: openers registrados (F11) ou
          build_editor_invocation ─► Exec(code --goto …) | OpenUri(vscode://file/…)
```

### 6.4. Notificação e foco
```
F05 fim de comando ─► long_command::should_notify ─► Notifier::notify_user
F13 (futuro) agente aguardando ─► AttentionRegistry::set + Notifier::notify_user(AgentAwaitingInput)
notify_user ─► sanitize ─► RateLimiter ─► D-Bus Notify(actions=["default"]) ─► ActionInvoked("default")
   ─► QuakeController::show_and_focus ─► emit os://focus-tab ─► useOsFocusTab ─► stores F06/F07 ativam aba
```

---

## 7. Tratamento de Erros

| Situação | Comportamento |
|----------|---------------|
| Portal ausente/negado | `ShortcutManager` cai para `CliOnly`, `status.error = os.portal_unavailable`, UI mostra snippet; sem panic |
| Acelerador em conflito | erro `os.shortcut_conflict`; atalho anterior re-registrado; settings **não** persistidos |
| Tray não pôde ser criada (`libayatana` ausente) | `tray = None`, `env.tray_available = false`, `decide_close` usa `Minimize`; `warn!` único |
| Servidor de notificações ausente | `NotifyOutcome::Failed` + `os.notify_failed` apenas para `os_notify` explícito; hooks internos só logam |
| Editor não encontrado | `os.editor_not_found` com mensagem "VS Code (`code`) não encontrado no PATH. Configure o editor em Preferências > Sistema." |
| Tab sem cwd conhecido | resolução só aceita caminhos absolutos e `~`; relativos retornam vazio (sem erro) |
| `shutdown_ptys` excede prazo | `os.shutdown_timeout`; Fase 02 mantém supervisão; `Failed`, janela acessível, sem saída bem-sucedida com sobreviventes |
| Segundo `request_quit` concorrente | estado compartilhado `Confirming/Stopping/Draining` torna a chamada idempotente |
| Panics em threads de D-Bus | capturados (`JoinHandle` com `catch_unwind` lógico); backend marcado como falho e status emitido |

Mensagens de erro nunca incluem conteúdo de comandos não sanitizado.

---

## 8. Performance

- Tray: refresh coalescido em 250 ms; `set_icon` apenas quando `TrayVisualState` muda; textos via `MenuItem::set_text`
  (sem reconstruir menu). Custo alvo < 1 ms por refresh.
- `StatusCounters::aggregate` O(n) sobre snapshot (n < 500 abas), sem locks longos (snapshot copiado).
- Toggle: alvo < 50 ms do evento ao `show()`; CLI `--toggle` < 150 ms ponta a ponta (CA-18).
- Links: parse por linha < 0,2 ms (benchmark Vitest com 10 000 linhas típicas de `cargo build`); IPC em lote; cache LRU;
  backend com no máximo 16 `metadata` simultâneos; nenhum trabalho fora de hover (provider é preguiçoso por natureza no xterm).
- Notificações: D-Bus assíncrono e stream compartilhado de ações; mapa de 32 ids/TTL e cancelamento, sem thread por mensagem.
- Nada desta fase toca o stream de output de PTY (Channels da Fase 02 permanecem intocados).

---

## 9. Segurança

- **Sem shell**: todo `Command` usa argv vetorizado; caminhos sempre absolutos canonicalizados (não iniciam com `-`).
- **Custom editor**: primeiro elemento resolvido com `which` (deve existir); placeholders substituídos somente dentro do
  elemento; `custom_argv` limitado a 16 elementos × 512 bytes.
- **URIs**: `vscode://file/` + caminho percent-encoded (`percent_encoding::utf8_percent_encode` com conjunto que preserva `/`);
  apenas esse esquema é aberto por `os_link_open`.
- **Links**: somente arquivos regulares existentes; batch ≤ 64; caminho ≤ 4096 bytes; `os_link_open` re-resolve no backend.
- **CLI/single-instance**: whitelist de 4 flags + URI `maxterminal` validada pelo router; `cwd` e argumentos desconhecidos descartados.
- **Notificações**: `sanitize()` (Fase 03) em título e corpo + truncamento; sem `body-markup` (texto puro, escapando `<`/`&`).
- **Encerramento**: a confirmação nunca é pulada para origens interativas com processos ativos; sinais do SO encerram sem diálogo.
- **Capabilities** mínimas (§3.3); frontend sem acesso direto a plugins.

---

## 10. Estratégia de Testes

### 10.1. Rust (`cargo test -p mt-os`)
| Arquivo | Cobertura |
|---------|-----------|
| `tests/close_decision.rs` | Tabela-verdade completa de `decide_close` (3 × 2 × 2 × 2 = 24 casos) |
| `tests/toggle.rs` | `decide_toggle` (8 combinações) e `drop_down_geometry` (monitores com offset, scale 1.25/2, altura 20–100%) |
| `tests/links.rs` | `LinkResolver::join` + `resolve` com `tempfile::TempDir` e `MockCwdProvider`: relativo, absoluto, `~`, `./`, `../`, symlink, diretório, inexistente, arquivo sem permissão de leitura (ainda é link), lote 65 ⇒ `os.link_rejected`, linha 0/overflow |
| `tests/editor_argv.rs` | `build_editor_invocation` para 4 editores; caminhos com espaço, `#`, `%`, `ç`; custom sem `{file}` ⇒ `os.settings_invalid` |
| `tests/rate_limit.rs` | Dedupe 10 s, teto 5/30 s inclusive `AgentAwaitingInput`, relógio injetado (`Instant` simulado), mapa/TTL limitados |
| `tests/cli.rs` | Whitelist, flags desconhecidas ignoradas, combinação `--start-hidden --toggle` (prioridade definida: `Toggle`) |
| `tests/environment.rs` | `classify_display` com mapas de env (KDE Wayland, Hyprland, GNOME, Sway, X11, XWayland, vazio) |
| `tests/settings.rs` | Defaults, `apply_patch` + `SettingsDiff`, `validate` (faixas), desserialização de `{}` e de JSON com campos extras |
| unit em `long_command.rs` | `should_notify` (limiar, foco, desligado, exit 0/≠0) |
| unit em `accelerator.rs` | parse/format plugin e portal; inválidos (`Ctrl+`, `Foo+A`, sem modificador) |

`mt-db`: `cargo test -p mt-db os_settings` (migration, linha default, `CHECK json_valid`, roundtrip).
Integração no binário com `tauri::test::mock_builder()`: `os_settings_update` persiste e emite `os://settings-changed`;
`os_attention_set` altera `os://counters`.

### 10.2. Frontend (Vitest + `@tauri-apps/api/mocks`)
- `parseLinkCandidates.spec.ts`: ≥ 40 casos (saídas reais de `php artisan`, `tsc`, `cargo`, `eslint`, `phpunit`, URLs,
  horários, versões, pontuação final, Unicode); cobertura ≥ 90%.
- `useEditorLinks.spec.ts`: terminal fake com `registerLinkProvider`; verifica lotes ≤ 64, cache por 5 s, invalidação por
  `cwdEpoch`, descarte de respostas obsoletas, `activate` chamando `osLinkOpen`.
- `window.spec.ts` / `osSettings.spec.ts`: `mockIPC` para comandos; `emit` simulado de eventos; rollback otimista em erro.
- `useShortcutRecorder.spec.ts`: sequência de `keydown` → string canônica; `Escape` cancela; sem modificador rejeita.
- `PreferencesSystemSection.spec.ts`: renderiza com ambiente Wayland/`cli_only` e mostra snippet; com tray indisponível mostra aviso.

### 10.3. Manual / matriz de ambiente (registrada no PR)
| Ambiente | Tray | Atalho | Drop-down | Notificação c/ clique |
|----------|------|--------|-----------|------------------------|
| KDE Plasma 6 Wayland | ✔ | portal | parcial (regra KWin) | ✔ |
| KDE Plasma 6 X11 | ✔ | plugin | ✔ | ✔ |
| Hyprland + waybar tray + `xdg-desktop-portal-hyprland` | ✔ | portal concedido / `exec --toggle` | regra compatível com versão instalada | ✔ se servidor anuncia ações |
| Hyprland sem tray | ✘ ⇒ minimizar ou permanecer visível | CLI | regra compatível com versão instalada | conforme servidor |

Verificação de órfãos (CA-11/CA-14): script `scripts/check-orphans.sh` (dev only, nesta pasta de testes manuais) que
registra `pgrep -P` recursivo antes e confirma lista vazia 5 s após a saída.

O script deve guardar identidade PID/start-time e conferir descendentes mesmo após reparenting; usar apenas PIDs
criados pelo fixture, não `pgrep node/sleep` global. Cenário extra: split `error(!seen)` + `continuous`, janela oculta
por 60 s com output intenso, reabrir sem duplicar sessão. Testar shutdown concorrente, sinal durante confirmação,
spawn em andamento, erro de WAL e sobrevivente simulado; nenhum cleanup começa só em `RunEvent::Exit`.

### 10.4. Contratos de infraestrutura e referências de API
- `tests/activation.rs`: cold/warm start, URI inicial recebida também pelo listener, fila durante restauração,
  TTL/saturação, scheme/escapes inválidos, handler ausente e teste fake sem importar `mt-bridge`.
- `tests/capabilities.rs` (integração do binário): comandos manifestados e acessíveis só por `main` local; plugins JS
  sem grants diretos; nenhuma capability adicional amplia origem remota ou oferece `process.exit` como bypass.
- Settings: falha de registrar novo atalho mantém anterior; falha DB após efeito compensa; versão futura não é
  sobrescrita; `.desktop` ausente difere de consulta falha; `start_minimized=false` reconcilia argv.
- Frontend: init assina antes do snapshot, eventos com revisão antiga são ignorados, ready não desfaz `--start-hidden`,
  links de dois splits isolados e ranges wide/Unicode corretos; listeners tardios removidos no dispose.
- Matriz manual registra distro, compositor/protocolo, versão do portal, versão das crates, executável instalado ou
  dev, suporte a ações e resultado real. Suporte em tabela é cenário alvo a verificar, não certificação automática.

Referências consultadas para os contratos Tauri v2:
- [Single Instance](https://v2.tauri.app/plugin/single-instance/): primeiro plugin, D-Bus no Linux, callback e ausência de API JS.
- [Deep Linking](https://v2.tauri.app/plugin/deep-linking/): feature `deep-link`, schemes estáticos, cold/warm start e `register_all`.
- [Capabilities](https://v2.tauri.app/security/capabilities/): fronteira da webview, composição de permissões e `AppManifest::commands`.
- [Global Shortcut](https://v2.tauri.app/plugin/global-shortcut/): registro e permissões de API JS.

As assinaturas específicas de `ashpd`, `notify-rust`, xterm e plugins devem ser conferidas contra as versões fixadas
antes de implementar; os links acima documentam os contratos de integração Tauri utilizados nesta fase.
