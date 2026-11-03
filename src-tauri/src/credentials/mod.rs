//! Módulo de segurança, cofre nativo e proteção de credenciais SSH cifradas no SQLite.

pub mod crypto;
pub mod service;

pub use service::{VaultService, VaultStatus};
