//! Subsistema de máquina de estados de painéis/abas, detecção de processos contínuos e registro em memória.

pub mod daemon;
pub mod machine;
pub mod registry;

pub use daemon::{
    CONTINUOUS_PROMOTION_THRESHOLD_MS, RUNNING_HYSTERESIS_MS, is_continuous_by_policy_or_command,
    matches_daemon_command, should_promote_to_continuous,
};
pub use machine::{
    PaneInternalState, TransitionInput, TransitionOutput, aggregate_tab_snapshot, transition_pane,
};
pub use registry::TabStateRegistry;
