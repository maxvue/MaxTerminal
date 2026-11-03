//! Testes de `decide_toggle` (8 combinações) e `drop_down_geometry` (`mt-os`).

use mt_os::{DropDownGeometry, ToggleDecision, decide_toggle, drop_down_geometry};

#[test]
fn decide_toggle_covers_all_8_combinations() {
    // Oculto (visible = false) -> sempre ShowAndFocus (4 casos)
    assert_eq!(
        decide_toggle(false, false, false),
        ToggleDecision::ShowAndFocus
    );
    assert_eq!(
        decide_toggle(false, false, true),
        ToggleDecision::ShowAndFocus
    );
    assert_eq!(
        decide_toggle(false, true, false),
        ToggleDecision::ShowAndFocus
    );
    assert_eq!(
        decide_toggle(false, true, true),
        ToggleDecision::ShowAndFocus
    );

    // Visível mas minimizado -> ShowAndFocus (2 casos)
    assert_eq!(
        decide_toggle(true, false, true),
        ToggleDecision::ShowAndFocus
    );
    assert_eq!(
        decide_toggle(true, true, true),
        ToggleDecision::ShowAndFocus
    );

    // Visível, não minimizado, sem foco -> Focus (1 caso)
    assert_eq!(decide_toggle(true, false, false), ToggleDecision::Focus);

    // Visível, não minimizado, com foco -> Hide (1 caso)
    assert_eq!(decide_toggle(true, true, false), ToggleDecision::Hide);
}

#[test]
fn drop_down_geometry_handles_offsets_and_clamping() {
    let g1 = drop_down_geometry((0, 0), (1920, 1080), 50);
    assert_eq!(
        g1,
        DropDownGeometry {
            x: 0,
            y: 0,
            width: 1920,
            height: 540,
        }
    );

    // Monitor secundário com offset negativo e altura 45%
    let g2 = drop_down_geometry((-2560, 120), (2560, 1440), 45);
    assert_eq!(
        g2,
        DropDownGeometry {
            x: -2560,
            y: 120,
            width: 2560,
            height: 648,
        }
    );

    // Clamping abaixo de 20% e acima de 100%
    let g_min = drop_down_geometry((10, 20), (1000, 1000), 5);
    assert_eq!(g_min.height, 200);

    let g_max = drop_down_geometry((10, 20), (1000, 1000), 150);
    assert_eq!(g_max.height, 1000);
}
