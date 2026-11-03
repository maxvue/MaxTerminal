//! Testes de integração para `Detector` (detecção paralela, timeout de 3 s e `CapabilityReport`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Instant;

use mt_agents::detect::Detector;
use mt_agents::registry::{AgentKind, AgentRegistry, CapabilitySupport};
use mt_agents::settings::AgentsSettings;
use tempfile::TempDir;

fn write_executable_script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
}

#[tokio::test]
async fn detects_installed_missing_and_hanging_binaries_in_parallel() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    // claude: retorna versão 2.1.76 imediatamente
    write_executable_script(
        &bin_dir,
        "claude",
        "#!/bin/sh\necho '2.1.76 (Claude Code)'\nexit 0\n",
    );

    // codex: dorme 10s para exercitar o timeout de 3s com kill_on_drop
    write_executable_script(&bin_dir, "codex", "#!/bin/sh\nsleep 10\n");

    // agy: retorna código de erro != 0
    write_executable_script(&bin_dir, "agy", "#!/bin/sh\necho 'error' >&2\nexit 2\n");

    // opencode: ausente do diretório bin_dir

    let reg = AgentRegistry::with_builtins();
    let detector = Detector::with_search_paths(vec![bin_dir]);

    let start = Instant::now();
    let results = detector.detect_all(&reg).await;
    let elapsed = start.elapsed();

    // Deve completar em ~3s (timeout do codex) e nunca esperar 10s
    assert!(
        elapsed.as_secs_f64() < 4.5,
        "Detecção paralela demorou demais: {elapsed:?}"
    );
    assert_eq!(results.len(), 4);

    let claude = &results[0];
    assert_eq!(claude.kind, AgentKind::Claude);
    assert!(claude.installed);
    assert_eq!(claude.version.as_deref(), Some("2.1.76"));
    assert!(claude.reason.is_none());

    let codex = &results[1];
    assert_eq!(codex.kind, AgentKind::Codex);
    assert!(!codex.installed);
    assert!(
        codex
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("Tempo esgotado")
    );

    let agy = &results[2];
    assert_eq!(agy.kind, AgentKind::Agy);
    assert!(!agy.installed);
    assert!(
        agy.reason
            .as_deref()
            .unwrap_or_default()
            .contains("código 2")
    );

    let opencode = &results[3];
    assert_eq!(opencode.kind, AgentKind::OpenCode);
    assert!(!opencode.installed);
    assert_eq!(
        opencode.reason.as_deref(),
        Some("Binário não encontrado no PATH")
    );

    // Verifica CapabilityReport com as 9 chaves canônicas
    let settings = AgentsSettings::default();
    let infos = detector.list_agent_info(&reg, &settings);
    assert_eq!(infos.len(), 4);

    let claude_caps = &infos[0].capabilities.features;
    assert_eq!(claude_caps.len(), 9);
    assert_eq!(claude_caps["launch"].support, CapabilitySupport::Available);
    assert_eq!(
        claude_caps["modelSelection"].support,
        CapabilitySupport::Available
    );
    assert_eq!(
        claude_caps["contextTelemetry"].support,
        CapabilitySupport::Available
    );
    assert_eq!(
        claude_caps["attention"].support,
        CapabilitySupport::Available
    );
    assert_eq!(
        claude_caps["skillsProject"].support,
        CapabilitySupport::Available
    );

    let agy_caps = &infos[2].capabilities.features;
    assert_eq!(agy_caps.len(), 9);
    assert_eq!(agy_caps["launch"].support, CapabilitySupport::Unsupported);
    assert_eq!(
        agy_caps["modelSelection"].support,
        CapabilitySupport::Unverified
    );
    assert_eq!(
        agy_caps["skillsProject"].support,
        CapabilitySupport::Unverified
    );
}

#[tokio::test]
async fn isolates_probe_working_directory_from_process_cwd() {
    let tmp = TempDir::new().unwrap();
    let bin_dir = tmp.path().join("bin");
    let fake_src_tauri = tmp.path().join("src-tauri");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&fake_src_tauri).unwrap();

    // Simula um binário/shell cujo startup cria um arquivo relativo no diretório atual (ex.: `-l`).
    write_executable_script(
        &bin_dir,
        "claude",
        "#!/bin/sh\n: > ./-l\necho '2.1.76'\nexit 0\n",
    );

    let prev_cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&fake_src_tauri).unwrap();
    let reg = AgentRegistry::with_builtins();
    let detector = Detector::with_search_paths(vec![bin_dir]);
    let results = detector.detect_all(&reg).await;
    std::env::set_current_dir(prev_cwd).unwrap();

    assert!(results[0].installed);
    assert!(
        !fake_src_tauri.join("-l").exists(),
        "A detecção de agentes não deve criar arquivos em src-tauri/ (evita loop de rebuild no tauri dev)"
    );
}
