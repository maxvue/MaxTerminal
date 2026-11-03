//! Testes de composição determinística de comandos sequenciais por dialeto de shell (`CA-05`).

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
use std::path::Path;

use mt_commands::groups::compose::SequentialComposer;
use mt_commands::groups::model::{GroupErrorCode, ShellDialect};

#[test]
fn preserves_raw_command_when_no_cwd_or_env_overrides() {
    let empty_env = BTreeMap::new();
    let bytes = SequentialComposer::compose_submission(
        ShellDialect::Zsh,
        Path::new("/usr/bin/zsh"),
        "npm test | tee report.log # comentario",
        None,
        &empty_env,
        true,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        "npm test | tee report.log # comentario\n"
    );
}

#[test]
fn isolates_cwd_and_env_overrides_using_env_minus_c_and_single_quote_escaping() {
    let mut env = BTreeMap::new();
    env.insert("APP_ENV".to_string(), "local".to_string());
    env.insert("QUOTED_MSG".to_string(), "it's ok".to_string());

    let bytes = SequentialComposer::compose_submission(
        ShellDialect::Bash,
        Path::new("/bin/bash"),
        "echo 'olá mundo'",
        Some(Path::new("/tmp/meu dir'teste")),
        &env,
        true,
    )
    .unwrap();
    let line = String::from_utf8(bytes).unwrap();

    assert!(line.starts_with("env -C '/tmp/meu dir'\\''teste'"));
    assert!(line.contains("'APP_ENV=local'"));
    assert!(line.contains("'QUOTED_MSG=it'\\''s ok'"));
    assert!(line.contains("'/bin/bash' -c 'echo '\\''olá mundo'\\'''\n"));
}

#[test]
fn quotes_fish_dialect_correctly() {
    let mut env = BTreeMap::new();
    env.insert("FISH_VAR".to_string(), "a\\b'c".to_string());

    let bytes = SequentialComposer::compose_submission(
        ShellDialect::Fish,
        Path::new("/usr/bin/fish"),
        "echo 'fish'",
        Some(Path::new("/tmp/proj")),
        &env,
        true,
    )
    .unwrap();
    let line = String::from_utf8(bytes).unwrap();
    assert!(line.contains(
        "env -C '/tmp/proj' 'FISH_VAR=a\\\\b\\'c' '/usr/bin/fish' -c 'echo \\'fish\\''\n"
    ));
}

#[test]
fn fails_when_env_chdir_unsupported_and_cwd_override_present() {
    let empty_env = BTreeMap::new();
    let err = SequentialComposer::compose_submission(
        ShellDialect::Bash,
        Path::new("/bin/bash"),
        "pwd",
        Some(Path::new("/tmp")),
        &empty_env,
        false,
    )
    .unwrap_err();
    assert_eq!(err.code.0, GroupErrorCode::EnvCwdUnavailable.as_str());
}
