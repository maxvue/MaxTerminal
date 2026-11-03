//! Serviço fachada `ShortcutService` para listagem e resolução de atalhos universais (`/`).

use std::fs;
use std::path::Path;
use std::sync::Arc;

use mt_core::{MtError, MtResult};

use crate::artisan::service::ArtisanService;
use crate::error::codes;
use crate::ports::{MaxCodeConfigLocator, ProjectLookup};
use crate::shortcuts::detect::{detect_project_managers, find_manifest_root, is_regular_file};
use crate::shortcuts::maxcode_config::MaxCodeConfigReader;
use crate::shortcuts::model::{ResolvedShortcut, ShortcutId, ShortcutList};
use crate::shortcuts::resolve::{ArtisanTestContext, resolve_all_shortcuts};

/// Serviço de detecção e resolução dos 7 atalhos universais (`/`).
pub struct ShortcutService {
    lookup: Arc<dyn ProjectLookup>,
    artisan: Option<Arc<ArtisanService>>,
    maxcode_reader: MaxCodeConfigReader,
}

impl ShortcutService {
    /// Cria uma nova instância de [`ShortcutService`].
    #[must_use]
    pub fn new(
        lookup: Arc<dyn ProjectLookup>,
        config_locator: Arc<dyn MaxCodeConfigLocator>,
        artisan: Option<Arc<ArtisanService>>,
    ) -> Self {
        Self {
            lookup,
            artisan,
            maxcode_reader: MaxCodeConfigReader::new(config_locator),
        }
    }

    /// Lista os gerenciadores detectados, avisos e os 7 atalhos resolvidos para `cwd`.
    pub fn list(&self, cwd: &Path) -> MtResult<ShortcutList> {
        let canonical_cwd = fs::canonicalize(cwd).map_err(|e| {
            MtError::new(
                codes::SHORTCUT_INVALID_CWD,
                format!(
                    "O diretório '{}' não existe ou está inacessível: {e}",
                    cwd.display()
                ),
            )
        })?;

        let Some((_, project_path)) = self.lookup.owning_project(&canonical_cwd) else {
            return Err(MtError::new(
                codes::SHORTCUT_INVALID_CWD,
                format!(
                    "O diretório '{}' não pertence a nenhum projeto cadastrado.",
                    canonical_cwd.display()
                ),
            ));
        };

        let canon_proj = fs::canonicalize(&project_path).unwrap_or(project_path);
        let manifest_root = find_manifest_root(&canonical_cwd, &canon_proj);
        let detection = detect_project_managers(&manifest_root);
        let overrides = self.maxcode_reader.resolve_for_root(&detection.root);
        let artisan_ctx =
            self.resolve_artisan_test_ctx(&detection.root, detection.composer.is_laravel);

        let items = resolve_all_shortcuts(&detection, &canonical_cwd, &overrides, artisan_ctx);

        Ok(ShortcutList {
            root: detection.root.display().to_string(),
            managers: detection.managers,
            warnings: detection.warnings,
            items,
        })
    }

    /// Resolve e valida um único [`ShortcutId`] para `cwd`, retornando erro se estiver desabilitado.
    pub fn resolve(&self, cwd: &Path, id: ShortcutId) -> MtResult<ResolvedShortcut> {
        let list = self.list(cwd)?;
        let item = list.items.into_iter().find(|i| i.id == id).ok_or_else(|| {
            MtError::new(
                codes::SHORTCUT_UNRESOLVED,
                format!("Atalho {} não encontrado.", id.label()),
            )
        })?;

        if item.line.is_none() {
            let reason = item.disabled_reason.clone().unwrap_or_else(|| {
                format!(
                    "O atalho {} está indisponível para este projeto.",
                    id.label()
                )
            });
            return Err(MtError::new(codes::SHORTCUT_UNRESOLVED, reason));
        }

        Ok(item)
    }

    fn resolve_artisan_test_ctx(&self, root: &Path, is_laravel: bool) -> ArtisanTestContext {
        if !is_laravel || !is_regular_file(&root.join("artisan")) {
            return ArtisanTestContext {
                has_artisan_test: false,
            };
        }
        if let Some(ref svc) = self.artisan {
            if let Some(cat) = svc.peek_memory(root) {
                return ArtisanTestContext {
                    has_artisan_test: cat.commands.iter().any(|c| c.name == "test"),
                };
            }
        }
        if let Ok(Some(cached)) = crate::artisan::cache::read_cache_files(root) {
            return ArtisanTestContext {
                has_artisan_test: cached.commands.iter().any(|c| c.name == "test"),
            };
        }
        // Em projetos Laravel com `artisan` presente onde o catálogo ainda não foi gerado,
        // `php artisan test` é o runner padrão do Laravel (>= 8).
        ArtisanTestContext {
            has_artisan_test: true,
        }
    }
}
