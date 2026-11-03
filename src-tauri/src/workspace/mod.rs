//! Domínio de Abas, Templates, Split Panes, Sessões e Scrollback da Fase 07 (`workspace`).

pub mod environment;
pub mod errors;
pub mod manager;
pub mod observer;
pub mod recorder;
pub mod restore;
pub mod spawn_hooks;

pub use environment::{EnvironmentResolver, ResolvedLaunchEnvironment};
pub use errors::map_db_error;
pub use manager::WorkspaceManager;
pub use observer::WorkspaceLifecycleObserver;
pub use recorder::SessionRecorder;
pub use restore::run_boot_reconciliation_and_restore;
pub use spawn_hooks::{
    SpawnHookChain, SpawnReservationRequest, WorkspaceSpawnGuard, WorkspaceSpawnHook,
};
