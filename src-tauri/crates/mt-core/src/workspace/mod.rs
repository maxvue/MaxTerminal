//! Tipos e operações puras de domínio do workspace de projetos (Fase 07):
//! templates de abas, árvore de split panes, ciclo de vida de abas/painéis e histórico de sessões.

pub mod layout;
pub mod session;
pub mod tab;
pub mod template;

pub use layout::{
    LayoutSizePatch, MAX_LAYOUT_DEPTH, MAX_PANES_PER_TAB, MIN_PANE_SIZE_PERCENT, PaneLayoutNode,
    SplitDirection,
};
pub use session::{
    CursorPage, ScrollbackChunk, ScrollbackChunkKind, SessionDetail, SessionEndReason,
    SessionPaneSummary, SessionSearchHit, SessionSummary,
};
pub use tab::{
    InitialDispatchState, PaneCloseOutcome, PaneExitInfo, PaneRuntimeState, PaneView, TabLifecycle,
    TabOrigin, TabView, WorkspaceRestoreFailure, WorkspaceRestoreReport, WorkspaceSnapshot,
};
pub use template::{
    CommandOverride, EnvValue, EnvironmentKind, LaunchSnapshot, MAX_ENV_KEY_LEN,
    MAX_ENV_TOTAL_BYTES, MAX_ENV_VALUE_BYTES, MAX_ENV_VARS, MAX_INITIAL_COMMAND_BYTES,
    MAX_TEMPLATE_ICON_CHARS, MAX_TEMPLATE_NAME_CHARS, PasswordCredentialOperation, SshAuth,
    SshAuthInput, TabOpenOverrides, TabOpenRequest, TabTemplate, TabTemplateInput, TemplateConfig,
    TemplateConnection, TemplateConnectionInput, TemplateEnvironment, is_forbidden_env_prefix,
    is_likely_secret_env_key, is_valid_agent_slug, is_valid_env_ident, is_valid_iconify_id,
    validate_command_text, validate_env_map, validate_template_input,
};
