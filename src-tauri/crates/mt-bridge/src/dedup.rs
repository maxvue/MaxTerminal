//! Cache LRU com TTL para deduplicação de `request_id` (`RecentRequestSet`).

use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use lru::LruCache;
use parking_lot::Mutex;
use uuid::Uuid;

/// Capacidade padrão do cache de deduplicação (512 requisições recentes).
pub const DEFAULT_DEDUP_CAPACITY: usize = 512;
/// Tempo de vida padrão de um `request_id` no cache de deduplicação (60 segundos).
pub const DEFAULT_DEDUP_TTL: Duration = Duration::from_secs(60);

/// Conjunto concorrente limitado por tamanho (LRU) e por tempo (TTL) para descartar reentregas duplicadas.
pub struct RecentRequestSet {
    ttl: Duration,
    cache: Mutex<LruCache<Uuid, Instant>>,
}

impl Default for RecentRequestSet {
    fn default() -> Self {
        Self::new(DEFAULT_DEDUP_CAPACITY, DEFAULT_DEDUP_TTL)
    }
}

impl RecentRequestSet {
    /// Cria um novo `RecentRequestSet` com a capacidade e TTL informados.
    #[must_use]
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap_or(NonZeroUsize::MIN);
        Self {
            ttl,
            cache: Mutex::new(LruCache::new(cap)),
        }
    }

    /// Registra `request_id` no instante atual.
    ///
    /// Retorna `true` se o ID é **novo** (deve ser processado) ou `false` se já foi visto dentro da janela `ttl` (duplicata).
    pub fn check_and_insert(&self, request_id: Uuid) -> bool {
        self.check_and_insert_at(request_id, Instant::now())
    }

    /// Variante determinística que recebe o `Instant` explicitamente (para testes unitários).
    pub fn check_and_insert_at(&self, request_id: Uuid, now: Instant) -> bool {
        let mut guard = self.cache.lock();
        if let Some(&seen_at) = guard.get(&request_id)
            && now
                .checked_duration_since(seen_at)
                .is_some_and(|d| d <= self.ttl)
        {
            return false;
        }
        guard.put(request_id, now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_within_ttl_and_accepts_after_expiry() {
        let set = RecentRequestSet::new(4, Duration::from_secs(60));
        let id = Uuid::new_v4();
        let t0 = Instant::now();

        assert!(set.check_and_insert_at(id, t0));
        assert!(!set.check_and_insert_at(id, t0 + Duration::from_secs(10)));
        assert!(set.check_and_insert_at(id, t0 + Duration::from_secs(61)));
    }
}
