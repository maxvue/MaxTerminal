//! Testes de `classify_display` com mapas simulados de variáveis de ambiente (`mt-os`).

use std::collections::HashMap;

use mt_os::{Compositor, DisplayServer, classify_display};

fn make_env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |key: &str| map.get(key).cloned()
}

#[test]
fn classifies_kde_hyprland_gnome_sway_x11_xwayland_and_empty() {
    // 1. KDE Plasma Wayland
    let env_kde = make_env(&[
        ("WAYLAND_DISPLAY", "wayland-0"),
        ("XDG_CURRENT_DESKTOP", "KDE"),
    ]);
    assert_eq!(
        classify_display(&env_kde),
        (DisplayServer::Wayland, Compositor::KdePlasma)
    );

    // 2. Hyprland Wayland
    let env_hypr = make_env(&[
        ("WAYLAND_DISPLAY", "wayland-1"),
        ("HYPRLAND_INSTANCE_SIGNATURE", "abc123_456"),
    ]);
    assert_eq!(
        classify_display(&env_hypr),
        (DisplayServer::Wayland, Compositor::Hyprland)
    );

    // 3. GNOME Wayland
    let env_gnome = make_env(&[
        ("WAYLAND_DISPLAY", "wayland-0"),
        ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"),
    ]);
    assert_eq!(
        classify_display(&env_gnome),
        (DisplayServer::Wayland, Compositor::Gnome)
    );

    // 4. Sway
    let env_sway = make_env(&[
        ("WAYLAND_DISPLAY", "wayland-1"),
        ("SWAYSOCK", "/run/user/1000/sway-ipc.sock"),
    ]);
    assert_eq!(
        classify_display(&env_sway),
        (DisplayServer::Wayland, Compositor::Sway)
    );

    // 5. XWayland (WAYLAND_DISPLAY + GDK_BACKEND=x11)
    let env_xwayland = make_env(&[
        ("WAYLAND_DISPLAY", "wayland-0"),
        ("GDK_BACKEND", "x11"),
        ("XDG_CURRENT_DESKTOP", "KDE"),
    ]);
    assert_eq!(
        classify_display(&env_xwayland),
        (DisplayServer::XWayland, Compositor::KdePlasma)
    );

    // 6. X11 puro
    let env_x11 = make_env(&[
        ("XDG_SESSION_TYPE", "x11"),
        ("DISPLAY", ":0"),
        ("XDG_CURRENT_DESKTOP", "KDE"),
    ]);
    assert_eq!(
        classify_display(&env_x11),
        (DisplayServer::X11, Compositor::KdePlasma)
    );

    // 7. Vazio
    let env_empty = make_env(&[]);
    assert_eq!(
        classify_display(&env_empty),
        (DisplayServer::Unknown, Compositor::Other)
    );
}
