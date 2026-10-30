# Implementação 03 — Adaptadores Binários, Comandos IPC, Eventos e Bindings Tauri

> **Fase:** 08 — Indicadores Contextuais: Git, Worktree e Portas Ativas
> **Unidade:** `03 - Adaptadores Binarios Comandos IPC Eventos e Bindings Tauri`
> **Referências Canônicas:** [`../instructions.md`](../instructions.md) §2.1, §3.1, §4.3, §5.1–§5.4; [`../spec.md`](../spec.md) §1.2–§1.3, §3.5–§3.6, §4.1–§4.2, §8, §10.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação integra os crates de domínio `mt-git` (Implementação 01) e `mt-ports` (Implementação 02) ao crate binário Tauri (`src-tauri/src/`, pacote `maxterminal`) e às fases predecessoras (Fase 01 `mt-core`, Fase 02 `mt-pty`, Fase 03 `mt-db::sanitizer`, Fase 05 `mt-shell` e Fase 07 `WorkspaceManager` / `WorkspaceObserver`).

Seguindo o princípio arquitetural de `spec.md` §1.3, **os adaptadores deste módulo são a única superfície que conhece os nomes concretos das Fases 02, 05 e 07**, isolando `mt-git` e `mt-ports` de mudanças futuras e mantendo a autoridade no backend Rust.

### 1.1. Objetivos Específicos
1. **Adaptador de Árvore de Processos e Fallback de CWD (`adapters/process_tree.rs`):** Implementar `mt_ports::tree::ProcessTreeSource` e `CwdFallback` sobre `mt_pty::PtyManager` (`summary`, `process_report`, `owned_processes`) e o registro de ownership da Fase 07 (`WorkspaceManager`).
2. **Adaptador de Sinalização Exata por `pidfd` (`adapters/exact_process_signaler.rs`):** Implementar `mt_ports::killer::ExactProcessSignaler` usando chamadas nativas Linux `pidfd_open(2)`, `pidfd_send_signal(2)` e `poll(2)` via `rustix`, validando atomicamente `PtyId`, `starttime`, UID, *network namespace* e a presença do inode do socket em `/proc/<pid>/fd/*` antes de cada sinal (`SIGTERM` e `SIGKILL`), sem jamais chamar `waitpid` ou `killpg`.
3. **Adaptador de Abertura de URL via `tauri-plugin-opener` (`adapters/port_opener.rs`):** Registrar `.plugin(tauri_plugin_opener::init())` no crate binário na Fase 08 e implementar `mt_ports::opener::PortUrlOpener` invocando `app.opener().open_url(url, None::<&str>)` dentro de `tokio::task::spawn_blocking`.
4. **Observador Autorizado de Workspace e Shell (`adapters/indicators.rs`):** Implementar `IndicatorWorkspaceObserver` conectado aos eventos de ciclo de vida da Fase 07 (`spawn`, `pty-replaced`, `exited`, `tab-closed`) e aos eventos da Fase 05 (mudança de `cwd` via OSC 7 e fim de comando via OSC 133 `D`), mantendo os bindings de `GitService` e `PortsService` sincronizados mesmo quando a WebView estiver oculta ou recarregando, além de coordenar a reserva de `cwd` entre abertura de aba e `git_worktree_remove`.
5. **Emissores de Eventos Tipados (`events/git.rs`, `events/ports.rs`):** Implementar `GitEventSink` e `PortEventSink` emitindo eventos tipados `tauri-specta` (`git://status-changed`, `port://opened`, `port://updated`, `port://closed`, `port://snapshot`) restritos à janela `main`.
6. **Comandos IPC Tauri (`commands/git.rs`, `commands/ports.rs`) e Geração de Bindings:** Expor os 9 comandos `git_*` e os 7 comandos `port_*`, sanitizar `stderr` de erros Git via `mt_db::sanitizer::sanitize()`, configurar capabilities para a janela `main` e regenerar `src/types/bindings.ts`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Modificações em Arquivos Existentes (`src-tauri/` e `src/types/`)
- `src-tauri/Cargo.toml` — Adicionar dependências `mt-git`, `mt-ports` e `tauri-plugin-opener = "2"` no crate binário.
- `src-tauri/capabilities/default.json` (ou `main.json`) — Autorizar os comandos `git_*` e `port_*` para a janela `main` (sem expor `opener:default` ao frontend).
- `src-tauri/src/adapters/mod.rs` — Exportar `process_tree`, `exact_process_signaler`, `port_opener` e `indicators`.
- `src-tauri/src/commands/mod.rs` — Exportar `git` e `ports`.
- `src-tauri/src/events/mod.rs` — Exportar `git` e `ports`.
- `src-tauri/src/lib.rs` — Inicializar `tauri_plugin_opener::init()`, instanciar `GitService`, `CliWorktreeManager` e `PortsService` no `State` do Tauri, registrar `IndicatorWorkspaceObserver`, registrar comandos e eventos no `tauri_specta::Builder` e exportar `src/types/bindings.ts`.
- `src/types/bindings.ts` — Regenerado automaticamente pelo pipeline `tauri-specta`.

### 2.2. Novos Arquivos no Crate Binário (`src-tauri/src/`)
- `src-tauri/src/adapters/process_tree.rs`
- `src-tauri/src/adapters/exact_process_signaler.rs`
- `src-tauri/src/adapters/port_opener.rs`
- `src-tauri/src/adapters/indicators.rs`
- `src-tauri/src/events/git.rs`
- `src-tauri/src/events/ports.rs`
- `src-tauri/src/commands/git.rs`
- `src-tauri/src/commands/ports.rs`

---

## 3. Especificação Técnica Detalhada e Assinaturas Rust

### 3.1. Adaptador `PtyProcessTreeAdapter` e `CwdFallback` (`src-tauri/src/adapters/process_tree.rs`)

```rust
use std::path::PathBuf;
use std::sync::Arc;
use mt_core::{PaneId, PtyId, TabId};
use mt_ports::error::PortsError;
use mt_ports::tree::{ProcId, ProcessTreeSource};
use mt_pty::PtyManager;

/// Consulta o `cwd` real do processo shell do PTY via Fase 02 quando o frontend/OSC 7 não informar `cwd`.
pub trait CwdFallback: Send + Sync + 'static {
    fn resolve_pane_cwd(&self, tab_id: TabId, pane_id: PaneId, pty_id: Option<PtyId>) -> Option<PathBuf>;
}

pub struct PtyProcessTreeAdapter {
    pty_manager: Arc<PtyManager>,
    // Referência ao registro do WorkspaceManager da Fase 07 para validar vínculo (tab_id, pane_id, pty_id)
}

impl PtyProcessTreeAdapter {
    pub fn new(pty_manager: Arc<PtyManager> /* , workspace: Arc<WorkspaceManager> */) -> Arc<Self>;

    /// Valida se `(tab_id, pane_id, pty_id)` corresponde ao `PtyOwner` registrado na Fase 02/07.
    pub fn verify_pane_ownership(&self, tab_id: TabId, pane_id: PaneId, pty_id: PtyId) -> Result<(), PortsError>;
}

impl ProcessTreeSource for PtyProcessTreeAdapter {
    fn root(&self, pty: PtyId) -> Result<Option<ProcId>, PortsError> {
        // 1. Consulta pty_manager.process_report(pty)
        // 2. Extrai `report.shell`: retorna Ok(Some(ProcId { pid: shell.pid as i32, starttime: shell.start_time }))
        // 3. Se PTY já saiu ou não existe, retorna Ok(None)
    }

    fn descendants(&self, pty: PtyId, root: ProcId) -> Result<Vec<ProcId>, PortsError> {
        // 1. Consulta pty_manager.owned_processes(pty) (inclui todo o cgroup do PTY: árvore PPID + daemons adotados)
        // 2. Confirma que `root` ainda está presente com o mesmo `start_time`
        // 3. Converte cada `ProcessIdentity { pid, start_time, .. }` em `ProcId { pid: pid as i32, starttime: start_time }`
    }

    fn owns(&self, pty: PtyId, process: ProcId) -> Result<bool, PortsError> {
        // 1. Consulta pty_manager.owned_processes(pty)
        // 2. Verifica se existe identidade com `pid == process.pid as u32 && start_time == process.starttime`
        // 3. Reconfirma `mt_ports::fdmap::read_starttime(process.pid) == Some(process.starttime)`
    }

    fn has_active_foreground(&self, pty: PtyId) -> bool {
        // Retorna true se `process_report(pty).foreground` existir e seu `pid` for diferente do `shell.pid`
    }
}

impl CwdFallback for PtyProcessTreeAdapter {
    fn resolve_pane_cwd(&self, _tab_id: TabId, _pane_id: PaneId, pty_id: Option<PtyId>) -> Option<PathBuf> {
        let pty = pty_id?;
        let report = self.pty_manager.process_report(pty).ok()?;
        if let Some(shell) = report.shell {
            if let Some(cwd) = shell.cwd {
                return Some(PathBuf::from(cwd));
            }
        }
        let summary = self.pty_manager.summary(pty).ok()?;
        Some(PathBuf::from(summary.cwd_initial))
    }
}
```

### 3.2. Adaptador `LinuxPidfdSignaler` (`src-tauri/src/adapters/exact_process_signaler.rs`)

```rust
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;
use mt_core::PtyId;
use mt_ports::error::PortsError;
use mt_ports::fdmap::{read_proc_stat, read_starttime};
use mt_ports::killer::{ExactProcessSignaler, ProcessHandle, SignalKind};
use mt_ports::procnet::read_netns_inode_for_pid;
use mt_ports::tree::{ProcId, ProcessTreeSource};
use rustix::process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal};

pub struct LinuxPidfdSignaler {
    tree: Arc<dyn ProcessTreeSource>,
}

struct PidfdInner {
    fd: OwnedFd,
}

impl LinuxPidfdSignaler {
    pub fn new(tree: Arc<dyn ProcessTreeSource>) -> Arc<Self> {
        Arc::new(Self { tree })
    }

    /// Verifica se `/proc/<pid>/fd` ainda contém pelo menos um `socket:[<inode>]` em `expected_inodes`.
    fn process_holds_any_inode(pid: i32, expected_inodes: &[u64]) -> Result<bool, PortsError>;
}

impl ExactProcessSignaler for LinuxPidfdSignaler {
    fn is_supported(&self) -> bool {
        // Testa suporte de kernel abrindo pidfd no próprio PID (`std::process::id()`)
        Pid::from_raw(std::process::id() as i32)
            .and_then(|pid| pidfd_open(pid, PidfdFlags::empty()).ok())
            .is_some()
    }

    fn acquire(
        &self,
        pty: PtyId,
        root: ProcId,
        owner: ProcId,
        expected_uid: u32,
        expected_netns: u64,
        expected_inodes: &[u64],
    ) -> Result<ProcessHandle, PortsError> {
        // 1. Recusa PID <= 1, PID do próprio MaxTerminal ou `owner == root` (`KillRootRefused`)
        // 2. Abre `pidfd_open(Pid::from_raw(owner.pid), PidfdFlags::empty())`:
        //    - `ESRCH` => `Err(PortsError::OwnerGone)`
        //    - `ENOSYS` / `EPERM` de syscall => `Err(PortsError::SafeSignalUnavailable)`
        // 3. Com o `pidfd` já aberto, relê `/proc/<pid>/stat` e confirma `starttime == owner.starttime`;
        //    se divergir, o PID foi reutilizado antes de `pidfd_open` => `Err(PortsError::OwnerGone)`.
        // 4. Confirma `self.tree.owns(pty, owner)? == true`, UID == `expected_uid` e `netns == expected_netns`
        //    (divergência => `Err(PortsError::NotTracked)`).
        // 5. Confirma `Self::process_holds_any_inode(owner.pid, expected_inodes)?`
        // 6. Retorna `ProcessHandle::new(pty, root, owner, expected_uid, expected_netns, PidfdInner { fd })`.
    }

    fn revalidate_and_signal(
        &self,
        handle: &ProcessHandle,
        expected_inodes: &[u64],
        signal: SignalKind,
    ) -> Result<bool, PortsError> {
        // 1. Se `self.has_exited(handle)?` for true => retorna `Err(PortsError::OwnerGone)`
        // 2. Confirma `read_starttime(handle.owner.pid) == Some(handle.owner.starttime)` e `self.tree.owns(handle.pty_id, handle.owner)?`
        // 3. Confirma `Self::process_holds_any_inode(handle.owner.pid, expected_inodes)?`:
        //    - Se retornar `false` (processo vivo mas fechou o socket) => retorna `Ok(false)` (`SocketReleased`!)
        // 4. Envia sinal pelo `pidfd` via `pidfd_send_signal(inner.fd.as_fd(), sig)`:
        //    - `ESRCH` => `Err(PortsError::OwnerGone)`
        //    - `EPERM` => `Err(PortsError::PermissionDenied)`
        // 5. Retorna `Ok(true)`
    }

    fn has_exited(&self, handle: &ProcessHandle) -> Result<bool, PortsError> {
        // 1. Poll não-bloqueante (`timeout = 0`) com `rustix::event::poll` (`PollFlags::IN`) no `pidfd`:
        //    se legível => processo terminou (`Ok(true)`).
        // 2. Além disso, inspeciona `read_proc_stat(handle.owner.pid)`: se `None`, se `starttime != handle.owner.starttime`
        //    ou se `state_char` for `'Z'` (Zombie) ou `'X'` (Dead) => retorna `Ok(true)`.
    }
}
```

### 3.3. Adaptador `TauriPortUrlOpener` (`src-tauri/src/adapters/port_opener.rs`)

```rust
use std::sync::Arc;
use mt_ports::error::PortsError;
use mt_ports::opener::PortUrlOpener;
use tauri::{AppHandle, Runtime};
use tauri_plugin_opener::OpenerExt;

pub struct TauriPortUrlOpener<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriPortUrlOpener<R> {
    pub fn new(app: AppHandle<R>) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl<R: Runtime> PortUrlOpener for TauriPortUrlOpener<R> {
    fn open(&self, url: &str) -> Result<(), PortsError> {
        let app = self.app.clone();
        let url_owned = url.to_string();
        // Executa abertura de URL fora da thread de async I/O caso chamada de contexto síncrono/blocking
        app.opener()
            .open_url(&url_owned, None::<&str>)
            .map_err(|err| PortsError::OpenerFailed(err.to_string()))
    }
}
```

### 3.4. Emissores de Eventos Tipados (`src-tauri/src/events/git.rs`, `src-tauri/src/events/ports.rs`)

```rust
// src-tauri/src/events/git.rs
use std::sync::Arc;
use mt_git::GitEventSink;
use mt_git::types::GitStatusChanged;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[tauri_specta(event_name = "git://status-changed")]
pub struct GitStatusChangedEvent(pub GitStatusChanged);

pub struct TauriGitEventSink<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriGitEventSink<R> {
    pub fn new(app: AppHandle<R>) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl<R: Runtime> GitEventSink for TauriGitEventSink<R> {
    fn status_changed(&self, payload: GitStatusChanged) {
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = GitStatusChangedEvent(payload).emit_to(&win);
        }
    }
}
```

```rust
// src-tauri/src/events/ports.rs
use std::sync::Arc;
use mt_ports::PortEventSink;
use mt_ports::types::{PortClosed, PortOpened, PortSnapshot, PortUpdated};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[tauri_specta(event_name = "port://opened")]
pub struct PortOpenedEvent(pub PortOpened);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[tauri_specta(event_name = "port://updated")]
pub struct PortUpdatedEvent(pub PortUpdated);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[tauri_specta(event_name = "port://closed")]
pub struct PortClosedEvent(pub PortClosed);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[tauri_specta(event_name = "port://snapshot")]
pub struct PortSnapshotEvent(pub PortSnapshot);

pub struct TauriPortEventSink<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriPortEventSink<R> {
    pub fn new(app: AppHandle<R>) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl<R: Runtime> PortEventSink for TauriPortEventSink<R> {
    fn opened(&self, event: PortOpened) {
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = PortOpenedEvent(event).emit_to(&win);
        }
    }
    fn updated(&self, event: PortUpdated) {
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = PortUpdatedEvent(event).emit_to(&win);
        }
    }
    fn closed(&self, event: PortClosed) {
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = PortClosedEvent(event).emit_to(&win);
        }
    }
    fn snapshot(&self, snapshot: PortSnapshot) {
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = PortSnapshotEvent(snapshot).emit_to(&win);
        }
    }
}
```

### 3.5. Comandos IPC Tauri (`src-tauri/src/commands/git.rs`, `src-tauri/src/commands/ports.rs`)

#### `src-tauri/src/commands/git.rs`
```rust
use std::path::PathBuf;
use std::sync::Arc;
use mt_core::{MtError, PaneId, TabId};
use mt_git::GitService;
use mt_git::error::GitError;
use mt_git::types::{
    GitContext, GitStatusChanged, RefreshReason, WorktreeCreateRequest, WorktreeEntry,
};
use mt_git::worktree::WorktreeManager;
use tauri::State;
use crate::adapters::process_tree::CwdFallback;

/// Sanitiza mensagens e `stderr` de `GitError` com `mt_db::sanitizer::sanitize` na fronteira IPC.
fn map_git_err(err: GitError) -> MtError {
    let sanitized = match err {
        GitError::CommandFailed { code, stderr } => {
            let clean = mt_db::sanitizer::sanitize(&stderr);
            let truncated: String = clean.chars().take(2048).collect();
            GitError::CommandFailed { code, stderr: truncated }
        }
        other => other,
    };
    MtError::from(sanitized)
}

#[tauri::command]
#[specta::specta]
pub async fn git_track_pane(
    git: State<'_, Arc<GitService>>,
    cwd_fallback: State<'_, Arc<dyn CwdFallback>>,
    tab_id: TabId,
    pane_id: PaneId,
    cwd: Option<String>,
    visible: bool,
) -> Result<GitStatusChanged, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn git_untrack_pane(
    git: State<'_, Arc<GitService>>,
    pane_id: PaneId,
) -> Result<(), MtError>;

#[tauri::command]
#[specta::specta]
pub async fn git_set_visible_tab(
    git: State<'_, Arc<GitService>>,
    tab_id: Option<TabId>,
) -> Result<(), MtError>;

#[tauri::command]
#[specta::specta]
pub async fn git_refresh(
    git: State<'_, Arc<GitService>>,
    pane_id: PaneId,
    reason: RefreshReason,
) -> Result<(), MtError>;

#[tauri::command]
#[specta::specta]
pub async fn git_status(
    git: State<'_, Arc<GitService>>,
    cwd: String,
) -> Result<Option<GitContext>, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn git_worktree_list(
    worktrees: State<'_, Arc<dyn WorktreeManager>>,
    repo_path: String,
) -> Result<Vec<WorktreeEntry>, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn git_worktree_create(
    worktrees: State<'_, Arc<dyn WorktreeManager>>,
    req: WorktreeCreateRequest,
) -> Result<WorktreeEntry, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn git_worktree_remove(
    worktrees: State<'_, Arc<dyn WorktreeManager>>,
    path: String,
    force: bool,
) -> Result<(), MtError>;

#[tauri::command]
#[specta::specta]
pub async fn git_worktree_prune(
    worktrees: State<'_, Arc<dyn WorktreeManager>>,
    repo_path: String,
) -> Result<(), MtError>;
```

#### `src-tauri/src/commands/ports.rs`
```rust
use std::sync::Arc;
use mt_core::{MtError, PaneId, PtyId, TabId, TabStatus};
use mt_ports::PortsService;
use mt_ports::types::{PortId, PortKillResult, PortSnapshot};
use tauri::State;
use crate::adapters::process_tree::PtyProcessTreeAdapter;

#[tauri::command]
#[specta::specta]
pub async fn port_track_pane(
    ports: State<'_, Arc<PortsService>>,
    tree_adapter: State<'_, Arc<PtyProcessTreeAdapter>>,
    tab_id: TabId,
    pane_id: PaneId,
    pty_id: PtyId,
) -> Result<PortSnapshot, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn port_untrack_pane(
    ports: State<'_, Arc<PortsService>>,
    tab_id: TabId,
    pane_id: PaneId,
) -> Result<(), MtError>;

#[tauri::command]
#[specta::specta]
pub async fn port_set_tab_status(
    ports: State<'_, Arc<PortsService>>,
    tab_id: TabId,
    status: TabStatus,
) -> Result<(), MtError>;

#[tauri::command]
#[specta::specta]
pub async fn port_list(
    ports: State<'_, Arc<PortsService>>,
    tab_id: TabId,
) -> Result<PortSnapshot, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn port_refresh(
    ports: State<'_, Arc<PortsService>>,
    tab_id: TabId,
) -> Result<PortSnapshot, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn port_open_browser(
    ports: State<'_, Arc<PortsService>>,
    tab_id: TabId,
    port_id: PortId,
) -> Result<(), MtError>;

#[tauri::command]
#[specta::specta]
pub async fn port_kill(
    ports: State<'_, Arc<PortsService>>,
    tab_id: TabId,
    port_id: PortId,
) -> Result<PortKillResult, MtError>;
```

### 3.6. Observador de Workspace e Shell (`src-tauri/src/adapters/indicators.rs`)

- Implementa o `IndicatorWorkspaceObserver` conectado à Fase 07 e aos eventos de shell da Fase 05:
  1. Ao abrir/restaurar aba ou dividir painel com `PtyId` válido: chama `git.track_pane(...)` e `ports.track_pane(...)`.
  2. Ao substituir o `PtyId` de um painel (`pane://pty-replaced`, reinício ou nova sessão): invalida o binding anterior e registra o novo `PtyId`.
  3. Ao encerrar painel/PTY (`pane://exited`) ou fechar aba (`tab://closed`): remove o rastreamento de portas do painel/aba e invalida os tokens `PortId` daquela execução.
  4. Ao receber mudança de `cwd` validada por OSC 7 da Fase 05: atualiza `git.track_pane` para o painel correspondente.
  5. Ao receber fim de comando (OSC 133 `D`) da Fase 05 para um `PtyId`: dispara `git.request_refresh(pane_id, RefreshReason::CommandFinished)` e `ports.notify_command_finished(tab_id)`.
  6. Expõe hook de reserva de `cwd` (`git.reserve_cwd(cwd)`) para que o fluxo de abertura de aba/painel da Fase 07 impeça `git_worktree_remove` concorrente no mesmo diretório.
