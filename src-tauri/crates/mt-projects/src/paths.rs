//! Resolução, expansão de `~`, canonicalização e validação de caminhos de projetos e raízes.

use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use mt_core::error::codes;
use mt_core::{MtError, MtResult};

/// Expande `"~"` e `"~/..."` usando o diretório home do usuário e rejeita caminhos relativos,
/// vazios, com bytes NUL, variáveis `$VAR` ou `~outro_usuario`.
pub fn expand_home(input: &str) -> MtResult<PathBuf> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(MtError::new(
            codes::PROJECT_INVALID_ARGUMENT,
            "O caminho da pasta não pode ser vazio.",
        ));
    }
    if trimmed.contains('\0') {
        return Err(MtError::new(
            codes::PROJECT_INVALID_ARGUMENT,
            "O caminho informado contém caracteres nulos inválidos.",
        ));
    }
    if trimmed.starts_with("file://") || trimmed.contains("://") {
        return Err(MtError::new(
            codes::PROJECT_INVALID_ARGUMENT,
            "URIs não são aceitas; informe um caminho absoluto do sistema de arquivos.",
        ));
    }
    if trimmed.contains('$') {
        return Err(MtError::new(
            codes::PROJECT_INVALID_ARGUMENT,
            "Variáveis de ambiente não são expandidas no caminho do projeto.",
        ));
    }

    if trimmed == "~" {
        return resolve_home_dir();
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = resolve_home_dir()?;
        return Ok(home.join(rest));
    }
    if trimmed.starts_with('~') {
        return Err(MtError::new(
            codes::PROJECT_INVALID_ARGUMENT,
            "Expansão de diretório pessoal de outro usuário (~usuario) não é suportada.",
        ));
    }

    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(MtError::new(
            codes::PROJECT_INVALID_ARGUMENT,
            "O caminho informado deve ser absoluto ou iniciar com '~/'.",
        ));
    }

    Ok(path)
}

/// Expande `~` e resolve o caminho canônico real no sistema de arquivos, exigindo que exista e seja diretório.
pub fn canonical_dir(input: &str) -> MtResult<PathBuf> {
    let expanded = expand_home(input)?;
    canonicalize_existing_dir(&expanded)
}

/// Valida e canonicaliza um caminho existente exigindo que seja diretório e representável em UTF-8.
pub fn canonicalize_existing_dir(path: &Path) -> MtResult<PathBuf> {
    let meta = fs::metadata(path).map_err(|err| map_path_io_error(&err))?;
    if !meta.is_dir() {
        return Err(MtError::new(
            codes::PROJECT_PATH_NOT_DIRECTORY,
            "O caminho informado não é um diretório.",
        ));
    }

    let canonical = fs::canonicalize(path).map_err(|err| map_path_io_error(&err))?;
    let canon_meta = fs::metadata(&canonical).map_err(|err| map_path_io_error(&err))?;
    if !canon_meta.is_dir() {
        return Err(MtError::new(
            codes::PROJECT_PATH_NOT_DIRECTORY,
            "O destino resolvido do caminho não é um diretório.",
        ));
    }

    if canonical.to_str().is_none() {
        return Err(MtError::new(
            codes::PROJECT_PATH_NOT_UTF8,
            "O caminho canônico contém bytes que não são UTF-8 válido.",
        ));
    }

    Ok(canonical)
}

/// Valida e canonicaliza o diretório de um projeto individual, proibindo `/` e `$HOME`.
pub fn validate_project_dir(input: &str) -> MtResult<(PathBuf, String)> {
    let canonical = canonical_dir(input)?;
    if canonical == Path::new("/") {
        return Err(MtError::new(
            codes::PROJECT_PATH_FORBIDDEN,
            "A raiz do sistema ('/') não pode ser cadastrada como um projeto.",
        ));
    }

    if let Ok(home) = resolve_home_dir() {
        let canon_home = fs::canonicalize(&home).unwrap_or(home);
        if canonical == canon_home {
            return Err(MtError::new(
                codes::PROJECT_PATH_FORBIDDEN,
                "A pasta pessoal ($HOME) não pode ser cadastrada diretamente como um projeto.",
            ));
        }
    }

    let utf8 = canonical
        .to_str()
        .ok_or_else(|| {
            MtError::new(
                codes::PROJECT_PATH_NOT_UTF8,
                "O caminho canônico contém bytes que não são UTF-8 válido.",
            )
        })?
        .to_string();

    Ok((canonical, utf8))
}

/// Valida e canonicaliza o diretório de uma Pasta de Projetos (`project_roots`), proibindo `/`.
pub fn validate_root_dir(input: &str) -> MtResult<(PathBuf, String)> {
    let canonical = canonical_dir(input)?;
    if canonical == Path::new("/") {
        return Err(MtError::new(
            codes::PROJECT_PATH_FORBIDDEN,
            "A raiz do sistema ('/') não pode ser cadastrada como Pasta de Projetos.",
        ));
    }

    let utf8 = canonical
        .to_str()
        .ok_or_else(|| {
            MtError::new(
                codes::PROJECT_PATH_NOT_UTF8,
                "O caminho canônico contém bytes que não são UTF-8 válido.",
            )
        })?
        .to_string();

    Ok((canonical, utf8))
}

/// Indica se o nome de uma entrada de diretório começa com `.` (ex.: `.git`, `.worktrees`, `.max-code-worktrees`).
#[must_use]
pub fn is_hidden(name: &OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// Indica se o caminho canônico está localizado dentro de `.git`, `.worktrees` ou `.max-code-worktrees`.
#[must_use]
pub fn is_inside_ignored_discovery_dir(path: &Path) -> bool {
    for comp in path.components() {
        if let Component::Normal(os) = comp {
            let s = os.to_string_lossy();
            if s == ".git" || s == ".worktrees" || s == ".max-code-worktrees" {
                return true;
            }
        }
    }
    false
}

/// Extrai o título padrão a partir do último componente do caminho, sem alterar a caixa.
#[must_use]
pub fn default_title(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| String::from("Projeto"))
}

fn resolve_home_dir() -> MtResult<PathBuf> {
    if let Some(env_home) = std::env::var_os("HOME")
        && !env_home.is_empty()
    {
        let p = PathBuf::from(env_home);
        if p.is_absolute() {
            return Ok(p);
        }
    }
    dirs::home_dir().ok_or_else(|| {
        MtError::new(
            codes::PATHS_UNRESOLVED,
            "Não foi possível determinar o diretório pessoal ($HOME) do usuário.",
        )
    })
}

fn map_path_io_error(err: &std::io::Error) -> MtError {
    match err.kind() {
        ErrorKind::NotFound => MtError::new(
            codes::PROJECT_PATH_NOT_FOUND,
            "A pasta informada não foi encontrada no sistema de arquivos.",
        ),
        ErrorKind::NotADirectory => MtError::new(
            codes::PROJECT_PATH_NOT_DIRECTORY,
            "O caminho informado não é um diretório.",
        ),
        ErrorKind::PermissionDenied => MtError::new(
            codes::PERMISSION_DENIED,
            "Permissão negada ao acessar a pasta informada.",
        ),
        _ => MtError::new(
            codes::IO,
            "Não foi possível acessar o caminho informado no sistema de arquivos.",
        ),
    }
}
