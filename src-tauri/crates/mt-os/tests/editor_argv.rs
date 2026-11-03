//! Testes de `build_editor_invocation` para os 4 editores (`mt-os`).

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use mt_os::{
    EditorInvocation, EditorKind, EditorSettings, ResolvedLink, build_editor_invocation, codes,
};

#[test]
fn builds_invocations_for_vscode_insiders_maxcode_and_custom() {
    let link = ResolvedLink {
        key: String::from("k1"),
        absolute_path: String::from("/home/user/meu projeto/ação #1%20.ts"),
        line: Some(42),
        column: Some(7),
    };

    // 1. VsCode
    let vs = build_editor_invocation(
        &EditorSettings {
            kind: EditorKind::VsCode,
            custom_argv: Vec::new(),
        },
        &link,
    )
    .unwrap();
    assert_eq!(
        vs,
        EditorInvocation::Exec {
            program: PathBuf::from("code"),
            args: vec![
                String::from("--goto"),
                String::from("/home/user/meu projeto/ação #1%20.ts:42:7"),
            ],
        }
    );

    // 2. VsCodeInsiders (sem linha/coluna informada -> padrão 1:1)
    let link_no_pos = ResolvedLink {
        key: String::from("k2"),
        absolute_path: String::from("/home/user/app.rs"),
        line: None,
        column: None,
    };
    let ins = build_editor_invocation(
        &EditorSettings {
            kind: EditorKind::VsCodeInsiders,
            custom_argv: Vec::new(),
        },
        &link_no_pos,
    )
    .unwrap();
    assert_eq!(
        ins,
        EditorInvocation::Exec {
            program: PathBuf::from("code-insiders"),
            args: vec![
                String::from("--goto"),
                String::from("/home/user/app.rs:1:1"),
            ],
        }
    );

    // 3. MaxCode (vscode://file/ com percent-encode preservando '/')
    let mc = build_editor_invocation(
        &EditorSettings {
            kind: EditorKind::MaxCode,
            custom_argv: Vec::new(),
        },
        &link,
    )
    .unwrap();
    assert_eq!(
        mc,
        EditorInvocation::OpenUri(String::from(
            "vscode://file/home/user/meu%20projeto/a%C3%A7%C3%A3o%20%231%2520.ts:42:7"
        ))
    );

    // 4. Custom com {file}, {line}, {column}
    let custom = build_editor_invocation(
        &EditorSettings {
            kind: EditorKind::Custom,
            custom_argv: vec![String::from("zed"), String::from("{file}:{line}:{column}")],
        },
        &link,
    )
    .unwrap();
    assert_eq!(
        custom,
        EditorInvocation::Exec {
            program: PathBuf::from("zed"),
            args: vec![String::from("/home/user/meu projeto/ação #1%20.ts:42:7")],
        }
    );

    // Custom sem {file} -> os.settings_invalid
    let bad_custom = build_editor_invocation(
        &EditorSettings {
            kind: EditorKind::Custom,
            custom_argv: vec![String::from("zed"), String::from("--new")],
        },
        &link,
    )
    .unwrap_err();
    assert_eq!(bad_custom.code.0, codes::SETTINGS_INVALID);
}
