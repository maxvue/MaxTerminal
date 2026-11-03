//! Decisão pura de fechamento (`decide_close`) e coordenador de desligamento (`ShutdownCoordinator`).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use mt_core::error::{MtError, MtResult};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::error::{codes, os_error};
use crate::settings::CloseBehavior;

/// Decisão pura tomada ao interceptar o pedido de fechamento da janela principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CloseDecision {
    /// Oculta a janela na bandeja do sistema mantendo PTYs e servidores vivos.
    HideToTray,
    /// Minimiza a janela na barra de tarefas quando a bandeja não está disponível.
    Minimize,
    /// Inicia o encerramento imediato da aplicação.
    Quit,
    /// Pede confirmação ao usuário antes de encerrar porque há processos ativos.
    ConfirmQuit,
}

/// Contexto avaliado pela função pura `decide_close`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseContext {
    /// Preferência configurada pelo usuário (`Auto`, `AlwaysTray` ou `AlwaysQuit`).
    pub behavior: CloseBehavior,
    /// `true` se houver qualquer painel/aba em execução, spawn pendente ou filho ocupado.
    pub has_active: bool,
    /// `true` se a bandeja do sistema (`StatusNotifierWatcher` + ícone) estiver ativa.
    pub tray_available: bool,
    /// `true` somente quando o encerramento já foi autorizado internamente pelo coordenador.
    pub force_quit: bool,
}

/// Função pura — tabela-verdade de fechamento da janela (CA-02: 3 × 2 × 2 × 2 = 24 combinações).
#[must_use]
pub const fn decide_close(ctx: CloseContext) -> CloseDecision {
    if ctx.force_quit {
        return CloseDecision::Quit;
    }
    let keep_alive = match ctx.behavior {
        CloseBehavior::AlwaysTray => true,
        CloseBehavior::AlwaysQuit => false,
        CloseBehavior::Auto => ctx.has_active,
    };
    match (keep_alive, ctx.tray_available, ctx.behavior, ctx.has_active) {
        (true, true, ..) => CloseDecision::HideToTray,
        (true, false, ..) => CloseDecision::Minimize,
        (false, _, CloseBehavior::AlwaysQuit, true) => CloseDecision::ConfirmQuit,
        (false, ..) => CloseDecision::Quit,
    }
}

/// Origem de uma solicitação de encerramento da aplicação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum QuitSource {
    /// Botão fechar (`X`) da janela principal.
    Window,
    /// Item `"Sair"` do menu da bandeja do sistema.
    Tray,
    /// Comando IPC `os_quit_request`.
    Ipc,
    /// Sinal de término do sistema operacional (`SIGTERM`, `SIGINT`, `SIGHUP`).
    Signal,
    /// Invocação via linha de comando.
    Cli,
}

/// Fase atual da máquina de estados do `ShutdownCoordinator`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownPhase {
    /// Aplicação operando normalmente.
    #[default]
    Idle,
    /// Aguardando resposta do usuário no diálogo de confirmação de saída.
    Confirming,
    /// Congelando novos spawns e encerrando PTYs/workspace.
    Stopping,
    /// Drenando `ShellService`, `BatchWriter` e executando checkpoint WAL.
    Draining,
    /// Limpeza falhou (sobreviventes remanescentes); supervisão continua ativa.
    Failed,
    /// Limpeza concluída sem sobreviventes; autorizado para `app.exit(0)`.
    ReadyToExit,
}

/// Desfecho retornado por `request_quit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum QuitOutcome {
    /// O processo de encerramento foi aceito e concluído.
    Quitting,
    /// O usuário cancelou o diálogo de confirmação.
    Cancelled,
}

/// Relatório de encerramento da árvore de PTYs supervisionados.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownReport {
    /// Quantidade de processos encerrados graciosamente (`SIGTERM`).
    pub terminated: u32,
    /// Quantidade de processos encerrados forçadamente (`SIGKILL`).
    pub killed: u32,
    /// PIDs sobreviventes que não puderam ser encerrados dentro do prazo.
    pub survivors: Vec<u32>,
}

/// Hooks de desligamento e confirmação implementados pelo adaptador do crate binário.
#[async_trait]
pub trait ShutdownHooks: Send + Sync + 'static {
    /// Retorna a quantidade atual de abas/painéis ativos para exibição no diálogo de confirmação.
    fn active_count(&self) -> u32;

    /// Exibe o diálogo nativo de confirmação quando `active_count > 0`; retorna `true` se o usuário confirmou.
    async fn confirm_quit(&self, active_count: u32) -> bool;

    /// Bloqueia a criação de novos spawns no workspace/PTY manager.
    async fn freeze_spawns(&self) -> MtResult<()>;

    /// Encerra o workspace (Fase 07), arquivando sessões com `app_exit`.
    async fn shutdown_workspace(&self) -> MtResult<()>;

    /// Encerra PTYs remanescentes (Fase 02) dentro do prazo `grace`.
    async fn shutdown_ptys(&self, grace: Duration) -> ShutdownReport;

    /// Drena o banco de dados (Fase 03) e realiza checkpoint WAL limitado.
    async fn flush_db(&self) -> MtResult<()>;

    /// Notifica o início do encerramento (`os://shutdown-started`).
    fn on_shutdown_started(&self, source: QuitSource);

    /// Notifica falha de encerramento (`os://shutdown-failed`).
    fn on_shutdown_failed(&self, error: &MtError);

    /// Solicita a saída final do runtime (`app.exit(0)`) somente após `ReadyToExit`.
    fn exit_application(&self);
}

/// Coordenador idempotente do ciclo de encerramento do `MaxTerminal`.
pub struct ShutdownCoordinator {
    hooks: Arc<dyn ShutdownHooks>,
    phase: Mutex<ShutdownPhase>,
    op_lock: tokio::sync::Mutex<()>,
    exit_authorized: AtomicBool,
    grace_period: Duration,
}

impl ShutdownCoordinator {
    /// Cria um novo `ShutdownCoordinator` com prazo padrão de 3500 ms para encerramento de PTYs.
    #[must_use]
    pub fn new(hooks: Arc<dyn ShutdownHooks>) -> Self {
        Self {
            hooks,
            phase: Mutex::new(ShutdownPhase::Idle),
            op_lock: tokio::sync::Mutex::new(()),
            exit_authorized: AtomicBool::new(false),
            grace_period: Duration::from_millis(3500),
        }
    }

    /// Retorna a fase atual do coordenador.
    #[must_use]
    pub fn phase(&self) -> ShutdownPhase {
        *self.phase.lock()
    }

    /// Retorna `true` se o encerramento completo foi autorizado sem sobreviventes.
    #[must_use]
    pub fn is_exit_authorized(&self) -> bool {
        self.exit_authorized.load(Ordering::Acquire)
    }

    /// Solicita o encerramento da aplicação, pedindo confirmação se houver abas ativas e a origem não for `Signal`.
    pub async fn request_quit(&self, source: QuitSource) -> MtResult<QuitOutcome> {
        let _guard = self.op_lock.lock().await;

        let current_phase = *self.phase.lock();
        if matches!(
            current_phase,
            ShutdownPhase::Stopping | ShutdownPhase::Draining | ShutdownPhase::ReadyToExit
        ) {
            return Ok(QuitOutcome::Quitting);
        }

        if source != QuitSource::Signal {
            let active = self.hooks.active_count();
            if active > 0 {
                *self.phase.lock() = ShutdownPhase::Confirming;
                let confirmed = self.hooks.confirm_quit(active).await;
                if !confirmed {
                    let mut p = self.phase.lock();
                    if *p == ShutdownPhase::Confirming {
                        *p = ShutdownPhase::Idle;
                    }
                    return Ok(QuitOutcome::Cancelled);
                }
            }
        }

        let _report = self.execute_shutdown_locked(source).await?;
        Ok(QuitOutcome::Quitting)
    }

    /// Executa o desligamento sem nova confirmação interativa; só autoriza saída se `survivors` estiver vazio.
    pub async fn shutdown(&self, source: QuitSource) -> MtResult<ShutdownReport> {
        let _guard = self.op_lock.lock().await;
        self.execute_shutdown_locked(source).await
    }

    async fn execute_shutdown_locked(&self, source: QuitSource) -> MtResult<ShutdownReport> {
        *self.phase.lock() = ShutdownPhase::Stopping;
        self.hooks.on_shutdown_started(source);

        if let Err(err) = self.hooks.freeze_spawns().await {
            tracing::warn!(target: "mt_os", error = %err, "Falha ao congelar spawns durante desligamento");
        }

        if let Err(err) = self.hooks.shutdown_workspace().await {
            tracing::warn!(target: "mt_os", error = %err, "Falha ao encerrar workspace durante desligamento");
        }

        let report = self.hooks.shutdown_ptys(self.grace_period).await;

        *self.phase.lock() = ShutdownPhase::Draining;
        if let Err(err) = self.hooks.flush_db().await {
            tracing::warn!(target: "mt_os", error = %err, "Aviso ao drenar banco de dados no desligamento");
        }

        if !report.survivors.is_empty() {
            *self.phase.lock() = ShutdownPhase::Failed;
            let err = os_error(
                codes::SHUTDOWN_TIMEOUT,
                format!(
                    "Não foi possível encerrar {} processo(s) supervisionado(s) dentro do tempo limite.",
                    report.survivors.len()
                ),
            );
            self.hooks.on_shutdown_failed(&err);
            return Err(err);
        }

        self.exit_authorized.store(true, Ordering::Release);
        *self.phase.lock() = ShutdownPhase::ReadyToExit;
        self.hooks.exit_application();
        Ok(report)
    }
}
