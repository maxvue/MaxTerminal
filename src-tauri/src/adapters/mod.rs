//! Adaptadores de infraestrutura que conectam as crates de domínio (`mt-git`, `mt-ports`, `mt-os`)
//! ao `WorkspaceManager`, `PtyManager` e runtime Tauri.

pub mod exact_process_signaler;
pub mod indicators;
pub mod os;
pub mod port_opener;
pub mod process_tree;
