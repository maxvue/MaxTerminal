//! Máquina de estados determinística e pura para painéis (`PaneInternalState`) e agregação de abas (`TabStateSnapshot`).
//!
//! Implementa os 5 estados canônicos de `AGENTS.md` §4.1:
//! 1. `Running` (🟢 Verde)
//! 2. `Continuous` (🔵 Azul-Teal)
//! 3. `CompletedUnseen` (🟠 Âmbar)
//! 4. `ErrorUnseen` (🔴 Vermelho)
//! 5. `Idle` (⚪ Cinza Neutro)
//!
//! Precedência multi-painel estrita:
//! `ErrorUnseen > Running > Continuous > CompletedUnseen > Idle`.

use mt_core::ids::{CommandBlockId, PaneId, PtyId, SessionId, TabId};
use mt_core::shell::{
    AttentionSnapshot, BlockEndReason, BlockStatus, CompletionOutcome, CompletionSnapshot,
    ContinuousPolicy, ExecutionRef, IntegrationMode, PaneStateDto, TabStateSnapshot, TabStatus,
};

use crate::state::daemon::{is_continuous_by_policy_or_command, should_promote_to_continuous};

/// Estado interno completo de um painel individual gerenciado pela máquina de estados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneInternalState {
    /// Identificador opcional do painel.
    pub pane_id: Option<PaneId>,
    /// Identificador do PTY ativo neste painel.
    pub pty_id: PtyId,
    /// Identificador da sessão SQLite vinculada.
    pub session_id: SessionId,
    /// Estado semântico consolidado atual deste painel.
    pub status: TabStatus,
    /// Modo de integração do shell neste painel.
    pub integration_mode: IntegrationMode,
    /// Política de execução contínua configurada para este painel.
    pub continuous_policy: ContinuousPolicy,
    /// Último diretório de trabalho sanitizado reportado.
    pub cwd: Option<String>,
    /// Execução em andamento, se houver.
    pub active_execution: Option<ExecutionRef>,
    /// Indica se a execução ativa já foi promovida para `Continuous`.
    pub active_is_continuous: bool,
    /// Indica se a execução ativa ainda aguarda o fim da janela de histerese (80 ms).
    pub pending_running_hysteresis: bool,
    /// Última execução concluída neste painel.
    pub last_completion: Option<CompletionSnapshot>,
    /// Timestamp UNIX em ms da última transição.
    pub updated_at: i64,
}

impl PaneInternalState {
    /// Cria um novo estado inicial de painel em `TabStatus::Idle`.
    #[must_use]
    pub fn new(
        pane_id: Option<PaneId>,
        pty_id: PtyId,
        session_id: SessionId,
        integration_mode: IntegrationMode,
        continuous_policy: ContinuousPolicy,
        initial_cwd: Option<String>,
        now_ms: i64,
    ) -> Self {
        Self {
            pane_id,
            pty_id,
            session_id,
            status: TabStatus::Idle,
            integration_mode,
            continuous_policy,
            cwd: initial_cwd,
            active_execution: None,
            active_is_continuous: false,
            pending_running_hysteresis: false,
            last_completion: None,
            updated_at: now_ms,
        }
    }

    /// Converte para o DTO público [`PaneStateDto`].
    #[must_use]
    pub fn to_dto(&self) -> PaneStateDto {
        PaneStateDto {
            pane_id: self.pane_id,
            pty_id: self.pty_id,
            session_id: self.session_id,
            status: self.status,
            integration_mode: self.integration_mode,
            continuous_policy: self.continuous_policy,
            cwd: self.cwd.clone(),
            active_execution: self.active_execution.clone(),
            last_completion: self.last_completion.clone(),
            updated_at: self.updated_at,
        }
    }
}

/// Evento de entrada para a função pura de transição [`transition_pane`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionInput {
    /// Um comando iniciou execução no painel.
    CommandStarted {
        /// Referência da execução iniciada.
        execution: ExecutionRef,
        /// Texto completo sanitizado do comando, se conhecido.
        command_text: Option<String>,
        /// Diretório de trabalho sanitizado no início do comando.
        cwd: Option<String>,
        /// Se `true`, aguarda o timer de histerese (80 ms) antes de mudar visualmente de `Idle` para `Running`.
        apply_hysteresis: bool,
        /// Timestamp UNIX em ms do evento.
        at_ms: i64,
    },
    /// O timer de histerese de 80 ms disparou enquanto o comando ainda estava ativo.
    RunningHysteresisElapsed {
        /// Identificador do bloco para validação de idempotência.
        block_id: CommandBlockId,
        /// Timestamp UNIX em ms.
        at_ms: i64,
    },
    /// O timer de promoção para `Continuous` (15 s) disparou enquanto o comando ainda estava ativo.
    ContinuousTimerElapsed {
        /// Identificador do bloco em execução.
        block_id: CommandBlockId,
        /// Tempo decorrido em ms desde `started_at`.
        elapsed_ms: i64,
        /// Timestamp UNIX em ms.
        at_ms: i64,
    },
    /// O comando em execução terminou.
    CommandFinished {
        /// Identificador do bloco finalizado.
        block_id: CommandBlockId,
        /// Status final do bloco.
        block_status: BlockStatus,
        /// Razão do encerramento.
        end_reason: BlockEndReason,
        /// Código de saída numérico, quando disponível.
        exit_code: Option<i32>,
        /// Sinal POSIX, quando aplicável.
        signal: Option<String>,
        /// Duração em ms.
        duration_ms: i64,
        /// Indica se a aba/painel está atualmente focado e visível pelo usuário no momento da conclusão.
        is_effectively_focused: bool,
        /// Timestamp UNIX em ms do término.
        at_ms: i64,
    },
    /// O usuário visualizou o painel (foco na aba ou chamada explícita a `tab_mark_seen`).
    MarkSeen {
        /// Timestamp UNIX em ms.
        at_ms: i64,
    },
    /// Alteração em tempo real da política `ContinuousPolicy` do painel.
    SetContinuousPolicy {
        /// Nova política.
        policy: ContinuousPolicy,
        /// Timestamp UNIX em ms.
        at_ms: i64,
    },
    /// Atualização de diretório de trabalho (`cwd`) do painel.
    CwdChanged {
        /// Novo caminho sanitizado.
        cwd: String,
        /// Timestamp UNIX em ms.
        at_ms: i64,
    },
    /// Atualização do modo de integração (`Osc133`, `FallbackProc`, `Disabled`).
    IntegrationModeChanged {
        /// Novo modo.
        mode: IntegrationMode,
        /// Timestamp UNIX em ms.
        at_ms: i64,
    },
    /// O processo do PTY encerrou.
    PtyExited {
        /// Código de saída do processo líder do PTY, se disponível.
        exit_code: Option<i32>,
        /// Sinal que encerrou o PTY, se disponível.
        signal: Option<String>,
        /// Indica se a aba está efetivamente focada no momento do encerramento.
        is_effectively_focused: bool,
        /// Timestamp UNIX em ms.
        at_ms: i64,
    },
}

/// Resultado da avaliação pura de [`transition_pane`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionOutput {
    /// Novo estado do painel.
    pub state: PaneInternalState,
    /// Indica se houve mudança semântica relevante que deve incrementar a `revision` da aba e emitir `tab://state-changed`.
    pub state_changed: bool,
}

/// Aplica de forma pura e determinística uma transição sobre [`PaneInternalState`].
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn transition_pane(current: &PaneInternalState, input: TransitionInput) -> TransitionOutput {
    let mut next = current.clone();
    let mut changed = false;

    match input {
        TransitionInput::CommandStarted {
            execution,
            command_text,
            cwd,
            apply_hysteresis,
            at_ms,
        } => {
            if next
                .active_execution
                .as_ref()
                .is_some_and(|active| active.block_id == execution.block_id)
            {
                return TransitionOutput {
                    state: next,
                    state_changed: false,
                };
            }

            if let Some(new_cwd) = cwd
                && next.cwd.as_ref() != Some(&new_cwd)
            {
                next.cwd = Some(new_cwd);
            }

            let cmd_ref = command_text
                .as_deref()
                .or(execution.command_preview.as_deref());
            let immediate_continuous =
                is_continuous_by_policy_or_command(next.continuous_policy, cmd_ref);

            next.active_execution = Some(execution);
            next.active_is_continuous = immediate_continuous;

            if immediate_continuous {
                next.pending_running_hysteresis = false;
                if next.status != TabStatus::Continuous {
                    next.status = TabStatus::Continuous;
                }
                next.updated_at = at_ms;
                changed = true;
            } else if apply_hysteresis {
                next.pending_running_hysteresis = true;
                next.updated_at = at_ms;
                // Aguarda os 80 ms antes de alterar `status` para `Running`, mas registra a execução ativa
                changed = true;
            } else {
                next.pending_running_hysteresis = false;
                next.status = TabStatus::Running;
                next.updated_at = at_ms;
                changed = true;
            }
        }

        TransitionInput::RunningHysteresisElapsed { block_id, at_ms } => {
            if next
                .active_execution
                .as_ref()
                .is_some_and(|ex| ex.block_id == block_id)
                && next.pending_running_hysteresis
            {
                next.pending_running_hysteresis = false;
                let target = if next.active_is_continuous {
                    TabStatus::Continuous
                } else {
                    TabStatus::Running
                };
                if next.status != target {
                    next.status = target;
                    next.updated_at = at_ms;
                    changed = true;
                }
            }
        }

        TransitionInput::ContinuousTimerElapsed {
            block_id,
            elapsed_ms,
            at_ms,
        } => {
            if let Some(active) = &next.active_execution
                && active.block_id == block_id
                && !next.active_is_continuous
                && should_promote_to_continuous(
                    next.continuous_policy,
                    active.command_preview.as_deref(),
                    elapsed_ms,
                )
            {
                next.active_is_continuous = true;
                next.pending_running_hysteresis = false;
                if next.status != TabStatus::Continuous {
                    next.status = TabStatus::Continuous;
                    next.updated_at = at_ms;
                    changed = true;
                }
            }
        }

        TransitionInput::CommandFinished {
            block_id,
            block_status,
            end_reason,
            exit_code,
            signal,
            duration_ms,
            is_effectively_focused,
            at_ms,
        } => {
            // Se já não é a execução ativa (e não está vazia), ignora conclusão atrasada de bloco antigo
            if let Some(active) = &next.active_execution
                && active.block_id != block_id
            {
                return TransitionOutput {
                    state: next,
                    state_changed: false,
                };
            }

            next.active_execution = None;
            next.active_is_continuous = false;
            next.pending_running_hysteresis = false;

            let outcome = classify_completion_outcome(block_status, end_reason, exit_code);
            let seen = match outcome {
                CompletionOutcome::Cancelled => true,
                CompletionOutcome::Success | CompletionOutcome::Error => is_effectively_focused,
            };

            let new_status = match outcome {
                CompletionOutcome::Cancelled => TabStatus::Idle,
                CompletionOutcome::Success => {
                    if is_effectively_focused {
                        TabStatus::Idle
                    } else {
                        TabStatus::CompletedUnseen
                    }
                }
                CompletionOutcome::Error => {
                    if is_effectively_focused {
                        TabStatus::Idle
                    } else {
                        TabStatus::ErrorUnseen
                    }
                }
            };

            next.last_completion = Some(CompletionSnapshot {
                block_id,
                pty_id: next.pty_id,
                pane_id: next.pane_id,
                outcome,
                exit_code,
                signal,
                end_reason,
                ended_at: at_ms,
                duration_ms,
                seen,
            });
            next.status = new_status;
            next.updated_at = at_ms;
            changed = true;
        }

        TransitionInput::MarkSeen { at_ms } => {
            if let Some(comp) = &mut next.last_completion
                && !comp.seen
            {
                comp.seen = true;
                changed = true;
            }
            if matches!(
                next.status,
                TabStatus::CompletedUnseen | TabStatus::ErrorUnseen
            ) {
                next.status = TabStatus::Idle;
                next.updated_at = at_ms;
                changed = true;
            }
        }

        TransitionInput::SetContinuousPolicy { policy, at_ms } => {
            if next.continuous_policy != policy {
                next.continuous_policy = policy;
                changed = true;
                if let Some(active) = &next.active_execution {
                    let elapsed = at_ms.saturating_sub(active.started_at);
                    let should_be_cont = should_promote_to_continuous(
                        policy,
                        active.command_preview.as_deref(),
                        elapsed,
                    );
                    next.active_is_continuous = should_be_cont;
                    let target = if should_be_cont {
                        TabStatus::Continuous
                    } else {
                        TabStatus::Running
                    };
                    if next.status != target {
                        next.status = target;
                    }
                }
                next.updated_at = at_ms;
            }
        }

        TransitionInput::CwdChanged { cwd, at_ms } => {
            if next.cwd.as_ref() != Some(&cwd) {
                next.cwd = Some(cwd);
                next.updated_at = at_ms;
                changed = true;
            }
        }

        TransitionInput::IntegrationModeChanged { mode, at_ms } => {
            if next.integration_mode != mode {
                next.integration_mode = mode;
                next.updated_at = at_ms;
                changed = true;
            }
        }

        TransitionInput::PtyExited {
            exit_code,
            signal,
            is_effectively_focused,
            at_ms,
        } => {
            if let Some(active) = next.active_execution.take() {
                next.active_is_continuous = false;
                next.pending_running_hysteresis = false;
                let duration_ms = at_ms.saturating_sub(active.started_at).max(0);
                let is_failure = exit_code.is_some_and(|c| c != 0) || signal.is_some();

                let (outcome, target_status, seen) = if is_failure {
                    let st = if is_effectively_focused {
                        TabStatus::Idle
                    } else {
                        TabStatus::ErrorUnseen
                    };
                    (CompletionOutcome::Error, st, is_effectively_focused)
                } else {
                    (CompletionOutcome::Cancelled, TabStatus::Idle, true)
                };

                next.last_completion = Some(CompletionSnapshot {
                    block_id: active.block_id,
                    pty_id: next.pty_id,
                    pane_id: next.pane_id,
                    outcome,
                    exit_code,
                    signal,
                    end_reason: BlockEndReason::PtyExited,
                    ended_at: at_ms,
                    duration_ms,
                    seen,
                });
                next.status = target_status;
                next.updated_at = at_ms;
                changed = true;
            } else if matches!(next.status, TabStatus::Running | TabStatus::Continuous) {
                next.status = TabStatus::Idle;
                next.updated_at = at_ms;
                changed = true;
            }
        }
    }

    TransitionOutput {
        state: next,
        state_changed: changed,
    }
}

fn classify_completion_outcome(
    block_status: BlockStatus,
    end_reason: BlockEndReason,
    exit_code: Option<i32>,
) -> CompletionOutcome {
    if block_status == BlockStatus::Cancelled
        || end_reason == BlockEndReason::CtrlC
        || exit_code == Some(130)
    {
        return CompletionOutcome::Cancelled;
    }

    if matches!(
        end_reason,
        BlockEndReason::ImplicitNextPrompt
            | BlockEndReason::ImplicitNextCommand
            | BlockEndReason::AppShutdown
            | BlockEndReason::RecoveredFromCrash
    ) && exit_code.is_none()
    {
        // Conclusão implícita sem código de saída conhecido não fabrica sucesso nem erro
        return CompletionOutcome::Cancelled;
    }

    match block_status {
        BlockStatus::Succeeded => CompletionOutcome::Success,
        BlockStatus::Failed | BlockStatus::TerminatedBySignal => CompletionOutcome::Error,
        BlockStatus::PtyExited => {
            if exit_code.is_some_and(|c| c != 0) {
                CompletionOutcome::Error
            } else {
                CompletionOutcome::Cancelled
            }
        }
        BlockStatus::Cancelled | BlockStatus::InterruptedByRestart | BlockStatus::Running => {
            CompletionOutcome::Cancelled
        }
    }
}

/// Consolida a fotografia agregada de uma aba (`TabStateSnapshot`) a partir dos seus painéis e alertas de atenção.
///
/// Ordem estrita de precedência:
/// `ErrorUnseen > Running > Continuous > CompletedUnseen > Idle`.
#[must_use]
pub fn aggregate_tab_snapshot(
    tab_id: TabId,
    revision: u64,
    focused: bool,
    panes: &[PaneInternalState],
    attention: Option<&AttentionSnapshot>,
    now_ms: i64,
) -> TabStateSnapshot {
    let mut best_status = TabStatus::Idle;
    for pane in panes {
        if pane.status.priority() > best_status.priority() {
            best_status = pane.status;
        }
    }

    // Alerta de atenção não visto de Agente CLI eleva a aba para `ErrorUnseen`
    if attention.is_some_and(|att| !att.seen) && !focused {
        best_status = TabStatus::ErrorUnseen;
    }

    let pane_dtos: Vec<PaneStateDto> = panes.iter().map(PaneInternalState::to_dto).collect();

    let last_completion = panes
        .iter()
        .filter_map(|p| p.last_completion.as_ref())
        .max_by_key(|c| (c.ended_at, !c.seen))
        .cloned();

    let updated_at = panes.iter().map(|p| p.updated_at).max().unwrap_or(now_ms);

    TabStateSnapshot {
        tab_id,
        revision,
        status: best_status,
        focused,
        panes: pane_dtos,
        last_completion,
        attention: attention.cloned(),
        updated_at,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use mt_core::shell::ObservationSource;

    use super::*;

    fn sample_pane() -> PaneInternalState {
        PaneInternalState::new(
            Some(PaneId::new()),
            PtyId::new(),
            SessionId::new(),
            IntegrationMode::Osc133,
            ContinuousPolicy::Auto,
            Some("/tmp".to_string()),
            1000,
        )
    }

    fn sample_exec(preview: &str) -> ExecutionRef {
        ExecutionRef {
            block_id: CommandBlockId::new(),
            command_seq: 1,
            started_at: 1000,
            command_preview: Some(preview.to_string()),
            source: ObservationSource::Osc133,
        }
    }

    #[test]
    fn transitions_through_running_continuous_unseen_and_seen() {
        let pane = sample_pane();
        let exec = sample_exec("sleep 2");

        // 1. Inicia comando normal -> Running
        let out1 = transition_pane(
            &pane,
            TransitionInput::CommandStarted {
                execution: exec.clone(),
                command_text: Some("sleep 2".to_string()),
                cwd: None,
                apply_hysteresis: false,
                at_ms: 1000,
            },
        );
        assert!(out1.state_changed);
        assert_eq!(out1.state.status, TabStatus::Running);

        // 2. Passam 15 s -> promove para Continuous
        let out2 = transition_pane(
            &out1.state,
            TransitionInput::ContinuousTimerElapsed {
                block_id: exec.block_id,
                elapsed_ms: 15_000,
                at_ms: 16_000,
            },
        );
        assert!(out2.state_changed);
        assert_eq!(out2.state.status, TabStatus::Continuous);

        // 3. Termina com exit 0 em aba não focada -> CompletedUnseen
        let out3 = transition_pane(
            &out2.state,
            TransitionInput::CommandFinished {
                block_id: exec.block_id,
                block_status: BlockStatus::Succeeded,
                end_reason: BlockEndReason::ExitZero,
                exit_code: Some(0),
                signal: None,
                duration_ms: 15_100,
                is_effectively_focused: false,
                at_ms: 16_100,
            },
        );
        assert_eq!(out3.state.status, TabStatus::CompletedUnseen);

        // 4. Usuário visualiza a aba -> Idle
        let out4 = transition_pane(&out3.state, TransitionInput::MarkSeen { at_ms: 17_000 });
        assert_eq!(out4.state.status, TabStatus::Idle);
        assert!(
            out4.state
                .last_completion
                .as_ref()
                .expect("deve ter conclusão")
                .seen
        );
    }

    #[test]
    fn ctrl_c_130_returns_to_idle_without_error_badge() {
        let pane = sample_pane();
        let exec = sample_exec("sleep 100");
        let running = transition_pane(
            &pane,
            TransitionInput::CommandStarted {
                execution: exec.clone(),
                command_text: Some("sleep 100".to_string()),
                cwd: None,
                apply_hysteresis: false,
                at_ms: 1000,
            },
        )
        .state;

        let finished = transition_pane(
            &running,
            TransitionInput::CommandFinished {
                block_id: exec.block_id,
                block_status: BlockStatus::Cancelled,
                end_reason: BlockEndReason::CtrlC,
                exit_code: Some(130),
                signal: Some("SIGINT".to_string()),
                duration_ms: 500,
                is_effectively_focused: false,
                at_ms: 1500,
            },
        )
        .state;

        assert_eq!(finished.status, TabStatus::Idle);
        assert_eq!(
            finished
                .last_completion
                .as_ref()
                .expect("deve ter conclusão")
                .outcome,
            CompletionOutcome::Cancelled
        );
    }
}
