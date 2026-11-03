//! Subsistema de migrations versionadas SQL e Rust (`MigrationRegistry` e `MigrationRunner`).

pub mod code;
pub mod registry;
pub mod runner;

pub use registry::{
    CodeMigration, CodeMigrationFn, MigrationEntry, MigrationRegistry, MigrationSource,
    compute_migration_checksum, parse_migration_filename,
};
pub use runner::{AppliedMigration, MigrationInfo, MigrationOutcome, MigrationRunner};
