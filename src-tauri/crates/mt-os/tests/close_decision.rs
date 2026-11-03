//! Testes exaustivos da tabela-verdade de `decide_close` (24 combinações) e `ShutdownCoordinator`.

use mt_os::{CloseBehavior, CloseContext, CloseDecision, decide_close};

#[test]
fn decide_close_covers_all_24_combinations() {
    let behaviors = [
        CloseBehavior::Auto,
        CloseBehavior::AlwaysTray,
        CloseBehavior::AlwaysQuit,
    ];
    let mut count = 0;

    for behavior in behaviors {
        for has_active in [false, true] {
            for tray_available in [false, true] {
                for force_quit in [false, true] {
                    let ctx = CloseContext {
                        behavior,
                        has_active,
                        tray_available,
                        force_quit,
                    };
                    let decision = decide_close(ctx);
                    count += 1;

                    if force_quit {
                        assert_eq!(decision, CloseDecision::Quit, "ctx={ctx:?}");
                        continue;
                    }

                    let expected = match (behavior, has_active, tray_available) {
                        // Auto ou AlwaysTray com tray e ativo (ou AlwaysTray sempre com tray) -> HideToTray
                        (CloseBehavior::Auto, true, true)
                        | (CloseBehavior::AlwaysTray, _, true) => CloseDecision::HideToTray,
                        // Auto com ativo sem tray ou AlwaysTray sem tray -> Minimize
                        (CloseBehavior::Auto, true, false)
                        | (CloseBehavior::AlwaysTray, _, false) => CloseDecision::Minimize,
                        // AlwaysQuit com ativo -> ConfirmQuit
                        (CloseBehavior::AlwaysQuit, true, _) => CloseDecision::ConfirmQuit,
                        // Auto ou AlwaysQuit sem ativo -> Quit
                        (CloseBehavior::Auto | CloseBehavior::AlwaysQuit, false, _) => {
                            CloseDecision::Quit
                        }
                    };

                    assert_eq!(decision, expected, "ctx={ctx:?}");
                }
            }
        }
    }

    assert_eq!(count, 24);
}
