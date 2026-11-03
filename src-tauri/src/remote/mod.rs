//! Gerenciamento de transporte remoto e integração com OpenSSH.

pub mod askpass;
pub mod ssh;

pub use askpass::{AskpassServer, run_askpass_helper};
pub use ssh::{SshSpawnSession, build_ssh_spawn};
