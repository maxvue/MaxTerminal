//! Identificador único de requisições de Inteligência Artificial (`AiRequestId`).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Identificador único de uma requisição de IA (UUID v7 ordenável no tempo).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct AiRequestId(pub uuid::Uuid);

impl AiRequestId {
    /// Gera um novo `AiRequestId` baseado em UUID v7.
    #[must_use]
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }

    /// Faz o parse de uma string UUID em `AiRequestId`.
    pub fn parse(raw: &str) -> Result<Self, uuid::Error> {
        uuid::Uuid::parse_str(raw).map(Self)
    }
}

impl Default for AiRequestId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AiRequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
