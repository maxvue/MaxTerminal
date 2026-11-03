//! Resolução da raiz Laravel a partir do `cwd` da aba/painel respeitando fronteiras de Git e worktrees.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use mt_core::{ErrorCode, MtError, MtResult, ProjectId};
use serde::Deserialize;

use crate::artisan::model::{ArtisanRoot, LaravelLayout};
use crate::error;
use crate::fs_util::read_capped;
use crate::ports::{ArtisanRunner, Clock, ProjectLookup};

const ROOT_CACHE_TTL_MS: i64 = 30_000;
const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const MAX_BOOTSTRAP_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    cwd: PathBuf,
    project_id: Option<ProjectId>,
}

/// Resolvedor de raízes Laravel com cache TTL de 30 s.
pub struct RootResolver {
    lookup: Arc<dyn ProjectLookup>,
    clock: Arc<dyn Clock>,
    memo: DashMap<CacheKey, (i64, Option<ArtisanRoot>)>,
}

impl RootResolver {
    /// Cria um novo [`RootResolver`].
    #[must_use]
    pub fn new(lookup: Arc<dyn ProjectLookup>, clock: Arc<dyn Clock>) -> Self {
        Self {
            lookup,
            clock,
            memo: DashMap::new(),
        }
    }

    /// Resolve a raiz Laravel mais próxima de `cwd` sem ultrapassar a fronteira do projeto ou de `.git`.
    pub fn resolve(
        &self,
        cwd: &Path,
        expected_project_id: Option<ProjectId>,
        runner: &dyn ArtisanRunner,
    ) -> MtResult<Option<ArtisanRoot>> {
        let canonical_cwd = fs::canonicalize(cwd).map_err(|e| {
            MtError::new(
                ErrorCode("core.not_found"),
                format!("Diretório '{}' não encontrado: {e}", cwd.display()),
            )
        })?;

        let key = CacheKey {
            cwd: canonical_cwd.clone(),
            project_id: expected_project_id,
        };
        let now = self.clock.now_ms();
        if let Some(entry) = self.memo.get(&key) {
            let (at, ref cached) = *entry;
            if now.saturating_sub(at) <= ROOT_CACHE_TTL_MS {
                return Ok(cached.clone());
            }
        }

        let Some((owning_id, project_path)) = self.lookup.owning_project(&canonical_cwd) else {
            self.memo.insert(key, (now, None));
            return Ok(None);
        };

        let boundary_path = if let Some(req_id) = expected_project_id {
            if req_id != owning_id {
                let Some(explicit_proj_path) = self.lookup.project_by_id(req_id) else {
                    self.memo.insert(key, (now, None));
                    return Ok(None);
                };
                let canon_proj =
                    fs::canonicalize(&explicit_proj_path).unwrap_or(explicit_proj_path);
                if !canonical_cwd.starts_with(&canon_proj) {
                    self.memo.insert(key, (now, None));
                    return Ok(None);
                }
                canon_proj
            } else {
                fs::canonicalize(&project_path).unwrap_or(project_path)
            }
        } else {
            fs::canonicalize(&project_path).unwrap_or(project_path)
        };

        let effective_project_id = expected_project_id.unwrap_or(owning_id);

        let mut dir = canonical_cwd.as_path();
        loop {
            let artisan_file = dir.join("artisan");
            let composer_file = dir.join("composer.json");
            if is_regular_non_symlink(&artisan_file) && is_laravel_composer(&composer_file)? {
                let root_info = ArtisanRoot {
                    root: dir.display().to_string(),
                    project_id: Some(effective_project_id),
                    is_worktree: is_worktree(dir),
                    invoker: runner.invoker(dir, &canonical_cwd),
                    invoker_tokens: runner.invoker_tokens(dir, &canonical_cwd),
                    runner: runner.kind(),
                    layout: detect_layout(dir),
                    laravel_version: laravel_version(dir),
                };
                self.memo.insert(key, (now, Some(root_info.clone())));
                return Ok(Some(root_info));
            }

            if dir == boundary_path || has_git_boundary(dir) {
                self.memo.insert(key, (now, None));
                return Ok(None);
            }

            match dir.parent() {
                Some(parent) if parent.starts_with(&boundary_path) => dir = parent,
                _ => {
                    self.memo.insert(key, (now, None));
                    return Ok(None);
                }
            }
        }
    }

    /// Valida que `root` é uma raiz Laravel autorizada e retorna seu [`ArtisanRoot`].
    pub fn require_authorized_laravel_root(
        &self,
        root: &Path,
        runner: &dyn ArtisanRunner,
    ) -> MtResult<(PathBuf, ArtisanRoot)> {
        let canonical_root = fs::canonicalize(root).map_err(|_| {
            error::root_forbidden(format!(
                "A raiz informada '{}' não existe ou não está acessível.",
                root.display()
            ))
        })?;

        let Some((project_id, project_path)) = self.lookup.owning_project(&canonical_root) else {
            return Err(error::root_forbidden(format!(
                "O diretório '{}' não pertence a nenhum projeto cadastrado no MaxTerminal.",
                canonical_root.display()
            )));
        };

        let canon_proj = fs::canonicalize(&project_path).unwrap_or(project_path);
        if !canonical_root.starts_with(&canon_proj) {
            return Err(error::root_forbidden(format!(
                "O diretório '{}' está fora da fronteira do projeto.",
                canonical_root.display()
            )));
        }

        let artisan_file = canonical_root.join("artisan");
        let composer_file = canonical_root.join("composer.json");
        if !is_regular_non_symlink(&artisan_file) || !is_laravel_composer(&composer_file)? {
            return Err(error::not_laravel(format!(
                "A pasta '{}' não contém uma aplicação Laravel válida (`artisan` + `laravel/framework`).",
                canonical_root.display()
            )));
        }

        let info = ArtisanRoot {
            root: canonical_root.display().to_string(),
            project_id: Some(project_id),
            is_worktree: is_worktree(&canonical_root),
            invoker: runner.invoker(&canonical_root, &canonical_root),
            invoker_tokens: runner.invoker_tokens(&canonical_root, &canonical_root),
            runner: runner.kind(),
            layout: detect_layout(&canonical_root),
            laravel_version: laravel_version(&canonical_root),
        };
        Ok((canonical_root, info))
    }

    /// Limpa o cache em memória de raízes resolvidas.
    pub fn clear_cache(&self) {
        self.memo.clear();
    }
}

fn is_regular_non_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| !m.file_type().is_symlink() && m.is_file())
        .unwrap_or(false)
}

fn has_git_boundary(dir: &Path) -> bool {
    dir.join(".git").symlink_metadata().is_ok()
}

/// Verifica se `composer.json` declara `laravel/framework` em `require` ou `require-dev` (exclui Lumen).
pub fn is_laravel_composer(path: &Path) -> MtResult<bool> {
    let Some(content) = read_capped(path, MAX_MANIFEST_BYTES)
        .map_err(|_| Ok::<bool, MtError>(false))
        .unwrap_or(None)
    else {
        return Ok(false);
    };

    #[derive(Deserialize)]
    struct PartialComposer {
        #[serde(default)]
        require: std::collections::HashMap<String, serde_json::Value>,
        #[serde(default, rename = "require-dev")]
        require_dev: std::collections::HashMap<String, serde_json::Value>,
    }

    let Ok(parsed) = serde_json::from_str::<PartialComposer>(&content) else {
        return Ok(false);
    };

    if parsed.require.contains_key("laravel/lumen-framework")
        && !parsed.require.contains_key("laravel/framework")
    {
        return Ok(false);
    }

    Ok(parsed.require.contains_key("laravel/framework")
        || parsed.require_dev.contains_key("laravel/framework"))
}

/// Detecta se `dir` é uma Git worktree (`.max-code-worktrees`, `.worktrees` ou `.git` arquivo com `gitdir:`).
#[must_use]
pub fn is_worktree(dir: &Path) -> bool {
    for comp in dir.components() {
        if let std::path::Component::Normal(os) = comp {
            if os == ".max-code-worktrees" || os == ".worktrees" {
                return true;
            }
        }
    }
    let git_entry = dir.join(".git");
    if let Ok(Some(text)) = read_capped(&git_entry, 4096) {
        if text.trim_start().starts_with("gitdir:") {
            return true;
        }
    }
    false
}

/// Detecta o layout Laravel (`Modern` se `bootstrap/app.php` usa `Application::configure` e carrega comandos/console; `Legacy` se `app/Console/Kernel.php` existe).
#[must_use]
pub fn detect_layout(dir: &Path) -> LaravelLayout {
    let bootstrap = dir.join("bootstrap/app.php");
    if let Ok(Some(content)) = read_capped(&bootstrap, MAX_BOOTSTRAP_BYTES) {
        let stripped = strip_php_comments(&content);
        if stripped.contains("Application::configure")
            && (stripped.contains("withRouting") || stripped.contains("commands:"))
        {
            return LaravelLayout::Modern;
        }
    }
    if is_regular_non_symlink(&dir.join("app/Console/Kernel.php")) {
        return LaravelLayout::Legacy;
    }
    LaravelLayout::Unknown
}

fn strip_php_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(bytes.len());
        } else if bytes[i] == b'#' && (i + 1 >= bytes.len() || bytes[i + 1] != b'[') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Extrai a versão de `laravel/framework` a partir de `composer.lock` sem o prefixo `v`.
#[must_use]
pub fn laravel_version(dir: &Path) -> Option<String> {
    let lock_path = dir.join("composer.lock");
    let content = read_capped(&lock_path, MAX_MANIFEST_BYTES).ok()??;

    #[derive(Deserialize)]
    struct LockPackage {
        name: String,
        version: String,
    }
    #[derive(Deserialize)]
    struct ComposerLock {
        #[serde(default)]
        packages: Vec<LockPackage>,
        #[serde(default, rename = "packages-dev")]
        packages_dev: Vec<LockPackage>,
    }

    let lock: ComposerLock = serde_json::from_str(&content).ok()?;
    lock.packages
        .into_iter()
        .chain(lock.packages_dev)
        .find(|p| p.name == "laravel/framework")
        .map(|p| p.version.trim_start_matches('v').to_string())
}
