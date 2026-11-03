//! Ponto de composição da biblioteca nativa do `MaxTerminal`.

#![cfg_attr(test, allow(clippy::unwrap_used))]
#![allow(
    missing_docs,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::expect_used,
    clippy::large_enum_variant,
    clippy::match_like_matches_macro,
    clippy::needless_borrow,
    clippy::unnecessary_lazy_evaluations,
    clippy::unnecessary_literal_unwrap
)]

#[macro_use]
pub mod commands;
#[macro_use]
pub mod perf;
pub mod adapters;
pub mod agents;
pub mod ai_context;
pub mod ai_state;
pub mod bridge;
pub mod command_groups;
pub mod events;
pub mod github;
pub mod horizon;
pub mod logging;
pub mod search;
pub mod security;
pub mod specta;
mod startup;
pub mod state;
pub mod updater;
pub mod workspace;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

use mt_ai::AiService;
use mt_ai::repo::AiRepo;
use mt_bridge::{BridgeEventSink, BridgeService};
use mt_core::paths::{AppPaths, SystemEnv};
use mt_db::DbConfig;
use mt_git::{CliGitRunner, CliWorktreeManager, GitService, WorktreeManager};
use mt_os::{
    AutostartSync, CloseDecision, DbusNotificationTransport, DefaultShortcutBackendFactory, OsDeps,
    OsIntegration, QuitSource, classify_display, parse_cli_args, probe,
};
use mt_ports::PortsService;
use mt_projects::{ProjectEventSink, ProjectService, SqliteProjectStore};
use mt_pty::{OutputTap, PtyConfig, PtyManager};
use mt_shell::{ShellEventSink, ShellService};
use mt_themes::{ThemeEventSink, ThemeService};
use state::{AppState, DbService, FrontendLogState};
use tauri::{Manager, RunEvent, WindowEvent};

use crate::adapters::exact_process_signaler::PtyExactProcessSignaler;
use crate::adapters::indicators::IndicatorsLifecycleObserver;
use crate::adapters::os::{
    AppShutdownHooks, OsLifecycleObserver, PaneCwdTracker, TauriWindowHost,
    WorkspaceTabCwdProvider, WorkspaceTabStatusProvider,
};
use crate::adapters::port_opener::default_port_url_opener;
use crate::adapters::process_tree::{WorkspaceActiveWorktreeGuard, WorkspaceProcessTreeSource};
use crate::agents::AppAgentState;
use crate::ai_context::TauriAiContextSource;
use crate::ai_state::AppAiState;
use crate::bridge::{
    BridgeActivationHandler, BridgeEditorOpener, DbBridgeSettingsStore, TauriBridgeHost,
};
use crate::command_groups::AppCommandGroupState;
use crate::commands::quick_commands::QuickCommandsState;
use crate::events::ai::spawn_ai_background_tasks;
use crate::events::bridge::TauriBridgeEventSink;
use crate::events::db::TauriDbEventSink;
use crate::events::git::TauriGitEventSink;
use crate::events::os::TauriOsEventSink;
use crate::events::ports::TauriPortsEventSink;
use crate::events::projects::TauriProjectEventSink;
use crate::events::pty::TauriEventListener;
use crate::events::quick_commands::TauriCommandsEventSink;
use crate::events::shell::TauriShellEventSink;
use crate::events::themes::TauriThemeEventSink;
use crate::github::AppGithubState;
use crate::logging::{SanitizerLogRedactor, init_logging};
use crate::workspace::{SessionRecorder, SpawnHookChain, WorkspaceManager};

/// Executa o ciclo de vida completo do aplicativo desktop `MaxTerminal`.
#[allow(clippy::too_many_lines)]
pub fn run() {
    let paths = match AppPaths::resolve(&SystemEnv) {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!(
                "Falha ao resolver diretórios do MaxTerminal: {}",
                err.message
            );
            std::process::exit(1);
        }
    };

    if let Err(err) = paths.ensure_dirs() {
        eprintln!("Falha ao criar diretórios do MaxTerminal: {}", err.message);
        std::process::exit(1);
    }

    let _log_guard = match init_logging(&paths.log_dir, Box::new(SanitizerLogRedactor)) {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("Falha ao inicializar logs do MaxTerminal: {}", err.message);
            std::process::exit(1);
        }
    };
    perf::init_tracing();

    let paths_for_db = paths.clone();
    let app_state = AppState::new(paths);
    let perf_collector = perf::PerfCollector::new();
    let version = app_state.info.version.clone();

    let pty_manager = match PtyManager::new(PtyConfig::default()) {
        Ok(mgr) => mgr,
        Err(err) => {
            eprintln!("Falha ao inicializar o gerenciador de PTYs: {err}");
            std::process::exit(1);
        }
    };

    let _guard_handle = match pty_manager.install_guards() {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("Falha ao instalar guards de supervisão do PtyManager: {err}");
            std::process::exit(1);
        }
    };

    tracing::info!(
        target: "maxterminal",
        event = "app_startup",
        version = %version,
        "MaxTerminal iniciado"
    );

    let specta_builder = specta::builder();
    let invoke_handler = specta_builder.invoke_handler();
    let pty_for_setup = Arc::clone(&pty_manager);
    let pty_for_run = Arc::clone(&pty_manager);
    let shutdown_complete = Arc::new(AtomicBool::new(false));
    let shutdown_complete_for_setup = Arc::clone(&shutdown_complete);
    let shutdown_in_progress = Arc::new(AtomicBool::new(false));

    tracing::info!(
        target: "maxterminal",
        event = "startup_stage",
        code = "startup.runtime.build_start",
        "Etapa de inicialização iniciada"
    );

    let app = match tauri::Builder::default()
        .manage(app_state)
        .manage(perf_collector)
        .manage(FrontendLogState::default())
        .manage(pty_manager)
        .setup(move |app| {
            let _rt_guard = tauri::async_runtime::handle().inner().enter();
            specta_builder.mount_events(app);
            let listener = Arc::new(TauriEventListener::new(app.handle().clone()));
            pty_for_setup.add_listener(listener);

            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.db.start",
                "Etapa de inicialização iniciada"
            );

            let Some(db) = startup::initialize_database(
                DbConfig::from_paths(&paths_for_db, &SystemEnv),
                Arc::new(TauriDbEventSink::new(app.handle().clone())),
                |code| app.handle().exit(code),
            ) else {
                // Evita que o Tauri transforme o erro esperado em pânico no event loop.
                return Ok(());
            };

            let db_arc = Arc::new(db);
            app.manage(DbService::new(Arc::clone(&db_arc)));
            app.manage(crate::search::AppSearchState::new(
                (*db_arc).clone(),
                Some(app.handle().clone()),
            ));

            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.db.ok",
                "Etapa de inicialização concluída"
            );

            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.workspace.start",
                "Etapa de inicialização iniciada"
            );

            // Subsistema de Temas e Importador Tabby (Fase 10)
            let theme_sink: Arc<dyn ThemeEventSink> =
                TauriThemeEventSink::new(app.handle().clone());
            let theme_service = ThemeService::new(db_arc.themes(), theme_sink)
                .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
            app.manage(theme_service);

            let tauri_shell_sink = Arc::new(TauriShellEventSink::new(app.handle().clone()));
            let shell_sink: Arc<dyn ShellEventSink> =
                Arc::clone(&tauri_shell_sink) as Arc<dyn ShellEventSink>;
            let runtime_dir = paths_for_db
                .runtime_dir
                .clone()
                .unwrap_or_else(|| paths_for_db.cache_dir.join("runtime"));
            let shell_service = ShellService::new(
                Arc::clone(&pty_for_setup),
                (*db_arc).clone(),
                runtime_dir,
                Some(shell_sink),
            );
            app.manage(Arc::clone(&shell_service));

            let project_store = Arc::new(SqliteProjectStore::from_db(&db_arc));
            let project_sink: Arc<dyn ProjectEventSink> =
                Arc::new(TauriProjectEventSink::new(app.handle().clone()));
            let project_service = ProjectService::new(project_store, project_sink);
            let project_service_bg = Arc::clone(&project_service);
            app.manage(Arc::clone(&project_service));

            let recorder = SessionRecorder::new((*db_arc).clone())
                .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
            let _recorder_tap =
                pty_for_setup.add_output_tap(Arc::clone(&recorder) as Arc<dyn OutputTap>);

            let workspace_manager = WorkspaceManager::new(
                (*db_arc).clone(),
                Arc::clone(&pty_for_setup),
                Arc::clone(&shell_service),
                recorder,
                SpawnHookChain::new(),
            );
            workspace_manager.attach_app_handle(app.handle().clone());
            workspace_manager.install_pty_listener();
            project_service.register_removal_hook(Arc::new(workspace_manager.clone()));

            // Subsistema Git e Worktrees (Fase 08)
            let git_runner = Arc::new(CliGitRunner::new());
            let git_sink = TauriGitEventSink::new(app.handle().clone());
            let git_service = GitService::new(git_runner.clone(), git_sink, true);
            let active_wt_guard =
                WorkspaceActiveWorktreeGuard::new(workspace_manager.clone(), git_service.clone());
            let worktree_manager: Arc<dyn WorktreeManager> =
                Arc::new(CliWorktreeManager::new(git_runner, active_wt_guard));
            let git_service_for_agents = git_service.clone();
            let git_service_for_github = git_service.clone();
            let worktree_manager_for_agents = Arc::clone(&worktree_manager);
            let worktree_manager_for_github = Arc::clone(&worktree_manager);
            app.manage(git_service.clone());
            app.manage(worktree_manager);

            // Subsistema de Portas Ativas (Fase 08)
            let tree_source = WorkspaceProcessTreeSource::new(
                workspace_manager.clone(),
                Arc::clone(&pty_for_setup),
            );
            let exact_signaler = PtyExactProcessSignaler::new(Arc::clone(&pty_for_setup));
            let url_opener = default_port_url_opener();
            let ports_sink = TauriPortsEventSink::new(app.handle().clone());
            let ports_service =
                PortsService::new(tree_source, exact_signaler, url_opener, ports_sink);
            ports_service.start_background_loop();
            app.manage(ports_service.clone());

            let indicators_observer = IndicatorsLifecycleObserver::new(
                workspace_manager.clone(),
                git_service.clone(),
                ports_service.clone(),
            );
            workspace_manager.register_observer(indicators_observer.clone());
            tauri_shell_sink.add_listener(indicators_observer);

            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.workspace.ok",
                "Etapa de inicialização concluída"
            );

            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.os.start",
                "Etapa de inicialização iniciada"
            );

            // Subsistema de Integração com o SO (Fase 09)
            let os_event_sink = TauriOsEventSink::new(app.handle().clone());
            let pane_cwd_tracker = PaneCwdTracker::new();
            let status_provider = WorkspaceTabStatusProvider::new(
                workspace_manager.clone(),
                Arc::clone(&shell_service),
                Arc::clone(&pty_for_setup),
            );
            let cwd_provider = WorkspaceTabCwdProvider::new(
                workspace_manager.clone(),
                Arc::clone(&pane_cwd_tracker),
            );
            let shutdown_hooks = AppShutdownHooks::new(
                app.handle().clone(),
                Arc::clone(&pty_for_setup),
                Arc::clone(&db_arc),
                Arc::clone(&shell_service),
                Arc::clone(&project_service),
                workspace_manager.clone(),
                git_service,
                ports_service,
                status_provider.clone(),
                os_event_sink.clone(),
                Arc::clone(&shutdown_complete_for_setup),
            );
            let window_host = TauriWindowHost::new(app.handle().clone(), os_event_sink.clone());
            let autostart_sync = AutostartSync::from_system_env().unwrap_or_else(|_| {
                AutostartSync::new(
                    paths_for_db.config_dir.join("autostart"),
                    std::env::current_exe().unwrap_or_default(),
                )
            });
            let startup_cli_action = parse_cli_args(std::env::args());
            let (display_server, compositor) = classify_display(&|k| std::env::var(k).ok());
            let initial_env = mt_os::OsEnvironment {
                display_server,
                compositor,
                window_positioning: display_server == mt_os::DisplayServer::X11,
                editors: mt_os::EditorLauncher::detect(),
                ..Default::default()
            };

            let os_deps = OsDeps {
                status: status_provider.clone(),
                cwd: cwd_provider,
                shutdown: shutdown_hooks,
                settings_repo: Arc::new(db_arc.os_settings()),
                window_host,
                shortcut_factory: Arc::new(DefaultShortcutBackendFactory),
                notification_transport: Arc::new(DbusNotificationTransport),
                autostart_sync,
                event_sink: os_event_sink,
                home_dir: std::env::var_os("HOME")
                    .map_or_else(|| paths_for_db.config_dir.clone(), std::path::PathBuf::from),
                startup_cli_action,
            };

            let os_integration =
                tauri::async_runtime::block_on(OsIntegration::init(os_deps, initial_env))
                    .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

            let os_observer = OsLifecycleObserver::new(
                Arc::clone(&os_integration),
                workspace_manager.clone(),
                pane_cwd_tracker,
            );
            workspace_manager.register_observer(os_observer.clone());
            tauri_shell_sink.add_listener(os_observer);

            let os_bg = Arc::clone(&os_integration);
            app.manage(Arc::clone(&os_integration));

            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.os.ok",
                "Etapa de inicialização concluída"
            );

            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.agents.start",
                "Etapa de inicialização iniciada"
            );

            // Subsistema de Integração Bidirecional com MaxCode (Fase 11)
            let bridge_settings_store = DbBridgeSettingsStore::new(Arc::clone(&db_arc));
            let bridge_host = TauriBridgeHost::new(
                Arc::clone(&db_arc),
                Arc::clone(&project_service),
                workspace_manager.clone(),
                Arc::clone(&shell_service),
                Arc::clone(&os_integration),
            );
            let bridge_sink: Arc<dyn BridgeEventSink> =
                TauriBridgeEventSink::new(app.handle().clone());
            let app_ver_for_bridge = app.package_info().version.to_string();
            let bridge_service = tauri::async_runtime::block_on(BridgeService::new(
                app_ver_for_bridge,
                bridge_settings_store,
                bridge_host,
                bridge_sink,
                None,
            ))
            .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;

            os_integration
                .editor
                .set_bridge_opener(BridgeEditorOpener::new(Arc::clone(&bridge_service)));
            let _ = tauri::async_runtime::block_on(
                os_integration
                    .activations
                    .register_handler(BridgeActivationHandler::new(Arc::clone(&bridge_service))),
            );
            let bridge_for_github = Arc::clone(&bridge_service);
            app.manage(bridge_service);

            // Subsistema de Integração com MaxAiManager (`@` / `?` — Fase 12)
            let ai_context_source = Arc::new(TauriAiContextSource::new(
                workspace_manager.clone(),
                Arc::clone(&project_service),
                Arc::clone(&shell_service),
            ));
            let ai_repo = Arc::new(AiRepo::new((*db_arc).clone()));
            let ai_service = tauri::async_runtime::block_on(AiService::new(
                ai_repo,
                Arc::clone(&ai_context_source) as Arc<dyn mt_ai::context::AiContextSource>,
            ))
            .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
            ai_context_source.bind_service(Arc::clone(&ai_service));
            workspace_manager.register_observer(ai_context_source);
            app.manage(AppAiState::new(
                Arc::clone(&ai_service),
                app.handle().clone(),
            ));
            let ai_service_for_groups = Arc::clone(&ai_service);
            spawn_ai_background_tasks(app.handle().clone(), ai_service);

            // Subsistema de Agentes CLI, Telemetria e MaxSkills (Fase 13)
            let agent_state = tauri::async_runtime::block_on(AppAgentState::init(
                app.handle().clone(),
                paths_for_db.clone(),
                (*db_arc).clone(),
                Arc::clone(&pty_for_setup),
                Arc::clone(&shell_service),
                Arc::clone(&tauri_shell_sink),
                Arc::clone(&project_service),
                workspace_manager.clone(),
                git_service_for_agents,
                worktree_manager_for_agents,
                Arc::clone(&os_integration),
            ))
            .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
            let memory_for_groups = Arc::clone(&agent_state.service.memory);
            let agent_state_for_github = agent_state.clone();
            app.manage(agent_state);

            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.agents.ok",
                "Etapa de inicialização concluída"
            );

            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.services.start",
                "Etapa de inicialização iniciada"
            );

            // Subsistema de Comandos Rápidos Artisan e Atalhos Universais (Fase 14)
            let qc_sink = TauriCommandsEventSink::new(app.handle().clone());
            app.manage(QuickCommandsState::new((*db_arc).clone(), qc_sink));

            // Subsistema de Grupos de Comandos (`>>` — Fase 15)
            let group_state = tauri::async_runtime::block_on(AppCommandGroupState::init(
                app.handle().clone(),
                (*db_arc).clone(),
                Arc::clone(&pty_for_setup),
                Arc::clone(&shell_service),
                Arc::clone(&tauri_shell_sink),
                Arc::clone(&project_service),
                workspace_manager.clone(),
                ai_service_for_groups,
                memory_for_groups,
            ))
            .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
            app.manage(group_state);

            // Subsistema de Aba Fixa de GitHub Issues (Fase 16)
            let github_state = AppGithubState::init(
                app.handle().clone(),
                &paths_for_db,
                (*db_arc).clone(),
                Arc::clone(&project_service),
                git_service_for_github,
                worktree_manager_for_github,
                workspace_manager.clone(),
                agent_state_for_github,
                bridge_for_github,
            )
            .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
            app.manage(github_state);

            // Subsistema Laravel Horizon
            let horizon_lookup: Arc<dyn mt_commands::ports::ProjectLookup> = Arc::new(
                crate::commands::quick_commands::DbProjectLookup::new((*db_arc).clone()),
            );
            let horizon_domain = Arc::new(mt_commands::horizon::HorizonDomainService::new(
                horizon_lookup,
                Arc::new(mt_commands::ports::SystemClock),
            ));
            let horizon_runtime = crate::horizon::HorizonRuntimeService::new(
                Arc::clone(&pty_for_setup),
                horizon_domain,
            );
            status_provider.attach_horizon(Arc::clone(&horizon_runtime));
            project_service.register_removal_hook(Arc::new(
                crate::horizon::HorizonProjectRemovalHook::new(Arc::clone(&horizon_runtime)),
            ));
            app.manage(Arc::clone(&horizon_runtime));

            let workspace_manager_bg = workspace_manager.clone();
            app.manage(workspace_manager);

            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.services.ok",
                "Etapa de inicialização concluída"
            );

            tauri::async_runtime::spawn(async move {
                let probed_env = probe(&|k| std::env::var(k).ok()).await;
                *os_bg.env.write() = probed_env;
                if let Err(err) = project_service_bg.start().await {
                    tracing::error!(
                        target: "mt_projects",
                        error = %err,
                        "Falha ao iniciar ProjectService"
                    );
                }
                if let Err(err) = workspace_manager_bg.ensure_restored().await {
                    tracing::error!(
                        target: "mt_workspace",
                        error = %err,
                        "Falha ao restaurar WorkspaceManager"
                    );
                }
                let _ = os_bg.activations.set_ready().await;
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                if let Some(os) = window.app_handle().try_state::<Arc<OsIntegration>>() {
                    match os.close_requested(false) {
                        CloseDecision::HideToTray => {
                            let _ = os.quake.hide();
                        }
                        CloseDecision::Minimize => {
                            let _ = os.quake.minimize();
                        }
                        CloseDecision::Quit | CloseDecision::ConfirmQuit => {
                            let os_arc = Arc::clone(&os);
                            tauri::async_runtime::spawn(async move {
                                let _ = os_arc.shutdown.request_quit(QuitSource::Window).await;
                            });
                        }
                    }
                }
            }
            WindowEvent::Focused(focused) => {
                if let Some(os) = window.app_handle().try_state::<Arc<OsIntegration>>() {
                    os.on_window_focus(*focused);
                }
            }
            _ => {}
        })
        .invoke_handler(invoke_handler)
        .build(tauri::generate_context!())
    {
        Ok(app) => {
            tracing::info!(
                target: "maxterminal",
                event = "startup_stage",
                code = "startup.runtime.build_ok",
                "Etapa de inicialização concluída"
            );
            app
        }
        Err(_err) => {
            tracing::error!(
                target: "maxterminal",
                event = "startup_failed",
                code = "startup.runtime.build_err",
                "Falha crítica ao construir o runtime Tauri"
            );
            eprintln!("Falha crítica ao construir o runtime Tauri do MaxTerminal.");
            std::process::exit(1);
        }
    };

    // Os guards de sinais já protegem os PTYs; após construir o app também encerram
    // o event loop, para que SIGTERM libere o lock do banco em vez de deixar a UI viva.
    let signal_handle = app.handle().clone();
    if let Err(err) = mt_pty::guard::install_signal_handler(move |signal| {
        signal_handle.exit(128 + signal);
    }) {
        tracing::error!(target: "maxterminal", error = %err, "Falha ao instalar encerramento por sinal");
    }

    // O runtime Wry/Linux pode retornar 0 mesmo para RequestExit(1).
    let requested_exit_code = Arc::new(AtomicI32::new(0));
    let requested_exit_code_for_run = Arc::clone(&requested_exit_code);
    let exit_code = app.run_return(move |app_handle, event| match event {
        RunEvent::ExitRequested { api, code, .. } => {
            if let Some(code) = code {
                requested_exit_code_for_run.store(code, Ordering::Release);
            }
            if shutdown_complete.load(Ordering::Acquire) {
                return;
            }
            api.prevent_exit();

            if !shutdown_in_progress.swap(true, Ordering::AcqRel) {
                let mgr = Arc::clone(&pty_for_run);
                let complete_flag = Arc::clone(&shutdown_complete);
                let in_progress_flag = Arc::clone(&shutdown_in_progress);
                let handle = app_handle.clone();

                tauri::async_runtime::spawn_blocking(move || {
                    if let Some(ports_svc) = handle.try_state::<PortsService>() {
                        ports_svc.shutdown();
                    }

                    if let Some(git_svc) = handle.try_state::<GitService>() {
                        git_svc.shutdown();
                    }

                    if let Some(proj_svc) = handle.try_state::<Arc<ProjectService>>() {
                        tauri::async_runtime::block_on(proj_svc.shutdown());
                    }

                    if let Some(cg_state) = handle.try_state::<AppCommandGroupState>() {
                        tauri::async_runtime::block_on(cg_state.service.shutdown());
                    }

                    if let Some(ws_mgr) = handle.try_state::<WorkspaceManager>() {
                        tauri::async_runtime::block_on(ws_mgr.shutdown());
                    }

                    if let Some(horizon_svc) = handle.try_state::<Arc<crate::horizon::HorizonRuntimeService>>() {
                        horizon_svc.shutdown();
                    }

                    if let Some(shell_svc) = handle.try_state::<Arc<ShellService>>() {
                        shell_svc.shutdown();
                    }

                    let report = mgr.shutdown(Duration::from_millis(3500));

                    if let Some(db_svc) = handle.try_state::<DbService>() {
                        if let Err(err) = tauri::async_runtime::block_on(db_svc.db().shutdown()) {
                            tracing::error!(
                                target: "mt_db",
                                error = %err,
                                "Falha durante desligamento do banco de dados"
                            );
                        }
                    }

                    if report.pending.is_empty() {
                        complete_flag.store(true, Ordering::Release);
                        handle.exit(code.unwrap_or(0));
                    } else {
                        tracing::error!(
                            target: "mt_pty",
                            event = "shutdown_cleanup_failed",
                            pending_count = report.pending.len(),
                            "Processos sobreviventes detectados durante o desligamento; mantendo supervisão ativa"
                        );
                        in_progress_flag.store(false, Ordering::Release);
                    }
                });
            }
        }
        RunEvent::Exit if !shutdown_complete.load(Ordering::Acquire) => {
            let state = app_handle.try_state::<Arc<PtyManager>>();
            if let Some(mgr) = state {
                mgr.emergency_kill_all();
            } else {
                pty_for_run.emergency_kill_all();
            }
        }
        _ => {}
    });
    // run_return permite drenar os logs inclusive quando a inicialização falha.
    drop(_log_guard);
    let requested_code = requested_exit_code.load(Ordering::Acquire);
    std::process::exit(if requested_code == 0 {
        exit_code
    } else {
        requested_code
    });
}
