//! Registro transversal de atenção de abas (`AttentionRegistry` e `AttentionReason`).

use std::collections::HashMap;

use mt_core::ids::TabId;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

/// Motivo pelo qual uma aba requer intervenção imediata do usuário.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason {
    /// Agente CLI ou comando interativo aguarda entrada de texto do usuário.
    AwaitingInput,
    /// Agente CLI aguarda aprovação de permissão ou ferramenta.
    AwaitingApproval,
}

/// Registro thread-safe de abas aguardando intervenção do usuário (ponto de extensão para a Fase 13).
#[derive(Debug, Default)]
pub struct AttentionRegistry {
    inner: RwLock<HashMap<TabId, AttentionReason>>,
}

impl AttentionRegistry {
    /// Cria um novo `AttentionRegistry` vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Define o motivo de atenção para uma aba; retorna `true` se o estado mudou.
    pub fn set(&self, tab: TabId, reason: AttentionReason) -> bool {
        let mut guard = self.inner.write();
        guard.insert(tab, reason) != Some(reason)
    }

    /// Remove a marcação de atenção de uma aba; retorna `true` se havia marcação ativa.
    pub fn clear(&self, tab: &TabId) -> bool {
        let mut guard = self.inner.write();
        guard.remove(tab).is_some()
    }

    /// Retorna a quantidade total de abas atualmente aguardando intervenção.
    #[must_use]
    pub fn count(&self) -> u32 {
        u32::try_from(self.inner.read().len()).unwrap_or(u32::MAX)
    }

    /// Obtém o motivo de atenção ativo para a aba informada, se houver.
    #[must_use]
    pub fn get(&self, tab: &TabId) -> Option<AttentionReason> {
        self.inner.read().get(tab).copied()
    }
}
