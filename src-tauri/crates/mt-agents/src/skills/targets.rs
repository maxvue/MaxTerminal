//! Resolução canônica de diretórios de destino por agente CLI e escopo (`Project` vs `Global`).

use std::path::{Component, Path, PathBuf};

use mt_core::MtResult;

use crate::error;
use crate::ports::ProjectRecord;
use crate::registry::AgentKind;
use crate::skills::{SkillSyncTarget, SyncScope};

/// Calcula o diretório padrão ou com override de skills para um agente e escopo (`spec.md` §2.11, §7.5).
pub fn agent_skill_dir(
    agent: AgentKind,
    scope: SyncScope,
    project_root: Option<&Path>,
    dest_override: Option<&Path>,
    home: &Path,
    xdg_config_home: &Path,
) -> MtResult<PathBuf> {
    if let Some(ov) = dest_override {
        if ov.as_os_str().is_empty() {
            return Err(error::skills_target_invalid(
                "Caminho de destino personalizado vazio",
            ));
        }
        return Ok(ov.to_path_buf());
    }

    match (agent, scope) {
        (AgentKind::Agy, _) => Err(error::skills_capability_unverified(
            "O agente Antigravity (agy) requer um diretório de destino explícito (destOverride)",
        )),
        (AgentKind::Claude, SyncScope::Project) => {
            let root = project_root.ok_or_else(|| {
                error::skills_target_invalid("Escopo de projeto requer raiz do projeto")
            })?;
            Ok(root.join(".claude").join("skills"))
        }
        (AgentKind::Claude, SyncScope::Global) => Ok(home.join(".claude").join("skills")),
        (AgentKind::Codex, SyncScope::Project) => {
            let root = project_root.ok_or_else(|| {
                error::skills_target_invalid("Escopo de projeto requer raiz do projeto")
            })?;
            Ok(root.join(".agents").join("skills"))
        }
        (AgentKind::Codex, SyncScope::Global) => Ok(home.join(".agents").join("skills")),
        (AgentKind::OpenCode, SyncScope::Project) => {
            let root = project_root.ok_or_else(|| {
                error::skills_target_invalid("Escopo de projeto requer raiz do projeto")
            })?;
            Ok(root.join(".opencode").join("skills"))
        }
        (AgentKind::OpenCode, SyncScope::Global) => {
            Ok(xdg_config_home.join("opencode").join("skills"))
        }
    }
}

/// Resolve e valida o diretório de destino e sua forma canônica para um `SkillSyncTarget`.
///
/// Verifica:
/// - Coerência entre `scope` e `project_id`.
/// - Quando `project_root` é informado (ex.: worktree), valida que pertence ao mesmo repositório Git do projeto.
/// - Recusa symlinks ancestrais dentro do projeto que escapem da raiz do projeto, salvo `dest_override` explícito (`CA-35`).
pub fn resolve_target_dest_dir(
    target: &SkillSyncTarget,
    project: Option<&ProjectRecord>,
    home: &Path,
    xdg_config_home: &Path,
) -> MtResult<(PathBuf, String)> {
    match target.scope {
        SyncScope::Project => {
            if target.project_id.is_none() {
                return Err(error::skills_target_invalid(
                    "Alvo de escopo 'project' exige projectId",
                ));
            }
        }
        SyncScope::Global => {
            if target.project_id.is_some() {
                return Err(error::skills_target_invalid(
                    "Alvo de escopo 'global' não deve possuir projectId",
                ));
            }
        }
    }

    let effective_project_root: Option<PathBuf> = if target.scope == SyncScope::Project {
        let proj = project.ok_or_else(|| {
            error::skills_target_invalid("Projeto não encontrado para o alvo de skills")
        })?;
        let canonical_proj = canonical_or_lexical_path(&proj.path);

        if let Some(ref custom_root_str) = target.project_root
            && !custom_root_str.trim().is_empty()
        {
            let custom_root = PathBuf::from(custom_root_str.trim());
            let canonical_custom = canonical_or_lexical_path(&custom_root);
            if canonical_custom != canonical_proj && !canonical_custom.starts_with(&canonical_proj)
            {
                // Verifica se ambos pertencem ao mesmo common_dir Git (worktree externa como .max-code-worktrees)
                let same_repo = match (
                    mt_git::discover_repository(&canonical_proj, None),
                    mt_git::discover_repository(&canonical_custom, None),
                ) {
                    (Ok(Some(d1)), Ok(Some(d2))) => {
                        canonical_or_lexical_path(&d1.common_dir)
                            == canonical_or_lexical_path(&d2.common_dir)
                    }
                    _ => false,
                };
                if !same_repo {
                    return Err(error::skills_target_invalid(
                        "projectRoot informado não pertence ao repositório do projeto",
                    ));
                }
            }
            Some(canonical_custom)
        } else {
            Some(canonical_proj)
        }
    } else {
        None
    };

    let dest_override_path = target
        .dest_override
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| expand_tilde(s, home));

    let dest_dir = agent_skill_dir(
        target.agent,
        target.scope,
        effective_project_root.as_deref(),
        dest_override_path.as_deref(),
        home,
        xdg_config_home,
    )?;

    // Se não houve dest_override explícito e estamos em escopo de projeto, verifica se algum ancestral é symlink que escapa da raiz
    if dest_override_path.is_none()
        && let Some(ref root) = effective_project_root
    {
        verify_no_escaping_symlink_ancestors(root, &dest_dir)?;
    }

    let canonical_str = canonical_or_lexical_path(&dest_dir)
        .to_string_lossy()
        .to_string();
    Ok((dest_dir, canonical_str))
}

fn verify_no_escaping_symlink_ancestors(project_root: &Path, dest_dir: &Path) -> MtResult<()> {
    let Ok(rel) = dest_dir.strip_prefix(project_root) else {
        return Err(error::skills_target_invalid(
            "Diretório de destino escapa da raiz do projeto",
        ));
    };

    let mut cur = project_root.to_path_buf();
    for comp in rel.components() {
        if matches!(comp, Component::ParentDir) {
            return Err(error::skills_target_invalid(
                "Caminho de destino não pode conter '..'",
            ));
        }
        cur.push(comp);
        if let Ok(meta) = std::fs::symlink_metadata(&cur) {
            if meta.file_type().is_symlink() {
                let resolved = std::fs::canonicalize(&cur).map_err(|e| {
                    error::skills_target_invalid(format!(
                        "Ancestral symlink inválido em {}: {e}",
                        cur.display()
                    ))
                })?;
                if !resolved.starts_with(project_root) {
                    return Err(error::skills_target_invalid(format!(
                        "Ancestral {} é um symlink que escapa do escopo do projeto para {}",
                        cur.display(),
                        resolved.display()
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Resolve o caminho canônico real quando existente (ou do ancestral mais próximo existente combinado com o sufixo lexical).
#[must_use]
pub fn canonical_or_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    if let Ok(canon) = std::fs::canonicalize(&normalized) {
        return canon;
    }

    let mut suffix = Vec::new();
    let mut cur = normalized.as_path();
    while let Some(parent) = cur.parent() {
        if let Some(name) = cur.file_name() {
            suffix.push(name.to_os_string());
        }
        if let Ok(canon_parent) = std::fs::canonicalize(parent) {
            let mut result = canon_parent;
            for part in suffix.into_iter().rev() {
                result.push(part);
            }
            return result;
        }
        cur = parent;
    }

    normalized
}

/// Expande `~/` para o diretório home informado.
#[must_use]
pub fn expand_tilde(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(raw)
    }
}
