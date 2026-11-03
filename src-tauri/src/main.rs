//! Entrypoint do binário executável do `MaxTerminal`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    maxterminal_lib::run();
}
