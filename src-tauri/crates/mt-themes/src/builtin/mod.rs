//! Catálogo dos 6 temas nativos embutidos no binário via `include_str!`.

use std::sync::LazyLock;

use crate::formats::{SchemeFileFormat, parse_schemes_by_format};
use crate::ids::{ThemeId, ThemeOrigin};
use crate::mapping::ResolvedTheme;
use crate::normalize::normalize_raw_scheme;
use crate::scheme::ColorScheme;

const MAX_NAVY_TEAL_YAML: &str = include_str!("max-navy-teal.yaml");
const MAX_LIGHT_YAML: &str = include_str!("max-light.yaml");
const DRACULA_YAML: &str = include_str!("dracula.yaml");
const NORD_YAML: &str = include_str!("nord.yaml");
const SOLARIZED_DARK_YAML: &str = include_str!("solarized-dark.yaml");
const SOLARIZED_LIGHT_YAML: &str = include_str!("solarized-light.yaml");

/// Lista ordenada dos identificadores canônicos dos 6 temas nativos embutidos.
pub const BUILTIN_THEME_IDS: [&str; 6] = [
    "max-navy-teal",
    "max-light",
    "dracula",
    "nord",
    "solarized-dark",
    "solarized-light",
];

static BUILTIN_THEMES: LazyLock<Vec<ResolvedTheme>> = LazyLock::new(|| {
    let specs: [(&str, &str, &str); 6] = [
        ("max-navy-teal", "max-navy-teal.yaml", MAX_NAVY_TEAL_YAML),
        ("max-light", "max-light.yaml", MAX_LIGHT_YAML),
        ("dracula", "dracula.yaml", DRACULA_YAML),
        ("nord", "nord.yaml", NORD_YAML),
        ("solarized-dark", "solarized-dark.yaml", SOLARIZED_DARK_YAML),
        (
            "solarized-light",
            "solarized-light.yaml",
            SOLARIZED_LIGHT_YAML,
        ),
    ];

    let mut list = Vec::with_capacity(specs.len());
    for (id_str, filename, yaml) in specs {
        let Ok(theme_id) = ThemeId::parse(id_str) else {
            panic!("ID de tema builtin inválido: {id_str}");
        };
        let Ok((_, mut raws)) = parse_schemes_by_format(
            yaml,
            Some(filename),
            Some(SchemeFileFormat::TabbySchemeYaml),
        ) else {
            panic!("YAML de tema builtin inválido: {filename}");
        };
        let raw = raws.remove(0);
        let Ok(normalized) = normalize_raw_scheme(
            raw,
            ThemeOrigin::Builtin,
            Some(theme_id),
            Some(filename.to_string()),
            0,
        ) else {
            panic!("Falha ao normalizar tema builtin: {filename}");
        };
        list.push(ResolvedTheme::from_scheme(normalized.scheme));
    }
    list
});

/// Retorna todos os 6 temas nativos já resolvidos (`ResolvedTheme`).
#[must_use]
pub fn builtin_resolved_themes() -> &'static [ResolvedTheme] {
    &BUILTIN_THEMES
}

/// Retorna os esquemas canônicos (`ColorScheme`) dos 6 temas nativos.
#[must_use]
pub fn builtin_schemes() -> Vec<ColorScheme> {
    BUILTIN_THEMES.iter().map(|t| t.scheme.clone()).collect()
}

/// Busca um tema nativo resolvido pelo `ThemeId`.
#[must_use]
pub fn find_builtin_resolved(id: &ThemeId) -> Option<&'static ResolvedTheme> {
    BUILTIN_THEMES.iter().find(|t| &t.scheme.id == id)
}

/// Verifica se um `ThemeId` pertence ao catálogo nativo embutido.
#[must_use]
pub fn is_builtin_theme_id(id: &ThemeId) -> bool {
    BUILTIN_THEME_IDS.contains(&id.as_str())
}
