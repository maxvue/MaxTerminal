//! Teste opcional de integração real com projeto Laravel (`MT_LARAVEL_FIXTURE`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

#[tokio::test]
#[ignore = "Requer projeto Laravel real apontado por MT_LARAVEL_FIXTURE"]
async fn real_laravel_fixture_discovery() {
    let Some(fixture) = std::env::var_os("MT_LARAVEL_FIXTURE") else {
        return;
    };
    let root = std::path::PathBuf::from(fixture);
    let runner = mt_commands::artisan::runner::LocalPhpRunner::new();
    let bytes = mt_commands::ports::ArtisanRunner::list_json(
        &runner,
        &root,
        mt_commands::ports::RunLimits::default(),
    )
    .await
    .expect("list_json");
    let cmds = mt_commands::artisan::parser::parse_artisan_list(&bytes).expect("parse");
    assert!(cmds.iter().any(|c| c.name == "migrate"));
}
