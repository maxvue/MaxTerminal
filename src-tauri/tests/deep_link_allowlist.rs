//! Testes de allowlist e validação por camada de deep links `maxterminal://` (`CA-34` — Fase 17).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use maxterminal_lib::bridge::BridgeActivationHandler;
use mt_bridge::{
    BridgeError, BridgeErrorCode, BridgeHost, BridgeProjectSummary, BridgeService, BridgeSettings,
    InMemoryBridgeSettingsStore, InspectableBlockContext, NoopBridgeEventSink, OpenTerminalRequest,
    OpenedTerminalInfo, Url, current_unix_ms, parse_maxterminal_uri,
};
use mt_os::{ActivationEnvelope, ActivationRouter, ActivationSource, codes};
use parking_lot::Mutex;
use uuid::Uuid;

struct TempTestDir(PathBuf);

impl TempTestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mt-deeplink-{}", Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct AllowlistTestHost {
    projects: Mutex<Vec<BridgeProjectSummary>>,
    focused: Mutex<Vec<String>>,
    opened_terminals: Mutex<Vec<OpenTerminalRequest>>,
}

impl AllowlistTestHost {
    fn new(registered_project: &Path) -> Self {
        let canon = fs::canonicalize(registered_project)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        Self {
            projects: Mutex::new(vec![BridgeProjectSummary {
                id: "proj-registered".to_string(),
                title: "RegisteredRepo".to_string(),
                path: canon,
            }]),
            focused: Mutex::new(Vec::new()),
            opened_terminals: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl BridgeHost for AllowlistTestHost {
    async fn active_project_path(&self) -> Option<String> {
        self.focused.lock().last().cloned()
    }

    async fn find_project_by_path(
        &self,
        canonical_path: &Path,
    ) -> Result<Option<BridgeProjectSummary>, BridgeError> {
        let target = canonical_path.to_string_lossy();
        Ok(self
            .projects
            .lock()
            .iter()
            .find(|p| p.path == target)
            .cloned())
    }

    async fn create_project_for_path(
        &self,
        canonical_path: &Path,
        preferred_name: Option<&str>,
    ) -> Result<BridgeProjectSummary, BridgeError> {
        let summary = BridgeProjectSummary {
            id: "proj-auto".to_string(),
            title: preferred_name.unwrap_or("Auto").to_string(),
            path: canonical_path.to_string_lossy().into_owned(),
        };
        self.projects.lock().push(summary.clone());
        Ok(summary)
    }

    async fn focus_project(&self, project_id: &str) -> Result<(), BridgeError> {
        self.focused.lock().push(project_id.to_string());
        Ok(())
    }

    async fn open_terminal_for_issue(
        &self,
        request: OpenTerminalRequest,
    ) -> Result<OpenedTerminalInfo, BridgeError> {
        self.opened_terminals.lock().push(request.clone());
        Ok(OpenedTerminalInfo {
            project_id: request.project_id,
            tab_id: "tab-issue".to_string(),
        })
    }

    async fn load_inspectable_block(
        &self,
        _block_id: &str,
    ) -> Result<InspectableBlockContext, BridgeError> {
        Err(BridgeError::new(
            BridgeErrorCode::BlockNotFound,
            "Não encontrado.",
        ))
    }

    async fn bring_window_to_front(&self) -> Result<(), BridgeError> {
        Ok(())
    }

    async fn launch_external_uri(&self, _uri: &str) -> Result<(), BridgeError> {
        Ok(())
    }
}

fn build_uri(route: &str, pairs: &[(&str, &str)]) -> String {
    let mut u = Url::parse(&format!("maxterminal://{route}")).unwrap();
    {
        let mut qp = u.query_pairs_mut();
        for &(k, v) in pairs {
            qp.append_pair(k, v);
        }
    }
    String::from(u)
}

#[tokio::test]
async fn deep_link_layered_validation_and_single_pipeline_cold_and_warm_start() {
    let tmp = TempTestDir::new();
    let registered_dir = tmp.path().join("registered-repo");
    let nested_dir = registered_dir.join("sub");
    let unknown_dir = tmp.path().join("unregistered-repo");
    let outside_wt = tmp.path().join("outside-worktree");
    let inside_wt = registered_dir.join(".worktrees").join("wt-42");

    fs::create_dir_all(&nested_dir).unwrap();
    fs::create_dir_all(&unknown_dir).unwrap();
    fs::create_dir_all(&outside_wt).unwrap();
    fs::create_dir_all(&inside_wt).unwrap();

    let now_ms = current_unix_ms();
    let now_str = now_ms.to_string();
    let t0 = Instant::now();

    // Camada 1: ActivationEnvelope::parse (mt-os) -> os.activation_invalid
    let oversized_uri = format!("maxterminal://focus?pad={}", "a".repeat(8200));
    for bad_raw in [
        "maxterminal://focus?bad=with\0nul",
        "maxterminal://focus?bad=with\r\ncrlf",
        "maxterminal://focus?bad=%ZZ",
        oversized_uri.as_str(),
    ] {
        let err =
            ActivationEnvelope::parse(bad_raw, ActivationSource::DeepLink, now_ms, t0).unwrap_err();
        assert_eq!(
            err.code.0,
            codes::ACTIVATION_INVALID,
            "Esperado os.activation_invalid para entrada malformada"
        );
    }

    // Camada 2: parse_maxterminal_uri (mt-bridge) -> bridge.invalid_uri para rotas ou parâmetros proibidos
    let req = Uuid::new_v4().to_string();
    let unknown_route = build_uri("run-shell", &[("v", "1"), ("req", &req), ("ts", &now_str)]);
    let err_route = parse_maxterminal_uri(&unknown_route, now_ms).unwrap_err();
    assert_eq!(err_route.code.canonical_code(), "bridge.invalid_uri");

    let reg_str = registered_dir.to_string_lossy().into_owned();
    for forbidden_param in [
        "cmd",
        "command",
        "initial_command",
        "args",
        "shell",
        "env",
        "flags",
        "unknown_extra",
    ] {
        let req_id = Uuid::new_v4().to_string();
        let uri_with_forbidden = build_uri(
            "open-terminal",
            &[
                ("v", "1"),
                ("req", &req_id),
                ("ts", &now_str),
                ("project", &reg_str),
                ("issue", "42"),
                (forbidden_param, "id"),
            ],
        );
        let err = parse_maxterminal_uri(&uri_with_forbidden, now_ms).unwrap_err();
        assert_eq!(
            err.code.canonical_code(),
            "bridge.invalid_uri",
            "Parâmetro proibido '{forbidden_param}' deveria retornar bridge.invalid_uri"
        );
    }

    // Camada 3: Caminhos relativos ou inexistentes -> bridge.invalid_path
    for bad_path in ["relative/repo", "/caminho/que/nao/existe/maxterminal-99999"] {
        let req_id = Uuid::new_v4().to_string();
        let uri_bad_path = build_uri(
            "open-project",
            &[
                ("v", "1"),
                ("req", &req_id),
                ("ts", &now_str),
                ("path", bad_path),
            ],
        );
        let err = parse_maxterminal_uri(&uri_bad_path, now_ms).unwrap_err();
        assert_eq!(err.code.canonical_code(), "bridge.invalid_path");
    }

    // Camada 4: Pipeline único ActivationRouter -> BridgeActivationHandler -> BridgeService
    let host = Arc::new(AllowlistTestHost::new(&registered_dir));
    let store = Arc::new(InMemoryBridgeSettingsStore::new(BridgeSettings {
        auto_register_unknown_project: false,
        ..BridgeSettings::default()
    }));
    let sink = Arc::new(NoopBridgeEventSink);
    let disc_dir = tmp.path().join("max-bridge");
    let svc = BridgeService::new("0.1.0", store, host.clone(), sink, Some(disc_dir))
        .await
        .unwrap();

    let router = ActivationRouter::new();
    router
        .register_handler(BridgeActivationHandler::new(Arc::clone(&svc)))
        .await
        .unwrap();

    // 4a. Cold start: URI com '..' que canonicaliza para projeto válido e cadastrado funciona no set_ready()
    let dotdot_valid_path = format!("{}/..", nested_dir.display());
    let req_cold = Uuid::new_v4().to_string();
    let cold_uri = build_uri(
        "open-project",
        &[
            ("v", "1"),
            ("req", &req_cold),
            ("ts", &now_str),
            ("path", &dotdot_valid_path),
        ],
    );
    let cold_env =
        ActivationEnvelope::parse(&cold_uri, ActivationSource::Startup, now_ms, t0).unwrap();
    router.enqueue(cold_env).await.unwrap();
    assert!(host.focused.lock().is_empty());

    router.set_ready().await.unwrap();
    assert_eq!(
        host.focused.lock().as_slice(),
        &["proj-registered".to_string()]
    );

    // 4b. Warm start: diretório existente porém não cadastrado (auto_register_unknown_project = false) -> bridge.project_not_found
    let req_unk = Uuid::new_v4().to_string();
    let unk_str = unknown_dir.to_string_lossy().into_owned();
    let unknown_proj_uri = build_uri(
        "open-project",
        &[
            ("v", "1"),
            ("req", &req_unk),
            ("ts", &now_str),
            ("path", &unk_str),
        ],
    );
    let warm_unknown_env = ActivationEnvelope::parse(
        &unknown_proj_uri,
        ActivationSource::DeepLink,
        now_ms,
        t0 + Duration::from_secs(2),
    )
    .unwrap();
    let err_not_found = router.enqueue(warm_unknown_env).await.unwrap_err();
    assert_eq!(err_not_found.code.0, "bridge.project_not_found");

    // 4c. Warm start: worktree fora da pasta do projeto -> bridge.cwd_outside_project
    let req_out = Uuid::new_v4().to_string();
    let out_wt_str = outside_wt.to_string_lossy().into_owned();
    let outside_wt_uri = build_uri(
        "open-terminal",
        &[
            ("v", "1"),
            ("req", &req_out),
            ("ts", &now_str),
            ("project", &reg_str),
            ("worktree", &out_wt_str),
            ("issue", "7"),
        ],
    );
    let warm_outside_env = ActivationEnvelope::parse(
        &outside_wt_uri,
        ActivationSource::DeepLink,
        now_ms,
        t0 + Duration::from_secs(4),
    )
    .unwrap();
    let err_outside = router.enqueue(warm_outside_env).await.unwrap_err();
    assert_eq!(err_outside.code.0, "bridge.cwd_outside_project");

    // 4d. Warm start: worktree legítima dentro de .worktrees/ do projeto é aceita sem initial_command
    let req_in = Uuid::new_v4().to_string();
    let in_wt_str = inside_wt.to_string_lossy().into_owned();
    let inside_wt_uri = build_uri(
        "open-terminal",
        &[
            ("v", "1"),
            ("req", &req_in),
            ("ts", &now_str),
            ("project", &reg_str),
            ("worktree", &in_wt_str),
            ("issue", "7"),
            ("agent", "claude"),
        ],
    );
    let warm_inside_env = ActivationEnvelope::parse(
        &inside_wt_uri,
        ActivationSource::DeepLink,
        now_ms,
        t0 + Duration::from_secs(6),
    )
    .unwrap();
    router.enqueue(warm_inside_env).await.unwrap();
    let opened = host.opened_terminals.lock();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].issue_number, 7);
    assert!(opened[0].initial_command.is_none());
}
