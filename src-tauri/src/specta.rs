//! Configuração central do `tauri-specta` para registro de comandos, tipos de domínio e exportação TypeScript.

use std::path::{Path, PathBuf};

use mt_agents::{
    attention::{
        AckReason, AttentionSnapshot as AgentAttentionSnapshot, AttentionSource, AttentionState,
    },
    detect::{AgentInfo, AgentInstallation},
    isolation::{
        AgentCloseInspection, AgentTabLink, AgentTabOpenRequest, WorktreeIsolation,
        WorktreeProposal, WorktreeResolution,
    },
    launch::AgentTemplateOptions,
    memory::{MemoryEntry, MemoryQuery},
    ports::AttentionKind,
    registry::{
        AgentCapabilities, AgentDescriptor, AgentKind as CliAgentKind, CapabilityEvidence,
        CapabilityReport, CapabilitySupport,
    },
    settings::{
        AgentsSettings, AttentionLevel, AttentionSettings, IsolationSettings, SkillsSettings,
        TelemetrySettings, WorktreeLocationDto,
    },
    skills::{
        ConflictKind, ConflictResolution, SkillCatalog, SkillEntry, SkillRelevance, SkillSelection,
        SkillSyncTarget, SyncAction, SyncMode, SyncPlan, SyncResult, SyncScope,
        watch::SkillsChangedPayload,
    },
    telemetry::{
        AgentTelemetry, ContextUsage, Field, MeasurementQuality, MetricAvailability, MetricKey,
        MetricState, ModelInfo, QuotaWindow, TelemetrySourceKind,
    },
};
use mt_ai::{
    catalog::{AiModel, CatalogSource, Harness, HarnessCatalog, ReasoningLevel},
    client::{
        dialect::{CapabilityState, ChatMessage, GatewayCapabilities, Role, Usage},
        http::{GatewayHealth, GatewayStatus},
    },
    config::{
        AiSelection, AiSettings, ContextSettings, DiagnosisSettings, RagSettings, TimeoutSettings,
    },
    context::{AiContext, OsInfo, RecentCommand},
    diagnose::{DiagnoseRequest, DiagnosisOutcome, DiagnosisSkip, InlineDiagnosis},
    ids::AiRequestId,
    rag::{RagResult, RagSearchRequest, RagSearchResponse, RagSlug},
    repo::{AiHistoryFilter, AiHistoryItem, AiHistoryKind, AiHistoryPage, CacheClearScope},
    risk::{RiskAssessment, RiskLevel, RiskReason},
    structured::{StructuredRequest, StructuredResponse},
    suggest::{AiStreamEvent, AiTrigger, CommandSuggestion, SuggestRequest},
};
use mt_bridge::{
    AgentKind, BridgeConnectionState, BridgeError, BridgeErrorCode, BridgeProjectSummary,
    BridgeProjectSyncedEvent, BridgeSettings, BridgeSettingsPatch, BridgeStatusSnapshot,
    BridgeTerminalOpenedEvent, BugCycleItemStage, BugCycleOverallStatus, BugCycleProgressPayload,
    BugCycleStepStatus, BugsCycleAckPayload, BugsCycleFilters, BugsCycleSkipEntry,
    BugsCycleStartPayload, DiscoveryEndpoint, ErrorPayload, HelloPayload, InspectAckPayload,
    InspectCommandErrorPayload, InspectDispatchResult, InspectErrorCode, InspectFallbackMode,
    IssueAckStatus, IssueActionDispatchOutcome, IssueCommandAckPayload, IssueCommandKind,
    IssueCommandPayload, IssueProtocolSupport, IssueStage, OpenFilePayload,
    OpenTerminalForIssuePayload, PeerRole, PingPayload, PongPayload, ProjectOpenedPayload,
    RpcEnvelope, RpcPayload, TerminalOpenedAckPayload, WelcomePayload,
};
use mt_commands::{
    artisan::{
        generator::form::{
            ArtisanCommandForm, ArtisanCreateResult, ArtisanParam, ArtisanPreview,
            ConsoleRoutesPatch, FieldIssue, IssueSeverity, ParamKind, ScheduleFrequency,
            ScheduleOption, ScheduleSpec, ValidationReport,
        },
        model::{
            ArtisanArgument, ArtisanCatalog, ArtisanCommand, ArtisanOption, ArtisanRoot,
            CatalogErrorInfo, CatalogSource as ArtisanCatalogSource, CatalogUpdatedPayload,
            CommandCreatedPayload, DiscoveryFailedPayload, LaravelLayout, RunnerKind,
        },
    },
    groups::{
        events::{
            GroupChangeOperation, GroupChangedEvent, GroupOutputFrame, GroupOutputFrameKind,
            GroupRunEvent, GroupRunItemEventDelta, OutputAttachReceipt,
        },
        model::{
            CommandGroup, CommandGroupCatalog, CommandGroupInput, CommandGroupItem,
            CommandGroupItemInput, DeleteGroupReceipt, ExecutionMode, GroupItemId, GroupRequestId,
            GroupRun, GroupRunCounts, GroupRunId, GroupRunItem, GroupRunItemId,
            GroupRunItemSnapshot, GroupRunListPage, GroupRunRequest, GroupRunSnapshot,
            GroupRunSummary, GroupScope, ItemStatus, OutputSubscriptionId, ReorderGroupEntry,
            RunStatus, SequentialSeparator, ShellDialect,
        },
        ports::RecurringCommandEntry,
        suggest::{GroupSuggestRequest, GroupSuggestResponse, RecurringCommandsResponse},
    },
    horizon::{
        HorizonAvailability, HorizonClearPreview, HorizonClearResult, HorizonContext,
        HorizonSessionDto, HorizonSnapshot, HorizonStatus, QueueClearCount, QueueScope,
    },
    shortcuts::model::{
        DetectedManager, ManagerEvidence, PackageManager, PartSource, ResolvedShortcut, ShortcutId,
        ShortcutList, ShortcutPart,
    },
};
use mt_core::{
    ErrorCode, MtError,
    app::{AppInfo, BuildProfile, FrontendLogEvent},
    ids::{
        CommandBlockId, CommandGroupId, PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId,
        TemplateId,
    },
    paths::AppPathsDto,
    shell::{
        AttentionSnapshot, BlockEndReason, BlockStatus, CommandBlockDto, CommandBlockHit,
        CommandBlockListQuery, CommandBlockSearchQuery, CommandFinished, CommandStarted,
        CommandTextQuality, CompletionOutcome, CompletionSnapshot, ContinuousPolicy, ErrorContext,
        ExecutionRef, FocusSnapshot, IntegrationInfo, IntegrationMode, ObservationConfidence,
        ObservationSource, OutputKind, PaneStateDto, PersistenceState, RuntimeBinding, SeenRequest,
        ShellCwdChangedEvent, ShellIntegrationModeEvent, ShellIntegrationRequest, ShellKind,
        StreamPosition, TabStateRemovedEvent, TabStateSnapshot, TabStatus,
    },
    workspace::{
        CommandOverride, EnvValue, EnvironmentKind, InitialDispatchState, LaunchSnapshot,
        LayoutSizePatch, PaneCloseOutcome, PaneExitInfo, PaneLayoutNode, PaneRuntimeState,
        PaneView, PasswordCredentialOperation, ScrollbackChunk, ScrollbackChunkKind, SessionDetail,
        SessionEndReason, SessionPaneSummary, SessionSearchHit, SessionSummary, SplitDirection,
        SshAuth, SshAuthInput, TabLifecycle, TabOpenOverrides, TabOpenRequest, TabOrigin,
        TabTemplate, TabTemplateInput, TabView, TemplateConfig, TemplateConnection,
        TemplateConnectionInput, TemplateEnvironment, WorkspaceRestoreFailure,
        WorkspaceRestoreReport, WorkspaceSnapshot,
    },
};
use mt_db::models::{
    BackupEntryDto, DbHealthStateDto, DbMaintenanceFinished, DbRecovered, DbRecoveryAction,
    DbStatusDto, DbWriterLag, RestoreScheduledDto, SettingChangeOrigin, SettingSnapshot,
    SettingsChanged,
};
use mt_db::search::{
    CancelOutcome, ExitFilter, HighlightSegment, RebuildJobHandle, SearchFilters, SearchHit,
    SearchIndexProgressEvent, SearchIndexStat, SearchInvalidatedEvent, SearchRequest,
    SearchResponse, SearchSection, SearchSort, SearchSourceKind, SearchTarget,
};
use mt_git::{
    DegradedReason, GitBranch, GitChanges, GitContext, GitStatusChanged, GitUpstream, GitWorktree,
    RefreshReason, RepoKey, WorktreeCreateRequest, WorktreeEntry, WorktreeKind, WorktreeLocation,
};
use mt_github::{
    error::{GithubError, GithubErrorKind},
    events::{
        GithubAuthChangedPayload, GithubBugCycleProgressEventPayload, GithubIssueUpdatedPayload,
        GithubIssuesChangedPayload, GithubLabelsSyncedPayload, GithubRateLimitPayload,
    },
    model::{
        BugCycleCandidateItem, BugCyclePreview, BugCycleRunState, BugCycleSkippedItem,
        CanonicalLabelGroup, CanonicalLabelSpec, CreateIssueInput, CreateIssueWorktreeRequest,
        ExclusiveLabelChange, ExtractedPlan, GhCliStatus, GithubAuthStatus, GithubRepoIdentity,
        GithubTransportKind, IssueComment, IssueCounts, IssueDetail, IssueFilter, IssueListPage,
        IssueSortOrder, IssueState, IssueStateFilter, IssueSummary, IssueTabDirectoryStrategy,
        IssueTabTargetKind, IssueWorktreeResolution, LabelEnsureReport, LabelUpdatePatch,
        MaxCodeIssueAction, OpenIssueTabReceipt, OpenIssueTabRequest, PlanSource,
        RateLimitSnapshot, RepoLabel, RepoResolution, RestTokenStatus, SendIssueToMaxCodeReceipt,
        SendIssueToMaxCodeRequest, StartBugCycleRequest,
    },
};
use mt_os::{
    AttentionReason, AutostartStatus, CliAction, CloseBehavior, CloseDecision, Compositor,
    DisplayServer, DropDownGeometry, EditorAvailability, EditorKind, EditorSettings,
    FocusTabPayload, FocusTabSource, LinkCandidate, LinkContext, NotificationKind,
    NotificationSettings, NotificationSettingsPatch, NotifyOutcome, NotifyRequest,
    OpenPreferencesPayload, OsEnvironment, OsRuntimeSnapshot, OsSettings, OsSettingsPatch,
    QuakeMode, QuakeSettings, QuakeSettingsPatch, QuitOutcome, QuitSource, ResolvedLink,
    ShortcutBackendKind, ShortcutBackendPreference, ShortcutStatus, ShutdownFailedPayload,
    ShutdownPhase, ShutdownReport, ShutdownStartedPayload, StatusCounters, ToggleDecision,
    ToggleDecisionDto, ToggleSource, TrayVisualState, WindowVisibilityPayload,
};
use mt_ports::{
    IpFamily, KillOutcome, ListeningPort, PollMode, PortAddress, PortClosed, PortId,
    PortKillResult, PortOpened, PortProtocol, PortSnapshot, PortUpdated,
};
use mt_projects::{
    DiscoveredFolder, DiscoveryMode, IdentitySource, NewProject, NewProjectRoot, Project,
    ProjectChangedPayload, ProjectDiscoveredPayload, ProjectIdentity, ProjectPatch,
    ProjectRemovedPayload, ProjectRoot, ProjectRootChangedPayload, ProjectRootId, ProjectRootPatch,
    ProjectSnapshot, RemovedKind, RootImportReport, ScanWarning, StackKind, WatchStatus,
};
use mt_pty::{
    AttachInfo, AttachmentId, ExitInfo, ExitReason, KillMode, OutputCursor, ProcState,
    ProcessIdentity, ProcessInfo, ProcessNode, PtyCleanupFailedEvent, PtyExitEvent,
    PtyForegroundChangedEvent, PtyInput, PtyOwner, PtyPids, PtyProcessReport, PtySize,
    PtySpawnedEvent, PtyStatus, PtySummary, SpawnSpec, U64Decimal,
};
use mt_themes::{
    AnsiPalette, AppearanceSettings, AppearanceSettingsPatch, BackgroundRamp, ColorScale11,
    ColorScheme, CursorStyle, ExportFormat, HexColor, ImportDirOptions, ImportFailure,
    ImportFileOptions, ImportReport, ImportWarning, LibraryChangeReason, MaxUiTokens,
    OverwritePolicy, ResolvedTheme, RgbaColor, SchemeFileFormat, SkipReason, SkippedEntry,
    TerminalUiTokens, ThemeChangedEvent, ThemeError, ThemeId, ThemeLibraryChangedEvent,
    ThemeMetadata, ThemeOrigin, ThemePolarity, ThemeScope, ThemeSummary, WarningCode,
    XtermThemePayload,
};
use specta_typescript::Typescript;
use tauri_specta::{Builder, ErrorHandlingMode, collect_commands, collect_events};

use crate::agents::AgentTabOpened;
use crate::commands::agents::{
    agent_attention_ack, agent_attention_get, agent_descriptors_reload, agent_detect_refresh,
    agent_list, agent_settings_get, agent_settings_update, agent_tab_close_check,
    agent_tab_link_get, agent_tab_open, agent_telemetry_get, agent_telemetry_list,
    agent_template_options_get, agent_template_options_set, agent_templates_ensure,
    agent_worktree_pending_list, agent_worktree_propose, agent_worktree_resolve,
};
use crate::commands::ai::{
    ai_cache_clear, ai_cancel, ai_catalog_get, ai_catalog_refresh, ai_classify_risk,
    ai_context_preview, ai_diagnose_block, ai_diagnosis_dismiss, ai_generate_structured, ai_health,
    ai_history_clear, ai_history_list, ai_rag_list_slugs, ai_rag_search, ai_settings_get,
    ai_settings_update, ai_suggest_command,
};
use crate::commands::app::{app_info, app_log_frontend};
use crate::commands::backup::{backup_create, backup_list, backup_restore_schedule};
use crate::commands::bridge::{
    bridge_bugs_cycle_preview, bridge_bugs_cycle_start, bridge_get_settings,
    bridge_inspect_command_error, bridge_issue_protocol_support, bridge_notify_project_selected,
    bridge_open_file_in_maxcode, bridge_reconnect_now, bridge_send_issue_command, bridge_status,
    bridge_update_settings,
};
use crate::commands::command_groups::{
    command_group_create, command_group_delete, command_group_get, command_group_list,
    command_group_recurring_commands, command_group_reorder, command_group_run,
    command_group_run_cancel, command_group_run_get, command_group_run_item_attach,
    command_group_run_item_detach, command_group_run_list, command_group_suggest,
    command_group_update,
};
use crate::commands::credentials::{
    vault_change_password, vault_get_state, vault_lock, vault_setup, vault_unlock,
};
use crate::credentials::VaultStatus;
use crate::commands::db::{
    db_recover, db_setting_snapshot, db_settings_get, db_settings_set, db_status,
};
use crate::commands::git::{
    git_status_for_path, git_status_refresh, git_worktree_create, git_worktree_list,
    git_worktree_remove,
};
use crate::commands::github::{
    github_auth_clear_token, github_auth_set_token, github_auth_status, github_bug_cycle_preview,
    github_bug_cycle_start, github_bug_cycle_status, github_issue_counts, github_issue_create,
    github_issue_detail, github_issue_labels_update, github_issue_open_tab,
    github_issue_send_maxcode, github_issue_worktree_create, github_issue_worktree_resolve,
    github_issues_list, github_labels_catalog, github_labels_ensure, github_refresh,
    github_repo_labels, github_repo_resolve,
};
use crate::commands::horizon::{
    horizon_clear_pending, horizon_clear_pending_preview, horizon_snapshot, horizon_start,
    horizon_stop,
};
use crate::commands::memory::{
    memory_forget, memory_hide, memory_list, memory_pin, memory_rebuild, memory_suggestions,
};
use crate::commands::os::{
    os_attention_set, os_environment_info, os_get_autostart, os_link_open, os_link_resolve,
    os_notify, os_quit_request, os_runtime_snapshot, os_set_autostart, os_settings_get,
    os_settings_update, os_shortcut_configure_portal, os_shortcut_status, os_ui_ready,
    os_window_hide, os_window_show, os_window_toggle,
};
use crate::commands::panes::{pane_close, pane_layout_update, pane_restart, pane_split};
use crate::commands::ports::{
    ports_kill, ports_list_for_tab, ports_open_browser, ports_set_poll_mode,
};
use crate::commands::projects::{
    project_add, project_discovery_accept, project_discovery_dismiss, project_discovery_list,
    project_list, project_preview_identity, project_redetect, project_refresh_missing,
    project_remove, project_reorder, project_root_add, project_root_list, project_root_remove,
    project_root_rescan, project_root_update, project_snapshot, project_update,
};
use crate::commands::pty::{
    pty_ack, pty_attach, pty_detach, pty_kill, pty_list, pty_process_info, pty_resize, pty_spawn,
    pty_write,
};
use crate::commands::quick_commands::{
    artisan_catalog_get, artisan_catalog_refresh, artisan_create_command, artisan_form_schema,
    artisan_format_maxcode_prompt, artisan_preview_command, artisan_resolve_root,
    artisan_validate_form, shortcut_list, shortcut_resolve,
};
use crate::commands::search::{
    search_cancel, search_index_rebuild, search_index_stats, search_query, search_release,
};
use crate::commands::sessions::{
    session_get, session_list, session_new, session_scrollback_get, session_search,
};
use crate::commands::shell::{
    command_block_error_context, command_block_get, command_block_list, command_block_search,
    shell_integration_info,
};
use crate::commands::skills::{
    skills_apply, skills_plan, skills_scan, skills_target_remove, skills_target_upsert,
    skills_targets_list,
};
use crate::commands::tab_state::{
    tab_mark_seen, tab_set_continuous, tab_set_focus, tab_state_get, tab_state_list,
};
use crate::commands::tabs::{tab_close, tab_focus, tab_list, tab_open, tab_reorder};
use crate::commands::templates::{
    template_create, template_delete, template_get, template_list, template_set_default,
    template_update,
};
use crate::commands::themes::{
    theme_appearance_get, theme_appearance_update, theme_delete, theme_export, theme_get,
    theme_import_dir, theme_import_file, theme_list, theme_set_active,
};
use crate::commands::updater::{app_update_check, app_update_install_confirmed};
use crate::commands::workspace::{workspace_initial_command_run, workspace_snapshot};
use crate::events::agents::{
    AgentAttentionTauriEvent, AgentInstallationsChangedTauriEvent, AgentLinkChangedTauriEvent,
    AgentSettingsChangedTauriEvent, AgentTelemetryTauriEvent, AgentWorktreeClosePendingTauriEvent,
    MemoryUpdatedPayload, MemoryUpdatedTauriEvent, SkillsChangedTauriEvent,
    SkillsSyncCompletedTauriEvent,
};
use crate::events::ai::{
    AiCatalogUpdatedTauriEvent, AiSettingsChangedTauriEvent, AiStatusChangedTauriEvent,
};
use crate::events::bridge::{
    BridgeBugCycleProgressTauriEvent, BridgeErrorTauriEvent, BridgeProjectSyncedTauriEvent,
    BridgeStatusChangedTauriEvent, BridgeTerminalOpenedTauriEvent,
};
use crate::events::command_groups::{
    CommandGroupChangedTauriEvent, CommandGroupRunFinishedTauriEvent,
    CommandGroupRunItemChangedTauriEvent, CommandGroupRunStartedTauriEvent,
};
use crate::events::db::{
    DbMaintenanceFinishedEvent, DbRecoveredEvent, DbWriterLagEvent, SettingsChangedEvent,
};
use crate::events::git::GitStatusChangedTauriEvent;
use crate::events::github::{
    GithubAuthChangedTauriEvent, GithubBugCycleProgressTauriEvent, GithubIssueUpdatedTauriEvent,
    GithubIssuesChangedTauriEvent, GithubLabelsSyncedTauriEvent, GithubRateLimitTauriEvent,
};
use crate::events::os::{
    OsCountersTauriEvent, OsEnvironmentChangedTauriEvent, OsFocusTabTauriEvent,
    OsLinkContextChangedTauriEvent, OsOpenPreferencesTauriEvent, OsSettingsChangedTauriEvent,
    OsShortcutStatusTauriEvent, OsShutdownFailedTauriEvent, OsShutdownStartedTauriEvent,
    OsWindowVisibilityTauriEvent,
};
use crate::events::ports::{PortClosedTauriEvent, PortOpenedTauriEvent, PortUpdatedTauriEvent};
use crate::events::projects::{
    ProjectChangedTauriEvent, ProjectDiscoveredTauriEvent, ProjectRemovedTauriEvent,
    ProjectRootChangedTauriEvent,
};
use crate::events::pty::BinaryPtyFrame;
use crate::events::quick_commands::{
    ArtisanCatalogUpdatedTauriEvent, ArtisanCommandCreatedTauriEvent,
    ArtisanDiscoveryFailedTauriEvent,
};
use crate::events::search::{SearchIndexProgressTauriEvent, SearchInvalidatedTauriEvent};
use crate::events::shell::{
    ShellCommandFinishedEvent, ShellCommandStartedEvent, ShellCwdChangedTauriEvent,
    ShellIntegrationModeTauriEvent, TabStateChangedEvent, TabStateRemovedTauriEvent,
};
use crate::events::themes::{ThemeChangedTauriEvent, ThemeLibraryChangedTauriEvent};
use crate::events::workspace::{
    PaneExitedEvent, PaneExitedPayload, PanePtyReplacedEvent, PanePtyReplacedPayload,
    SessionArchivedEvent, SessionStartedEvent, TabClosedEvent, TabClosedPayload,
    TabLayoutChangedEvent, TabOpenedEvent, TabReorderedEvent, TabReorderedPayload,
    TemplateChangedEvent, TemplateChangedPayload, WorkspaceRestoredEvent,
};
#[cfg(debug_assertions)]
use crate::perf::perf_snapshot;
use crate::perf::{PerfSnapshotDto, PtyFlowCountersDto, SpanSummaryDto};
use crate::updater::{UpdateCapabilityDto, UpdateCheckResultDto};

macro_rules! register_all_commands {
    ($builder:expr $(, $extra:ident)*) => {
        $builder.commands(collect_commands![
            app_info,
            app_log_frontend,
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill,
            pty_list,
            pty_process_info,
            pty_ack,
            pty_attach,
            pty_detach,
            db_settings_get,
            db_settings_set,
            db_setting_snapshot,
            db_status,
            db_recover,
            shell_integration_info,
            command_block_list,
            command_block_get,
            command_block_error_context,
            command_block_search,
            tab_state_list,
            tab_state_get,
            tab_mark_seen,
            tab_set_focus,
            tab_set_continuous,
            project_snapshot,
            project_list,
            project_add,
            project_update,
            project_remove,
            project_reorder,
            project_redetect,
            project_preview_identity,
            project_refresh_missing,
            project_root_list,
            project_root_add,
            project_root_update,
            project_root_remove,
            project_root_rescan,
            project_discovery_list,
            project_discovery_accept,
            project_discovery_dismiss,
            template_list,
            template_get,
            template_create,
            template_update,
            template_delete,
            template_set_default,
            tab_list,
            tab_open,
            tab_close,
            tab_reorder,
            tab_focus,
            pane_split,
            pane_close,
            pane_restart,
            pane_layout_update,
            session_new,
            session_list,
            session_get,
            session_scrollback_get,
            session_search,
            workspace_snapshot,
            workspace_initial_command_run,
            git_status_for_path,
            git_status_refresh,
            git_worktree_list,
            git_worktree_create,
            git_worktree_remove,
            ports_list_for_tab,
            ports_open_browser,
            ports_kill,
            ports_set_poll_mode,
            os_settings_get,
            os_settings_update,
            os_environment_info,
            os_shortcut_status,
            os_shortcut_configure_portal,
            os_window_toggle,
            os_window_show,
            os_window_hide,
            os_quit_request,
            os_link_resolve,
            os_link_open,
            os_notify,
            os_attention_set,
            os_get_autostart,
            os_runtime_snapshot,
            os_ui_ready,
            os_set_autostart,
            theme_list,
            theme_get,
            theme_appearance_get,
            theme_appearance_update,
            theme_import_file,
            theme_import_dir,
            theme_set_active,
            theme_delete,
            theme_export,
            bridge_status,
            bridge_get_settings,
            bridge_update_settings,
            bridge_inspect_command_error,
            bridge_notify_project_selected,
            bridge_open_file_in_maxcode,
            bridge_reconnect_now,
            bridge_issue_protocol_support,
            bridge_send_issue_command,
            bridge_bugs_cycle_preview,
            bridge_bugs_cycle_start,
            ai_health,
            ai_settings_get,
            ai_settings_update,
            ai_catalog_get,
            ai_catalog_refresh,
            ai_context_preview,
            ai_suggest_command,
            ai_cancel,
            ai_classify_risk,
            ai_diagnose_block,
            ai_diagnosis_dismiss,
            ai_rag_list_slugs,
            ai_rag_search,
            ai_generate_structured,
            ai_history_list,
            ai_history_clear,
            ai_cache_clear,
            agent_list,
            agent_detect_refresh,
            agent_settings_get,
            agent_settings_update,
            agent_descriptors_reload,
            agent_templates_ensure,
            agent_template_options_get,
            agent_template_options_set,
            agent_worktree_propose,
            agent_tab_open,
            agent_tab_link_get,
            agent_tab_close_check,
            agent_worktree_resolve,
            agent_worktree_pending_list,
            agent_telemetry_get,
            agent_telemetry_list,
            agent_attention_get,
            agent_attention_ack,
            skills_scan,
            skills_targets_list,
            skills_target_upsert,
            skills_target_remove,
            skills_plan,
            skills_apply,
            memory_suggestions,
            memory_list,
            memory_pin,
            memory_hide,
            memory_forget,
            memory_rebuild,
            artisan_resolve_root,
            artisan_catalog_get,
            artisan_catalog_refresh,
            artisan_form_schema,
            artisan_validate_form,
            artisan_preview_command,
            artisan_create_command,
            artisan_format_maxcode_prompt,
            shortcut_list,
            shortcut_resolve,
            command_group_list,
            command_group_get,
            command_group_create,
            command_group_update,
            command_group_delete,
            command_group_reorder,
            command_group_run,
            command_group_run_cancel,
            command_group_run_list,
            command_group_run_get,
            command_group_run_item_attach,
            command_group_run_item_detach,
            command_group_suggest,
            command_group_recurring_commands,
            github_repo_resolve,
            github_auth_status,
            github_auth_set_token,
            github_auth_clear_token,
            github_issues_list,
            github_issue_detail,
            github_issue_counts,
            github_refresh,
            github_labels_catalog,
            github_repo_labels,
            github_labels_ensure,
            github_issue_create,
            github_issue_labels_update,
            github_issue_send_maxcode,
            github_issue_worktree_resolve,
            github_issue_worktree_create,
            github_issue_open_tab,
            github_bug_cycle_preview,
            github_bug_cycle_start,
            github_bug_cycle_status,
            search_query,
            search_cancel,
            search_release,
            search_index_stats,
            search_index_rebuild,
            backup_create,
            backup_list,
            backup_restore_schedule,
            app_update_check,
            app_update_install_confirmed,
            vault_get_state,
            vault_setup,
            vault_unlock,
            vault_lock,
            vault_change_password,
            horizon_snapshot,
            horizon_start,
            horizon_stop,
            horizon_clear_pending_preview,
            horizon_clear_pending
            $(, $extra)*
        ])
    };
}

/// Constrói o `tauri_specta::Builder<R>` parametrizado pelo runtime Tauri (`Wry` ou `MockRuntime`)
/// com todos os comandos, eventos e tipos das Fases 01 a 17.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn builder_for<R: tauri::Runtime>() -> Builder<R> {
    let base = Builder::<R>::new()
        .error_handling(ErrorHandlingMode::Result)
        .dangerously_cast_bigints_to_number();
    #[cfg(debug_assertions)]
    let with_cmds = register_all_commands!(base, perf_snapshot);
    #[cfg(not(debug_assertions))]
    let with_cmds = register_all_commands!(base);
    with_cmds
        .events(collect_events![
            SettingsChangedEvent,
            DbRecoveredEvent,
            DbMaintenanceFinishedEvent,
            DbWriterLagEvent,
            TabStateChangedEvent,
            TabStateRemovedTauriEvent,
            ShellCommandStartedEvent,
            ShellCommandFinishedEvent,
            ShellCwdChangedTauriEvent,
            ShellIntegrationModeTauriEvent,
            ProjectDiscoveredTauriEvent,
            ProjectRemovedTauriEvent,
            ProjectChangedTauriEvent,
            ProjectRootChangedTauriEvent,
            TabOpenedEvent,
            TabLayoutChangedEvent,
            TabClosedEvent,
            TabReorderedEvent,
            PanePtyReplacedEvent,
            PaneExitedEvent,
            SessionStartedEvent,
            SessionArchivedEvent,
            TemplateChangedEvent,
            WorkspaceRestoredEvent,
            GitStatusChangedTauriEvent,
            PortOpenedTauriEvent,
            PortUpdatedTauriEvent,
            PortClosedTauriEvent,
            OsFocusTabTauriEvent,
            OsWindowVisibilityTauriEvent,
            OsSettingsChangedTauriEvent,
            OsShortcutStatusTauriEvent,
            OsCountersTauriEvent,
            OsOpenPreferencesTauriEvent,
            OsShutdownStartedTauriEvent,
            OsShutdownFailedTauriEvent,
            OsEnvironmentChangedTauriEvent,
            OsLinkContextChangedTauriEvent,
            ThemeChangedTauriEvent,
            ThemeLibraryChangedTauriEvent,
            BridgeStatusChangedTauriEvent,
            BridgeProjectSyncedTauriEvent,
            BridgeTerminalOpenedTauriEvent,
            BridgeBugCycleProgressTauriEvent,
            BridgeErrorTauriEvent,
            AiStatusChangedTauriEvent,
            AiCatalogUpdatedTauriEvent,
            AiSettingsChangedTauriEvent,
            AgentInstallationsChangedTauriEvent,
            AgentSettingsChangedTauriEvent,
            AgentTelemetryTauriEvent,
            AgentAttentionTauriEvent,
            AgentLinkChangedTauriEvent,
            AgentWorktreeClosePendingTauriEvent,
            SkillsChangedTauriEvent,
            SkillsSyncCompletedTauriEvent,
            MemoryUpdatedTauriEvent,
            ArtisanCatalogUpdatedTauriEvent,
            ArtisanDiscoveryFailedTauriEvent,
            ArtisanCommandCreatedTauriEvent,
            CommandGroupChangedTauriEvent,
            CommandGroupRunStartedTauriEvent,
            CommandGroupRunItemChangedTauriEvent,
            CommandGroupRunFinishedTauriEvent,
            GithubIssuesChangedTauriEvent,
            GithubIssueUpdatedTauriEvent,
            GithubLabelsSyncedTauriEvent,
            GithubAuthChangedTauriEvent,
            GithubBugCycleProgressTauriEvent,
            GithubRateLimitTauriEvent,
            SearchIndexProgressTauriEvent,
            SearchInvalidatedTauriEvent,
        ])
        .typ::<ProjectId>()
        .typ::<TabId>()
        .typ::<PaneId>()
        .typ::<PtyId>()
        .typ::<SessionId>()
        .typ::<TabSessionId>()
        .typ::<TemplateId>()
        .typ::<CommandBlockId>()
        .typ::<CommandGroupId>()
        .typ::<ErrorCode>()
        .typ::<MtError>()
        .typ::<AppPathsDto>()
        .typ::<BuildProfile>()
        .typ::<AppInfo>()
        .typ::<FrontendLogEvent>()
        .typ::<U64Decimal>()
        .typ::<BinaryPtyFrame>()
        .typ::<SpawnSpec>()
        .typ::<PtyOwner>()
        .typ::<PtySize>()
        .typ::<PtyInput>()
        .typ::<KillMode>()
        .typ::<ExitReason>()
        .typ::<ExitInfo>()
        .typ::<PtyStatus>()
        .typ::<PtyPids>()
        .typ::<PtySummary>()
        .typ::<AttachmentId>()
        .typ::<OutputCursor>()
        .typ::<AttachInfo>()
        .typ::<ProcState>()
        .typ::<ProcessIdentity>()
        .typ::<ProcessInfo>()
        .typ::<ProcessNode>()
        .typ::<PtyProcessReport>()
        .typ::<PtySpawnedEvent>()
        .typ::<PtyForegroundChangedEvent>()
        .typ::<PtyExitEvent>()
        .typ::<PtyCleanupFailedEvent>()
        .typ::<SettingSnapshot>()
        .typ::<SettingChangeOrigin>()
        .typ::<BackupEntryDto>()
        .typ::<DbHealthStateDto>()
        .typ::<DbStatusDto>()
        .typ::<DbRecoveryAction>()
        .typ::<SettingsChanged>()
        .typ::<DbRecovered>()
        .typ::<DbMaintenanceFinished>()
        .typ::<DbWriterLag>()
        .typ::<ShellKind>()
        .typ::<ContinuousPolicy>()
        .typ::<ShellIntegrationRequest>()
        .typ::<IntegrationMode>()
        .typ::<ObservationSource>()
        .typ::<ObservationConfidence>()
        .typ::<CommandTextQuality>()
        .typ::<PersistenceState>()
        .typ::<BlockStatus>()
        .typ::<BlockEndReason>()
        .typ::<RuntimeBinding>()
        .typ::<ExecutionRef>()
        .typ::<StreamPosition>()
        .typ::<TabStatus>()
        .typ::<CompletionOutcome>()
        .typ::<CompletionSnapshot>()
        .typ::<AttentionSnapshot>()
        .typ::<PaneStateDto>()
        .typ::<TabStateSnapshot>()
        .typ::<SeenRequest>()
        .typ::<FocusSnapshot>()
        .typ::<OutputKind>()
        .typ::<CommandStarted>()
        .typ::<CommandFinished>()
        .typ::<ErrorContext>()
        .typ::<CommandBlockDto>()
        .typ::<CommandBlockListQuery>()
        .typ::<CommandBlockSearchQuery>()
        .typ::<CommandBlockHit>()
        .typ::<IntegrationInfo>()
        .typ::<TabStateRemovedEvent>()
        .typ::<ShellCwdChangedEvent>()
        .typ::<ShellIntegrationModeEvent>()
        .typ::<ProjectRootId>()
        .typ::<StackKind>()
        .typ::<IdentitySource>()
        .typ::<DiscoveryMode>()
        .typ::<WatchStatus>()
        .typ::<ProjectIdentity>()
        .typ::<Project>()
        .typ::<ProjectRoot>()
        .typ::<NewProject>()
        .typ::<ProjectPatch>()
        .typ::<NewProjectRoot>()
        .typ::<ProjectRootPatch>()
        .typ::<DiscoveredFolder>()
        .typ::<ScanWarning>()
        .typ::<RootImportReport>()
        .typ::<ProjectSnapshot>()
        .typ::<ProjectDiscoveredPayload>()
        .typ::<RemovedKind>()
        .typ::<ProjectRemovedPayload>()
        .typ::<ProjectChangedPayload>()
        .typ::<ProjectRootChangedPayload>()
        .typ::<EnvironmentKind>()
        .typ::<TemplateEnvironment>()
        .typ::<EnvValue>()
        .typ::<CommandOverride>()
        .typ::<TabOpenOverrides>()
        .typ::<TabOpenRequest>()
        .typ::<TemplateConfig>()
        .typ::<TemplateConnection>()
        .typ::<SshAuth>()
        .typ::<PasswordCredentialOperation>()
        .typ::<SshAuthInput>()
        .typ::<TemplateConnectionInput>()
        .typ::<VaultStatus>()
        .typ::<TabTemplateInput>()
        .typ::<TabTemplate>()
        .typ::<LaunchSnapshot>()
        .typ::<SplitDirection>()
        .typ::<PaneLayoutNode>()
        .typ::<LayoutSizePatch>()
        .typ::<PaneRuntimeState>()
        .typ::<PaneExitInfo>()
        .typ::<PaneView>()
        .typ::<TabLifecycle>()
        .typ::<TabOrigin>()
        .typ::<InitialDispatchState>()
        .typ::<TabView>()
        .typ::<PaneCloseOutcome>()
        .typ::<WorkspaceRestoreFailure>()
        .typ::<WorkspaceRestoreReport>()
        .typ::<WorkspaceSnapshot>()
        .typ::<SessionEndReason>()
        .typ::<SessionSummary>()
        .typ::<SessionPaneSummary>()
        .typ::<SessionDetail>()
        .typ::<ScrollbackChunkKind>()
        .typ::<ScrollbackChunk>()
        .typ::<SessionSearchHit>()
        .typ::<TabClosedPayload>()
        .typ::<TabReorderedPayload>()
        .typ::<PanePtyReplacedPayload>()
        .typ::<PaneExitedPayload>()
        .typ::<TemplateChangedPayload>()
        .typ::<RepoKey>()
        .typ::<GitContext>()
        .typ::<GitBranch>()
        .typ::<GitChanges>()
        .typ::<GitUpstream>()
        .typ::<WorktreeKind>()
        .typ::<GitWorktree>()
        .typ::<DegradedReason>()
        .typ::<WorktreeEntry>()
        .typ::<WorktreeCreateRequest>()
        .typ::<WorktreeLocation>()
        .typ::<RefreshReason>()
        .typ::<GitStatusChanged>()
        .typ::<IpFamily>()
        .typ::<PortProtocol>()
        .typ::<PortId>()
        .typ::<PortAddress>()
        .typ::<ListeningPort>()
        .typ::<KillOutcome>()
        .typ::<PortKillResult>()
        .typ::<PollMode>()
        .typ::<PortSnapshot>()
        .typ::<PortOpened>()
        .typ::<PortUpdated>()
        .typ::<PortClosed>()
        .typ::<CloseBehavior>()
        .typ::<QuakeMode>()
        .typ::<QuakeSettings>()
        .typ::<QuakeSettingsPatch>()
        .typ::<EditorKind>()
        .typ::<EditorAvailability>()
        .typ::<EditorSettings>()
        .typ::<NotificationSettings>()
        .typ::<NotificationSettingsPatch>()
        .typ::<OsSettings>()
        .typ::<OsSettingsPatch>()
        .typ::<DisplayServer>()
        .typ::<Compositor>()
        .typ::<OsEnvironment>()
        .typ::<StatusCounters>()
        .typ::<AttentionReason>()
        .typ::<TrayVisualState>()
        .typ::<CloseDecision>()
        .typ::<QuitSource>()
        .typ::<ShutdownPhase>()
        .typ::<QuitOutcome>()
        .typ::<ShutdownReport>()
        .typ::<ToggleDecision>()
        .typ::<ToggleDecisionDto>()
        .typ::<DropDownGeometry>()
        .typ::<ToggleSource>()
        .typ::<WindowVisibilityPayload>()
        .typ::<ShortcutBackendKind>()
        .typ::<ShortcutBackendPreference>()
        .typ::<ShortcutStatus>()
        .typ::<CliAction>()
        .typ::<LinkCandidate>()
        .typ::<ResolvedLink>()
        .typ::<LinkContext>()
        .typ::<NotificationKind>()
        .typ::<NotifyRequest>()
        .typ::<NotifyOutcome>()
        .typ::<AutostartStatus>()
        .typ::<FocusTabSource>()
        .typ::<FocusTabPayload>()
        .typ::<OpenPreferencesPayload>()
        .typ::<ShutdownStartedPayload>()
        .typ::<ShutdownFailedPayload>()
        .typ::<OsRuntimeSnapshot>()
        .typ::<ThemeId>()
        .typ::<ThemeOrigin>()
        .typ::<ThemePolarity>()
        .typ::<HexColor>()
        .typ::<RgbaColor>()
        .typ::<CursorStyle>()
        .typ::<AnsiPalette>()
        .typ::<ThemeMetadata>()
        .typ::<ColorScheme>()
        .typ::<ColorScale11>()
        .typ::<BackgroundRamp>()
        .typ::<TerminalUiTokens>()
        .typ::<MaxUiTokens>()
        .typ::<XtermThemePayload>()
        .typ::<ResolvedTheme>()
        .typ::<SchemeFileFormat>()
        .typ::<WarningCode>()
        .typ::<ImportWarning>()
        .typ::<OverwritePolicy>()
        .typ::<ImportFileOptions>()
        .typ::<ImportDirOptions>()
        .typ::<SkipReason>()
        .typ::<SkippedEntry>()
        .typ::<ImportFailure>()
        .typ::<ThemeSummary>()
        .typ::<ImportReport>()
        .typ::<ExportFormat>()
        .typ::<AppearanceSettings>()
        .typ::<AppearanceSettingsPatch>()
        .typ::<ThemeScope>()
        .typ::<ThemeChangedEvent>()
        .typ::<LibraryChangeReason>()
        .typ::<ThemeLibraryChangedEvent>()
        .typ::<ThemeError>()
        .typ::<BridgeErrorCode>()
        .typ::<BridgeError>()
        .typ::<InspectFallbackMode>()
        .typ::<BridgeSettings>()
        .typ::<BridgeSettingsPatch>()
        .typ::<DiscoveryEndpoint>()
        .typ::<PeerRole>()
        .typ::<AgentKind>()
        .typ::<HelloPayload>()
        .typ::<WelcomePayload>()
        .typ::<PingPayload>()
        .typ::<PongPayload>()
        .typ::<ProjectOpenedPayload>()
        .typ::<OpenFilePayload>()
        .typ::<InspectErrorCode>()
        .typ::<InspectCommandErrorPayload>()
        .typ::<InspectAckPayload>()
        .typ::<OpenTerminalForIssuePayload>()
        .typ::<TerminalOpenedAckPayload>()
        .typ::<ErrorPayload>()
        .typ::<RpcPayload>()
        .typ::<RpcEnvelope>()
        .typ::<BridgeConnectionState>()
        .typ::<BridgeStatusSnapshot>()
        .typ::<BridgeProjectSummary>()
        .typ::<BridgeProjectSyncedEvent>()
        .typ::<BridgeTerminalOpenedEvent>()
        .typ::<InspectDispatchResult>()
        .typ::<IssueCommandKind>()
        .typ::<IssueStage>()
        .typ::<IssueCommandPayload>()
        .typ::<IssueAckStatus>()
        .typ::<IssueCommandAckPayload>()
        .typ::<BugsCycleFilters>()
        .typ::<BugsCycleStartPayload>()
        .typ::<BugsCycleSkipEntry>()
        .typ::<BugsCycleAckPayload>()
        .typ::<BugCycleOverallStatus>()
        .typ::<BugCycleItemStage>()
        .typ::<BugCycleStepStatus>()
        .typ::<BugCycleProgressPayload>()
        .typ::<IssueActionDispatchOutcome>()
        .typ::<IssueProtocolSupport>()
        .typ::<AiRequestId>()
        .typ::<CapabilityState>()
        .typ::<GatewayCapabilities>()
        .typ::<Role>()
        .typ::<ChatMessage>()
        .typ::<Usage>()
        .typ::<GatewayStatus>()
        .typ::<GatewayHealth>()
        .typ::<CatalogSource>()
        .typ::<ReasoningLevel>()
        .typ::<AiModel>()
        .typ::<Harness>()
        .typ::<HarnessCatalog>()
        .typ::<AiSelection>()
        .typ::<DiagnosisSettings>()
        .typ::<RagSettings>()
        .typ::<ContextSettings>()
        .typ::<TimeoutSettings>()
        .typ::<AiSettings>()
        .typ::<OsInfo>()
        .typ::<RecentCommand>()
        .typ::<AiContext>()
        .typ::<RiskLevel>()
        .typ::<RiskReason>()
        .typ::<RiskAssessment>()
        .typ::<AiTrigger>()
        .typ::<SuggestRequest>()
        .typ::<CommandSuggestion>()
        .typ::<AiStreamEvent>()
        .typ::<InlineDiagnosis>()
        .typ::<DiagnosisSkip>()
        .typ::<DiagnosisOutcome>()
        .typ::<DiagnoseRequest>()
        .typ::<RagSlug>()
        .typ::<RagSearchRequest>()
        .typ::<RagResult>()
        .typ::<RagSearchResponse>()
        .typ::<StructuredRequest>()
        .typ::<StructuredResponse>()
        .typ::<AiHistoryKind>()
        .typ::<CacheClearScope>()
        .typ::<AiHistoryFilter>()
        .typ::<AiHistoryItem>()
        .typ::<AiHistoryPage>()
        .typ::<CliAgentKind>()
        .typ::<AgentCapabilities>()
        .typ::<AgentDescriptor>()
        .typ::<AgentInstallation>()
        .typ::<CapabilitySupport>()
        .typ::<CapabilityEvidence>()
        .typ::<CapabilityReport>()
        .typ::<AgentInfo>()
        .typ::<WorktreeLocationDto>()
        .typ::<IsolationSettings>()
        .typ::<AttentionLevel>()
        .typ::<AttentionSettings>()
        .typ::<TelemetrySettings>()
        .typ::<AgentsSettings>()
        .typ::<SkillsSettings>()
        .typ::<AgentTemplateOptions>()
        .typ::<WorktreeIsolation>()
        .typ::<WorktreeProposal>()
        .typ::<AgentTabOpenRequest>()
        .typ::<WorktreeResolution>()
        .typ::<AgentTabLink>()
        .typ::<AgentCloseInspection>()
        .typ::<AgentTabOpened>()
        .typ::<TelemetrySourceKind>()
        .typ::<MeasurementQuality>()
        .typ::<MetricAvailability>()
        .typ::<MetricKey>()
        .typ::<MetricState>()
        .typ::<ModelInfo>()
        .typ::<ContextUsage>()
        .typ::<QuotaWindow>()
        .typ::<Field<ModelInfo>>()
        .typ::<Field<ContextUsage>>()
        .typ::<Field<QuotaWindow>>()
        .typ::<Field<f64>>()
        .typ::<AgentTelemetry>()
        .typ::<AttentionKind>()
        .typ::<AttentionSource>()
        .typ::<AttentionState>()
        .typ::<AckReason>()
        .typ::<AgentAttentionSnapshot>()
        .typ::<SkillRelevance>()
        .typ::<SkillEntry>()
        .typ::<SkillCatalog>()
        .typ::<SyncScope>()
        .typ::<SyncMode>()
        .typ::<SkillSelection>()
        .typ::<SkillSyncTarget>()
        .typ::<ConflictKind>()
        .typ::<SyncAction>()
        .typ::<SyncPlan>()
        .typ::<ConflictResolution>()
        .typ::<SyncResult>()
        .typ::<SkillsChangedPayload>()
        .typ::<MemoryEntry>()
        .typ::<MemoryQuery>()
        .typ::<MemoryUpdatedPayload>()
        .typ::<ArtisanCommand>()
        .typ::<ArtisanOption>()
        .typ::<ArtisanArgument>()
        .typ::<RunnerKind>()
        .typ::<LaravelLayout>()
        .typ::<ArtisanCatalogSource>()
        .typ::<ArtisanRoot>()
        .typ::<CatalogErrorInfo>()
        .typ::<ArtisanCatalog>()
        .typ::<CatalogUpdatedPayload>()
        .typ::<DiscoveryFailedPayload>()
        .typ::<CommandCreatedPayload>()
        .typ::<ArtisanCommandForm>()
        .typ::<ParamKind>()
        .typ::<ArtisanParam>()
        .typ::<ScheduleSpec>()
        .typ::<ScheduleOption>()
        .typ::<ScheduleFrequency>()
        .typ::<IssueSeverity>()
        .typ::<FieldIssue>()
        .typ::<ValidationReport>()
        .typ::<ConsoleRoutesPatch>()
        .typ::<ArtisanPreview>()
        .typ::<ArtisanCreateResult>()
        .typ::<ShortcutId>()
        .typ::<PackageManager>()
        .typ::<ManagerEvidence>()
        .typ::<DetectedManager>()
        .typ::<PartSource>()
        .typ::<ShortcutPart>()
        .typ::<ResolvedShortcut>()
        .typ::<ShortcutList>()
        .typ::<GroupItemId>()
        .typ::<GroupRunId>()
        .typ::<GroupRunItemId>()
        .typ::<GroupRequestId>()
        .typ::<OutputSubscriptionId>()
        .typ::<GroupScope>()
        .typ::<ExecutionMode>()
        .typ::<SequentialSeparator>()
        .typ::<ShellDialect>()
        .typ::<ItemStatus>()
        .typ::<RunStatus>()
        .typ::<CommandGroupItemInput>()
        .typ::<CommandGroupInput>()
        .typ::<CommandGroupItem>()
        .typ::<CommandGroup>()
        .typ::<CommandGroupCatalog>()
        .typ::<ReorderGroupEntry>()
        .typ::<DeleteGroupReceipt>()
        .typ::<GroupRunRequest>()
        .typ::<GroupRunSnapshot>()
        .typ::<GroupRunItemSnapshot>()
        .typ::<GroupRunCounts>()
        .typ::<GroupRunItem>()
        .typ::<GroupRunSummary>()
        .typ::<GroupRun>()
        .typ::<GroupRunListPage>()
        .typ::<GroupChangeOperation>()
        .typ::<GroupChangedEvent>()
        .typ::<GroupRunItemEventDelta>()
        .typ::<GroupRunEvent>()
        .typ::<GroupOutputFrameKind>()
        .typ::<GroupOutputFrame>()
        .typ::<OutputAttachReceipt>()
        .typ::<GroupSuggestRequest>()
        .typ::<GroupSuggestResponse>()
        .typ::<RecurringCommandEntry>()
        .typ::<RecurringCommandsResponse>()
        .typ::<GithubErrorKind>()
        .typ::<GithubError>()
        .typ::<GithubRepoIdentity>()
        .typ::<RepoResolution>()
        .typ::<GithubTransportKind>()
        .typ::<GhCliStatus>()
        .typ::<RestTokenStatus>()
        .typ::<GithubAuthStatus>()
        .typ::<RateLimitSnapshot>()
        .typ::<CanonicalLabelGroup>()
        .typ::<CanonicalLabelSpec>()
        .typ::<RepoLabel>()
        .typ::<LabelEnsureReport>()
        .typ::<IssueState>()
        .typ::<IssueStateFilter>()
        .typ::<IssueSortOrder>()
        .typ::<IssueFilter>()
        .typ::<PlanSource>()
        .typ::<ExtractedPlan>()
        .typ::<IssueSummary>()
        .typ::<IssueComment>()
        .typ::<IssueDetail>()
        .typ::<IssueListPage>()
        .typ::<IssueCounts>()
        .typ::<CreateIssueInput>()
        .typ::<ExclusiveLabelChange>()
        .typ::<LabelUpdatePatch>()
        .typ::<MaxCodeIssueAction>()
        .typ::<SendIssueToMaxCodeRequest>()
        .typ::<SendIssueToMaxCodeReceipt>()
        .typ::<IssueWorktreeResolution>()
        .typ::<CreateIssueWorktreeRequest>()
        .typ::<IssueTabTargetKind>()
        .typ::<IssueTabDirectoryStrategy>()
        .typ::<OpenIssueTabRequest>()
        .typ::<OpenIssueTabReceipt>()
        .typ::<BugCycleCandidateItem>()
        .typ::<BugCycleSkippedItem>()
        .typ::<BugCyclePreview>()
        .typ::<StartBugCycleRequest>()
        .typ::<BugCycleRunState>()
        .typ::<GithubIssuesChangedPayload>()
        .typ::<GithubIssueUpdatedPayload>()
        .typ::<GithubLabelsSyncedPayload>()
        .typ::<GithubAuthChangedPayload>()
        .typ::<GithubBugCycleProgressEventPayload>()
        .typ::<GithubRateLimitPayload>()
        .typ::<RestoreScheduledDto>()
        .typ::<SearchSourceKind>()
        .typ::<ExitFilter>()
        .typ::<SearchSort>()
        .typ::<SearchFilters>()
        .typ::<SearchRequest>()
        .typ::<HighlightSegment>()
        .typ::<SearchTarget>()
        .typ::<SearchHit>()
        .typ::<SearchSection>()
        .typ::<SearchResponse>()
        .typ::<CancelOutcome>()
        .typ::<SearchIndexStat>()
        .typ::<RebuildJobHandle>()
        .typ::<SearchIndexProgressEvent>()
        .typ::<SearchInvalidatedEvent>()
        .typ::<SpanSummaryDto>()
        .typ::<PtyFlowCountersDto>()
        .typ::<PerfSnapshotDto>()
        .typ::<UpdateCapabilityDto>()
        .typ::<UpdateCheckResultDto>()
        .typ::<HorizonContext>()
        .typ::<HorizonAvailability>()
        .typ::<HorizonStatus>()
        .typ::<QueueScope>()
        .typ::<HorizonSessionDto>()
        .typ::<HorizonSnapshot>()
        .typ::<HorizonClearPreview>()
        .typ::<QueueClearCount>()
        .typ::<HorizonClearResult>()
}

/// Constrói o `tauri_specta::Builder<tauri::Wry>` padrão da aplicação.
#[must_use]
pub fn builder() -> Builder<tauri::Wry> {
    builder_for::<tauri::Wry>()
}

/// Retorna o caminho canônico de `src/types/bindings.ts` resolvido a partir de `CARGO_MANIFEST_DIR`.
#[must_use]
pub fn canonical_bindings_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/types/bindings.ts")
}

/// Configura o exportador TypeScript com cabeçalho determinístico.
#[must_use]
pub fn typescript_exporter() -> Typescript {
    Typescript::default().header("// @ts-nocheck\n// Arquivo gerado automaticamente por tauri-specta (`npm run bindings`). Não edite manualmente.\nexport type Page<T> = { items: T[]; nextCursor: string | null };\nexport type CursorPage<T> = { items: T[]; nextCursor: string | null };\nexport type ProjectEvent<T> = { epoch: string; revision: number; payload: T };\n")
}

/// Exporta os bindings TypeScript para o caminho informado sem inicializar `WebView` ou Tauri runtime.
pub fn export_bindings_to(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Falha ao criar diretório de bindings: {err}"))?;
    }
    builder()
        .export(typescript_exporter(), path)
        .map_err(|err| format!("Falha ao exportar bindings TypeScript via Specta: {err}"))
}

/// Exporta os bindings TypeScript para `src/types/bindings.ts`.
pub fn export_bindings() -> Result<(), String> {
    export_bindings_to(&canonical_bindings_path())
}
