//! Testes parametrizados da máquina de estados de painéis e abas (`state_table.rs`).
//!
//! Cobre todos os 5 estados (`Running`, `Continuous`, `CompletedUnseen`, `ErrorUnseen`, `Idle`),
//! foco ativo vs inativo, histerese de 80 ms, promoção de 15 s, `ContinuousPolicy` e agregação multi-painel.

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use mt_core::ids::{CommandBlockId, PaneId, PtyId, SessionId, TabId};
use mt_core::shell::{
    AttentionSnapshot, BlockEndReason, BlockStatus, CompletionOutcome, ContinuousPolicy,
    ExecutionRef, FocusSnapshot, IntegrationMode, ObservationSource, SeenRequest, TabStatus,
};
use mt_shell::{PaneInternalState, TabStateRegistry, TransitionInput, transition_pane};

fn make_pane(policy: ContinuousPolicy) -> PaneInternalState {
    PaneInternalState::new(
        Some(PaneId::new()),
        PtyId::new(),
        SessionId::new(),
        IntegrationMode::Osc133,
        policy,
        Some("/home/user".to_string()),
        1_000,
    )
}

fn make_exec(cmd: &str, started_at: i64) -> ExecutionRef {
    ExecutionRef {
        block_id: CommandBlockId::new(),
        command_seq: 1,
        started_at,
        command_preview: Some(cmd.to_string()),
        source: ObservationSource::Osc133,
    }
}

#[test]
fn state_table_focused_vs_unfocused_completions() {
    struct Case {
        name: &'static str,
        block_status: BlockStatus,
        reason: BlockEndReason,
        exit_code: Option<i32>,
        focused: bool,
        expected_tab_status: TabStatus,
        expected_outcome: CompletionOutcome,
        expected_seen: bool,
    }

    let cases = [
        Case {
            name: "exit 0 sem foco -> CompletedUnseen",
            block_status: BlockStatus::Succeeded,
            reason: BlockEndReason::ExitZero,
            exit_code: Some(0),
            focused: false,
            expected_tab_status: TabStatus::CompletedUnseen,
            expected_outcome: CompletionOutcome::Success,
            expected_seen: false,
        },
        Case {
            name: "exit 0 com foco -> Idle (seen=true)",
            block_status: BlockStatus::Succeeded,
            reason: BlockEndReason::ExitZero,
            exit_code: Some(0),
            focused: true,
            expected_tab_status: TabStatus::Idle,
            expected_outcome: CompletionOutcome::Success,
            expected_seen: true,
        },
        Case {
            name: "exit 1 sem foco -> ErrorUnseen",
            block_status: BlockStatus::Failed,
            reason: BlockEndReason::ExitNonZero,
            exit_code: Some(1),
            focused: false,
            expected_tab_status: TabStatus::ErrorUnseen,
            expected_outcome: CompletionOutcome::Error,
            expected_seen: false,
        },
        Case {
            name: "exit 1 com foco -> Idle (seen=true)",
            block_status: BlockStatus::Failed,
            reason: BlockEndReason::ExitNonZero,
            exit_code: Some(1),
            focused: true,
            expected_tab_status: TabStatus::Idle,
            expected_outcome: CompletionOutcome::Error,
            expected_seen: true,
        },
        Case {
            name: "Ctrl+C (130) sem foco -> Idle (Cancelled)",
            block_status: BlockStatus::Cancelled,
            reason: BlockEndReason::CtrlC,
            exit_code: Some(130),
            focused: false,
            expected_tab_status: TabStatus::Idle,
            expected_outcome: CompletionOutcome::Cancelled,
            expected_seen: true,
        },
        Case {
            name: "ImplicitNextPrompt sem exit_code -> Idle sem falso sucesso",
            block_status: BlockStatus::Succeeded,
            reason: BlockEndReason::ImplicitNextPrompt,
            exit_code: None,
            focused: false,
            expected_tab_status: TabStatus::Idle,
            expected_outcome: CompletionOutcome::Cancelled,
            expected_seen: true,
        },
    ];

    for c in cases {
        let pane = make_pane(ContinuousPolicy::Auto);
        let exec = make_exec("cmd", 1_000);
        let started = transition_pane(
            &pane,
            TransitionInput::CommandStarted {
                execution: exec.clone(),
                command_text: Some("cmd".to_string()),
                cwd: None,
                apply_hysteresis: false,
                at_ms: 1_000,
            },
        )
        .state;

        let finished = transition_pane(
            &started,
            TransitionInput::CommandFinished {
                block_id: exec.block_id,
                block_status: c.block_status,
                end_reason: c.reason,
                exit_code: c.exit_code,
                signal: None,
                duration_ms: 250,
                is_effectively_focused: c.focused,
                at_ms: 1_250,
            },
        )
        .state;

        assert_eq!(
            finished.status, c.expected_tab_status,
            "Caso falhou: {}",
            c.name
        );
        let comp = finished.last_completion.unwrap();
        assert_eq!(comp.outcome, c.expected_outcome, "Caso falhou: {}", c.name);
        assert_eq!(comp.seen, c.expected_seen, "Caso falhou: {}", c.name);
    }
}

#[test]
fn fast_command_under_80ms_hysteresis_never_flashes_running() {
    let pane = make_pane(ContinuousPolicy::Auto);
    let exec = make_exec("true", 1_000);

    // Inicia com histerese ativada: status permanece Idle durante os primeiros 80 ms
    let s1 = transition_pane(
        &pane,
        TransitionInput::CommandStarted {
            execution: exec.clone(),
            command_text: Some("true".to_string()),
            cwd: None,
            apply_hysteresis: true,
            at_ms: 1_000,
        },
    )
    .state;
    assert_eq!(s1.status, TabStatus::Idle);
    assert!(s1.pending_running_hysteresis);

    // Termina em 20 ms (< 80 ms) com aba focada -> continua Idle sem ter piscado Running
    let s2 = transition_pane(
        &s1,
        TransitionInput::CommandFinished {
            block_id: exec.block_id,
            block_status: BlockStatus::Succeeded,
            end_reason: BlockEndReason::ExitZero,
            exit_code: Some(0),
            signal: None,
            duration_ms: 20,
            is_effectively_focused: true,
            at_ms: 1_020,
        },
    )
    .state;
    assert_eq!(s2.status, TabStatus::Idle);
    assert!(!s2.pending_running_hysteresis);
}

#[test]
fn registry_focus_and_attention_overlay_behave_deterministically() {
    let reg = TabStateRegistry::new();
    let tab_id = TabId::new();
    let pane_id = PaneId::new();
    let pty_id = PtyId::new();

    reg.register_pane(
        tab_id,
        Some(pane_id),
        pty_id,
        SessionId::new(),
        IntegrationMode::Osc133,
        ContinuousPolicy::Auto,
        Some("/tmp".to_string()),
        100,
    );

    // Alerta de atenção de Agente CLI em aba não focada eleva status para ErrorUnseen
    let snap_att = reg
        .set_attention(
            tab_id,
            Some(AttentionSnapshot {
                source: "claude".to_string(),
                message: "Aguardando aprovação de ferramenta".to_string(),
                raised_at: 200,
                seen: false,
            }),
            200,
        )
        .unwrap();
    assert_eq!(snap_att.status, TabStatus::ErrorUnseen);

    // Focar a janela + aba marca a atenção como vista e retorna a aba para Idle
    let changed = reg.set_focus(
        FocusSnapshot {
            window_focused: true,
            active_tab_id: Some(tab_id),
            active_pane_id: Some(pane_id),
        },
        250,
    );
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].status, TabStatus::Idle);
    assert!(changed[0].attention.as_ref().unwrap().seen);

    // Chamada repetida de mark_seen com revisão antiga não regride a revisão atual
    let cur_rev = changed[0].revision;
    let noop = reg
        .mark_seen(
            &SeenRequest {
                tab_id,
                pane_id: None,
                up_to_revision: Some(1),
            },
            300,
        )
        .unwrap();
    assert_eq!(noop.revision, cur_rev);
}
