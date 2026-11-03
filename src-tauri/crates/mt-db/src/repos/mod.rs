//! Repositórios de domínio das Fases 05, 06 e 07 (`SessionsRepo`, `CommandBlocksRepo`, `ProjectsRepo`,
//! `TemplatesRepo`, `TabsRepo`, `PanesRepo`, `TabSessionsRepo`, `ScrollbackRepo` e `WorkspaceOperationsRepo`).

pub mod command_blocks;
pub mod os_settings;
pub mod panes;
pub mod projects;
pub mod scrollback;
pub mod sessions;
pub mod tab_sessions;
pub mod tabs;
pub mod templates;
pub mod themes;
pub mod workspace_operations;

pub use command_blocks::{
    COMMAND_BLOCKS_RETENTION_RULE, CommandBlocksRepo, FinishCommandBlock, NewCommandBlock,
};
pub use os_settings::{OsSettingsRepo, OsSettingsRow};
pub use panes::{
    CONSTRAINT_PANE_NOT_FOUND, PanesRepo, StoredPaneRow, bump_pane_generation_tx,
    close_pane_row_tx, get_pane_conn, insert_pane_tx, list_all_panes_by_tab_conn,
    list_open_panes_by_tab_conn, update_pane_cwd_tx,
};
pub use projects::{
    CONSTRAINT_PROJECT_DUPLICATE_PATH, CONSTRAINT_PROJECT_NOT_FOUND,
    CONSTRAINT_PROJECT_REORDER_MISMATCH, CONSTRAINT_PROJECT_ROOT_DUPLICATE_PATH,
    CONSTRAINT_PROJECT_ROOT_NOT_FOUND, CatalogRows, ProjectCommit, ProjectMutation, ProjectsRepo,
    StoredDismissed, StoredIdentityUpdate, StoredMissingUpdate, StoredNewProject, StoredProject,
    StoredReconcileBatch, StoredRoot, StoredRootEntry,
};
pub use scrollback::{
    MAX_SCROLLBACK_PAGE_BYTES, SESSION_SCROLLBACK_FTS_SPEC, SESSIONS_ARCHIVED_RETENTION_RULE,
    ScrollbackRecord, ScrollbackRepo, append_scrollback_chunk_tx, mark_session_truncated_tx,
};
pub use sessions::{
    NewSession, SessionDto, SessionEnd, SessionsRepo, close_session_tx, insert_session_tx,
};
pub use tab_sessions::{
    CONSTRAINT_PERSISTENCE_PENDING, CONSTRAINT_SESSION_NOT_FOUND, CONSTRAINT_SESSION_QUERY_INVALID,
    TabSessionsRepo, archive_closed_session_tx, archive_tab_session_tx, attach_pane_session_tx,
    close_pane_session_tx, enforce_tab_sessions_retention_tx, get_active_tab_session_conn,
    get_tab_session_detail_conn, get_tab_session_summary_conn, recover_crash_sessions_tx,
    start_tab_session_tx,
};
pub use tabs::{
    CONSTRAINT_LAYOUT_CONFLICT, CONSTRAINT_LAYOUT_INVALID, CONSTRAINT_PANE_LIMIT_REACHED,
    CONSTRAINT_TAB_NOT_FOUND, CONSTRAINT_TAB_REORDER_MISMATCH, NewTabTxInput, StoredTabRow,
    TabsRepo, close_tab_tx, create_tab_with_initial_pane_tx, delete_project_workspace_tx,
    find_workspace_tab_for_template_conn, get_tab_conn, list_all_open_tabs_conn,
    list_open_tabs_by_project_conn, remove_pane_from_layout_tx, reopen_workspace_tab_tx,
    split_pane_tx,
};
pub use templates::{
    CONSTRAINT_TEMPLATE_CONFLICT, CONSTRAINT_TEMPLATE_DEFAULT_REQUIRED,
    CONSTRAINT_TEMPLATE_INVALID, CONSTRAINT_TEMPLATE_NOT_FOUND, CONSTRAINT_TEMPLATE_SECRET_LITERAL,
    DefaultTemplateContributor, TemplatesRepo,
};
pub use themes::{
    StoredAppearanceRow, StoredColorSchemeRow, StoredProjectThemeOverrideRow, ThemesRepo,
};
pub use workspace_operations::{
    CONSTRAINT_WORKSPACE_OPERATION_INTERRUPTED, CONSTRAINT_WORKSPACE_REQUEST_CONFLICT,
    OperationStage, PrepareOperationOutcome, WorkspaceOperationRow, WorkspaceOperationsRepo,
    get_operation_conn, mark_previous_boots_interrupted_tx, prepare_operation_tx,
    update_operation_stage_tx,
};
