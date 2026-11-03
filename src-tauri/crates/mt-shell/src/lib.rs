//! Crate `mt-shell` — Shell Integration `OSC 133`/`633`/`7`, captura sanitizada de blocos de comando
//! e máquina de estados das abas/painéis do `MaxTerminal` (Fase 05).

pub mod capture;
pub mod fallback;
pub mod inject;
pub mod kind;
pub mod osc;
pub mod persist;
pub mod runtime;
pub mod service;
pub mod state;
pub mod tracker;

pub use capture::{
    ERROR_EXCERPT_BUDGET_BYTES, FinalizedOutput, HEAD_BUDGET_BYTES, OUTPUT_PREVIEW_MAX_BYTES,
    OutputCapture, TAIL_BUDGET_BYTES, TOTAL_OUTPUT_BUDGET_BYTES, TRUNCATION_MARKER,
};
pub use fallback::{
    FOREGROUND_DEBOUNCE_MS, FallbackAction, FallbackTracker, OSC_ACTIVATION_WINDOW_MS,
};
pub use inject::{
    InjectionOutcome, cleanup_injection_dir, embedded_scripts_version, generate_shell_nonce,
    inject_for_spawn,
};
pub use kind::{can_inject_shell_integration, detect_shell_kind};
pub use osc::{
    DEFAULT_MAX_OSC_PAYLOAD, OscKind, OscScanner, RawOscEvent, ScanOutput, ScanSegment, TextSpan,
    decode_base64_utf8, decode_file_uri_cwd, normalize_and_validate_cwd, unescape_osc_value,
};
pub use persist::{NullShellEventSink, ShellEventSink, ShellPersister};
pub use runtime::{
    MAX_CACHED_BLOCKS_PER_SESSION, PendingSpawnContext, PtyShellRuntime, RecentBlocksCache,
};
pub use service::{EXTERNAL_COMMAND_MAX_BYTES, ShellService, now_unix_ms};
pub use state::{
    CONTINUOUS_PROMOTION_THRESHOLD_MS, PaneInternalState, RUNNING_HYSTERESIS_MS, TabStateRegistry,
    TransitionInput, TransitionOutput, aggregate_tab_snapshot, is_continuous_by_policy_or_command,
    matches_daemon_command, should_promote_to_continuous, transition_pane,
};
pub use tracker::{CommandTracker, OpenCommandBlock, PromptPhase, TrackerEvent, open_block_to_dto};
