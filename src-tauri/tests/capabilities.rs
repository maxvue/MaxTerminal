//! Testes de integração para validação de capabilities, ACL e contratos de segurança Tauri v2.

use serde_json::Value;
use std::{collections::HashSet, fs, path::Path};

fn load_tauri_conf() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Falha ao ler tauri.conf.json em {}: {e}", path.display()));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("JSON inválido em tauri.conf.json: {e}"))
}

fn load_capability_default() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities/default.json");
    let content = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Falha ao ler capabilities/default.json em {}: {e}",
            path.display()
        )
    });
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("JSON inválido em capabilities/default.json: {e}"))
}

/// Valida que as capabilities restringem o acesso exclusivamente à janela 'main'
/// e aos comandos autorizados na allowlist (Fase 01 a Fase 09), sem conceder 'core:default'.
#[test]
#[allow(clippy::too_many_lines)]
fn capabilities_restrict_to_main_window_and_allowed_commands() {
    let cap = load_capability_default();

    assert_eq!(cap["identifier"], "foundation");
    assert_eq!(cap["local"], true);
    assert!(cap.get("remote").is_none());
    assert!(cap.get("urls").is_none());

    // Janelas autorizadas: estritamente ["main"]
    let windows = cap["windows"]
        .as_array()
        .unwrap_or_else(|| panic!("Campo 'windows' deve ser um array"));
    let window_labels: Vec<&str> = windows.iter().filter_map(|w| w.as_str()).collect();
    assert_eq!(window_labels, vec!["main"]);

    // Permissões autorizadas: app_info, app_log_frontend, 9 comandos pty_*, 5 comandos db_* e core:event:default
    let permissions = cap["permissions"]
        .as_array()
        .unwrap_or_else(|| panic!("Campo 'permissions' deve ser um array"));
    let perm_strings: Vec<&str> = permissions.iter().filter_map(|p| p.as_str()).collect();

    assert_eq!(
        perm_strings,
        vec![
            "allow-app-info",
            "allow-app-log-frontend",
            "allow-pty-spawn",
            "allow-pty-write",
            "allow-pty-resize",
            "allow-pty-kill",
            "allow-pty-list",
            "allow-pty-process-info",
            "allow-pty-ack",
            "allow-pty-attach",
            "allow-pty-detach",
            "allow-db-settings-get",
            "allow-db-settings-set",
            "allow-db-setting-snapshot",
            "allow-db-status",
            "allow-db-recover",
            "allow-shell-integration-info",
            "allow-command-block-list",
            "allow-command-block-get",
            "allow-command-block-error-context",
            "allow-command-block-search",
            "allow-tab-state-list",
            "allow-tab-state-get",
            "allow-tab-mark-seen",
            "allow-tab-set-focus",
            "allow-tab-set-continuous",
            "allow-project-snapshot",
            "allow-project-list",
            "allow-project-add",
            "allow-project-update",
            "allow-project-remove",
            "allow-project-reorder",
            "allow-project-redetect",
            "allow-project-preview-identity",
            "allow-project-refresh-missing",
            "allow-project-root-list",
            "allow-project-root-add",
            "allow-project-root-update",
            "allow-project-root-remove",
            "allow-project-root-rescan",
            "allow-project-discovery-list",
            "allow-project-discovery-accept",
            "allow-project-discovery-dismiss",
            "allow-template-list",
            "allow-template-get",
            "allow-template-create",
            "allow-template-update",
            "allow-template-delete",
            "allow-template-set-default",
            "allow-tab-list",
            "allow-tab-open",
            "allow-tab-close",
            "allow-tab-reorder",
            "allow-tab-focus",
            "allow-pane-split",
            "allow-pane-close",
            "allow-pane-restart",
            "allow-pane-layout-update",
            "allow-session-new",
            "allow-session-list",
            "allow-session-get",
            "allow-session-scrollback-get",
            "allow-session-search",
            "allow-workspace-snapshot",
            "allow-workspace-initial-command-run",
            "allow-git-status-for-path",
            "allow-git-status-refresh",
            "allow-git-worktree-list",
            "allow-git-worktree-create",
            "allow-git-worktree-remove",
            "allow-ports-list-for-tab",
            "allow-ports-open-browser",
            "allow-ports-kill",
            "allow-ports-set-poll-mode",
            "allow-os-settings-get",
            "allow-os-settings-update",
            "allow-os-environment-info",
            "allow-os-shortcut-status",
            "allow-os-shortcut-configure-portal",
            "allow-os-window-toggle",
            "allow-os-window-show",
            "allow-os-window-hide",
            "allow-os-quit-request",
            "allow-os-link-resolve",
            "allow-os-link-open",
            "allow-os-notify",
            "allow-os-attention-set",
            "allow-os-get-autostart",
            "allow-os-runtime-snapshot",
            "allow-os-ui-ready",
            "allow-os-set-autostart",
            "allow-theme-list",
            "allow-theme-get",
            "allow-theme-appearance-get",
            "allow-theme-appearance-update",
            "allow-theme-import-file",
            "allow-theme-import-dir",
            "allow-theme-set-active",
            "allow-theme-delete",
            "allow-theme-export",
            "allow-bridge-status",
            "allow-bridge-get-settings",
            "allow-bridge-update-settings",
            "allow-bridge-inspect-command-error",
            "allow-bridge-notify-project-selected",
            "allow-bridge-open-file-in-maxcode",
            "allow-bridge-reconnect-now",
            "allow-bridge-issue-protocol-support",
            "allow-bridge-send-issue-command",
            "allow-bridge-bugs-cycle-preview",
            "allow-bridge-bugs-cycle-start",
            "allow-ai-health",
            "allow-ai-settings-get",
            "allow-ai-settings-update",
            "allow-ai-catalog-get",
            "allow-ai-catalog-refresh",
            "allow-ai-context-preview",
            "allow-ai-suggest-command",
            "allow-ai-cancel",
            "allow-ai-classify-risk",
            "allow-ai-diagnose-block",
            "allow-ai-diagnosis-dismiss",
            "allow-ai-rag-list-slugs",
            "allow-ai-rag-search",
            "allow-ai-generate-structured",
            "allow-ai-history-list",
            "allow-ai-history-clear",
            "allow-ai-cache-clear",
            "allow-agent-list",
            "allow-agent-detect-refresh",
            "allow-agent-settings-get",
            "allow-agent-settings-update",
            "allow-agent-descriptors-reload",
            "allow-agent-templates-ensure",
            "allow-agent-template-options-get",
            "allow-agent-template-options-set",
            "allow-agent-worktree-propose",
            "allow-agent-tab-open",
            "allow-agent-tab-link-get",
            "allow-agent-tab-close-check",
            "allow-agent-worktree-resolve",
            "allow-agent-worktree-pending-list",
            "allow-agent-telemetry-get",
            "allow-agent-telemetry-list",
            "allow-agent-attention-get",
            "allow-agent-attention-ack",
            "allow-skills-scan",
            "allow-skills-targets-list",
            "allow-skills-target-upsert",
            "allow-skills-target-remove",
            "allow-skills-plan",
            "allow-skills-apply",
            "allow-memory-suggestions",
            "allow-memory-list",
            "allow-memory-pin",
            "allow-memory-hide",
            "allow-memory-forget",
            "allow-memory-rebuild",
            "allow-artisan-resolve-root",
            "allow-artisan-catalog-get",
            "allow-artisan-catalog-refresh",
            "allow-artisan-form-schema",
            "allow-artisan-validate-form",
            "allow-artisan-preview-command",
            "allow-artisan-create-command",
            "allow-artisan-format-maxcode-prompt",
            "allow-shortcut-list",
            "allow-shortcut-resolve",
            "allow-command-group-list",
            "allow-command-group-get",
            "allow-command-group-create",
            "allow-command-group-update",
            "allow-command-group-delete",
            "allow-command-group-reorder",
            "allow-command-group-run",
            "allow-command-group-run-cancel",
            "allow-command-group-run-list",
            "allow-command-group-run-get",
            "allow-command-group-run-item-attach",
            "allow-command-group-run-item-detach",
            "allow-command-group-suggest",
            "allow-command-group-recurring-commands",
            "allow-github-repo-resolve",
            "allow-github-auth-status",
            "allow-github-auth-set-token",
            "allow-github-auth-clear-token",
            "allow-github-issues-list",
            "allow-github-issue-detail",
            "allow-github-issue-counts",
            "allow-github-refresh",
            "allow-github-labels-catalog",
            "allow-github-repo-labels",
            "allow-github-labels-ensure",
            "allow-github-issue-create",
            "allow-github-issue-labels-update",
            "allow-github-issue-send-maxcode",
            "allow-github-issue-worktree-resolve",
            "allow-github-issue-worktree-create",
            "allow-github-issue-open-tab",
            "allow-github-bug-cycle-preview",
            "allow-github-bug-cycle-start",
            "allow-github-bug-cycle-status",
            "allow-search-query",
            "allow-search-cancel",
            "allow-search-release",
            "allow-search-index-stats",
            "allow-search-index-rebuild",
            "allow-backup-create",
            "allow-backup-list",
            "allow-backup-restore-schedule",
            "allow-app-update-check",
            "allow-app-update-install-confirmed",
            "core:event:default",
        ]
    );
    assert!(
        !perm_strings.contains(&"core:default"),
        "A permissão abrangente 'core:default' NUNCA deve ser concedida!"
    );
}

/// Valida o contrato de devUrl e CSP de release conforme CA-30.
#[test]
fn csp_and_dev_url_contract() {
    let conf = load_tauri_conf();

    // devUrl deve apontar estritamente para 127.0.0.1:5177 (porta de dev fixa)
    let dev_url = conf["build"]["devUrl"]
        .as_str()
        .unwrap_or_else(|| panic!("build.devUrl ausente"));
    assert_eq!(dev_url, "http://127.0.0.1:5177");

    // Dimensões da janela principal
    let windows = conf["app"]["windows"]
        .as_array()
        .unwrap_or_else(|| panic!("app.windows ausente"));
    assert_eq!(windows.len(), 1, "Esperada exatamente 1 janela configurada");
    let main_win = &windows[0];
    assert_eq!(main_win["label"], "main");
    assert_eq!(main_win["width"], 1440);
    assert_eq!(main_win["height"], 900);
    assert_eq!(main_win["minWidth"], 960);
    assert_eq!(main_win["minHeight"], 600);

    // Bundle desativado na fundação
    assert_eq!(conf["bundle"]["active"], false);

    // CSP de produção
    let csp = conf["app"]["security"]["csp"]
        .as_str()
        .unwrap_or_else(|| panic!("app.security.csp ausente"));
    assert!(
        csp.contains("default-src 'self'"),
        "CSP de produção deve conter default-src 'self'"
    );
    assert!(
        !csp.contains("127.0.0.1:5177"),
        "CSP de produção NÃO deve conter porta ou URL do servidor de desenvolvimento"
    );
    assert!(
        !csp.contains("unsafe-eval"),
        "CSP de produção NÃO deve conter 'unsafe-eval'"
    );
    assert!(
        !csp.contains("http://") || csp.contains("http://ipc.localhost"),
        "CSP de produção não deve autorizar origens HTTP externas"
    );

    // devCsp de desenvolvimento restrita a 127.0.0.1:5177
    let dev_csp = conf["app"]["security"]["devCsp"]
        .as_str()
        .unwrap_or_else(|| panic!("app.security.devCsp ausente"));
    assert!(dev_csp.contains("http://127.0.0.1:5177"));
    assert!(dev_csp.contains("ws://127.0.0.1:5177"));
}

/// Simula e valida a decisão de autorização da ACL a partir dos artefatos reais gerados
/// pelo `tauri_build::AppManifest` para janelas e comandos (CA-29 da Fase 01 e CA-35 da Fase 02).
#[test]
#[allow(clippy::too_many_lines)]
fn acl_runtime_allows_main_and_denies_unauthorized_window_or_command() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cap = load_capability_default();

    let allowed_windows: HashSet<String> = cap["windows"]
        .as_array()
        .unwrap_or_else(|| panic!("windows deve ser array"))
        .iter()
        .filter_map(|w| w.as_str().map(String::from))
        .collect();

    let allowed_permissions: HashSet<String> = cap["permissions"]
        .as_array()
        .unwrap_or_else(|| panic!("permissions deve ser array"))
        .iter()
        .filter_map(|p| p.as_str().map(String::from))
        .collect();

    let default_perm_toml = fs::read_to_string(manifest_dir.join("permissions/default.toml"))
        .unwrap_or_else(|e| panic!("Falha ao ler permissions/default.toml: {e}"));
    assert!(default_perm_toml.contains("[default]"));

    let expected_commands = [
        ("allow-app-info", "app_info", "app_info.toml"),
        (
            "allow-app-log-frontend",
            "app_log_frontend",
            "app_log_frontend.toml",
        ),
        ("allow-pty-spawn", "pty_spawn", "pty_spawn.toml"),
        ("allow-pty-write", "pty_write", "pty_write.toml"),
        ("allow-pty-resize", "pty_resize", "pty_resize.toml"),
        ("allow-pty-kill", "pty_kill", "pty_kill.toml"),
        ("allow-pty-list", "pty_list", "pty_list.toml"),
        (
            "allow-pty-process-info",
            "pty_process_info",
            "pty_process_info.toml",
        ),
        ("allow-pty-ack", "pty_ack", "pty_ack.toml"),
        ("allow-pty-attach", "pty_attach", "pty_attach.toml"),
        ("allow-pty-detach", "pty_detach", "pty_detach.toml"),
        (
            "allow-db-settings-get",
            "db_settings_get",
            "db_settings_get.toml",
        ),
        (
            "allow-db-settings-set",
            "db_settings_set",
            "db_settings_set.toml",
        ),
        (
            "allow-db-setting-snapshot",
            "db_setting_snapshot",
            "db_setting_snapshot.toml",
        ),
        ("allow-db-status", "db_status", "db_status.toml"),
        ("allow-db-recover", "db_recover", "db_recover.toml"),
        (
            "allow-shell-integration-info",
            "shell_integration_info",
            "shell_integration_info.toml",
        ),
        (
            "allow-command-block-list",
            "command_block_list",
            "command_block_list.toml",
        ),
        (
            "allow-command-block-get",
            "command_block_get",
            "command_block_get.toml",
        ),
        (
            "allow-command-block-error-context",
            "command_block_error_context",
            "command_block_error_context.toml",
        ),
        (
            "allow-command-block-search",
            "command_block_search",
            "command_block_search.toml",
        ),
        (
            "allow-tab-state-list",
            "tab_state_list",
            "tab_state_list.toml",
        ),
        ("allow-tab-state-get", "tab_state_get", "tab_state_get.toml"),
        ("allow-tab-mark-seen", "tab_mark_seen", "tab_mark_seen.toml"),
        ("allow-tab-set-focus", "tab_set_focus", "tab_set_focus.toml"),
        (
            "allow-tab-set-continuous",
            "tab_set_continuous",
            "tab_set_continuous.toml",
        ),
        (
            "allow-project-snapshot",
            "project_snapshot",
            "project_snapshot.toml",
        ),
        ("allow-project-list", "project_list", "project_list.toml"),
        ("allow-project-add", "project_add", "project_add.toml"),
        (
            "allow-project-update",
            "project_update",
            "project_update.toml",
        ),
        (
            "allow-project-remove",
            "project_remove",
            "project_remove.toml",
        ),
        (
            "allow-project-reorder",
            "project_reorder",
            "project_reorder.toml",
        ),
        (
            "allow-project-redetect",
            "project_redetect",
            "project_redetect.toml",
        ),
        (
            "allow-project-preview-identity",
            "project_preview_identity",
            "project_preview_identity.toml",
        ),
        (
            "allow-project-refresh-missing",
            "project_refresh_missing",
            "project_refresh_missing.toml",
        ),
        (
            "allow-project-root-list",
            "project_root_list",
            "project_root_list.toml",
        ),
        (
            "allow-project-root-add",
            "project_root_add",
            "project_root_add.toml",
        ),
        (
            "allow-project-root-update",
            "project_root_update",
            "project_root_update.toml",
        ),
        (
            "allow-project-root-remove",
            "project_root_remove",
            "project_root_remove.toml",
        ),
        (
            "allow-project-root-rescan",
            "project_root_rescan",
            "project_root_rescan.toml",
        ),
        (
            "allow-project-discovery-list",
            "project_discovery_list",
            "project_discovery_list.toml",
        ),
        (
            "allow-project-discovery-accept",
            "project_discovery_accept",
            "project_discovery_accept.toml",
        ),
        (
            "allow-project-discovery-dismiss",
            "project_discovery_dismiss",
            "project_discovery_dismiss.toml",
        ),
        ("allow-template-list", "template_list", "template_list.toml"),
        ("allow-template-get", "template_get", "template_get.toml"),
        (
            "allow-template-create",
            "template_create",
            "template_create.toml",
        ),
        (
            "allow-template-update",
            "template_update",
            "template_update.toml",
        ),
        (
            "allow-template-delete",
            "template_delete",
            "template_delete.toml",
        ),
        (
            "allow-template-set-default",
            "template_set_default",
            "template_set_default.toml",
        ),
        ("allow-tab-list", "tab_list", "tab_list.toml"),
        ("allow-tab-open", "tab_open", "tab_open.toml"),
        ("allow-tab-close", "tab_close", "tab_close.toml"),
        ("allow-tab-reorder", "tab_reorder", "tab_reorder.toml"),
        ("allow-tab-focus", "tab_focus", "tab_focus.toml"),
        ("allow-pane-split", "pane_split", "pane_split.toml"),
        ("allow-pane-close", "pane_close", "pane_close.toml"),
        ("allow-pane-restart", "pane_restart", "pane_restart.toml"),
        (
            "allow-pane-layout-update",
            "pane_layout_update",
            "pane_layout_update.toml",
        ),
        ("allow-session-new", "session_new", "session_new.toml"),
        ("allow-session-list", "session_list", "session_list.toml"),
        ("allow-session-get", "session_get", "session_get.toml"),
        (
            "allow-session-scrollback-get",
            "session_scrollback_get",
            "session_scrollback_get.toml",
        ),
        (
            "allow-session-search",
            "session_search",
            "session_search.toml",
        ),
        (
            "allow-workspace-snapshot",
            "workspace_snapshot",
            "workspace_snapshot.toml",
        ),
        (
            "allow-workspace-initial-command-run",
            "workspace_initial_command_run",
            "workspace_initial_command_run.toml",
        ),
        (
            "allow-git-status-for-path",
            "git_status_for_path",
            "git_status_for_path.toml",
        ),
        (
            "allow-git-status-refresh",
            "git_status_refresh",
            "git_status_refresh.toml",
        ),
        (
            "allow-git-worktree-list",
            "git_worktree_list",
            "git_worktree_list.toml",
        ),
        (
            "allow-git-worktree-create",
            "git_worktree_create",
            "git_worktree_create.toml",
        ),
        (
            "allow-git-worktree-remove",
            "git_worktree_remove",
            "git_worktree_remove.toml",
        ),
        (
            "allow-ports-list-for-tab",
            "ports_list_for_tab",
            "ports_list_for_tab.toml",
        ),
        (
            "allow-ports-open-browser",
            "ports_open_browser",
            "ports_open_browser.toml",
        ),
        ("allow-ports-kill", "ports_kill", "ports_kill.toml"),
        (
            "allow-ports-set-poll-mode",
            "ports_set_poll_mode",
            "ports_set_poll_mode.toml",
        ),
        (
            "allow-os-settings-get",
            "os_settings_get",
            "os_settings_get.toml",
        ),
        (
            "allow-os-settings-update",
            "os_settings_update",
            "os_settings_update.toml",
        ),
        (
            "allow-os-environment-info",
            "os_environment_info",
            "os_environment_info.toml",
        ),
        (
            "allow-os-shortcut-status",
            "os_shortcut_status",
            "os_shortcut_status.toml",
        ),
        (
            "allow-os-shortcut-configure-portal",
            "os_shortcut_configure_portal",
            "os_shortcut_configure_portal.toml",
        ),
        (
            "allow-os-window-toggle",
            "os_window_toggle",
            "os_window_toggle.toml",
        ),
        (
            "allow-os-window-show",
            "os_window_show",
            "os_window_show.toml",
        ),
        (
            "allow-os-window-hide",
            "os_window_hide",
            "os_window_hide.toml",
        ),
        (
            "allow-os-quit-request",
            "os_quit_request",
            "os_quit_request.toml",
        ),
        (
            "allow-os-link-resolve",
            "os_link_resolve",
            "os_link_resolve.toml",
        ),
        ("allow-os-link-open", "os_link_open", "os_link_open.toml"),
        ("allow-os-notify", "os_notify", "os_notify.toml"),
        (
            "allow-os-attention-set",
            "os_attention_set",
            "os_attention_set.toml",
        ),
        (
            "allow-os-get-autostart",
            "os_get_autostart",
            "os_get_autostart.toml",
        ),
        (
            "allow-os-runtime-snapshot",
            "os_runtime_snapshot",
            "os_runtime_snapshot.toml",
        ),
        ("allow-os-ui-ready", "os_ui_ready", "os_ui_ready.toml"),
        (
            "allow-os-set-autostart",
            "os_set_autostart",
            "os_set_autostart.toml",
        ),
        ("allow-theme-list", "theme_list", "theme_list.toml"),
        ("allow-theme-get", "theme_get", "theme_get.toml"),
        (
            "allow-theme-appearance-get",
            "theme_appearance_get",
            "theme_appearance_get.toml",
        ),
        (
            "allow-theme-appearance-update",
            "theme_appearance_update",
            "theme_appearance_update.toml",
        ),
        (
            "allow-theme-import-file",
            "theme_import_file",
            "theme_import_file.toml",
        ),
        (
            "allow-theme-import-dir",
            "theme_import_dir",
            "theme_import_dir.toml",
        ),
        (
            "allow-theme-set-active",
            "theme_set_active",
            "theme_set_active.toml",
        ),
        ("allow-theme-delete", "theme_delete", "theme_delete.toml"),
        ("allow-theme-export", "theme_export", "theme_export.toml"),
        ("allow-bridge-status", "bridge_status", "bridge_status.toml"),
        (
            "allow-bridge-get-settings",
            "bridge_get_settings",
            "bridge_get_settings.toml",
        ),
        (
            "allow-bridge-update-settings",
            "bridge_update_settings",
            "bridge_update_settings.toml",
        ),
        (
            "allow-bridge-inspect-command-error",
            "bridge_inspect_command_error",
            "bridge_inspect_command_error.toml",
        ),
        (
            "allow-bridge-notify-project-selected",
            "bridge_notify_project_selected",
            "bridge_notify_project_selected.toml",
        ),
        (
            "allow-bridge-open-file-in-maxcode",
            "bridge_open_file_in_maxcode",
            "bridge_open_file_in_maxcode.toml",
        ),
        (
            "allow-bridge-reconnect-now",
            "bridge_reconnect_now",
            "bridge_reconnect_now.toml",
        ),
        (
            "allow-bridge-issue-protocol-support",
            "bridge_issue_protocol_support",
            "bridge_issue_protocol_support.toml",
        ),
        (
            "allow-bridge-send-issue-command",
            "bridge_send_issue_command",
            "bridge_send_issue_command.toml",
        ),
        (
            "allow-bridge-bugs-cycle-preview",
            "bridge_bugs_cycle_preview",
            "bridge_bugs_cycle_preview.toml",
        ),
        (
            "allow-bridge-bugs-cycle-start",
            "bridge_bugs_cycle_start",
            "bridge_bugs_cycle_start.toml",
        ),
        ("allow-ai-health", "ai_health", "ai_health.toml"),
        (
            "allow-ai-settings-get",
            "ai_settings_get",
            "ai_settings_get.toml",
        ),
        (
            "allow-ai-settings-update",
            "ai_settings_update",
            "ai_settings_update.toml",
        ),
        (
            "allow-ai-catalog-get",
            "ai_catalog_get",
            "ai_catalog_get.toml",
        ),
        (
            "allow-ai-catalog-refresh",
            "ai_catalog_refresh",
            "ai_catalog_refresh.toml",
        ),
        (
            "allow-ai-context-preview",
            "ai_context_preview",
            "ai_context_preview.toml",
        ),
        (
            "allow-ai-suggest-command",
            "ai_suggest_command",
            "ai_suggest_command.toml",
        ),
        ("allow-ai-cancel", "ai_cancel", "ai_cancel.toml"),
        (
            "allow-ai-classify-risk",
            "ai_classify_risk",
            "ai_classify_risk.toml",
        ),
        (
            "allow-ai-diagnose-block",
            "ai_diagnose_block",
            "ai_diagnose_block.toml",
        ),
        (
            "allow-ai-diagnosis-dismiss",
            "ai_diagnosis_dismiss",
            "ai_diagnosis_dismiss.toml",
        ),
        (
            "allow-ai-rag-list-slugs",
            "ai_rag_list_slugs",
            "ai_rag_list_slugs.toml",
        ),
        ("allow-ai-rag-search", "ai_rag_search", "ai_rag_search.toml"),
        (
            "allow-ai-generate-structured",
            "ai_generate_structured",
            "ai_generate_structured.toml",
        ),
        (
            "allow-ai-history-list",
            "ai_history_list",
            "ai_history_list.toml",
        ),
        (
            "allow-ai-history-clear",
            "ai_history_clear",
            "ai_history_clear.toml",
        ),
        (
            "allow-ai-cache-clear",
            "ai_cache_clear",
            "ai_cache_clear.toml",
        ),
        ("allow-agent-list", "agent_list", "agent_list.toml"),
        (
            "allow-agent-detect-refresh",
            "agent_detect_refresh",
            "agent_detect_refresh.toml",
        ),
        (
            "allow-agent-settings-get",
            "agent_settings_get",
            "agent_settings_get.toml",
        ),
        (
            "allow-agent-settings-update",
            "agent_settings_update",
            "agent_settings_update.toml",
        ),
        (
            "allow-agent-descriptors-reload",
            "agent_descriptors_reload",
            "agent_descriptors_reload.toml",
        ),
        (
            "allow-agent-templates-ensure",
            "agent_templates_ensure",
            "agent_templates_ensure.toml",
        ),
        (
            "allow-agent-template-options-get",
            "agent_template_options_get",
            "agent_template_options_get.toml",
        ),
        (
            "allow-agent-template-options-set",
            "agent_template_options_set",
            "agent_template_options_set.toml",
        ),
        (
            "allow-agent-worktree-propose",
            "agent_worktree_propose",
            "agent_worktree_propose.toml",
        ),
        (
            "allow-agent-tab-open",
            "agent_tab_open",
            "agent_tab_open.toml",
        ),
        (
            "allow-agent-tab-link-get",
            "agent_tab_link_get",
            "agent_tab_link_get.toml",
        ),
        (
            "allow-agent-tab-close-check",
            "agent_tab_close_check",
            "agent_tab_close_check.toml",
        ),
        (
            "allow-agent-worktree-resolve",
            "agent_worktree_resolve",
            "agent_worktree_resolve.toml",
        ),
        (
            "allow-agent-worktree-pending-list",
            "agent_worktree_pending_list",
            "agent_worktree_pending_list.toml",
        ),
        (
            "allow-agent-telemetry-get",
            "agent_telemetry_get",
            "agent_telemetry_get.toml",
        ),
        (
            "allow-agent-telemetry-list",
            "agent_telemetry_list",
            "agent_telemetry_list.toml",
        ),
        (
            "allow-agent-attention-get",
            "agent_attention_get",
            "agent_attention_get.toml",
        ),
        (
            "allow-agent-attention-ack",
            "agent_attention_ack",
            "agent_attention_ack.toml",
        ),
        ("allow-skills-scan", "skills_scan", "skills_scan.toml"),
        (
            "allow-skills-targets-list",
            "skills_targets_list",
            "skills_targets_list.toml",
        ),
        (
            "allow-skills-target-upsert",
            "skills_target_upsert",
            "skills_target_upsert.toml",
        ),
        (
            "allow-skills-target-remove",
            "skills_target_remove",
            "skills_target_remove.toml",
        ),
        ("allow-skills-plan", "skills_plan", "skills_plan.toml"),
        ("allow-skills-apply", "skills_apply", "skills_apply.toml"),
        (
            "allow-memory-suggestions",
            "memory_suggestions",
            "memory_suggestions.toml",
        ),
        ("allow-memory-list", "memory_list", "memory_list.toml"),
        ("allow-memory-pin", "memory_pin", "memory_pin.toml"),
        ("allow-memory-hide", "memory_hide", "memory_hide.toml"),
        ("allow-memory-forget", "memory_forget", "memory_forget.toml"),
        (
            "allow-memory-rebuild",
            "memory_rebuild",
            "memory_rebuild.toml",
        ),
        (
            "allow-artisan-resolve-root",
            "artisan_resolve_root",
            "artisan_resolve_root.toml",
        ),
        (
            "allow-artisan-catalog-get",
            "artisan_catalog_get",
            "artisan_catalog_get.toml",
        ),
        (
            "allow-artisan-catalog-refresh",
            "artisan_catalog_refresh",
            "artisan_catalog_refresh.toml",
        ),
        (
            "allow-artisan-form-schema",
            "artisan_form_schema",
            "artisan_form_schema.toml",
        ),
        (
            "allow-artisan-validate-form",
            "artisan_validate_form",
            "artisan_validate_form.toml",
        ),
        (
            "allow-artisan-preview-command",
            "artisan_preview_command",
            "artisan_preview_command.toml",
        ),
        (
            "allow-artisan-create-command",
            "artisan_create_command",
            "artisan_create_command.toml",
        ),
        (
            "allow-artisan-format-maxcode-prompt",
            "artisan_format_maxcode_prompt",
            "artisan_format_maxcode_prompt.toml",
        ),
        ("allow-shortcut-list", "shortcut_list", "shortcut_list.toml"),
        (
            "allow-shortcut-resolve",
            "shortcut_resolve",
            "shortcut_resolve.toml",
        ),
        (
            "allow-command-group-list",
            "command_group_list",
            "command_group_list.toml",
        ),
        (
            "allow-command-group-get",
            "command_group_get",
            "command_group_get.toml",
        ),
        (
            "allow-command-group-create",
            "command_group_create",
            "command_group_create.toml",
        ),
        (
            "allow-command-group-update",
            "command_group_update",
            "command_group_update.toml",
        ),
        (
            "allow-command-group-delete",
            "command_group_delete",
            "command_group_delete.toml",
        ),
        (
            "allow-command-group-reorder",
            "command_group_reorder",
            "command_group_reorder.toml",
        ),
        (
            "allow-command-group-run",
            "command_group_run",
            "command_group_run.toml",
        ),
        (
            "allow-command-group-run-cancel",
            "command_group_run_cancel",
            "command_group_run_cancel.toml",
        ),
        (
            "allow-command-group-run-list",
            "command_group_run_list",
            "command_group_run_list.toml",
        ),
        (
            "allow-command-group-run-get",
            "command_group_run_get",
            "command_group_run_get.toml",
        ),
        (
            "allow-command-group-run-item-attach",
            "command_group_run_item_attach",
            "command_group_run_item_attach.toml",
        ),
        (
            "allow-command-group-run-item-detach",
            "command_group_run_item_detach",
            "command_group_run_item_detach.toml",
        ),
        (
            "allow-command-group-suggest",
            "command_group_suggest",
            "command_group_suggest.toml",
        ),
        (
            "allow-command-group-recurring-commands",
            "command_group_recurring_commands",
            "command_group_recurring_commands.toml",
        ),
        (
            "allow-github-repo-resolve",
            "github_repo_resolve",
            "github_repo_resolve.toml",
        ),
        (
            "allow-github-auth-status",
            "github_auth_status",
            "github_auth_status.toml",
        ),
        (
            "allow-github-auth-set-token",
            "github_auth_set_token",
            "github_auth_set_token.toml",
        ),
        (
            "allow-github-auth-clear-token",
            "github_auth_clear_token",
            "github_auth_clear_token.toml",
        ),
        (
            "allow-github-issues-list",
            "github_issues_list",
            "github_issues_list.toml",
        ),
        (
            "allow-github-issue-detail",
            "github_issue_detail",
            "github_issue_detail.toml",
        ),
        (
            "allow-github-issue-counts",
            "github_issue_counts",
            "github_issue_counts.toml",
        ),
        (
            "allow-github-refresh",
            "github_refresh",
            "github_refresh.toml",
        ),
        (
            "allow-github-labels-catalog",
            "github_labels_catalog",
            "github_labels_catalog.toml",
        ),
        (
            "allow-github-repo-labels",
            "github_repo_labels",
            "github_repo_labels.toml",
        ),
        (
            "allow-github-labels-ensure",
            "github_labels_ensure",
            "github_labels_ensure.toml",
        ),
        (
            "allow-github-issue-create",
            "github_issue_create",
            "github_issue_create.toml",
        ),
        (
            "allow-github-issue-labels-update",
            "github_issue_labels_update",
            "github_issue_labels_update.toml",
        ),
        (
            "allow-github-issue-send-maxcode",
            "github_issue_send_maxcode",
            "github_issue_send_maxcode.toml",
        ),
        (
            "allow-github-issue-worktree-resolve",
            "github_issue_worktree_resolve",
            "github_issue_worktree_resolve.toml",
        ),
        (
            "allow-github-issue-worktree-create",
            "github_issue_worktree_create",
            "github_issue_worktree_create.toml",
        ),
        (
            "allow-github-issue-open-tab",
            "github_issue_open_tab",
            "github_issue_open_tab.toml",
        ),
        (
            "allow-github-bug-cycle-preview",
            "github_bug_cycle_preview",
            "github_bug_cycle_preview.toml",
        ),
        (
            "allow-github-bug-cycle-start",
            "github_bug_cycle_start",
            "github_bug_cycle_start.toml",
        ),
        (
            "allow-github-bug-cycle-status",
            "github_bug_cycle_status",
            "github_bug_cycle_status.toml",
        ),
        ("allow-search-query", "search_query", "search_query.toml"),
        ("allow-search-cancel", "search_cancel", "search_cancel.toml"),
        (
            "allow-search-release",
            "search_release",
            "search_release.toml",
        ),
        (
            "allow-search-index-stats",
            "search_index_stats",
            "search_index_stats.toml",
        ),
        (
            "allow-search-index-rebuild",
            "search_index_rebuild",
            "search_index_rebuild.toml",
        ),
        ("allow-backup-create", "backup_create", "backup_create.toml"),
        ("allow-backup-list", "backup_list", "backup_list.toml"),
        (
            "allow-backup-restore-schedule",
            "backup_restore_schedule",
            "backup_restore_schedule.toml",
        ),
    ];

    let autogen_dir = manifest_dir.join("permissions/autogenerated/commands");
    let legacy_autogen_dir = manifest_dir.join("permissions/autogenerated");
    let mut permission_to_command = std::collections::HashMap::new();

    for (_, _, file_name) in expected_commands {
        let path = if autogen_dir.join(file_name).is_file() {
            autogen_dir.join(file_name)
        } else {
            legacy_autogen_dir.join(file_name)
        };
        let content =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("Falha ao ler {file_name}: {e}"));
        let mut current_id: Option<String> = None;
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("identifier = \"")
                && let Some(id) = rest.strip_suffix('"')
            {
                current_id = Some(id.to_string());
            } else if let Some(rest) = trimmed.strip_prefix("commands.allow = [\"")
                && let Some(cmd) = rest.strip_suffix("\"]")
                && let Some(id) = current_id.take()
            {
                permission_to_command.insert(id, cmd.to_string());
            }
        }
    }

    for (perm_id, cmd_name, _) in expected_commands {
        assert!(
            default_perm_toml.contains(perm_id),
            "permissions/default.toml deve conter {perm_id}"
        );
        assert_eq!(
            permission_to_command.get(perm_id).map(String::as_str),
            Some(cmd_name)
        );
    }

    let allowed_commands: HashSet<&str> = allowed_permissions
        .iter()
        .filter_map(|p| permission_to_command.get(p.as_str()).map(String::as_str))
        .collect();

    let is_command_allowed = |window: &str, command: &str| -> bool {
        if !allowed_windows.contains(window) {
            return false;
        }
        allowed_commands.contains(command)
    };

    // 1. Na janela 'main', todos os 11 comandos (2 fundação + 9 PTY) são autorizados
    for (_, cmd_name, _) in expected_commands {
        assert!(
            is_command_allowed("main", cmd_name),
            "{cmd_name} deve ser permitido na janela 'main'"
        );
        // 2. Em janela 'secondary' (não autorizada), todos são negados
        assert!(
            !is_command_allowed("secondary", cmd_name),
            "{cmd_name} DEVE ser negado em janela não autorizada ('secondary')"
        );
    }

    // 3. Comandos arbitrários ou de fases futuras são negados em qualquer janela
    assert!(
        !is_command_allowed("main", "db_execute"),
        "Comando não autorizado 'db_execute' deve ser negado"
    );
    assert!(
        !is_command_allowed("main", "unknown_command"),
        "Comando inexistente deve ser negado"
    );
}
