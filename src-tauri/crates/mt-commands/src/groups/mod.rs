//! Domínio, validação, repositório SQLite, motores de execução (`Sequential`, `Parallel`, `MultiTabs`),
//! sugestão assistida via IA e orquestração da Fase 15 — Grupos de Comandos (`>>`).

pub mod aggregate;
pub mod compose;
pub mod events;
pub mod model;
pub mod multi_tab;
pub mod plan;
pub mod ports;
pub mod repo;
pub mod resolve;
pub mod runner;
pub mod sequential;
pub mod service;
pub mod suggest;
pub mod validate;
