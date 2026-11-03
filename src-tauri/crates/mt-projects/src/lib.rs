//! Crate `mt-projects`: gerenciamento de projetos, Pastas de Projetos (`project_roots`),
//! auto-detecção de stack, identidade visual e auto-descoberta via `notify` (Fase 06).

pub mod detect;
pub mod events;
pub mod identity;
pub mod model;
pub mod paths;
pub mod scanner;
pub mod service;
pub mod settings;
pub mod store;
pub mod watcher;

pub use detect::{DetectionReport, FolderProbe, StackDetector, StackRule};
pub use events::{
    MemoryProjectEventSink, NoopProjectEventSink, ProjectChangedPayload, ProjectDiscoveredPayload,
    ProjectEvent, ProjectEventSink, ProjectRemovedPayload, ProjectRootChangedPayload, RemovedKind,
};
pub use identity::{
    FALLBACK_COLORS, FALLBACK_ICONS, IdentityMap, fnv1a_64, validate_color, validate_icon,
    validate_title,
};
pub use model::{
    DiscoveredFolder, DiscoveryMode, IdentitySource, NewProject, NewProjectRoot, Project,
    ProjectIdentity, ProjectPatch, ProjectRoot, ProjectRootId, ProjectRootPatch, ProjectSnapshot,
    RootImportReport, ScanWarning, StackKind, WatchStatus,
};
pub use paths::{
    canonical_dir, canonicalize_existing_dir, default_title, expand_home, is_hidden,
    is_inside_ignored_discovery_dir, validate_project_dir, validate_root_dir,
};
pub use scanner::{RootEntryRow, ScanDiff, ScanEntry, ScanSnapshot, diff, scan_level1};
pub use service::{ProjectActivityGuard, ProjectRemovalHook, ProjectService};
pub use settings::{register_db, register_settings};
pub use store::{
    CatalogMutation, CatalogStateRows, CommitResult, DismissedRow, IdentityUpdateRow,
    MemoryProjectStore, MissingUpdateRow, NewProjectRow, ProjectRootRow, ProjectStore,
    ReconcileBatch, SqliteProjectStore,
};
pub use watcher::{RootWatcher, WatchSignal};
