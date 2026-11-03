//! Testes de integração para geração determinística e validação de bindings TypeScript Specta.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::default_trait_access
)]

use std::{fs, path::Path};

/// Exporta bindings para `src/types/bindings.ts` sob demanda (usado por `npm run bindings`).
#[test]
#[ignore = "utilitário de atualização explícita dos bindings"]
fn export() {
    maxterminal_lib::specta::export_bindings()
        .unwrap_or_else(|e| panic!("Falha ao exportar bindings TypeScript: {e}"));
}

/// Valida determinismo de geração dupla, ausência de drift contra o repositório e detecção de mutações.
#[test]
fn bindings_are_up_to_date_and_deterministic() {
    let tmp_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp/bindings");
    fs::create_dir_all(&tmp_dir).unwrap_or_else(|e| {
        panic!("Falha ao criar diretório temporário para testes de bindings: {e}")
    });

    let tmp_file_1 = tmp_dir.join("bindings_run_1.ts");
    let tmp_file_2 = tmp_dir.join("bindings_run_2.ts");

    // 1. Exporta duas vezes seguidas
    maxterminal_lib::specta::export_bindings_to(&tmp_file_1)
        .unwrap_or_else(|e| panic!("Primeira exportação falhou: {e}"));
    maxterminal_lib::specta::export_bindings_to(&tmp_file_2)
        .unwrap_or_else(|e| panic!("Segunda exportação falhou: {e}"));

    let content_1 = fs::read_to_string(&tmp_file_1)
        .unwrap_or_else(|e| panic!("Falha ao ler primeira exportação: {e}"));
    let content_2 = fs::read_to_string(&tmp_file_2)
        .unwrap_or_else(|e| panic!("Falha ao ler segunda exportação: {e}"));

    // 2. Determinismo estrito byte a byte
    assert_eq!(
        content_1, content_2,
        "As duas exportações geraram saídas distintas: determinismo violado!"
    );

    // 3. Ausência de drift contra o arquivo versionado
    let canonical_path = maxterminal_lib::specta::canonical_bindings_path();
    assert!(
        canonical_path.exists(),
        "Arquivo canônico de bindings não encontrado em: {}",
        canonical_path.display()
    );

    let canonical_content = fs::read_to_string(&canonical_path)
        .unwrap_or_else(|e| panic!("Falha ao ler bindings canônicos: {e}"));

    assert_eq!(
        content_1, canonical_content,
        "Drift detectado em src/types/bindings.ts! Execute 'npm run bindings' ou 'cargo run --bin export_bindings'."
    );

    // 4. Detecção de mutação artificial em memória
    let mutated_content = content_1.replace("export type TabSessionId = string;", "");
    assert_ne!(
        content_1, mutated_content,
        "A mutação artificial deve diferir do conteúdo original"
    );

    // Limpeza de arquivos temporários
    let _ = fs::remove_file(tmp_file_1);
    let _ = fs::remove_file(tmp_file_2);
}

/// Valida presença de todos os símbolos exportados obrigatórios conforme CA-17.
#[test]
fn exports_expected_symbols() {
    let canonical_path = maxterminal_lib::specta::canonical_bindings_path();
    let content = fs::read_to_string(&canonical_path)
        .unwrap_or_else(|e| panic!("Falha ao ler bindings canônicos: {e}"));

    // Comandos Fase 01 + Fase 02 + Fase 03
    assert!(
        content.contains("commands = {"),
        "Objeto 'commands' ausente"
    );
    for cmd_sig in [
        "appInfo: () =>",
        "appLogFrontend: (event: FrontendLogEvent) =>",
        "ptySpawn: (spec: SpawnSpec_Deserialize, integration: ShellIntegrationRequest_Deserialize | null) =>",
        "ptyWrite: (ptyId: PtyId, input: PtyInput) =>",
        "ptyResize: (ptyId: PtyId, size: PtySize) =>",
        "ptyKill: (ptyId: PtyId, mode: KillMode | null) =>",
        "ptyList: () =>",
        "ptyProcessInfo: (ptyId: PtyId) =>",
        "ptyAck: (ptyId: PtyId, attachmentId: AttachmentId, consumedBytes: string) =>",
        "ptyAttach: (ptyId: PtyId, cursor: OutputCursor | null, onOutput: Channel<BinaryPtyFrame>) =>",
        "ptyDetach: (ptyId: PtyId, attachmentId: AttachmentId) =>",
        "dbSettingsGet: (keys: string[] | null) =>",
        "dbSettingsSet: (key: string, value: Value, expectedRevision: number | null) =>",
        "dbSettingSnapshot: (key: string) =>",
        "dbStatus: () =>",
        "dbRecover: (action: DbRecoveryAction) =>",
        "shellIntegrationInfo: (ptyId: PtyId) =>",
        "commandBlockList: (query: CommandBlockListQuery_Deserialize) =>",
        "commandBlockGet: (blockId: CommandBlockId) =>",
        "commandBlockErrorContext: (blockId: CommandBlockId) =>",
        "commandBlockSearch: (query: CommandBlockSearchQuery_Deserialize) =>",
        "tabStateList: () =>",
        "tabStateGet: (tabId: TabId) =>",
        "tabMarkSeen: (request: SeenRequest_Deserialize) =>",
        "tabSetFocus: (focus: FocusSnapshot_Deserialize) =>",
        "tabSetContinuous: (tabId: TabId, paneId: PaneId | null, policy: ContinuousPolicy) =>",
        "projectSnapshot: () =>",
        "projectList: () =>",
        "projectAdd: (input: NewProject_Deserialize) =>",
        "projectUpdate: (id: ProjectId, patch: ProjectPatch_Deserialize) =>",
        "projectRemove: (id: ProjectId) =>",
        "projectReorder: (ids: ProjectId[]) =>",
        "projectRedetect: (id: ProjectId) =>",
        "projectPreviewIdentity: (path: string) =>",
        "projectRefreshMissing: () =>",
        "projectRootList: () =>",
        "projectRootAdd: (input: NewProjectRoot) =>",
        "projectRootUpdate: (id: ProjectRootId, patch: ProjectRootPatch_Deserialize) =>",
        "projectRootRemove: (id: ProjectRootId) =>",
        "projectRootRescan: (id: ProjectRootId) =>",
        "projectDiscoveryList: () =>",
        "projectDiscoveryAccept: (paths: string[]) =>",
        "projectDiscoveryDismiss: (paths: string[]) =>",
        "templateList: (projectId: ProjectId) =>",
        "templateGet: (templateId: TemplateId) =>",
        "templateCreate: (projectId: ProjectId, input: TabTemplateInput_Deserialize) =>",
        "templateUpdate: (templateId: TemplateId, expectedRevision: number, input: TabTemplateInput_Deserialize) =>",
        "templateDelete: (templateId: TemplateId, expectedRevision: number) =>",
        "templateSetDefault: (templateId: TemplateId) =>",
        "tabList: (projectId: ProjectId) =>",
        "tabOpen: (request: TabOpenRequest_Deserialize) =>",
        "tabClose: (tabId: TabId, requestId: string) =>",
        "tabReorder: (projectId: ProjectId, orderedTabIds: TabId[]) =>",
        "tabFocus: (tabId: TabId, paneId: PaneId | null) =>",
        "paneSplit: (paneId: PaneId, direction: SplitDirection, expectedLayoutRevision: number, requestId: string) =>",
        "paneClose: (paneId: PaneId, requestId: string) =>",
        "paneRestart: (paneId: PaneId, cwdFallbackToProject: boolean, requestId: string) =>",
        "paneLayoutUpdate: (tabId: TabId, expectedLayoutRevision: number, patches: LayoutSizePatch[]) =>",
        "sessionNew: (tabId: TabId, requestId: string) =>",
        "sessionList: (tabId: TabId, cursor: string | null, limit: number) =>",
        "sessionGet: (tabSessionId: TabSessionId) =>",
        "sessionScrollbackGet: (tabSessionId: TabSessionId, sessionId: SessionId, cursor: string | null, limit: number) =>",
        "sessionSearch: (tabId: TabId, query: string, limit: number) =>",
        "workspaceSnapshot: () =>",
        "workspaceInitialCommandRun: (tabId: TabId, paneId: PaneId, generation: number, requestId: string) =>",
        "commandGroupList: (projectId: ProjectId | null) =>",
        "commandGroupGet: (groupId: CommandGroupId) =>",
        "commandGroupCreate: (input: CommandGroupInput_Deserialize) =>",
        "commandGroupUpdate: (groupId: CommandGroupId, expectedRevision: number, input: CommandGroupInput_Deserialize) =>",
        "commandGroupDelete: (groupId: CommandGroupId, expectedRevision: number) =>",
        "commandGroupReorder: (scope: GroupScope, projectId: ProjectId | null, entries: ReorderGroupEntry[]) =>",
        "commandGroupRun: (request: GroupRunRequest_Deserialize) =>",
        "commandGroupRunCancel: (runId: GroupRunId, itemId: GroupRunItemId | null) =>",
        "commandGroupRunList: (projectId: ProjectId, cursor: string | null, limit: number, activeOnly: boolean | null) =>",
        "commandGroupRunGet: (runId: GroupRunId) =>",
        "commandGroupRunItemAttach: (runId: GroupRunId, itemId: GroupRunItemId, afterSeq: number | null, onFrame: Channel<GroupOutputFrame_Deserialize>) =>",
        "commandGroupRunItemDetach: (subscriptionId: OutputSubscriptionId) =>",
        "commandGroupSuggest: (request: GroupSuggestRequest_Deserialize) =>",
        "commandGroupRecurringCommands: (projectId: ProjectId, limit: number) =>",
    ] {
        assert!(
            content.contains(cmd_sig),
            "Assinatura de comando esperada ausente em bindings.ts: {cmd_sig}"
        );
    }

    // Tipos estruturais Fase 01 + Fase 02 + Fase 03 (CA-03)
    for expected_type in [
        "export type AppInfo = {",
        "export type AppPathsDto = {",
        "export type BuildProfile =",
        "export type AttachInfo = {",
        "export type OutputCursor = {",
        "export type PtySize = {",
        "export type PtySummary =",
        "export type SpawnSpec =",
        "export type PtyExitEvent =",
        "export type PtySpawnedEvent =",
        "export type PtyForegroundChangedEvent =",
        "export type PtyCleanupFailedEvent = {",
        "export type ProcessInfo =",
        "export type PtyProcessReport =",
        "export type U64Decimal = string;",
        "export type SettingSnapshot =",
        "export type SettingChangeOrigin =",
        "export type BackupEntryDto = {",
        "export type DbHealthStateDto =",
        "export type DbStatusDto = {",
        "export type DbRecoveryAction =",
        "export type SettingsChanged =",
        "export type DbRecovered =",
        "export type DbMaintenanceFinished = {",
        "export type DbWriterLag = {",
        "export type ShellKind =",
        "export type ContinuousPolicy =",
        "export type ShellIntegrationRequest =",
        "export type IntegrationMode =",
        "export type ObservationSource =",
        "export type ObservationConfidence =",
        "export type CommandTextQuality =",
        "export type PersistenceState =",
        "export type BlockStatus =",
        "export type BlockEndReason =",
        "export type RuntimeBinding =",
        "export type ExecutionRef =",
        "export type StreamPosition = {",
        "export type TabStatus =",
        "export type CompletionOutcome =",
        "export type CompletionSnapshot =",
        "export type AttentionSnapshot =",
        "export type PaneStateDto =",
        "export type TabStateSnapshot =",
        "export type SeenRequest =",
        "export type FocusSnapshot =",
        "export type OutputKind =",
        "export type CommandStarted =",
        "export type CommandFinished =",
        "export type ErrorContext =",
        "export type CommandBlockDto =",
        "export type CommandBlockListQuery =",
        "export type CommandBlockSearchQuery =",
        "export type CommandBlockHit =",
        "export type Page<T> =",
        "export type IntegrationInfo =",
        "export type TabStateRemovedEvent = {",
        "export type ShellCwdChangedEvent =",
        "export type ShellIntegrationModeEvent =",
        "export type ProjectRootId = string;",
        "export type StackKind =",
        "export type IdentitySource =",
        "export type DiscoveryMode =",
        "export type WatchStatus =",
        "export type ProjectIdentity =",
        "export type Project =",
        "export type ProjectRoot =",
        "export type DiscoveredFolder =",
        "export type RootImportReport =",
        "export type ScanWarning = {",
        "export type ProjectSnapshot =",
        "export type ProjectDiscoveredPayload =",
        "export type RemovedKind =",
        "export type ProjectRemovedPayload =",
        "export type ProjectChangedPayload =",
        "export type ProjectRootChangedPayload =",
        "export type ProjectEvent<T> =",
    ] {
        assert!(
            content.contains(expected_type),
            "Tipo estrutural esperado ausente em bindings.ts: {expected_type}"
        );
    }
    assert!(
        content.contains("\"debug\" |"),
        "Variante 'debug' de BuildProfile ausente"
    );
    assert!(
        content.contains("\"release\";"),
        "Variante 'release' de BuildProfile ausente"
    );

    // Erros e eventos
    assert!(
        content.contains("export type MtError ="),
        "Tipo MtError ausente"
    );
    assert!(
        content.contains("export type ErrorCode = string;"),
        "Tipo ErrorCode ausente ou não mapeado para string"
    );
    assert!(
        content.contains("export type FrontendLogEvent ="),
        "Tipo FrontendLogEvent ausente"
    );
    assert!(
        content.contains("\"vue_error\""),
        "Variante 'vue_error' ausente"
    );
    assert!(
        content.contains("\"window_error\""),
        "Variante 'window_error' ausente"
    );
    assert!(
        content.contains("\"unhandled_rejection\""),
        "Variante 'unhandled_rejection' ausente"
    );
    assert!(
        content.contains("\"bootstrap_failed\""),
        "Variante 'bootstrap_failed' ausente"
    );

    // 9 Identificadores de domínio (todos devem ser mapeados para string)
    let expected_ids = [
        "ProjectId",
        "TabId",
        "PaneId",
        "PtyId",
        "SessionId",
        "TabSessionId",
        "TemplateId",
        "CommandBlockId",
        "CommandGroupId",
    ];

    for id_name in expected_ids {
        let expected_line = format!("export type {id_name} = string;");
        assert!(
            content.contains(&expected_line),
            "ID '{id_name}' deve ser exportado exatamente como '{expected_line}'"
        );
    }

    // Campo 'source' de MtError NUNCA deve ser exposto na API pública/bindings
    let mt_error_idx = content
        .find("export type MtError_Serialize = {")
        .expect("Bloco MtError_Serialize deve existir");
    let mt_error_end = content[mt_error_idx..]
        .find("};")
        .expect("Fechamento de MtError_Serialize deve existir");
    let mt_error_block = &content[mt_error_idx..mt_error_idx + mt_error_end];
    assert!(
        !mt_error_block.contains("source:"),
        "O campo 'source' de MtError NUNCA deve ser exposto no TypeScript!"
    );
}

/// Exercita os comandos IPC `pty_*` via `tauri::test::mock_builder` e comprova a serialização
/// de `MtError` (`{ code, message, details? }`) em `pt-BR` (CA-29 e CA-35).
#[test]
fn pty_ipc_commands_and_mt_error_serialization_via_mock_runtime() {
    use mt_db::{Db, DbConfig};
    use mt_pty::containment::FakeContainmentProvider;
    use mt_pty::proc::{PidfdSignaler, ProcfsSource};
    use mt_pty::{DefaultEnvPolicy, PtyConfig, PtyManager, PtySummary};
    use mt_shell::ShellService;
    use serde_json::json;
    use std::sync::Arc;
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{INVOKE_KEY, get_ipc_response, mock_builder};

    let tmp_dir = std::env::temp_dir().join(format!("mt-ipc-mock-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp_dir);
    fs::create_dir_all(&tmp_dir).unwrap_or_else(|e| panic!("Falha ao criar dir de teste: {e}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp_dir, fs::Permissions::from_mode(0o700));
    }

    let config = PtyConfig {
        registry_dir: tmp_dir.clone(),
        orphan_sweep_on_startup: false,
        ..PtyConfig::default()
    };

    let proc_source = Arc::new(ProcfsSource::new());
    let signaler = Arc::new(PidfdSignaler::new(proc_source.clone()));
    let containment = Arc::new(FakeContainmentProvider::new(tmp_dir.clone(), true));
    let env_policy = Arc::new(DefaultEnvPolicy);

    let manager =
        PtyManager::with_providers(config, containment, proc_source, signaler, env_policy)
            .unwrap_or_else(|e| panic!("Falha ao criar PtyManager para teste IPC: {e}"));

    let db_dir = tmp_dir.join("db");
    fs::create_dir_all(&db_dir).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&db_dir, fs::Permissions::from_mode(0o700));
    }
    let db = Db::open(DbConfig::for_path(db_dir.join("test.db")))
        .unwrap_or_else(|e| panic!("Falha ao abrir Db para teste IPC: {e}"));
    let shell_service = ShellService::new(
        Arc::clone(&manager),
        db.clone(),
        tmp_dir.join("runtime"),
        None,
    );

    let specta_builder = maxterminal_lib::specta::builder_for::<tauri::test::MockRuntime>();
    let app = mock_builder()
        .manage(Arc::clone(&manager))
        .manage(Arc::clone(&shell_service))
        .invoke_handler(specta_builder.invoke_handler())
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| panic!("Falha ao construir mock app Tauri: {e}"));

    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap_or_else(|e| panic!("Falha ao criar WebviewWindow 'main': {e}"));

    let invoke_cmd = |cmd: &str, payload: serde_json::Value| {
        get_ipc_response(
            &webview,
            tauri::webview::InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: InvokeBody::Json(payload),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
    };

    // 1. pty_spawn
    let spawn_res = invoke_cmd(
        "pty_spawn",
        json!({
            "spec": {
                "shell": "/bin/sh",
                "args": ["-c", "sleep 2"],
                "login": false,
                "cwd": "/tmp",
                "env": {},
                "envRemove": [],
                "size": { "rows": 24, "cols": 80, "pixelWidth": 0, "pixelHeight": 0 },
                "owner": { "tabId": null, "paneId": null }
            }
        }),
    )
    .expect("pty_spawn deve ter sucesso");

    let summary: PtySummary = spawn_res
        .deserialize()
        .expect("Resposta de pty_spawn deve deserializar em PtySummary");
    let pty_id_str = summary.id.to_string();

    // 2. pty_list
    let list_res = invoke_cmd("pty_list", json!({})).expect("pty_list deve ter sucesso");
    let listed: Vec<PtySummary> = list_res
        .deserialize()
        .expect("Resposta de pty_list deve deserializar em Vec<PtySummary>");
    assert!(listed.iter().any(|s| s.id == summary.id));

    // 3. pty_process_info
    let info_res = invoke_cmd("pty_process_info", json!({ "ptyId": pty_id_str }))
        .expect("pty_process_info deve ter sucesso");
    let report_val: serde_json::Value = info_res.deserialize().unwrap();
    assert_eq!(report_val["ptyId"], pty_id_str);

    // 4. pty_resize válido e inválido (verificando MtError em pt-BR)
    invoke_cmd(
        "pty_resize",
        json!({
            "ptyId": pty_id_str,
            "size": { "rows": 40, "cols": 120, "pixelWidth": 0, "pixelHeight": 0 }
        }),
    )
    .expect("pty_resize válido deve ter sucesso");

    let invalid_resize_err = invoke_cmd(
        "pty_resize",
        json!({
            "ptyId": pty_id_str,
            "size": { "rows": 0, "cols": 120, "pixelWidth": 0, "pixelHeight": 0 }
        }),
    )
    .expect_err("pty_resize com rows=0 deve falhar");
    assert_eq!(invalid_resize_err["code"], "pty.resize_invalid");
    assert!(
        invalid_resize_err["message"]
            .as_str()
            .unwrap_or_default()
            .contains("inválidas"),
        "Mensagem de erro deve estar em pt-BR: {invalid_resize_err:?}"
    );

    // 5. pty_write
    invoke_cmd(
        "pty_write",
        json!({
            "ptyId": pty_id_str,
            "input": { "kind": "text", "text": "echo ok\n" }
        }),
    )
    .expect("pty_write deve ter sucesso");

    // 6. pty_ack inválido -> erro tipado pty.ack_invalid
    let invalid_ack_err = invoke_cmd(
        "pty_ack",
        json!({
            "ptyId": pty_id_str,
            "attachmentId": "01960000-0000-7000-8000-000000000099",
            "consumedBytes": "not-a-number"
        }),
    )
    .expect_err("pty_ack com valor não numérico deve falhar");
    assert_eq!(invalid_ack_err["code"], "pty.ack_invalid");

    // 7. pty_detach (idempotente para geração não ativa)
    invoke_cmd(
        "pty_detach",
        json!({
            "ptyId": pty_id_str,
            "attachmentId": "01960000-0000-7000-8000-000000000099"
        }),
    )
    .expect("pty_detach idempotente deve ter sucesso");

    // 7b. Comandos IPC da Fase 05 (shell_* e tab_state_*)
    let shell_info_res = invoke_cmd("shell_integration_info", json!({ "ptyId": pty_id_str }))
        .expect("shell_integration_info deve ter sucesso");
    let shell_info_val: serde_json::Value = shell_info_res.deserialize().unwrap();
    assert_eq!(shell_info_val["ptyId"], pty_id_str);

    let tab_list_res =
        invoke_cmd("tab_state_list", json!({})).expect("tab_state_list deve ter sucesso");
    let _tab_list_val: serde_json::Value = tab_list_res.deserialize().unwrap();

    let focus_res = invoke_cmd(
        "tab_set_focus",
        json!({
            "focus": {
                "windowFocused": true,
                "activeTabId": null,
                "activePaneId": null
            }
        }),
    )
    .expect("tab_set_focus deve ter sucesso");
    let _focus_val: serde_json::Value = focus_res.deserialize().unwrap();

    let block_list_res = invoke_cmd("command_block_list", json!({ "query": {} }))
        .expect("command_block_list deve ter sucesso");
    let block_list_val: serde_json::Value = block_list_res.deserialize().unwrap();
    assert!(block_list_val["items"].is_array());

    // 8. pty_kill
    invoke_cmd(
        "pty_kill",
        json!({
            "ptyId": pty_id_str,
            "mode": "force"
        }),
    )
    .expect("pty_kill deve ter sucesso");

    shell_service.shutdown();
    let _ = tauri::async_runtime::block_on(db.shutdown());
    let _ = fs::remove_dir_all(&tmp_dir);
}
