//! Testes de validação determinística e bloqueio de segredos de Grupos de Comandos (`CA-04`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::collections::BTreeMap;

use mt_commands::groups::model::{
    CommandGroupInput, CommandGroupItemInput, ExecutionMode, GroupErrorCode, GroupScope,
    SequentialSeparator,
};
use mt_commands::groups::validate::{
    MAX_COMMAND_BYTES, MAX_GROUP_ITEMS, MAX_GROUP_NAME_CHARS, normalize_key, validate_group_input,
};
use mt_core::ids::ProjectId;
use tempfile::TempDir;

fn base_item(cmd: &str) -> CommandGroupItemInput {
    CommandGroupItemInput {
        id: None,
        title: "Item".to_string(),
        command: cmd.to_string(),
        cwd: None,
        env: BTreeMap::new(),
        continuous: false,
    }
}

fn base_input(
    scope: GroupScope,
    project_id: Option<ProjectId>,
    items: Vec<CommandGroupItemInput>,
) -> CommandGroupInput {
    CommandGroupInput {
        scope,
        project_id,
        name: "Subir Ambiente".to_string(),
        description: "Grupo de teste".to_string(),
        labels: vec!["dev".to_string(), "local".to_string()],
        mode: ExecutionMode::Sequential,
        separator: SequentialSeparator::And,
        cwd: None,
        env: BTreeMap::new(),
        items,
    }
}

#[test]
fn normalize_key_folds_case_and_nfc() {
    let decomposed = "  A\u{0301}rvore   De   Comandos ";
    let precomposed = "árvore   de   comandos";
    assert_eq!(normalize_key(decomposed), normalize_key(precomposed));
}

#[test]
fn rejects_zero_or_more_than_max_items() {
    let empty = base_input(GroupScope::Global, None, vec![]);
    let err = validate_group_input(&empty, None).unwrap_err();
    assert_eq!(err.code.0, GroupErrorCode::Validation.as_str());

    let too_many: Vec<_> = (0..=MAX_GROUP_ITEMS)
        .map(|i| base_item(&format!("echo {i}")))
        .collect();
    let full = base_input(GroupScope::Global, None, too_many);
    let err = validate_group_input(&full, None).unwrap_err();
    assert_eq!(err.code.0, GroupErrorCode::Validation.as_str());

    let max_ok: Vec<_> = (0..MAX_GROUP_ITEMS)
        .map(|i| base_item(&format!("echo {i}")))
        .collect();
    let ok_input = base_input(GroupScope::Global, None, max_ok);
    assert!(validate_group_input(&ok_input, None).is_ok());
}

#[test]
fn validates_name_and_command_constraints() {
    let mut bad_name = base_input(GroupScope::Global, None, vec![base_item("echo ok")]);
    bad_name.name = "   ".to_string();
    assert_eq!(
        validate_group_input(&bad_name, None).unwrap_err().code.0,
        GroupErrorCode::Validation.as_str()
    );

    bad_name.name = "x".repeat(MAX_GROUP_NAME_CHARS + 1);
    assert_eq!(
        validate_group_input(&bad_name, None).unwrap_err().code.0,
        GroupErrorCode::Validation.as_str()
    );

    for bad_cmd in [
        String::new(),
        "   ".to_string(),
        "echo a\necho b".to_string(),
        "echo a\recho b".to_string(),
        "echo a\0b".to_string(),
        "a".repeat(MAX_COMMAND_BYTES + 1),
    ] {
        let input = base_input(GroupScope::Global, None, vec![base_item(&bad_cmd)]);
        let err = validate_group_input(&input, None).unwrap_err();
        assert_eq!(err.code.0, GroupErrorCode::Validation.as_str());
    }
}

#[test]
fn validates_posix_env_keys_and_blocks_maxterminal_prefix() {
    for forbidden_key in ["123INVALID", "BAD-KEY", "MAXTERMINAL_TOKEN"] {
        let mut item = base_item("echo ok");
        item.env.insert(forbidden_key.to_string(), "1".to_string());
        let input = base_input(GroupScope::Global, None, vec![item]);
        let err = validate_group_input(&input, None).unwrap_err();
        assert_eq!(err.code.0, GroupErrorCode::Validation.as_str());
    }

    let mut valid_item = base_item("echo ok");
    valid_item
        .env
        .insert("NODE_ENV".to_string(), "development".to_string());
    valid_item
        .env
        .insert("_PRIVATE_FLAG_1".to_string(), "true".to_string());
    let ok_input = base_input(GroupScope::Global, None, vec![valid_item]);
    assert!(validate_group_input(&ok_input, None).is_ok());
}

#[test]
fn validates_cwd_confinement_and_prevents_traversal() {
    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("proj");
    let sub_dir = project_root.join("packages/api");
    let outside_dir = tmp.path().join("outside");
    std::fs::create_dir_all(&sub_dir).unwrap();
    std::fs::create_dir_all(&outside_dir).unwrap();

    let pid = ProjectId::new();

    let mut item_ok = base_item("npm test");
    item_ok.cwd = Some("packages/api".to_string());
    let input_ok = base_input(GroupScope::Local, Some(pid), vec![item_ok]);
    assert!(validate_group_input(&input_ok, Some(&project_root)).is_ok());

    let mut item_traversal = base_item("npm test");
    item_traversal.cwd = Some("../outside".to_string());
    let input_traversal = base_input(GroupScope::Local, Some(pid), vec![item_traversal]);
    assert_eq!(
        validate_group_input(&input_traversal, Some(&project_root))
            .unwrap_err()
            .code
            .0,
        GroupErrorCode::CwdOutsideProject.as_str()
    );

    let mut item_abs_out = base_item("npm test");
    item_abs_out.cwd = Some(outside_dir.to_string_lossy().into_owned());
    let input_abs_out = base_input(GroupScope::Local, Some(pid), vec![item_abs_out]);
    assert_eq!(
        validate_group_input(&input_abs_out, Some(&project_root))
            .unwrap_err()
            .code
            .0,
        GroupErrorCode::CwdOutsideProject.as_str()
    );
}

#[test]
fn blocks_secrets_in_command_and_env_with_field_details() {
    let mut item = base_item(
        "curl -H 'Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U' https://api.example.com",
    );
    item.env.insert(
        "APP_KEY".to_string(),
        "base64:dGhpcyBpcyBhIHNlY3JldCBrZXkgZm9yIHRlc3Rpbmc=".to_string(),
    );

    let input = base_input(GroupScope::Global, None, vec![item]);
    let err = validate_group_input(&input, None).unwrap_err();
    assert_eq!(err.code.0, GroupErrorCode::SecretDetected.as_str());

    let fields = err
        .details
        .as_ref()
        .and_then(|d| d.get("fields"))
        .and_then(|f| f.as_array())
        .expect("deve informar details.fields");
    let field_names: Vec<&str> = fields.iter().filter_map(|v| v.as_str()).collect();
    assert!(field_names.contains(&"items[0].command"));
    assert!(field_names.contains(&"items[0].env.APP_KEY"));
}
