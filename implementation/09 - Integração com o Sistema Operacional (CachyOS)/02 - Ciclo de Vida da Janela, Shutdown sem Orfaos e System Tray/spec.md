# Implementação 02 — Ciclo de Vida da Janela, Shutdown sem Órfãos e System Tray (`spec.md`)

> Escopo desta sub-implementação: implementar o controle de ciclo de vida da janela (`lifecycle.rs`), a decisão pura de fechamento (`decide_close` / `decide_close_action`), o coordenador serializado de encerramento sem processos órfãos (`ShutdownCoordinator`), o controlador da bandeja do sistema (`tray.rs` com `TrayController` e `TrayVisualState`), a recuperação automática quando o `StatusNotifierWatcher` desaparece do D-Bus e os testes automatizados/script de verificação anti-órfãos (`tests/close_decision.rs`, `scripts/check-orphans.sh`).

---

## 1. Ciclo de Vida da Janela e `ShutdownCoordinator` (`crates/mt-os/src/lifecycle.rs`)

### 1.1. Decisão Pura de Fechamento (`decide_close` / `decide_close_action`)

```rust
use crate::settings::CloseBehavior;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CloseDecision {
    HideToTray,
    Minimize,
    Quit,
    ConfirmQuit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseContext {
    pub behavior: CloseBehavior,
    /// Calculado no instante do fechamento via `ActivitySnapshot::has_live_activity(awaiting_input)`.
    pub has_active: bool,
    /// Exige simultaneamente `StatusNotifierWatcher` ativo no D-Bus **e** `TrayIcon` registrado com sucesso.
    pub tray_available: bool,
    /// Aprovação interna já concedida pelo `ShutdownCoordinator` (nunca exposta como parâmetro público de bypass).
    pub force_quit: bool,
}

/// Função pura — tabela-verdade de 24 combinações (3 CloseBehavior × 2 has_active × 2 tray_available × 2 force_quit).
pub fn decide_close(ctx: CloseContext) -> CloseDecision {
    use CloseBehavior::*;
    if ctx.force_quit {
        return CloseDecision::Quit;
    }
    let keep_alive = match ctx.behavior {
        AlwaysTray => true,
        AlwaysQuit => false,
        Auto => ctx.has_active,
    };
    match (keep_alive, ctx.tray_available, ctx.behavior, ctx.has_active) {
        (true, true, ..) => CloseDecision::HideToTray,
        (true, false, ..) => CloseDecision::Minimize,
        (false, _, AlwaysQuit, true) => CloseDecision::ConfirmQuit,
        (false, ..) => CloseDecision::Quit,
    }
}

/// Alias semântico para consumidores que referenciam `decide_close_action`.
#[inline]
pub fn decide_close_action(ctx: CloseContext) -> CloseDecision {
    decide_close(ctx)
}
```

#### Tabela-Verdade Completa (`force_quit = false`, 12 casos + 12 casos `force_quit = true` ⇒ `Quit`):

| `behavior` | `has_active` | `tray_available` | `keep_alive` | `CloseDecision` |
|---|---|---|---|---|
| `Auto` | `false` | `false` | `false` | `Quit` |
| `Auto` | `false` | `true` | `false` | `Quit` |
| `Auto` | `true` | `false` | `true` | `Minimize` |
| `Auto` | `true` | `true` | `true` | `HideToTray` |
| `AlwaysTray` | `false` | `false` | `true` | `Minimize` |
| `AlwaysTray` | `false` | `true` | `true` | `HideToTray` |
| `AlwaysTray` | `true` | `false` | `true` | `Minimize` |
| `AlwaysTray` | `true` | `true` | `true` | `HideToTray` |
| `AlwaysQuit` | `false` | `false` | `false` | `Quit` |
| `AlwaysQuit` | `false` | `true` | `false` | `Quit` |
| `AlwaysQuit` | `true` | `false` | `false` | `ConfirmQuit` |
| `AlwaysQuit` | `true` | `true` | `false` | `ConfirmQuit` |

---

### 1.2. Contratos de Encerramento (`ShutdownHooks`, `ShutdownReport`, `ShutdownCoordinator`)

```rust
use crate::error::{codes, os_err, OsResult};
use crate::status::TabStatusProvider;
use mt_core::error::MtError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Runtime};
use tokio::sync::{watch, Mutex};

pub const DEFAULT_PTY_SHUTDOWN_GRACE: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum QuitSource {
    Window,
    Tray,
    Ipc,
    Signal,
    Cli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum QuitOutcome {
    Quitting,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownPhase {
    Idle,
    Confirming,
    Stopping,
    Draining,
    Failed,
    ReadyToExit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownReport {
    pub terminated: u32,
    pub killed: u32,
    pub survivors: Vec<u32>,
}

#[async_trait::async_trait]
pub trait ShutdownHooks: Send + Sync + 'static {
    /// Bloqueia novos spawns de PTY/painéis imediatamente após a confirmação de saída.
    async fn freeze_spawns(&self) -> OsResult<()>;
    /// Reativa spawns caso o shutdown falhe e o usuário precise retomar a sessão.
    async fn unfreeze_spawns(&self) -> OsResult<()> {
        Ok(())
    }
    /// Fase 07: encerra PTYs do workspace, aguarda taps de saída, drena recorder e arquiva sessões com `app_exit`.
    async fn shutdown_workspace(&self) -> OsResult<()>;
    /// Fase 02: reconcilia quaisquer PTYs remanescentes e retorna relatório com sobreviventes (identidade PID+start_time).
    async fn shutdown_ptys(&self, grace: Duration) -> ShutdownReport;
    /// Fase 03/05: drena filas do ShellService/DbWriter e executa checkpoint WAL limitado (`PASSIVE`/`TRUNCATE`).
    async fn flush_db(&self) -> OsResult<()>;
}

pub struct ShutdownCoordinator<R: Runtime> {
    app: AppHandle<R>,
    hooks: Arc<dyn ShutdownHooks>,
    status_provider: Arc<dyn TabStatusProvider>,
    attention: Arc<crate::attention::AttentionRegistry>,
    phase: Mutex<ShutdownPhase>,
    in_flight_tx: watch::Sender<Option<Result<QuitOutcome, MtError>>>,
    exit_authorized: AtomicBool,
}
```

### 1.3. Regras de Negócio e Concorrência do `ShutdownCoordinator`

1. **Máquina de Estados Serializada**:
   - Estados: `Idle → Confirming → Stopping → Draining → ReadyToExit` (ou `Failed` se restarem sobreviventes ou erro crítico).
   - Se `request_quit(source)` for chamado enquanto a fase já estiver em `Confirming`, `Stopping` ou `Draining`:
     - Se a nova chamada vier de `QuitSource::Signal`, ela **promove** imediatamente um `Confirming` para `Stopping` (sinais do SO não aguardam diálogo interativo).
     - Caso contrário, a chamada concorrente inscreve-se no canal `in_flight_tx` e aguarda o mesmo resultado (idempotência garantida para CA-34: apenas 1 diálogo e 1 sequência de shutdown).
2. **Diálogo de Confirmação (`Confirming`)**:
   - Quando `source != QuitSource::Signal` e houver atividade viva (`activity.has_live_activity(awaiting_count)`):
     - Calcula `active_count = counters.running + counters.continuous + activity.active_panes.max(1)` (exibindo a contagem de abas/painéis em execução, nunca confundindo com número de PIDs).
     - Abre diálogo nativo via `tauri-plugin-dialog`:
       - Título: `"Sair do MaxTerminal?"`
       - Mensagem: `"Existem {N} abas em execução. Sair encerrará todos os processos (inclusive servidores)."`
       - Tipo: `MessageDialogKind::Warning`
       - Botões: `OkCancelCustom("Sair e encerrar tudo", "Cancelar")`
     - Funciona mesmo com a janela principal oculta na bandeja (CA-11).
     - Se o usuário clicar em `"Cancelar"`, a fase retorna para `Idle` e retorna `Ok(QuitOutcome::Cancelled)`.
3. **Execução do Encerramento (`Stopping → Draining`)**:
   - Emite `os://shutdown-started` com `{ source }`.
   - Chama `hooks.freeze_spawns().await` para bloquear novos processos.
   - Chama `hooks.shutdown_workspace().await` (Fase 07 encerra PTYs via Fase 02 e arquiva sessões com motivo `app_exit`).
   - Chama `hooks.shutdown_ptys(DEFAULT_PTY_SHUTDOWN_GRACE).await` para reconciliar eventuais PTYs fora do workspace.
   - Transiciona para `Draining` e chama `hooks.flush_db().await`.
4. **Verificação de Sobreviventes (`survivors`)**:
   - Se `!report.survivors.is_empty()`:
     - Transiciona para `ShutdownPhase::Failed` (mantendo supervisão viva e reabilitando interação na janela).
     - Torna a janela visível (`unminimize + show + set_focus`) para que o usuário não fique com um processo invisível travado.
     - Emite `os://shutdown-failed` com `{ error, phase: "failed" }`.
     - Retorna `Err(MtError::new(codes::SHUTDOWN_TIMEOUT, format!("Não foi possível encerrar {} processo(s): {:?}.", report.survivors.len(), report.survivors)))`.
     - **Nunca** chama `app.exit(0)` quando há sobreviventes (CA-11, CA-34).
   - Se `report.survivors.is_empty()`:
     - Define `exit_authorized.store(true, Ordering::SeqCst)`, transiciona para `ReadyToExit` e chama `app.exit(0)`.
5. **Barreira `RunEvent::ExitRequested` e Sinais Unix**:
   - No handler de `RunEvent::ExitRequested { api, .. }`: se `!coordinator.is_exit_authorized()`, chama obrigatoriamente `api.prevent_exit()` (mesmo quando `code: Some(_)` estiver presente) e dispara `coordinator.request_quit(QuitSource::Window)` em task assíncrona (CA-34).
   - Listener dedicado com `tokio::signal::unix::signal` para `SIGTERM`, `SIGINT` e `SIGHUP`: aciona `coordinator.shutdown(QuitSource::Signal)` sem abrir diálogo (CA-14).
   - Ocultar a janela (`HideToTray` ou `Minimize`) **jamais** chama `ShutdownHooks`: todos os PTYs, leitores de output, supervisão `/proc` e gravação sanitizada continuam operando normalmente (CA-07, CA-08, CA-33).

---

## 2. System Tray e Recuperação D-Bus (`crates/mt-os/src/tray.rs`)

### 2.1. Estados Visuais e Estrutura do `TrayController`

```rust
use crate::error::{codes, os_err, OsResult};
use crate::status::StatusCounters;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Weak;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Runtime,
};
use tokio::sync::mpsc;

pub const TRAY_DEBOUNCE_WINDOW: Duration = Duration::from_millis(250);

pub const ICON_TRAY_IDLE: &[u8] = include_bytes!("../icons/tray/idle.png");
pub const ICON_TRAY_ACTIVE: &[u8] = include_bytes!("../icons/tray/active.png");
pub const ICON_TRAY_ATTENTION: &[u8] = include_bytes!("../icons/tray/attention.png");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TrayVisualState {
    Idle,
    Active,
    Attention,
}

impl TrayVisualState {
    pub fn from_counters(c: &StatusCounters) -> Self {
        if c.needs_attention() {
            Self::Attention
        } else if c.has_active() {
            Self::Active
        } else {
            Self::Idle
        }
    }
}

pub struct TrayMenuItems<R: Runtime> {
    pub toggle: MenuItem<R>,
    pub count_running: MenuItem<R>,
    pub count_continuous: MenuItem<R>,
    pub count_finished: MenuItem<R>,
    pub count_error: MenuItem<R>,
    pub count_awaiting: MenuItem<R>,
    pub preferences: MenuItem<R>,
    pub quit: MenuItem<R>,
}

pub struct TrayController<R: Runtime> {
    tray: TrayIcon<R>,
    items: TrayMenuItems<R>,
    last: Mutex<(StatusCounters, TrayVisualState, bool)>,
    refresh_tx: mpsc::Sender<()>,
}
```

### 2.2. Especificação do Menu e Debounce de 250 ms

1. **Itens e IDs Canônicos (pt-BR)**:
   - `tray.toggle` (habilitado): `"Ocultar MaxTerminal"` quando `visible == true`, `"Mostrar MaxTerminal"` quando `visible == false`.
   - Separador (`PredefinedMenuItem::separator`).
   - `tray.count.running` (desabilitado): `"● Executando: {running}"`.
   - `tray.count.continuous` (desabilitado): `"● Execução contínua: {continuous}"`.
   - `tray.count.finished` (desabilitado): `"● Finalizado: {finished_unseen}"`.
   - `tray.count.error` (desabilitado): `"● Com erro: {error_unseen}"`.
   - `tray.count.awaiting` (desabilitado, sempre visível mesmo quando `0`): `"● Aguardando intervenção: {awaiting_input}"`.
   - Separador (`PredefinedMenuItem::separator`).
   - `tray.preferences` (habilitado): `"Preferências…"` → chama `QuakeController::show_and_focus()` e emite `os://open-preferences` com `{ section: "system" }`.
   - `tray.quit` (habilitado): `"Sair"` → dispara `ShutdownCoordinator::request_quit(QuitSource::Tray)`.
2. **Coalescência / Debounce de 250 ms**:
   - `request_refresh(&self)` faz `let _ = self.refresh_tx.try_send(());` em canal de capacidade `1`.
   - Uma task em background lê do `refresh_rx`, aguarda `tokio::time::sleep(Duration::from_millis(250))`, drena sinais extras acumulados na janela, calcula `StatusCounters::aggregate`, lê a visibilidade da janela, emite `os://counters` e invoca `apply(counters, visible)` na thread principal.
   - `apply` atualiza os textos via `MenuItem::set_text` (sem reconstruir o menu) e chama `tray.set_icon(...)` **somente** quando `TrayVisualState` muda em relação ao valor em `self.last`. Também atualiza `tray.set_title(Some(&format!("{} ativas", counters.running + counters.continuous)))`.

### 2.3. Monitoramento D-Bus `StatusNotifierWatcher` e Recuperação de Janela Oculta (CA-13, CA-35)

- Uma task assíncrona observa o sinal `NameOwnerChanged` do barramento de sessão D-Bus (`org.freedesktop.DBus`) filtrando `org.kde.StatusNotifierWatcher` e `org.freedesktop.StatusNotifierWatcher`.
- Quando o watcher perde o proprietário (`new_owner.is_empty()`):
  1. Atualiza `env.write().tray_available = false` e emite `os://environment-changed`.
  2. Se a janela principal estiver invisível (`!window.is_visible()`), executa recuperação imediata:
     - `window.set_skip_taskbar(false)`
     - `window.unminimize()`
     - `window.show()`
     - Tenta `window.minimize()` apenas se suportado pelo compositor mantendo entrada na taskbar; se não houver confirmação de suporte (ex.: Wayland tiling), mantém a janela visível na tela (CA-35).
- Quando o watcher reaparece (`!new_owner.is_empty()`):
  1. Revalida ou recria o `TrayController`, atualiza `env.write().tray_available = true` e emite `os://environment-changed`.

---

## 3. Script de Verificação Anti-Órfãos (`scripts/check-orphans.sh`)

Script executável POSIX/Bash para validação automatizada e manual de CA-09, CA-11 e CA-14:
- Captura a árvore de processos descendentes a partir do PID da instância de teste do MaxTerminal (ou dos PIDs dos shells registrados no fixture), gravando pares `(pid, starttime_ticks)` lidos do campo 22 de `/proc/<pid>/stat` antes do encerramento.
- Mesmo que processos netos (ex.: `node` filho de `npm run dev` ou `sleep 600` em subshell) sofram reparenting para `PID 1` / `systemd --user`, o script verifica todos os pares `(pid, starttime_ticks)` capturados por até 5 segundos após o encerramento do MaxTerminal.
- Falha com `exit 1` se qualquer processo com o mesmo `(pid, starttime_ticks)` permanecer vivo após 5 s.

---

## 4. Mapeamento para Critérios de Aceite

- **CA-02**: `tests/close_decision.rs` cobre os 24 casos da tabela-verdade de `decide_close()` e a serialização concorrente do `ShutdownCoordinator`.
- **CA-07, CA-08**: `CloseDecision::HideToTray` apenas oculta a janela e preserva PTYs e servidores HTTP vivos.
- **CA-09, CA-11, CA-14**: `ShutdownCoordinator` encerra árvore de processos, aguarda `ShutdownReport` sem sobreviventes e responde a `SIGTERM`/`SIGINT` sem órfãos.
- **CA-10, CA-12**: Menu do tray reflete contadores em ≤ 500 ms (debounce de 250 ms) e alterna ícones `idle`, `active` e `attention`.
- **CA-13, CA-35**: Queda ou ausência de `StatusNotifierWatcher` degrada `HideToTray` para `Minimize` e recupera janela oculta automaticamente.
- **CA-33, CA-34**: Fechamento inspeciona todos os painéis via `ActivitySnapshot`; pedidos simultâneos de saída são deduplicados e `ExitRequested` nunca pula o coordenador.
