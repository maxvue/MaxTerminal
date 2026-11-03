//! Núcleo de gerenciamento de pseudo-terminais (`portable-pty`), pipeline binário de I/O,
//! contenção Linux via cgroup v2 e supervisão de processos via `/proc`.

pub mod config;
pub mod containment;
pub mod error;
pub mod events;
pub mod guard;
pub mod instance;
pub mod io;
pub mod kill;
pub mod manager;
pub mod proc;
pub mod sink;
pub mod spawn;
pub mod supervisor;
pub mod tap;
pub mod types;

pub use config::PtyConfig;
pub use error::{PtyError, PtyResult};
pub use events::{
    EventHub, PtyCleanupFailedEvent, PtyEvent, PtyEventListener, PtyExitEvent,
    PtyForegroundChangedEvent, PtySpawnedEvent,
};
pub use instance::{PtyInstance, SpawnContext};
pub use kill::{KillControl, KillCoordinator, KillOutcome, KillPlan};
pub use manager::{GuardHandle, PtyManager};
pub use sink::{NullSink, OutputSink, SinkClosed, VecSink};
pub use spawn::{
    APPIMAGE_COLON_PATH_VARS, APPIMAGE_DIRECT_VARS, AppImageEnvSnapshot, DefaultEnvPolicy,
    EnvPolicy, PreparedSpawn, ResolvedShell, SpawnLifecycle, prepare_spawn,
};

/// Reexportação de políticas de ambiente (incluindo higienização `AppImageEnvSnapshot` — Fase 17).
pub mod env_policy {
    pub use crate::spawn::env::*;
}
pub use tap::{OutputTap, TapHandle, TapRegistry};
pub use types::{
    AttachInfo, AttachmentId, ExitInfo, ExitReason, KillMode, OutputCursor, ProcState,
    ProcessIdentity, ProcessInfo, ProcessNode, PtyInput, PtyOwner, PtyPids, PtyProcessReport,
    PtySize, PtyStatus, PtySummary, ShutdownReport, SpawnSpec, TrackedProcess, U64Decimal,
};
