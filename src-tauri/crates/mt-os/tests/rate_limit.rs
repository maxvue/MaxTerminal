//! Testes de `RateLimiter` e sanitização de notificações (`mt-os`).

use std::time::{Duration, Instant};

use mt_core::ids::TabId;
use mt_os::{LimitDecision, NotificationKind, RateLimiter, sanitize_and_truncate_notification};

#[test]
fn rate_limiter_deduplicates_within_10s_and_enforces_5_per_30s_cap() {
    let mut limiter = RateLimiter::new();
    let t0 = Instant::now();
    let tab = Some(TabId::new());

    let k1 = RateLimiter::compute_key(tab, NotificationKind::CommandFinished, "Concluído", "cmd 1");
    assert_eq!(
        limiter.check_with_tab(k1, tab, NotificationKind::CommandFinished, t0),
        LimitDecision::Allow
    );

    // Mesma chave em +3s -> Suppress
    assert_eq!(
        limiter.check_with_tab(
            k1,
            tab,
            NotificationKind::CommandFinished,
            t0 + Duration::from_secs(3)
        ),
        LimitDecision::Suppress
    );

    // Mesma aba e kind, mas corpo diferente em +4s -> Replace(k1)
    let k2 = RateLimiter::compute_key(tab, NotificationKind::CommandFinished, "Concluído", "cmd 2");
    assert_eq!(
        limiter.check_with_tab(
            k2,
            tab,
            NotificationKind::CommandFinished,
            t0 + Duration::from_secs(4)
        ),
        LimitDecision::Replace(k1)
    );

    // Mais 3 notificações diferentes dentro da janela de 30s atingem o teto de 5
    for i in 3..=5 {
        let ki =
            RateLimiter::compute_key(None, NotificationKind::Info, "Info", &format!("msg {i}"));
        assert_eq!(
            limiter.check(ki, NotificationKind::Info, t0 + Duration::from_secs(i)),
            LimitDecision::Allow
        );
    }

    // 6ª notificação dentro de 30s (mesmo AgentAwaitingInput) -> Suppress
    let k_agent = RateLimiter::compute_key(
        tab,
        NotificationKind::AgentAwaitingInput,
        "Agente",
        "aguardando aprovação",
    );
    assert_eq!(
        limiter.check_with_tab(
            k_agent,
            tab,
            NotificationKind::AgentAwaitingInput,
            t0 + Duration::from_secs(8)
        ),
        LimitDecision::Suppress
    );

    // Após 31s do início, a janela global libera espaço
    assert_eq!(
        limiter.check_with_tab(
            k_agent,
            tab,
            NotificationKind::AgentAwaitingInput,
            t0 + Duration::from_secs(31)
        ),
        LimitDecision::Allow
    );
}

#[test]
fn sanitizes_secrets_and_escapes_markup() {
    let (title, body) = sanitize_and_truncate_notification(
        "Erro <script>&",
        "Falha com Authorization: Bearer sk-1234567890abcdef1234567890abcdef",
    );
    assert_eq!(title, "Erro &lt;script&gt;&amp;");
    assert!(!body.contains("sk-1234567890abcdef"));
    assert!(body.contains("[REDACTED"));
}
