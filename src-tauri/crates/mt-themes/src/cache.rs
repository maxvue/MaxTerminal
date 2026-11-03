//! Cache em memória (`RwLock` + LRU de `ResolvedTheme`) do catálogo de temas e configurações de aparência.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use lru::LruCache;
use parking_lot::RwLock;

use crate::ids::ThemeId;
use crate::mapping::ResolvedTheme;
use crate::scheme::ColorScheme;
use crate::service::AppearanceSettings;

const RESOLVED_LRU_CAPACITY: usize = 32;

#[derive(Debug)]
struct ThemeCacheInner {
    custom_schemes: BTreeMap<ThemeId, ColorScheme>,
    appearance: AppearanceSettings,
    resolved_lru: LruCache<ThemeId, ResolvedTheme>,
}

/// Cache thread-safe em memória para resolução em `< 1 ms` de temas e configurações de aparência.
#[derive(Debug)]
pub struct ThemeCache {
    inner: RwLock<ThemeCacheInner>,
}

impl Default for ThemeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeCache {
    /// Cria uma nova instância vazia com capacidade LRU de 32 temas resolvidos.
    #[must_use]
    pub fn new() -> Self {
        let cap = NonZeroUsize::new(RESOLVED_LRU_CAPACITY).unwrap_or(NonZeroUsize::MIN);
        Self {
            inner: RwLock::new(ThemeCacheInner {
                custom_schemes: BTreeMap::new(),
                appearance: AppearanceSettings::default(),
                resolved_lru: LruCache::new(cap),
            }),
        }
    }

    /// Substitui integralmente o snapshot em memória após carga inicial ou mutação no SQLite.
    pub fn replace_all(
        &self,
        custom_schemes: BTreeMap<ThemeId, ColorScheme>,
        appearance: AppearanceSettings,
    ) {
        let mut guard = self.inner.write();
        guard.custom_schemes = custom_schemes;
        guard.appearance = appearance;
        guard.resolved_lru.clear();
    }

    /// Atualiza apenas as configurações de aparência em memória.
    pub fn set_appearance(&self, appearance: AppearanceSettings) {
        self.inner.write().appearance = appearance;
    }

    /// Retorna um clone das configurações de aparência atuais.
    #[must_use]
    pub fn appearance(&self) -> AppearanceSettings {
        self.inner.read().appearance.clone()
    }

    /// Retorna todos os esquemas `imported` e `custom` armazenados em memória.
    #[must_use]
    pub fn list_custom_schemes(&self) -> Vec<ColorScheme> {
        self.inner.read().custom_schemes.values().cloned().collect()
    }

    /// Busca um esquema `imported`/`custom` pelo `ThemeId`.
    #[must_use]
    pub fn get_custom_scheme(&self, id: &ThemeId) -> Option<ColorScheme> {
        self.inner.read().custom_schemes.get(id).cloned()
    }

    /// Insere ou atualiza um esquema `imported`/`custom` no cache e invalida sua entrada LRU.
    pub fn put_custom_scheme(&self, scheme: ColorScheme) {
        let mut guard = self.inner.write();
        guard.resolved_lru.pop(&scheme.id);
        guard.custom_schemes.insert(scheme.id.clone(), scheme);
    }

    /// Remove um esquema `imported`/`custom` do cache.
    pub fn remove_custom_scheme(&self, id: &ThemeId) -> Option<ColorScheme> {
        let mut guard = self.inner.write();
        guard.resolved_lru.pop(id);
        guard.custom_schemes.remove(id)
    }

    /// Obtém ou computa o `ResolvedTheme` para um `ColorScheme` customizado/importado usando o LRU.
    #[must_use]
    pub fn get_or_resolve(&self, scheme: &ColorScheme) -> ResolvedTheme {
        let mut guard = self.inner.write();
        if let Some(hit) = guard.resolved_lru.get(&scheme.id) {
            return hit.clone();
        }
        let resolved = ResolvedTheme::from_scheme(scheme.clone());
        guard.resolved_lru.put(scheme.id.clone(), resolved.clone());
        resolved
    }
}
