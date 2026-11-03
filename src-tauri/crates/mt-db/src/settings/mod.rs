//! Subsistema de configurações chave/valor (`SettingsRegistry` e `SettingsRepo`) com inspeção anti-segredos.

pub mod registry;
pub mod repo;

pub use registry::{SettingDescriptor, SettingLookup, SettingsRegistry};
pub use repo::SettingsRepo;
