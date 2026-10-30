# Implementação 01 — Fundação `mt-os`, Settings SQLite, Detecção de Ambiente e Agregador de Estado (`spec.md`)

> Escopo desta sub-implementação: estabelecer a fundação do crate `mt-os` e a persistência no `mt-db` sem acoplamento reverso às Fases 02, 05, 07, 11 ou 13. Inclui códigos de erro `os.*`, modelo e validação de `OsSettings` com merge profundo (`OsSettingsPatch`) e preservação de campos desconhecidos, migration SQLite `os_settings` e repositório `OsSettingsRepo`, detecção pura e assíncrona do ambiente Linux/CachyOS (`environment.rs`), agregador de contadores e atividade de painéis (`status.rs`), registro de atenção (`attention.rs`) e contratos das traits de fronteira (`TabStatusProvider`, `TabCwdProvider`, `ShutdownHooks`).

---

## 1. Estrutura de Arquivos e Dependências

### 1.1. `src-tauri/crates/mt-os/Cargo.toml`

```toml
[package]
name = "mt-os"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
mt-core = { path = "../mt-core" }
mt-db = { path = "../mt-db" }
tauri = { workspace = true, features = ["tray-icon", "image-png"] }
tauri-plugin-global-shortcut = "2"
tauri-plugin-notification = "2"
tauri-plugin-autostart = "2"
tauri-plugin-dialog = "2"
tauri-plugin-opener = "2"
ashpd = { version = "0.11", default-features = false, features = ["tokio"] }
notify-rust = { version = "4", default-features = false, features = ["z", "images"] }
zbus = { version = "5", default-features = false, features = ["tokio"] }
tokio = { workspace = true, features = ["sync", "time", "fs", "rt", "signal", "process"] }
async-trait = "0.1"
futures = "0.3"
url = "2"
uuid = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
specta = { workspace = true }
tracing = { workspace = true }
percent-encoding = "2"
which = "7"
parking_lot = "0.12"
lru = "0.12"

[dev-dependencies]
tempfile = "3"
tokio = { workspace = true, features = ["rt-multi-thread", "macros", "test-util"] }
```

---

## 2. Códigos de Erro do Domínio (`crates/mt-os/src/error.rs`)

Seguindo o contrato da Fase 01 (`MtError` e `ErrorCode` baseados em constantes estáticas por domínio):

```rust
use mt_core::error::{MtError, MtResult};

/// Códigos canônicos de erro do domínio `os.*`.
pub mod codes {
    pub const SETTINGS_INVALID: &str = "os.settings_invalid";
    pub const SHORTCUT_INVALID: &str = "os.shortcut_invalid";
    pub const SHORTCUT_CONFLICT: &str = "os.shortcut_conflict";
    pub const SHORTCUT_UNSUPPORTED: &str = "os.shortcut_unsupported";
    pub const PORTAL_UNAVAILABLE: &str = "os.portal_unavailable";
    pub const TRAY_UNAVAILABLE: &str = "os.tray_unavailable";
    pub const LINK_NOT_FOUND: &str = "os.link_not_found";
    pub const LINK_REJECTED: &str = "os.link_rejected";
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

/// Construtor auxiliar para erros do domínio `os.*` com mensagem em pt-BR.
pub fn os_err(code: &'static str, message: impl Into<String>) -> MtError {
    MtError::new(code, message.into())
}

pub type OsResult<T> = MtResult<T>;
```

---

## 3. Persistência no `mt-db` (`NNNN_09_os_settings.sql` e `os_settings_repo.rs`)

### 3.1. Migration SQL (`crates/mt-db/migrations/NNNN_09_os_settings.sql`)

```sql
-- Fase 09: configurações de integração com o SO (linha única, JSON versionado).
CREATE TABLE IF NOT EXISTS os_settings (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version INTEGER NOT NULL DEFAULT 1 CHECK (schema_version >= 1),
    value_json     TEXT    NOT NULL CHECK (json_valid(value_json) AND json_type(value_json) = 'object'),
    updated_at     INTEGER NOT NULL -- epoch ms
) STRICT;

INSERT OR IGNORE INTO os_settings (id, schema_version, value_json, updated_at)
VALUES (1, 1, '{}', CAST(strftime('%s','now') AS INTEGER) * 1000);
```

### 3.2. Repositório (`crates/mt-db/src/repos/os_settings.rs`)

> Nota: também re-exportado como `crates/mt-db/src/repos/os_settings_repo.rs` (ou módulo `os_settings` em `repos/mod.rs`) para satisfazer consumidores que importam `mt_db::repos::os_settings::OsSettingsRepo`. O `mt-db` **não** importa `mt-os`.

```rust
use mt_core::error::{MtError, MtResult};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Linha crua da tabela `os_settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsSettingsRow {
    pub schema_version: u32,
    pub value_json: String,
    pub updated_at: i64,
}

/// Repositório desacoplado de `mt-os` para leitura e gravação atômica do JSON de configurações de SO.
#[derive(Clone)]
pub struct OsSettingsRepo {
    db: crate::DbPool, // Pool/Writer padronizado na Fase 03
}

impl OsSettingsRepo {
    pub fn new(db: crate::DbPool) -> Self {
        Self { db }
    }

    /// Lê a linha única (id = 1) da tabela `os_settings`.
    pub async fn get_row(&self) -> MtResult<OsSettingsRow> {
        self.db
            .read(|conn: &Connection| {
                conn.query_row(
                    "SELECT schema_version, value_json, updated_at FROM os_settings WHERE id = 1",
                    [],
                    |row| {
                        Ok(OsSettingsRow {
                            schema_version: row.get::<_, u32>(0)?,
                            value_json: row.get::<_, String>(1)?,
                            updated_at: row.get::<_, i64>(2)?,
                        })
                    },
                )
                .map_err(|e| MtError::new("db.query_failed", format!("Falha ao ler os_settings: {e}")))
            })
            .await
    }

    /// Atualiza transacionalmente a linha única (id = 1), garantindo que exatamente 1 linha foi afetada.
    pub async fn save_row(&self, schema_version: u32, value_json: String, updated_at: i64) -> MtResult<OsSettingsRow> {
        self.db
            .write(move |conn: &Connection| {
                let affected = conn
                    .execute(
                        "UPDATE os_settings
                         SET schema_version = ?1, value_json = ?2, updated_at = ?3
                         WHERE id = 1",
                        params![schema_version, value_json, updated_at],
                    )
                    .map_err(|e| MtError::new("db.write_failed", format!("Falha ao salvar os_settings: {e}")))?;

                if affected != 1 {
                    return Err(MtError::new(
                        "db.write_failed",
                        format!("Esperado atualizar 1 linha em os_settings, mas {affected} foram afetadas."),
                    ));
                }

                Ok(OsSettingsRow {
                    schema_version,
                    value_json,
                    updated_at,
                })
            })
            .await
    }
}
```

---

## 4. Configurações e Validação (`crates/mt-os/src/settings.rs`)

### 4.1. Estruturas de Dados e Tipos Specta

```rust
use crate::error::{codes, os_err, OsResult};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const CURRENT_OS_SETTINGS_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_QUAKE_ACCELERATOR: &str = "Ctrl+Alt+Backquote";
pub const MAX_CUSTOM_ARGV_LEN: usize = 16;
pub const MAX_CUSTOM_ARG_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    #[default]
    Auto,
    AlwaysTray,
    AlwaysQuit,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum QuakeMode {
    #[default]
    Normal,
    DropDown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutBackendPreference {
    #[default]
    Auto,
    Plugin,
    Portal,
    CliOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EditorKind {
    #[default]
    VsCode,
    VsCodeInsiders,
    MaxCode,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct QuakeSettings {
    pub enabled: bool,
    pub accelerator: String,
    pub mode: QuakeMode,
    pub drop_down_height_pct: u8,
    pub always_on_top: bool,
    pub hide_on_blur: bool,
    pub preferred_backend: ShortcutBackendPreference,
}

impl Default for QuakeSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            accelerator: DEFAULT_QUAKE_ACCELERATOR.to_string(),
            mode: QuakeMode::Normal,
            drop_down_height_pct: 45,
            always_on_top: true,
            hide_on_blur: false,
            preferred_backend: ShortcutBackendPreference::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct EditorSettings {
    pub kind: EditorKind,
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

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct NotificationSettings {
    pub enabled: bool,
    pub long_command_enabled: bool,
    pub long_command_threshold_secs: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct OsSettings {
    pub schema_version: u32,
    pub close_behavior: CloseBehavior,
    pub quake: QuakeSettings,
    pub editor: EditorSettings,
    pub notifications: NotificationSettings,
    pub autostart: bool,
    pub start_minimized: bool,
}

impl Default for OsSettings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_OS_SETTINGS_SCHEMA_VERSION,
            close_behavior: CloseBehavior::Auto,
            quake: QuakeSettings::default(),
            editor: EditorSettings::default(),
            notifications: NotificationSettings::default(),
            autostart: false,
            start_minimized: true,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuakeSettingsPatch {
    pub enabled: Option<bool>,
    pub accelerator: Option<String>,
    pub mode: Option<QuakeMode>,
    pub drop_down_height_pct: Option<u8>,
    pub always_on_top: Option<bool>,
    pub hide_on_blur: Option<bool>,
    pub preferred_backend: Option<ShortcutBackendPreference>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettingsPatch {
    pub enabled: Option<bool>,
    pub long_command_enabled: Option<bool>,
    pub long_command_threshold_secs: Option<u32>,
    pub only_when_unfocused: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OsSettingsPatch {
    pub close_behavior: Option<CloseBehavior>,
    pub quake: Option<QuakeSettingsPatch>,
    pub editor: Option<EditorSettings>,
    pub notifications: Option<NotificationSettingsPatch>,
    pub autostart: Option<bool>,
    pub start_minimized: Option<bool>,
}

/// Subsistemas que precisam ser reaplicados após uma alteração.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SettingsDiff {
    pub shortcut: bool,
    pub autostart: bool,
    pub quake_geometry: bool,
    pub editor: bool,
}
```

### 4.2. Regras de Negócio e Validação de `OsSettings`

1. **Verificação de `schema_version`**:
   - Se a linha no banco ou o JSON tiver `schema_version > CURRENT_OS_SETTINGS_SCHEMA_VERSION`, `from_row` falha com `os.settings_invalid` ("Versão de esquema de configurações não suportada"), impedindo sobrescrita destrutiva de versão futura (CA-40).
   - Se `value_json` for `'{}'`, `OsSettings::from_json_ preserving_unknown` preenche todos os campos ausentes com `Default::default()`, mas **rejeita** tipos inválidos (ex.: `"autostart": "sim"` retorna erro `os.settings_invalid`, sem substituir silenciosamente por default).
   - Campos desconhecidos na raiz ou sub-objetos são retidos em um `Map<String, Value>` auxiliar durante a mesclagem para que atualizações parciais não apaguem chaves de versões compatíveis.
2. **Faixas Numéricas e Invariantes (`OsSettings::validate`)**:
   - `quake.drop_down_height_pct`: deve estar em `20..=100`. Fora da faixa ⇒ `os.settings_invalid` ("A altura do modo drop-down deve estar entre 20% e 100%.").
   - `notifications.long_command_threshold_secs`: deve estar em `3..=3600`. Fora da faixa ⇒ `os.settings_invalid` ("O limiar de comando longo deve estar entre 3 e 3600 segundos.").
   - `quake.accelerator`: validado sintaticamente (não vazio, ≥ 1 modificador `Ctrl|Alt|Shift|Super` + 1 tecla principal válida). Inválido ⇒ `os.shortcut_invalid`.
   - `editor`:
     - Quando `editor.kind == EditorKind::Custom`:
       - `custom_argv` não pode ser vazio e deve ter no máximo `16` elementos (`MAX_CUSTOM_ARGV_LEN`), cada um com `1..=512` bytes (`MAX_CUSTOM_ARG_BYTES`) e sem byte NUL (`\0`).
       - Pelo menos um argumento após o programa (`custom_argv[1..]`) deve conter o placeholder `{file}`.
       - Nenhum argumento pode conter padrões de segredos/tokens (verificado via heurística ou detector do sanitizador da Fase 03: `Bearer `, `ghp_`, `sk-`, `APP_KEY=`, etc.); se detectado, rejeitar com `os.settings_invalid` ("Argumentos do editor não podem conter credenciais ou tokens em texto claro.").
3. **Cálculo de `SettingsDiff` em `apply_patch`**:
   - `diff.shortcut`: mudou `quake.enabled`, `quake.accelerator` ou `quake.preferred_backend`.
   - `diff.quake_geometry`: mudou `quake.mode`, `quake.drop_down_height_pct`, `quake.always_on_top` ou `quake.hide_on_blur`.
   - `diff.autostart`: mudou `autostart` ou `start_minimized`.
   - `diff.editor`: mudou `editor.kind` ou `editor.custom_argv`.

---

## 5. Detecção de Ambiente (`crates/mt-os/src/environment.rs`)

### 5.1. Assinaturas e Tipos

```rust
use crate::editor::EditorKind;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DisplayServer {
    X11,
    Wayland,
    XWayland,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Compositor {
    KdePlasma,
    Hyprland,
    Gnome,
    Sway,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditorAvailability {
    pub kind: EditorKind,
    pub available: bool,
    pub binary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OsEnvironment {
    pub display_server: DisplayServer,
    pub compositor: Compositor,
    pub tray_available: bool,
    pub portal_global_shortcuts: bool,
    pub notification_server: Option<String>,
    pub notification_actions: bool,
    pub editors: Vec<EditorAvailability>,
    pub window_positioning: bool,
}

pub const DBUS_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Função pura: classifica servidor gráfico e compositor a partir de um leitor de variáveis de ambiente.
pub fn classify_display(env: &impl Fn(&str) -> Option<String>) -> (DisplayServer, Compositor);

/// Sondagem assíncrona de D-Bus (com timeout de 500 ms por consulta) e binários de editor.
pub async fn probe(env: &impl Fn(&str) -> Option<String>) -> OsEnvironment;
```

### 5.2. Regras de Classificação Pura (`classify_display`)

1. **`DisplayServer`**:
   - Ler `WAYLAND_DISPLAY` (não vazio), `GDK_BACKEND`, `XDG_SESSION_TYPE` (comparação case-insensitive) e `DISPLAY` (não vazio).
   - Se `WAYLAND_DISPLAY` está definido e não vazio:
     - Se `GDK_BACKEND` (normalizado em minúsculas) for `"x11"`, retorna `DisplayServer::XWayland`.
     - Caso contrário, retorna `DisplayServer::Wayland`.
   - Senão, se `XDG_SESSION_TYPE` (minúsculas) for `"wayland"`:
     - Se `GDK_BACKEND == "x11"`, retorna `DisplayServer::XWayland`, senão `DisplayServer::Wayland`.
   - Senão, se `XDG_SESSION_TYPE == "x11"` ou `DISPLAY` estiver definido e não vazio:
     - Retorna `DisplayServer::X11`.
   - Caso contrário, retorna `DisplayServer::Unknown`.
2. **`Compositor`**:
   - Se `HYPRLAND_INSTANCE_SIGNATURE` está definido e não vazio ⇒ `Compositor::Hyprland`.
   - Senão, se `SWAYSOCK` está definido e não vazio ⇒ `Compositor::Sway`.
   - Senão, inspecionar `XDG_CURRENT_DESKTOP` e `DESKTOP_SESSION` (em maiúsculas):
     - Contém `"KDE"` ou `"PLASMA"` ⇒ `Compositor::KdePlasma`.
     - Contém `"HYPRLAND"` ⇒ `Compositor::Hyprland`.
     - Contém `"GNOME"` ⇒ `Compositor::Gnome`.
     - Contém `"SWAY"` ⇒ `Compositor::Sway`.
     - Caso contrário ⇒ `Compositor::Other`.
3. **`window_positioning`**:
   - `true` somente quando `display_server == DisplayServer::X11`; `false` em `Wayland`, `XWayland` e `Unknown`.

### 5.3. Sondagem Assíncrona D-Bus (`probe`)

Cada verificação D-Bus usa a conexão de sessão `zbus::Connection::session().await` envolvida em `tokio::time::timeout(Duration::from_millis(500), ...)`. Qualquer erro ou timeout resulta em `false`/`None` sem pânico:
- **`tray_available`**: chama `org.freedesktop.DBus.NameHasOwner("org.kde.StatusNotifierWatcher")` ou verifica proprietário de `org.freedesktop.StatusNotifierWatcher`.
- **`portal_global_shortcuts`**: consulta a propriedade `version` da interface `org.freedesktop.portal.GlobalShortcuts` em `/org/freedesktop/portal/desktop` (destino `org.freedesktop.portal.Desktop`); verdadeiro se `version >= 1`.
- **`notification_server` e `notification_actions`**: chama `GetServerInformation()` e `GetCapabilities()` em `org.freedesktop.Notifications` (`/org/freedesktop/Notifications`); `notification_actions` é verdadeiro se a lista de capacidades contiver `"actions"`.

---

## 6. Agregador de Estado, Atenção e Traits de Fronteira (`status.rs`, `attention.rs`)

### 6.1. Registro de Atenção (`crates/mt-os/src/attention.rs`)

```rust
use mt_core::ids::TabId;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason {
    AwaitingInput,
    AwaitingApproval,
}

#[derive(Default, Debug)]
pub struct AttentionRegistry {
    inner: RwLock<HashMap<TabId, AttentionReason>>,
}

impl AttentionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra ou atualiza a razão de atenção da aba. Retorna `true` se o estado mudou.
    pub fn set(&self, tab: TabId, reason: AttentionReason) -> bool {
        let mut guard = self.inner.write();
        guard.insert(tab, reason) != Some(reason)
    }

    /// Remove a atenção da aba (quando a intervenção é resolvida ou a aba é fechada). Retorna `true` se havia registro.
    pub fn clear(&self, tab: &TabId) -> bool {
        self.inner.write().remove(tab).is_some()
    }

    pub fn count(&self) -> u32 {
        self.inner.read().len() as u32
    }

    pub fn get(&self, tab: &TabId) -> Option<AttentionReason> {
        self.inner.read().get(tab).copied()
    }

    pub fn contains(&self, tab: &TabId) -> bool {
        self.inner.read().contains_key(tab)
    }
}
```

### 6.2. Agregador de Estado e Provedor de Status (`crates/mt-os/src/status.rs`)

```rust
use crate::attention::AttentionRegistry;
use mt_core::ids::{ProjectId, TabId};
use serde::{Deserialize, Serialize};

/// Espelho desacoplado do bucket de status da Fase 05.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TabStatusKind {
    Running,
    Continuous,
    Finished,
    Error,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabStatusEntry {
    pub tab_id: TabId,
    /// Preenchido pelo adaptador com o `badge_bucket` da Fase 05.
    pub status: TabStatusKind,
    pub seen: bool,
    pub title: String,
}

/// Snapshot real de atividade de todos os painéis vivos, independente do badge visual agregado da aba.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActivitySnapshot {
    pub active_panes: u32,
    pub pending_spawns: u32,
    pub supervised_busy: bool,
    pub incomplete: bool,
}

impl ActivitySnapshot {
    /// Verdadeiro se qualquer painel estiver executando, houver spawn pendente, processo supervisionado ocupado,
    /// snapshot incompleto ou aba com agente vivo aguardando intervenção.
    pub fn has_live_activity(&self, awaiting_input_count: u32) -> bool {
        self.active_panes > 0
            || self.pending_spawns > 0
            || self.supervised_busy
            || self.incomplete
            || awaiting_input_count > 0
    }
}

pub trait TabStatusProvider: Send + Sync + 'static {
    fn snapshot(&self) -> Vec<TabStatusEntry>;
    fn activity(&self) -> ActivitySnapshot;
    fn project_of(&self, tab_id: &TabId) -> Option<ProjectId>;
    fn tab_exists(&self, tab_id: &TabId) -> bool {
        self.project_of(tab_id).is_some()
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatusCounters {
    pub running: u32,
    pub continuous: u32,
    pub finished_unseen: u32,
    pub error_unseen: u32,
    pub idle: u32,
    pub awaiting_input: u32,
}

impl StatusCounters {
    /// Agregação pura O(n): cada aba entra em exatamente um dos 5 buckets primários;
    /// `awaiting_input` é transversal e conta apenas abas presentes no snapshot.
    pub fn aggregate(entries: &[TabStatusEntry], attention: &AttentionRegistry) -> Self {
        let mut counters = Self::default();
        for entry in entries {
            match entry.status {
                TabStatusKind::Running => counters.running += 1,
                TabStatusKind::Continuous => counters.continuous += 1,
                TabStatusKind::Finished => {
                    if entry.seen {
                        counters.idle += 1;
                    } else {
                        counters.finished_unseen += 1;
                    }
                }
                TabStatusKind::Error => {
                    if entry.seen {
                        counters.idle += 1;
                    } else {
                        counters.error_unseen += 1;
                    }
                }
                TabStatusKind::Idle => counters.idle += 1,
            }
            if attention.contains(&entry.tab_id) {
                counters.awaiting_input += 1;
            }
        }
        counters
    }

    /// Indicador visual de abas ativas para o Tray (`TrayVisualState::Active`).
    pub fn has_active(&self) -> bool {
        self.running + self.continuous > 0
    }

    /// Indicador visual de atenção para o Tray (`TrayVisualState::Attention`).
    pub fn needs_attention(&self) -> bool {
        self.error_unseen + self.awaiting_input > 0
    }
}
```

### 6.3. Traits de Fronteira Adicionais (`TabCwdProvider` e `ShutdownHooks`)

Definidas nos módulos correspondentes e re-exportadas em `crates/mt-os/src/lib.rs` para uso pelos adaptadores do binário:
- `TabCwdProvider` (em `links.rs` / re-exportada em `lib.rs`):
  ```rust
  pub trait TabCwdProvider: Send + Sync + 'static {
      fn cwd(&self, context: &LinkContext) -> MtResult<Option<std::path::PathBuf>>;
  }
  ```
- `ShutdownHooks` (em `lifecycle.rs` / re-exportada em `lib.rs`):
  ```rust
  #[async_trait::async_trait]
  pub trait ShutdownHooks: Send + Sync + 'static {
      async fn freeze_spawns(&self) -> MtResult<()>;
      async fn shutdown_workspace(&self) -> MtResult<()>;
      async fn shutdown_ptys(&self, grace: std::time::Duration) -> ShutdownReport;
      async fn flush_db(&self) -> MtResult<()>;
  }
  ```

---

## 7. Mapeamento para Critérios de Aceite

- **CA-01**: `cargo clippy -p mt-os --all-targets -- -D warnings` sem avisos nos módulos base.
- **CA-02**: Testes unitários de `settings.rs`, `environment.rs` (mapas simulados KDE Wayland, Hyprland, GNOME, Sway, X11, XWayland) e `status.rs`/`attention.rs`.
- **CA-03**: `cargo test -p mt-db os_settings` validando migration, linha default `'{}'`, constraint `CHECK (json_valid(value_json))`, `id = 1` e roundtrip com campos ausentes/desconhecidos.
- **CA-31**: Mensagens de erro de validação e do repositório estritamente em `pt-BR`; identificadores em `en-US`.
- **CA-33**: `ActivitySnapshot::has_live_activity` inspeciona painéis reais (`active_panes`, `pending_spawns`, `supervised_busy`, `incomplete`) separadamente do bucket visual da aba, garantindo que um split em erro não esconda um servidor rodando em outro painel da mesma aba.
- **CA-40**: Preservação de versão futura (`schema_version > 1` rejeitado sem sobrescrever) e preservação de chaves JSON desconhecidas.
