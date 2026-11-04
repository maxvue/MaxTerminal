//! Módulo do Laravel Horizon para MaxTerminal.

pub mod cleaner;
pub mod model;
pub mod probe;
pub mod service;

pub use cleaner::execute_horizon_clear;
pub use model::{
    HorizonAvailability, HorizonClearPreview, HorizonClearResult, HorizonContext,
    HorizonSessionDto, HorizonSnapshot, HorizonStatus, QueueClearCount, QueueScope,
};
pub use probe::{
    EMBEDDED_HORIZON_PROBE_PHP, RawHorizonProbeOutput, RawQueueScope, build_snapshot_from_probe,
    compute_capabilities, has_static_horizon_manifest, run_horizon_probe,
};
pub use service::HorizonDomainService;
