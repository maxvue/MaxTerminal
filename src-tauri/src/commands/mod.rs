//! Módulos de comandos IPC Tauri organizados por domínio.

#[macro_use]
pub mod agents;
#[macro_use]
pub mod ai;
#[macro_use]
pub mod app;
#[macro_use]
pub mod backup;
#[macro_use]
pub mod bridge;
#[macro_use]
pub mod command_groups;
#[macro_use]
pub mod db;
#[macro_use]
pub mod git;
#[macro_use]
pub mod github;
#[macro_use]
pub mod memory;
#[macro_use]
pub mod os;
#[macro_use]
pub mod panes;
#[macro_use]
pub mod ports;
#[macro_use]
pub mod projects;
#[macro_use]
pub mod pty;
#[macro_use]
pub mod quick_commands;
#[macro_use]
pub mod search;
#[macro_use]
pub mod sessions;
#[macro_use]
pub mod shell;
#[macro_use]
pub mod skills;
#[macro_use]
pub mod tab_state;
#[macro_use]
pub mod tabs;
#[macro_use]
pub mod templates;
#[macro_use]
pub mod themes;
#[macro_use]
pub mod updater;
#[macro_use]
pub mod workspace;
