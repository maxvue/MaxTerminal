//! Registro das configurações da Fase 06 (`projects.selected_id` e `layout.columns.left_size`) no `mt-db`.

use mt_db::{DbOpenOptions, SettingsRegistry};

/// Registra os descritores de configuração da Fase 06 em um [`SettingsRegistry`].
pub fn register_settings(registry: &mut SettingsRegistry) {
    registry.register_phase06_settings();
}

/// Registra os descritores da Fase 06 diretamente nas [`DbOpenOptions`] antes de `Db::open_with`.
pub fn register_db(options: &mut DbOpenOptions) {
    options.settings.register_phase06_settings();
}
