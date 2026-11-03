//! Cache em memória RAM de snapshots materializados de busca global (`SnapshotCache`).
//!
//! Impõe limites estritos por janela (`SearchOwner`):
//! - Máximo de `2` snapshots simultâneos por `SearchOwner`.
//! - Orçamento máximo de `16 MiB` por janela.
//! - TTL de `120_000 ms` (120 s).

use std::collections::{BTreeMap, VecDeque};

use parking_lot::Mutex;
use uuid::Uuid;

use crate::search::error::SearchError;
use crate::search::model::{SearchHit, SearchSourceKind};

/// Identificador da janela/cliente dona do snapshot e das requisições de busca.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchOwner {
    /// Rótulo da janela Tauri (ex.: `"main"`).
    pub window_label: String,
}

impl SearchOwner {
    /// Cria um `SearchOwner` para o rótulo de janela informado.
    #[must_use]
    pub fn new(window_label: impl Into<String>) -> Self {
        Self {
            window_label: window_label.into(),
        }
    }
}

/// Seção materializada completa armazenada dentro de um snapshot.
#[derive(Debug, Clone)]
pub struct MaterializedSection {
    /// Fonte da seção.
    pub source: SearchSourceKind,
    /// Todos os hits coletados para esta seção (até o limite global do snapshot).
    pub items: Vec<SearchHit>,
    /// Indica se a coleta desta seção foi truncada pelo limite global.
    pub truncated: bool,
}

/// Snapshot materializado congelado para paginação consistente sob escritas concorrentes.
#[derive(Debug, Clone)]
pub struct MaterializedSnapshot {
    /// Identificador único do snapshot (`snap_<uuid>`).
    pub snapshot_id: String,
    /// Janela dona do snapshot.
    pub owner: SearchOwner,
    /// Impressão digital da consulta original.
    pub query_fingerprint: String,
    /// Timestamp UNIX em milissegundos de criação.
    pub created_at_ms: i64,
    /// Timestamp UNIX em milissegundos de expiração (`created_at_ms + ttl_ms`).
    pub expires_at_ms: i64,
    /// Timestamp máximo de dados indexados no momento da coleta.
    pub indexed_through_ms: Option<i64>,
    /// Estimativa de bytes ocupados em memória.
    pub approx_bytes: usize,
    /// Seções na ordem canônica.
    pub sections: Vec<MaterializedSection>,
}

impl MaterializedSnapshot {
    /// Localiza uma seção pelo `SearchSourceKind`.
    #[must_use]
    pub fn section(&self, source: SearchSourceKind) -> Option<&MaterializedSection> {
        self.sections.iter().find(|s| s.source == source)
    }
}

/// Cache thread-safe de snapshots materializados particionado por `SearchOwner`.
#[derive(Debug)]
pub struct SnapshotCache {
    max_snapshots_per_owner: usize,
    max_bytes_per_owner: usize,
    ttl_ms: i64,
    by_owner: Mutex<BTreeMap<String, VecDeque<MaterializedSnapshot>>>,
}

impl SnapshotCache {
    /// Cria um novo `SnapshotCache` com os limites especificados.
    #[must_use]
    pub const fn new(
        max_snapshots_per_owner: usize,
        max_bytes_per_owner: usize,
        ttl_ms: i64,
    ) -> Self {
        Self {
            max_snapshots_per_owner,
            max_bytes_per_owner,
            ttl_ms,
            by_owner: Mutex::new(BTreeMap::new()),
        }
    }

    /// Insere um novo snapshot para `owner`, aplicando evicção de expirados e de limite FIFO.
    pub fn insert(
        &self,
        owner: SearchOwner,
        query_fingerprint: String,
        now_ms: i64,
        indexed_through_ms: Option<i64>,
        sections: Vec<MaterializedSection>,
    ) -> MaterializedSnapshot {
        let snapshot_id = format!("snap_{}", Uuid::new_v4().simple());
        let expires_at_ms = now_ms.saturating_add(self.ttl_ms);
        let approx_bytes = estimate_sections_bytes(&sections);

        let snapshot = MaterializedSnapshot {
            snapshot_id,
            owner: owner.clone(),
            query_fingerprint,
            created_at_ms: now_ms,
            expires_at_ms,
            indexed_through_ms,
            approx_bytes,
            sections,
        };

        let mut guard = self.by_owner.lock();
        let queue = guard.entry(owner.window_label).or_default();

        // Remove snapshots expirados
        queue.retain(|s| now_ms < s.expires_at_ms);

        // Evicta snapshots mais antigos se exceder contagem ou orçamento de bytes da janela
        while queue.len() >= self.max_snapshots_per_owner.max(1)
            || (sum_queue_bytes(queue).saturating_add(approx_bytes) > self.max_bytes_per_owner
                && !queue.is_empty())
        {
            let _ = queue.pop_front();
        }

        queue.push_back(snapshot.clone());
        snapshot
    }

    /// Busca um snapshot válido para `(owner, snapshot_id)` verificando expiração em `now_ms`.
    pub fn get(
        &self,
        owner: &SearchOwner,
        snapshot_id: &str,
        now_ms: i64,
    ) -> Result<MaterializedSnapshot, SearchError> {
        let mut guard = self.by_owner.lock();
        let Some(queue) = guard.get_mut(&owner.window_label) else {
            return Err(SearchError::CursorExpired);
        };

        if let Some(pos) = queue.iter().position(|s| s.snapshot_id == snapshot_id) {
            if now_ms >= queue[pos].expires_at_ms {
                let _ = queue.remove(pos);
                return Err(SearchError::CursorExpired);
            }
            return Ok(queue[pos].clone());
        }

        Err(SearchError::CursorExpired)
    }

    /// Libera explicitamente um snapshot de uma janela (`search_release`).
    pub fn release(&self, owner: &SearchOwner, snapshot_id: &str) -> bool {
        let mut guard = self.by_owner.lock();
        let Some(queue) = guard.get_mut(&owner.window_label) else {
            return false;
        };
        let before = queue.len();
        queue.retain(|s| s.snapshot_id != snapshot_id);
        queue.len() != before
    }

    /// Invalida todos os snapshots de todas as janelas e retorna pares `(window_label, snapshot_id)`.
    pub fn invalidate_all(&self) -> Vec<(String, String)> {
        let mut guard = self.by_owner.lock();
        let mut removed = Vec::new();
        for (window_label, queue) in guard.iter_mut() {
            for snap in queue.drain(..) {
                removed.push((window_label.clone(), snap.snapshot_id));
            }
        }
        guard.clear();
        removed
    }

    /// Estima o tamanho em bytes de um `SearchHit` individual.
    #[must_use]
    pub fn estimate_hit_bytes(hit: &SearchHit) -> usize {
        let title_bytes: usize = hit.title.iter().map(|s| s.text.len() + 16).sum();
        let excerpt_bytes: usize = hit.excerpt.iter().map(|s| s.text.len() + 16).sum();
        128 + hit.key.len() + title_bytes + excerpt_bytes
    }
}

fn estimate_sections_bytes(sections: &[MaterializedSection]) -> usize {
    sections
        .iter()
        .map(|sec| {
            64 + sec
                .items
                .iter()
                .map(SnapshotCache::estimate_hit_bytes)
                .sum::<usize>()
        })
        .sum()
}

fn sum_queue_bytes(queue: &VecDeque<MaterializedSnapshot>) -> usize {
    queue.iter().map(|s| s.approx_bytes).sum()
}
