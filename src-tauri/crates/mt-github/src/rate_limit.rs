//! Rastreador de cota (`X-RateLimit-*` e `Retry-After`) da API do GitHub.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::model::RateLimitSnapshot;

/// Rastreador thread-safe do estado de rate limit do GitHub.
#[derive(Debug, Clone, Default)]
pub struct RateLimitTracker {
    latest: Arc<RwLock<Option<RateLimitSnapshot>>>,
}

impl RateLimitTracker {
    /// Cria um novo `RateLimitTracker`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Retorna o snapshot mais recente de rate limit, se conhecido.
    #[must_use]
    pub fn snapshot(&self) -> Option<RateLimitSnapshot> {
        self.latest.read().clone()
    }

    /// Define manualmente o snapshot de rate limit (ex.: restaurado do SQLite).
    pub fn set_snapshot(&self, snap: RateLimitSnapshot) {
        *self.latest.write() = Some(snap);
    }

    /// Verifica se estamos em janela de bloqueio ativo (`retry_after_until_ms` ou `remaining == 0` antes de `reset_at_ms`).
    #[must_use]
    pub fn active_cooldown_until(&self, now_ms: i64) -> Option<i64> {
        let snap = self.latest.read();
        let s = (*snap).as_ref()?;
        if let Some(until) = s.retry_after_until_ms {
            if until > now_ms {
                return Some(until);
            }
        }
        if s.remaining == 0 && s.reset_at_ms > now_ms {
            return Some(s.reset_at_ms);
        }
        None
    }

    /// Atualiza o estado a partir dos cabeçalhos HTTP (case-insensitive) de uma resposta do GitHub.
    pub fn update_from_headers(
        &self,
        headers: &HashMap<String, String>,
        status_code: u16,
        now_ms: i64,
    ) -> Option<RateLimitSnapshot> {
        let get_u32 = |key: &str| -> Option<u32> {
            headers.get(key).and_then(|v| v.trim().parse::<u32>().ok())
        };
        let get_i64 = |key: &str| -> Option<i64> {
            headers.get(key).and_then(|v| v.trim().parse::<i64>().ok())
        };

        let prev = self.latest.read().clone();
        let limit = get_u32("x-ratelimit-limit").or_else(|| prev.as_ref().map(|p| p.limit))?;
        let remaining =
            get_u32("x-ratelimit-remaining").or_else(|| prev.as_ref().map(|p| p.remaining))?;
        let used = get_u32("x-ratelimit-used").unwrap_or_else(|| limit.saturating_sub(remaining));
        let reset_at_ms = get_i64("x-ratelimit-reset")
            .map(|secs| {
                if secs > 10_000_000_000 {
                    secs
                } else {
                    secs * 1000
                }
            })
            .or_else(|| prev.as_ref().map(|p| p.reset_at_ms))
            .unwrap_or(now_ms + 60_000);

        let retry_after_secs = get_i64("retry-after");
        let retry_after_until_ms = if let Some(secs) = retry_after_secs {
            Some(now_ms + secs.max(1) * 1000)
        } else if status_code == 429 || (status_code == 403 && remaining == 0) {
            Some(reset_at_ms.max(now_ms + 1_000))
        } else {
            None
        };

        let low_remaining = limit > 0 && remaining.saturating_mul(10) < limit;

        let snap = RateLimitSnapshot {
            limit,
            remaining,
            used,
            reset_at_ms,
            retry_after_until_ms,
            low_remaining,
        };
        *self.latest.write() = Some(snap.clone());
        Some(snap)
    }
}
