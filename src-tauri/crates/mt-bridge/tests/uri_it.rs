//! Testes de integração de URIs (`maxterminal://...` e `vscode://johnattas.maxcode/...`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::os::unix::fs::PermissionsExt;

use mt_bridge::{
    AgentKind, BridgeErrorCode, InspectCommandErrorPayload, InspectErrorCode,
    InspectFallbackArtifact, IssueCommandKind, IssueCommandPayload, IssueStage, OpenFilePayload,
    ParsedMaxTerminalUri, ParsedVsCodeMaxCodeUri, build_vscode_inspect_uri,
    build_vscode_issue_action_uri, build_vscode_open_file_uri, current_unix_ms,
    format_plan_mode_inspect_prompt, parse_maxterminal_uri, parse_vscode_maxcode_uri,
};
use tempfile::TempDir;
use uuid::Uuid;

#[test]
fn roundtrip_maxterminal_open_project_and_open_terminal_uris() {
    let tmp = TempDir::new().unwrap();
    let proj_dir = tmp.path().join("meu-projeto");
    let wt_dir = tmp.path().join("wt-42");
    fs::create_dir_all(&proj_dir).unwrap();
    fs::create_dir_all(&wt_dir).unwrap();

    let now = current_unix_ms();
    let req = Uuid::new_v4();
    let uri = format!(
        "maxterminal://open-project?v=1&req={req}&ts={now}&path={}&name=MeuProjeto&origin=maxcode",
        url::form_urlencoded::byte_serialize(proj_dir.to_string_lossy().as_bytes())
            .collect::<String>()
    );

    let parsed = parse_maxterminal_uri(&uri, now).expect("parse open-project");
    match parsed {
        ParsedMaxTerminalUri::OpenProject {
            request_id,
            payload,
        } => {
            assert_eq!(request_id, req);
            assert_eq!(
                payload.project_path,
                fs::canonicalize(&proj_dir).unwrap().to_string_lossy()
            );
            assert_eq!(payload.project_name.as_deref(), Some("MeuProjeto"));
            assert_eq!(payload.origin_id, "maxcode");
        }
        _ => panic!("esperado OpenProject"),
    }

    let req2 = Uuid::new_v4();
    let term_uri = format!(
        "maxterminal://open-terminal?v=1&req={req2}&ts={now}&project={}&worktree={}&branch=fix/42&issue=42&agent=claude",
        url::form_urlencoded::byte_serialize(proj_dir.to_string_lossy().as_bytes())
            .collect::<String>(),
        url::form_urlencoded::byte_serialize(wt_dir.to_string_lossy().as_bytes())
            .collect::<String>()
    );
    let parsed_term = parse_maxterminal_uri(&term_uri, now).expect("parse open-terminal");
    match parsed_term {
        ParsedMaxTerminalUri::OpenTerminalForIssue {
            request_id,
            payload,
        } => {
            assert_eq!(request_id, req2);
            assert_eq!(payload.issue_number, 42);
            assert_eq!(payload.agent, Some(AgentKind::Claude));
            assert_eq!(payload.branch.as_deref(), Some("fix/42"));
            assert!(payload.initial_command.is_none());
        }
        _ => panic!("esperado OpenTerminalForIssue"),
    }
}

#[test]
fn rejects_expired_or_duplicate_param_or_relative_path_uris() {
    let tmp = TempDir::new().unwrap();
    let now = current_unix_ms();
    let req = Uuid::new_v4();

    // URI expirada (> 60s)
    let expired = format!("maxterminal://focus?v=1&req={req}&ts={}", now - 120_000);
    let err = parse_maxterminal_uri(&expired, now).unwrap_err();
    assert_eq!(err.code, BridgeErrorCode::InvalidUri);

    // Parâmetro duplicado
    let dup = format!("maxterminal://focus?v=1&v=1&req={req}&ts={now}");
    let err_dup = parse_maxterminal_uri(&dup, now).unwrap_err();
    assert_eq!(err_dup.code, BridgeErrorCode::InvalidUri);

    // Caminho relativo
    let rel = format!("maxterminal://open-project?v=1&req={req}&ts={now}&path=relativo/pasta");
    let err_rel = parse_maxterminal_uri(&rel, now).unwrap_err();
    assert_eq!(err_rel.code, BridgeErrorCode::InvalidTargetPath);

    // Caminho com '..'
    let trav = format!(
        "maxterminal://open-project?v=1&req={req}&ts={now}&path={}/../etc",
        tmp.path().display()
    );
    let err_trav = parse_maxterminal_uri(&trav, now).unwrap_err();
    assert_eq!(err_trav.code, BridgeErrorCode::InvalidTargetPath);
}

#[test]
fn vscode_inspect_uri_inline_and_large_artifact_fallback() {
    let tmp = TempDir::new().unwrap();
    let artifact_dir = tmp.path().join("max-bridge");
    let req = Uuid::new_v4();

    let small_prompt = format_plan_mode_inspect_prompt("cargo test", "falhou assertion");
    let small_payload = InspectCommandErrorPayload {
        block_id: "blk-1".to_string(),
        session_id: "sess-1".to_string(),
        tab_id: "tab-1".to_string(),
        project_path: "/home/dev/proj".to_string(),
        cwd: "/home/dev/proj".to_string(),
        command: "cargo test".to_string(),
        error_output: "falhou assertion".to_string(),
        exit_status: InspectErrorCode::ExitCode { code: 101 },
        duration_ms: Some(250),
        mode: "plan".to_string(),
        formatted_prompt: small_prompt.clone(),
    };

    let inline_uri =
        build_vscode_inspect_uri(req, &small_payload, &artifact_dir).expect("inline uri");
    let parsed = parse_vscode_maxcode_uri(&inline_uri, current_unix_ms()).expect("parse inline");
    match parsed {
        ParsedVsCodeMaxCodeUri::Inspect {
            prompt,
            payload_file,
            mode,
            exit_code,
            ..
        } => {
            assert_eq!(mode, "plan");
            assert_eq!(exit_code, 101);
            assert_eq!(prompt.as_deref(), Some(small_prompt.as_str()));
            assert!(payload_file.is_none());
        }
        _ => panic!("esperado Inspect"),
    }

    // Prompt grande (> 6 KiB) deve usar payload_file com permissão 0600
    let big_err = "E".repeat(9000);
    let big_prompt = format_plan_mode_inspect_prompt("npm run build", &big_err);
    let big_payload = InspectCommandErrorPayload {
        formatted_prompt: big_prompt.clone(),
        error_output: big_err,
        ..small_payload
    };

    let file_uri =
        build_vscode_inspect_uri(req, &big_payload, &artifact_dir).expect("artifact uri");
    let parsed_big =
        parse_vscode_maxcode_uri(&file_uri, current_unix_ms()).expect("parse artifact uri");
    match parsed_big {
        ParsedVsCodeMaxCodeUri::Inspect {
            prompt,
            payload_file: Some(path_str),
            ..
        } => {
            assert!(prompt.is_none());
            let meta = fs::metadata(&path_str).expect("artifact exists");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
            let raw = fs::read_to_string(&path_str).unwrap();
            let art: InspectFallbackArtifact = serde_json::from_str(&raw).unwrap();
            assert_eq!(art.payload.formatted_prompt, big_prompt);
        }
        _ => panic!("esperado Inspect com payload_file"),
    }
}

#[test]
fn vscode_open_file_and_issue_action_uris_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let file_path = tmp.path().join("User.php");
    fs::write(&file_path, "<?php\n").unwrap();

    let req = Uuid::new_v4();
    let open_uri = build_vscode_open_file_uri(
        req,
        &OpenFilePayload {
            file_path: file_path.to_string_lossy().into_owned(),
            line: Some(42),
            column: Some(5),
            project_path: Some(tmp.path().to_string_lossy().into_owned()),
            focus_editor: true,
        },
    )
    .unwrap();

    let parsed = parse_vscode_maxcode_uri(&open_uri, current_unix_ms()).unwrap();
    match parsed {
        ParsedVsCodeMaxCodeUri::OpenFile { payload, .. } => {
            assert_eq!(payload.line, Some(42));
            assert_eq!(payload.column, Some(5));
        }
        _ => panic!("esperado OpenFile"),
    }

    let issue_uri = build_vscode_issue_action_uri(
        req,
        &IssueCommandPayload {
            kind: IssueCommandKind::Fix,
            project_path: "/home/dev/maxterminal".to_string(),
            repo_full_name: "johnattas/MaxTerminal".to_string(),
            issue_number: 99,
            issue_title: "Corrigir crash".to_string(),
            issue_url: None,
            labels: vec!["Bug".to_string()],
            stage: IssueStage::BugFix,
            focus_editor: true,
        },
    )
    .unwrap();

    let parsed_issue = parse_vscode_maxcode_uri(&issue_uri, current_unix_ms()).unwrap();
    match parsed_issue {
        ParsedVsCodeMaxCodeUri::IssueAction {
            action,
            issue_number,
            title,
            ..
        } => {
            assert_eq!(action, "fix");
            assert_eq!(issue_number, 99);
            assert_eq!(title.as_deref(), Some("Corrigir crash"));
        }
        _ => panic!("esperado IssueAction"),
    }
}
