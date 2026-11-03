//! Gerenciamento seguro de linked worktrees (`WorktreeManager` e `CliWorktreeManager`).

use std::fs;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use crate::discover::{classify_worktree_kind, discover_repository, extract_worktree_name};
use crate::error::{GitError, GitResult};
use crate::runner::GitRunner;
use crate::types::{WorktreeCreateRequest, WorktreeEntry, WorktreeKind, WorktreeLocation};

/// Valida o nome de uma worktree segundo a regex `^[A-Za-z0-9._-]{1,64}$`,
/// proibindo `.`, `..`, `/` e `\0`.
pub fn validate_worktree_name(name: &str) -> GitResult<()> {
    if name.is_empty() || name.len() > 64 {
        return Err(GitError::WorktreeNameInvalid {
            name: name.to_string(),
            reason: "O nome deve ter entre 1 e 64 caracteres.".to_string(),
        });
    }
    if name == "." || name == ".." || name.starts_with('-') {
        return Err(GitError::WorktreeNameInvalid {
            name: name.to_string(),
            reason: "O nome não pode ser '.', '..' ou iniciar com '-'.".to_string(),
        });
    }
    let valid_chars = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !valid_chars {
        return Err(GitError::WorktreeNameInvalid {
            name: name.to_string(),
            reason: "Use apenas letras ASCII, números, '.', '_' ou '-'.".to_string(),
        });
    }
    Ok(())
}

/// Valida um nome de branch seguro (proíbe opções iniciadas por `-`, `..`, espaços ou caracteres de controle).
pub fn validate_branch_or_ref(value: &str, field_label: &str) -> GitResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return Err(GitError::WorktreeNameInvalid {
            name: value.to_string(),
            reason: format!("{field_label} deve ter entre 1 e 128 caracteres."),
        });
    }
    if trimmed.starts_with('-')
        || trimmed.contains("..")
        || trimmed.contains("//")
        || trimmed.ends_with('/')
        || Path::new(trimmed)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lock"))
        || trimmed
            .bytes()
            .any(|b| b <= b' ' || matches!(b, b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\'))
    {
        return Err(GitError::WorktreeNameInvalid {
            name: value.to_string(),
            reason: format!("{field_label} contém sequência inválida para referência Git."),
        });
    }
    Ok(())
}

/// Trait para verificar se um diretório de worktree está em uso por algum painel ativo.
pub trait ActiveWorktreeGuard: Send + Sync + 'static {
    /// Retorna `true` se houver qualquer aba ou painel aberto cujo CWD esteja dentro de `worktree_path`.
    fn is_worktree_in_use(&self, worktree_path: &Path) -> bool;
}

/// Implementação nula de [`ActiveWorktreeGuard`] que nunca bloqueia (útil em testes isolados).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopActiveWorktreeGuard;

impl ActiveWorktreeGuard for NoopActiveWorktreeGuard {
    fn is_worktree_in_use(&self, _worktree_path: &Path) -> bool {
        false
    }
}

/// Trait de gerenciamento de worktrees para injeção em comandos IPC e testes.
#[async_trait]
pub trait WorktreeManager: Send + Sync + 'static {
    /// Lista todas as worktrees do repositório ao qual `project_root` pertence.
    async fn list(&self, project_root: &Path) -> GitResult<Vec<WorktreeEntry>>;

    /// Cria uma nova worktree gerenciada em `.worktrees/` ou `.max-code-worktrees/`.
    async fn create(&self, req: WorktreeCreateRequest) -> GitResult<WorktreeEntry>;

    /// Remove uma worktree gerenciada, validando uso ativo e alterações locais.
    async fn remove(&self, worktree_path: &Path, force: bool) -> GitResult<()>;
}

/// Implementação padrão de [`WorktreeManager`] apoiada em [`GitRunner`].
pub struct CliWorktreeManager {
    runner: Arc<dyn GitRunner>,
    active_guard: Arc<dyn ActiveWorktreeGuard>,
}

impl std::fmt::Debug for CliWorktreeManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliWorktreeManager").finish_non_exhaustive()
    }
}

impl CliWorktreeManager {
    /// Cria um novo [`CliWorktreeManager`].
    #[must_use]
    pub fn new(runner: Arc<dyn GitRunner>, active_guard: Arc<dyn ActiveWorktreeGuard>) -> Self {
        Self {
            runner,
            active_guard,
        }
    }
}

#[async_trait]
impl WorktreeManager for CliWorktreeManager {
    async fn list(&self, project_root: &Path) -> GitResult<Vec<WorktreeEntry>> {
        let discovered =
            discover_repository(project_root, None)?.ok_or_else(|| GitError::NotRepository {
                path: project_root.to_path_buf(),
            })?;
        self.runner
            .worktree_list(&discovered.main_worktree_path)
            .await
    }

    async fn create(&self, req: WorktreeCreateRequest) -> GitResult<WorktreeEntry> {
        validate_worktree_name(&req.name)?;

        let branch_name = match &req.branch {
            Some(b) if !b.trim().is_empty() => {
                validate_branch_or_ref(b, "A branch")?;
                b.trim().to_string()
            }
            _ => format!("wt/{}", req.name),
        };

        let base_ref = match &req.base_ref {
            Some(r) if !r.trim().is_empty() => {
                validate_branch_or_ref(r, "A referência base")?;
                r.trim().to_string()
            }
            _ => "HEAD".to_string(),
        };

        let discovered = discover_repository(&req.project_root, None)?.ok_or_else(|| {
            GitError::NotRepository {
                path: req.project_root.clone(),
            }
        })?;

        let parent_dir = match req.location {
            WorktreeLocation::MaxTerminal => discovered.main_worktree_path.join(".worktrees"),
            WorktreeLocation::MaxCode => discovered.main_worktree_path.join(".max-code-worktrees"),
        };

        fs::create_dir_all(&parent_dir).map_err(|err| GitError::InvalidPath {
            path: parent_dir.clone(),
            reason: format!("Não foi possível criar diretório de worktrees: {err}"),
        })?;

        let target_path = parent_dir.join(&req.name);
        if target_path.exists() {
            return Err(GitError::WorktreeConflict {
                target: target_path.display().to_string(),
                reason: format!(
                    "O diretório de destino '{}' já existe.",
                    target_path.display()
                ),
            });
        }

        let full_branch_ref = format!("refs/heads/{branch_name}");
        let branch_already_exists = self
            .runner
            .ref_exists(&discovered.main_worktree_path, &full_branch_ref)
            .await?;

        // Se a branch já existir, verificamos se ela já está em checkout em outra worktree.
        let existing_worktrees = self
            .runner
            .worktree_list(&discovered.main_worktree_path)
            .await?;
        if existing_worktrees
            .iter()
            .any(|wt| wt.branch.as_deref() == Some(branch_name.as_str()))
        {
            return Err(GitError::WorktreeConflict {
                target: branch_name.clone(),
                reason: format!("A branch '{branch_name}' já está ativa em outra worktree."),
            });
        }

        self.runner
            .worktree_add(
                &discovered.main_worktree_path,
                &target_path,
                &branch_name,
                &base_ref,
                !branch_already_exists,
            )
            .await?;

        let updated_list = self
            .runner
            .worktree_list(&discovered.main_worktree_path)
            .await?;
        let canonical_target = fs::canonicalize(&target_path).unwrap_or(target_path);

        if let Some(found) = updated_list
            .into_iter()
            .find(|wt| wt.path == canonical_target)
        {
            return Ok(found);
        }

        let kind = match req.location {
            WorktreeLocation::MaxTerminal => WorktreeKind::MaxTerminal,
            WorktreeLocation::MaxCode => WorktreeKind::MaxCode,
        };
        Ok(WorktreeEntry {
            name: extract_worktree_name(&canonical_target),
            path: canonical_target,
            head_oid: String::new(),
            branch: Some(branch_name),
            detached: false,
            bare: false,
            locked: None,
            prunable: None,
            kind,
            is_main: false,
        })
    }

    async fn remove(&self, worktree_path: &Path, force: bool) -> GitResult<()> {
        let discovered =
            discover_repository(worktree_path, None)?.ok_or_else(|| GitError::NotRepository {
                path: worktree_path.to_path_buf(),
            })?;

        let kind = classify_worktree_kind(
            &discovered.worktree_root,
            &discovered.main_worktree_path,
            discovered.is_linked,
        );

        if !matches!(kind, WorktreeKind::MaxTerminal | WorktreeKind::MaxCode) {
            return Err(GitError::WorktreeNotManaged {
                path: discovered.worktree_root,
            });
        }

        if self
            .active_guard
            .is_worktree_in_use(&discovered.worktree_root)
        {
            return Err(GitError::WorktreeInUse {
                path: discovered.worktree_root,
            });
        }

        if !force {
            let status = self.runner.status(&discovered.worktree_root).await?;
            if status.changes.dirty {
                return Err(GitError::WorktreeDirty {
                    path: discovered.worktree_root,
                });
            }
        }

        self.runner
            .worktree_remove(
                &discovered.main_worktree_path,
                &discovered.worktree_root,
                force,
            )
            .await
    }
}
