//! Verifica estruturalmente que o crate `mt-ai` não depende de `mt-pty`, `mt-shell` ou `tauri` (`CA-15`).

#![allow(clippy::expect_used)]

#[test]
fn mt_ai_manifest_has_no_pty_or_tauri_dependency() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let cargo_toml =
        std::fs::read_to_string(format!("{manifest_dir}/Cargo.toml")).expect("ler Cargo.toml");

    assert!(
        !cargo_toml.contains("mt-pty"),
        "mt-ai JAMAIS deve depender de mt-pty"
    );
    assert!(
        !cargo_toml.contains("mt-shell"),
        "mt-ai JAMAIS deve depender de mt-shell"
    );
    assert!(
        !cargo_toml.contains("portable-pty"),
        "mt-ai JAMAIS deve depender de portable-pty"
    );
    assert!(
        !cargo_toml.contains("tauri ="),
        "mt-ai JAMAIS deve depender de tauri"
    );
}
