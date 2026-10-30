# Implementação 04 — Links Clicáveis para Editor, Notificações Nativas, Autostart e IPC Tauri (`spec.md`)

> Escopo desta sub-implementação: implementar o resolvedor de links com validação de época de diretório (`links.rs`), o construtor de invocação e abridor de editores para VS Code, VS Code Insiders, MaxCode e Custom sem shell (`editor.rs`), o subsistema de notificações nativas D-Bus com sanitização, rate limiting e stream compartilhado de cliques (`notify.rs` / `notifications.rs`), o detector de fim de comando longo (`long_command.rs`), a sincronização de autostart Desktop Entry (`autostart.rs`), a fachada `OsIntegration` (`lib.rs`), e toda a camada do crate binário (`adapters/os.rs`, 16 comandos `commands/os.rs`, `events/os.rs`, `permissions/os.toml`, `capabilities/os.json` e `capabilities/default.json`).

---

## 1. Resolução de Links no Backend (`crates/mt-os/src/links.rs`)

### 1.1. Estruturas e Trait `TabCwdProvider`

```rust
use crate::error::{codes, os_err, OsResult};
use futures::stream::{self, StreamExt};
use mt_core::ids::{PaneId, PtyId, TabId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const MAX_BATCH: usize = 64;
pub const MAX_PATH_LEN: usize = 4096;
pub const MAX_LINE_COL: u32 = 10_000_000;
pub const METADATA_CONCURRENCY: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkCandidate {
    pub key: String,
    pub path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLink {
    pub key: String,
    pub absolute_path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LinkContext {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub pty_id: PtyId,
    pub cwd_epoch: u32,
}

/// Implementado em `src-tauri/src/adapters/os.rs` consultando o estado do painel (Fase 05/07)
/// com fallback `/proc/<pid_shell>/cwd` (Fase 02).
pub trait TabCwdProvider: Send + Sync + 'static {
    /// Valida que `(tab_id, pane_id, pty_id, cwd_epoch)` ainda corresponde ao painel vivo atual.
    /// Retorna `Err(os.tab_not_found)` se aba/painel não existir, `Err(os.link_context_stale)` se `pty_id` ou `cwd_epoch` mudou,
    /// ou `Ok(Option<PathBuf>)` com o diretório atual do painel.
    fn cwd(&self, context: &LinkContext) -> OsResult<Option<PathBuf>>;
}

pub struct LinkResolver {
    cwd_provider: Arc<dyn TabCwdProvider>,
    home: PathBuf,
}
```

### 1.2. Regras de Negócio e Validação de `LinkResolver`

1. **Validação de Lote e Contexto**:
   - Se `candidates.len() > MAX_BATCH` (64), retorna imediatamente `Err(os_err(codes::LINK_REJECTED, "Lote de candidatos excede o limite de 64 itens."))`.
   - Chama `self.cwd_provider.cwd(context)?` **antes** de iniciar o I/O e **novamente após** concluir o I/O de `metadata`/`canonicalize`. Se o contexto tiver mudado durante a resolução (ex.: `cd`, troca de PTY ou fechamento do painel), descarta o resultado e retorna `Err(os_err(codes::LINK_CONTEXT_STALE, "O contexto do painel mudou durante a resolução do link."))` (CA-38).
2. **Validação de Candidato e `LinkResolver::join` (Função Pura)**:
   - Rejeita `path` vazio, `path.len() > 4096` (`MAX_PATH_LEN`), strings contendo `\0` ou caracteres de controle (`< 0x20`), esquemas de URL (`contains("://")` ou prefixo `mailto:`/`vscode:`), e expansão de til de outro usuário (`~usuario/...`).
   - Valida `line` e `column`:
     - Se `column.is_some() && line.is_none()` ⇒ rejeita candidato.
     - Se `line == Some(0)` ou `line > Some(10_000_000)` ⇒ rejeita candidato.
     - Se `column == Some(0)` ou `column > Some(10_000_000)` ⇒ rejeita candidato.
   - `LinkResolver::join(cwd: Option<&Path>, home: &Path, raw: &str) -> Option<PathBuf>`:
     - `raw == "~"` ⇒ `Some(home.to_path_buf())`.
     - `raw.starts_with("~/")` ⇒ `Some(home.join(&raw[2..]))`.
     - `Path::new(raw).is_absolute()` ⇒ `Some(PathBuf::from(raw))`.
     - Caminho relativo (`./`, `../` ou `app/Models/User.php`): requer `cwd.is_some()` ⇒ `Some(cwd?.join(raw))`; se a aba não tiver `cwd` conhecido, retorna `None` (sem erro).
3. **Verificação no Sistema de Arquivos (`resolve`)**:
   - Executa com concorrência limitada via `stream::iter(...).buffer_unordered(16)` (nunca `join_all` ilimitado).
   - Para cada caminho candidato: chama `tokio::fs::canonicalize(&joined).await` e `tokio::fs::metadata(&canonical).await`.
   - Aceita **somente** se `metadata.is_file()` for `true` (diretórios, sockets, FIFOs e arquivos inexistentes são descartados silenciosamente da lista retornada).
   - Caminho canônico resultante sempre começa com `/`, eliminando risco de argumento iniciado por `-` no editor.

---

## 2. Abertura no Editor (`crates/mt-os/src/editor.rs`)

### 2.1. Construção Pura de Invocação (`build_editor_invocation`)

```rust
use crate::environment::EditorAvailability;
use crate::error::{codes, os_err, OsResult};
use crate::links::ResolvedLink;
use crate::settings::{EditorKind, EditorSettings, OsSettings};
use parking_lot::RwLock;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use std::path::PathBuf;
use std::sync::Arc;

/// Conjunto de caracteres percent-encoded no caminho de `vscode://file/...` (preserva `/`, `-`, `_`, `.`, `~`).
const VSCODE_PATH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'?')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'>')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorInvocation {
    Exec { program: PathBuf, args: Vec<String> },
    OpenUri(String),
}

/// Função pura (CA-02, CA-21, CA-24, CA-32):
/// - `VsCode` ⇒ `Exec { program: "code", args: ["--goto", "<abs>:<line>:<col>"] }`
/// - `VsCodeInsiders` ⇒ `Exec { program: "code-insiders", args: ["--goto", "<abs>:<line>:<col>"] }`
/// - `MaxCode` ⇒ `OpenUri("vscode://file/<abs_encoded>:<line>:<col>")` (sem barra dupla após `file`)
/// - `Custom` ⇒ `Exec` substituindo `{file}`, `{line}` e `{column}` dentro de cada elemento de `custom_argv`
pub fn build_editor_invocation(
    settings: &EditorSettings,
    link: &ResolvedLink,
) -> OsResult<EditorInvocation>;
```

- Regras de `build_editor_invocation`:
  - `line` padrão = `1`, `column` padrão = `1`.
  - Garante que `link.absolute_path` começa com `/` e não contém `\0`.
  - Em `EditorKind::MaxCode`: remove a `/` inicial antes de concatenar com `vscode://file/` para produzir exatamente `vscode://file/home/...:42:1`, codificando cada segmento ou o caminho com `utf8_percent_encode(..., VSCODE_PATH_ENCODE_SET)` (preservando `/`, mas codificando espaços como `%20`, `#` como `%23`, `%` como `%25`, `:` como `%3A` e caracteres UTF-8 acentuados como `ç`).
  - Em `EditorKind::Custom`: valida `custom_argv` (não vazio, `<= 16` itens, presença de `{file}` em `custom_argv[1..]`), substitui `{file}`, `{line}` e `{column}` individualmente dentro de cada item **sem** re-tokenizar por espaços e **nunca** invoca `sh -c` (CA-32).

### 2.2. `EditorOpener` e `EditorLauncher`

```rust
#[async_trait::async_trait]
pub trait EditorOpener: Send + Sync + 'static {
    fn handles(&self, kind: EditorKind) -> bool;
    async fn open(&self, link: &ResolvedLink) -> OsResult<()>;
}

pub struct EditorLauncher<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
    openers: RwLock<Vec<Arc<dyn EditorOpener>>>,
    settings: Arc<RwLock<OsSettings>>,
}
```

- `register_opener`: insere no início do vetor (`insert(0, opener)`) para que a ponte loopback da Fase 11 tenha prioridade sobre o fallback de URI.
- `open`:
  1. Consulta `openers` registrados que aceitam `settings.editor.kind`. Se um opener registrado falhar, faz fallback explícito para `build_editor_invocation`.
  2. Para `EditorInvocation::Exec { program, args }`: resolve o binário via `which::which(&program)` (retornando `os.editor_not_found` em pt-BR se ausente) e dispara `tokio::process::Command::new(resolved_bin).args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()`, fazendo reap do processo em background task sem bloquear a chamada IPC.
  3. Para `EditorInvocation::OpenUri(uri)`: usa `tauri_plugin_opener::OpenerExt` (`app.opener().open_url(uri, None::<&str>)`).

---

## 3. Notificações Nativas, Comando Longo e Autostart (`notify.rs`, `long_command.rs`, `autostart.rs`)

### 3.1. Rate Limiter e Notifier (`crates/mt-os/src/notify.rs` + re-export `notifications.rs`)

```rust
use crate::error::OsResult;
use mt_core::ids::{PaneId, TabId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

pub const DEDUPE_WINDOW: Duration = Duration::from_secs(10);
pub const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(30);
pub const MAX_NOTIFICATIONS_PER_WINDOW: usize = 5;
pub const MAX_TITLE_CHARS: usize = 80;
pub const MAX_BODY_CHARS: usize = 200;
pub const MAX_TRACKED_ACTIONS: usize = 32;
pub const ACTION_ENTRY_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    CommandFinished,
    CommandFailed,
    AgentAwaitingInput,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotifyRequest {
    pub kind: NotificationKind,
    pub title: String,
    pub body: String,
    pub tab_id: Option<TabId>,
    #[serde(default)]
    pub pane_id: Option<PaneId>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotifyOutcome {
    Shown,
    Replaced,
    Suppressed,
    Disabled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitDecision {
    Allow,
    Replace(u64),
    Suppress,
}

pub struct RateLimiter {
    recent_timestamps: VecDeque<Instant>,
    dedupe_map: HashMap<u64, Instant>,
}

impl RateLimiter {
    pub fn new() -> Self;
    pub fn check(&mut self, key: u64, _kind: NotificationKind, now: Instant) -> LimitDecision;
}
```

- **Pipeline de `Notifier::notify_user` (CA-26, CA-28, CA-29, CA-39)**:
  1. Se `!settings.notifications.enabled`, retorna `NotifyOutcome::Disabled`.
  2. Sanitiza `title` e `body` via `mt_db::sanitizer::sanitize()`, escapa `<` e `&` e trunca em `80` e `200` caracteres Unicode (adicionando `…` quando truncado).
  3. Calcula hash de dedupe sobre `(tab_id, kind, sanitized_title, sanitized_body)` e avalia `RateLimiter::check`:
     - Se a mesma chave ocorreu há `< 10 s`, retorna `LimitDecision::Replace(key)` (reutiliza `replaces_id` D-Bus sem consumir nova cota).
     - Se já houve `>= 5` notificações nos últimos `30 s`, retorna `LimitDecision::Suppress` (`NotifyOutcome::Suppressed`).
  4. Envia via proxy assíncrono `zbus` (`org.freedesktop.Notifications.Notify`) ou `notify-rust`, registrando o `replaces_id` no mapa de ações (`MAX_TRACKED_ACTIONS = 32`, `TTL = 5 min`).
  5. Um **único** stream compartilhado `zbus` cancelável escuta os sinais `ActionInvoked(id, action_key)` e `NotificationClosed(id, reason)`:
     - Quando `action_key == "default"` para um `id` rastreado: chama `QuakeController::show_and_focus()` e, se a aba ainda existir no `TabStatusProvider`, emite `os://focus-tab` com `{ tabId, paneId, projectId, source: "notification" }`.

### 3.2. Hook de Fim de Comando Longo (`crates/mt-os/src/long_command.rs`)

```rust
use crate::notify::NotificationKind;
use crate::settings::NotificationSettings;
use mt_core::ids::{PaneId, TabId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEndReason {
    Completed,
    Shutdown,
    MissingEnd,
    ModeSwitched,
}

#[derive(Debug, Clone)]
pub struct CommandFinishedInfo {
    pub block_id: String,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub command: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub end_reason: CommandEndReason,
}

/// Função pura (CA-27):
pub fn should_notify(
    info: &CommandFinishedInfo,
    s: &NotificationSettings,
    window_focused: bool,
    _tab_visible: bool,
) -> Option<NotificationKind> {
    if !s.enabled || !s.long_command_enabled {
        return None;
    }
    if info.end_reason != CommandEndReason::Completed {
        return None;
    }
    let threshold_ms = (s.long_command_threshold_secs as u64) * 1000;
    if info.duration_ms < threshold_ms {
        return None;
    }
    if s.only_when_unfocused && window_focused {
        return None;
    }
    match info.exit_code {
        Some(0) => Some(NotificationKind::CommandFinished),
        Some(_) => Some(NotificationKind::CommandFailed),
        None => None,
    }
}
```

### 3.3. Sincronização de Autostart (`crates/mt-os/src/autostart.rs`)

- Gerencia `~/.config/autostart/maxterminal.desktop` (respeitando `$XDG_CONFIG_HOME/autostart`).
- Quando `enabled == true`: grava atomicamente (via arquivo temporário no mesmo diretório + `rename`) uma entrada Desktop Entry válida com `Exec=<caminho_escapado> --minimized` (quando `start_minimized == true`) ou `Exec=<caminho_escapado>` (quando `start_minimized == false`).
- Quando `enabled == false`: remove `~/.config/autostart/maxterminal.desktop` se existir.
- No boot: se o arquivo `.desktop` foi removido manualmente pelo usuário fora do app (`status.enabled != settings.autostart`), reconcilia `settings.autostart = status.enabled` no banco (fonte da verdade = sistema — CA-30, CA-40).

---

## 4. Comandos IPC, Eventos e Capabilities no Crate Binário (`src-tauri/src/`)

### 4.1. Envelope de Eventos (`src-tauri/src/events/os.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OsEvent<T> {
    pub revision: u32,
    pub payload: T,
}
```
Emite os 10 eventos canônicos (`os://focus-tab`, `os://window-visibility`, `os://settings-changed`, `os://shortcut-status`, `os://counters`, `os://open-preferences`, `os://shutdown-started`, `os://shutdown-failed`, `os://environment-changed`, `os://link-context-changed`) com revisão monotônica incrementada atomicamente.

### 4.2. Os 16 Comandos IPC (`src-tauri/src/commands/os.rs`)

Todos anotados com `#[tauri::command]` e `#[specta::specta]`, validando que a invocação parte da janela local `"main"`:
1. `os_settings_get`
2. `os_settings_update` (com compensação em caso de falha de persistência — CA-40)
3. `os_environment_info`
4. `os_shortcut_status`
5. `os_shortcut_configure_portal`
6. `os_window_toggle`
7. `os_window_show`
8. `os_window_hide`
9. `os_quit_request`
10. `os_link_resolve`
11. `os_link_open` (re-resolve o candidato no backend antes de abrir — CA-21, CA-38)
12. `os_notify`
13. `os_attention_set`
14. `os_autostart_status`
15. `os_runtime_snapshot`
16. `os_ui_ready`

### 4.3. Capabilities e Permissões (`permissions/os.toml`, `capabilities/os.json`, `capabilities/default.json`)

- `src-tauri/build.rs`: registra os 16 comandos `os_*` no `tauri_build::AppManifest::new().commands(&[...])`.
- `src-tauri/permissions/os.toml`: define a permissão `allow-os-integration` liberando exclusivamente os 16 comandos `os_*`.
- `src-tauri/capabilities/os.json` (e referência em `capabilities/default.json`): concede `allow-os-integration`, `core:event:allow-listen` e `core:event:allow-unlisten` restritos à janela `"main"` e plataforma `"linux"`, **sem** expor permissões diretas de plugins (`opener`, `global-shortcut`, `autostart`, `notification`, `dialog`, `deep-link`) para o JS (CA-37).

---

## 5. Mapeamento para Critérios de Aceite

- **CA-02**: `tests/links.rs` (≥ 25 casos), `tests/editor_argv.rs` (4 editores + caracteres especiais), `tests/rate_limit.rs` (dedupe 10 s + teto 5/30 s) e unit tests de `long_command::should_notify`.
- **CA-06**: `npm run bindings` exporta os 16 comandos e todos os DTOs em `src/types/bindings.ts`.
- **CA-21..CA-24, CA-32, CA-38**: Resolução de links com validação de `cwd_epoch`, abertura segura em vetor sem shell e URI `vscode://file/` percent-encoded.
- **CA-26..CA-29, CA-39**: Notificações sanitizadas, rate-limited, stream D-Bus cancelável e foco na aba ao clicar.
- **CA-30, CA-40**: Autostart `.desktop` sincronizado, reconciliação de `start_minimized` e compensação transacional em `os_settings_update`.
- **CA-37**: Capabilities restritas à webview `"main"` local, testadas em `tests/capabilities.rs`.
