//! Crate `mt-commands`: descoberta e geração de Comandos Artisan (`>`), resolvedor de
//! Atalhos Universais (`/`) e (na Fase 15) Grupos de Comandos (`>>`).

#![cfg_attr(test, allow(clippy::unwrap_used))]
#![allow(
    missing_docs,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::expect_used,
    clippy::large_enum_variant,
    clippy::match_like_matches_macro,
    clippy::needless_borrow,
    clippy::unnecessary_lazy_evaluations,
    clippy::unnecessary_literal_unwrap,
    clippy::unused_async_trait_impl
)]

pub mod artisan;
pub mod error;
pub mod fs_util;
pub mod groups;
pub mod ports;
pub mod shortcuts;
