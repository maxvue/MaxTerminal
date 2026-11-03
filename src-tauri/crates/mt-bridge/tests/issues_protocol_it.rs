//! Testes de conformidade do protocolo de Issues (`issues_protocol_it.rs` — Subetapa 11/06).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use mt_bridge::{
    BugCycleItemStage, BugCycleOverallStatus, BugCycleStepStatus, IssueAckStatus, IssueCommandKind,
    IssueStage, RpcEnvelope, RpcPayload,
};

fn load_fixture_value() -> serde_json::Value {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures/maxcode/issues-protocol.v1.json");
    let raw = std::fs::read_to_string(&fixture_path)
        .expect("fixtures/maxcode/issues-protocol.v1.json deve existir");
    serde_json::from_str(&raw).expect("JSON válido na fixture")
}

#[test]
fn roundtrip_issues_protocol_v1_fixture_envelopes() {
    let root = load_fixture_value();

    let cmd_env: RpcEnvelope =
        serde_json::from_value(root["issueCommand"].clone()).expect("issueCommand");
    match &cmd_env.message {
        RpcPayload::IssueCommand(p) => {
            assert_eq!(p.kind, IssueCommandKind::Plan);
            assert_eq!(p.stage, IssueStage::BugPlan);
            assert_eq!(p.issue_number, 42);
            assert_eq!(p.repo_full_name, "johnattas/MaxTerminal");
        }
        other => panic!("esperado IssueCommand, recebido {other:?}"),
    }
    assert_eq!(
        serde_json::to_value(&cmd_env).unwrap(),
        root["issueCommand"]
    );

    let ack_env: RpcEnvelope =
        serde_json::from_value(root["issueCommandAck"].clone()).expect("issueCommandAck");
    match &ack_env.message {
        RpcPayload::IssueCommandAck(ack) => {
            assert_eq!(ack.status, IssueAckStatus::Accepted);
            assert_eq!(ack.conversation_id.as_deref(), Some("conv-42"));
        }
        other => panic!("esperado IssueCommandAck, recebido {other:?}"),
    }
    assert_eq!(
        serde_json::to_value(&ack_env).unwrap(),
        root["issueCommandAck"]
    );

    let start_env: RpcEnvelope =
        serde_json::from_value(root["bugsCycleStart"].clone()).expect("bugsCycleStart");
    match &start_env.message {
        RpcPayload::BugsCycleStart(start) => {
            assert_eq!(start.issue_numbers, vec![42, 43, 44]);
            assert!(!start.dry_run);
        }
        other => panic!("esperado BugsCycleStart, recebido {other:?}"),
    }
    assert_eq!(
        serde_json::to_value(&start_env).unwrap(),
        root["bugsCycleStart"]
    );

    let cycle_ack_env: RpcEnvelope =
        serde_json::from_value(root["bugsCycleAck"].clone()).expect("bugsCycleAck");
    match &cycle_ack_env.message {
        RpcPayload::BugsCycleAck(ack) => {
            assert!(ack.accepted);
            assert_eq!(ack.queued_issues, vec![42, 43]);
            assert_eq!(ack.skipped_issues.len(), 1);
            assert_eq!(ack.skipped_issues[0].issue_number, 44);
        }
        other => panic!("esperado BugsCycleAck, recebido {other:?}"),
    }
    assert_eq!(
        serde_json::to_value(&cycle_ack_env).unwrap(),
        root["bugsCycleAck"]
    );

    let prog_env: RpcEnvelope =
        serde_json::from_value(root["bugCycleProgress"].clone()).expect("bugCycleProgress");
    match &prog_env.message {
        RpcPayload::BugCycleProgress(prog) => {
            assert_eq!(prog.overall_status, BugCycleOverallStatus::Running);
            assert_eq!(prog.stage, BugCycleItemStage::Planning);
            assert_eq!(prog.step_status, BugCycleStepStatus::InProgress);
            assert_eq!(prog.current_issue_number, Some(42));
        }
        other => panic!("esperado BugCycleProgress, recebido {other:?}"),
    }
    assert_eq!(
        serde_json::to_value(&prog_env).unwrap(),
        root["bugCycleProgress"]
    );
}
