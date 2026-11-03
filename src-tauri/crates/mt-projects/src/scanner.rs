//! Varredura de nível 1 (`scan_level1`) e cálculo de diferenças (`diff`) de Pastas de Projetos.

use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use mt_core::error::codes;
use mt_core::{MtError, MtResult};

use crate::model::{ProjectRootId, ScanWarning};
use crate::paths::{is_hidden, is_inside_ignored_discovery_dir};

/// Entrada válida de nível 1 observada dentro de uma Pasta de Projetos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanEntry {
    /// Caminho absoluto lexical da entrada imediata (`<root_canonico>/<nome>`).
    pub entry_path: PathBuf,
    /// Caminho absoluto canônico resolvido (após seguir symlink de nível 1, se houver).
    pub canonical_path: PathBuf,
    /// Nome da entrada imediata na pasta-mãe.
    pub name: String,
}

/// Linha de origem persistida em `project_root_entries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootEntryRow {
    /// Identificador da Pasta de Projetos proprietária.
    pub root_id: ProjectRootId,
    /// Caminho lexical da entrada imediata.
    pub entry_path: String,
    /// Caminho canônico resolvido na última observação.
    pub canonical_path: String,
    /// Timestamp UNIX em ms da observação.
    pub observed_at: i64,
}

/// Resultado da varredura de nível 1 de uma Pasta de Projetos.
#[derive(Debug, Clone, Default)]
pub struct ScanSnapshot {
    /// Entradas de diretório válidas ordenadas por `(name, entry_path)`.
    pub entries: Vec<ScanEntry>,
    /// Avisos não fatais coletados durante a leitura.
    pub warnings: Vec<ScanWarning>,
    /// Indica se o diretório inteiro foi lido sem falhas parciais de permissão ou I/O.
    pub complete: bool,
    /// Quantidade de itens ignorados (ocultos, arquivos regulares, links quebrados, worktrees).
    pub skipped_non_candidates: u32,
}

/// Diferença calculada entre o [`ScanSnapshot`] atual e as origens anteriormente conhecidas.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanDiff {
    /// Entradas presentes que devem ser inseridas ou atualizadas em `project_root_entries`.
    pub upserted_entries: Vec<ScanEntry>,
    /// Caminhos `entry_path` que deixaram de existir confirmadamente.
    pub removed_entry_paths: Vec<String>,
}

/// Lista subpastas de nível 1 de `root` sem recursão.
///
/// - Aceita diretórios reais e symlinks de nível 1 que apontem para diretórios válidos.
/// - Ignora entradas ocultas (`.git`, `.worktrees`, `.max-code-worktrees`, etc.), arquivos regulares,
///   links simbólicos quebrados e symlinks apontando para a própria raiz, ancestral, `/` ou `$HOME`.
/// - Erros de permissão/I/O em entradas individuais marcam `complete = false` e geram `ScanWarning`
///   sem abortar a varredura.
#[allow(clippy::too_many_lines)]
pub fn scan_level1(root: &Path) -> MtResult<ScanSnapshot> {
    let canonical_root = match fs::canonicalize(root) {
        Ok(p) => p,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return Err(MtError::new(
                codes::PROJECT_ROOT_MISSING,
                "A Pasta de Projetos não foi encontrada no disco.",
            ));
        }
        Err(err) => {
            return Err(MtError::new(
                codes::PROJECT_ROOT_SCAN_FAILED,
                "Não foi possível acessar a Pasta de Projetos para varredura.",
            )
            .with_source(err));
        }
    };

    let read_dir = fs::read_dir(&canonical_root).map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            MtError::new(
                codes::PROJECT_ROOT_MISSING,
                "A Pasta de Projetos não foi encontrada no disco.",
            )
        } else {
            MtError::new(
                codes::PROJECT_ROOT_SCAN_FAILED,
                "Não foi possível listar as subpastas da Pasta de Projetos.",
            )
            .with_source(err)
        }
    })?;

    let home_canonical = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .map(|h| fs::canonicalize(&h).unwrap_or(h));

    let mut snapshot = ScanSnapshot {
        complete: true,
        ..ScanSnapshot::default()
    };

    for item in read_dir {
        let dir_entry = match item {
            Ok(e) => e,
            Err(err) => {
                snapshot.complete = false;
                snapshot.warnings.push(ScanWarning {
                    path: canonical_root.to_string_lossy().into_owned(),
                    code: codes::IO.0.to_string(),
                    message: format!("Erro ao ler entrada da Pasta de Projetos: {}", err.kind()),
                });
                continue;
            }
        };

        let file_name = dir_entry.file_name();
        if is_hidden(&file_name) {
            snapshot.skipped_non_candidates = snapshot.skipped_non_candidates.saturating_add(1);
            continue;
        }

        let Some(name_str) = file_name.to_str() else {
            snapshot.skipped_non_candidates = snapshot.skipped_non_candidates.saturating_add(1);
            snapshot.warnings.push(ScanWarning {
                path: canonical_root
                    .join(&file_name)
                    .to_string_lossy()
                    .into_owned(),
                code: codes::PROJECT_PATH_NOT_UTF8.0.to_string(),
                message: String::from("Subpasta ignorada por conter nome fora do padrão UTF-8."),
            });
            continue;
        };

        let entry_path = canonical_root.join(name_str);
        if entry_path.to_str().is_none() {
            snapshot.skipped_non_candidates = snapshot.skipped_non_candidates.saturating_add(1);
            continue;
        }

        match fs::metadata(&entry_path) {
            Ok(meta) => {
                if !meta.is_dir() {
                    snapshot.skipped_non_candidates =
                        snapshot.skipped_non_candidates.saturating_add(1);
                    continue;
                }

                let canonical_target = match fs::canonicalize(&entry_path) {
                    Ok(c) => c,
                    Err(err) => {
                        if err.kind() == ErrorKind::PermissionDenied {
                            snapshot.complete = false;
                            snapshot.warnings.push(ScanWarning {
                                path: entry_path.to_string_lossy().into_owned(),
                                code: codes::PERMISSION_DENIED.0.to_string(),
                                message: String::from(
                                    "Permissão negada ao resolver o destino da subpasta.",
                                ),
                            });
                        } else {
                            snapshot.skipped_non_candidates =
                                snapshot.skipped_non_candidates.saturating_add(1);
                        }
                        continue;
                    }
                };

                if canonical_target.to_str().is_none() {
                    snapshot.skipped_non_candidates =
                        snapshot.skipped_non_candidates.saturating_add(1);
                    snapshot.warnings.push(ScanWarning {
                        path: entry_path.to_string_lossy().into_owned(),
                        code: codes::PROJECT_PATH_NOT_UTF8.0.to_string(),
                        message: String::from(
                            "Destino da subpasta ignorado por não ser UTF-8 válido.",
                        ),
                    });
                    continue;
                }

                // Ignora symlink que aponte para a própria raiz, um ancestral da raiz, `/`, `$HOME`
                // ou diretórios ocultos de VCS/worktree
                if canonical_target == Path::new("/")
                    || canonical_target == canonical_root
                    || canonical_root.starts_with(&canonical_target)
                    || is_inside_ignored_discovery_dir(&canonical_target)
                    || home_canonical
                        .as_ref()
                        .is_some_and(|h| &canonical_target == h)
                {
                    snapshot.skipped_non_candidates =
                        snapshot.skipped_non_candidates.saturating_add(1);
                    continue;
                }

                snapshot.entries.push(ScanEntry {
                    entry_path,
                    canonical_path: canonical_target,
                    name: name_str.to_string(),
                });
            }
            Err(err) => {
                if err.kind() == ErrorKind::NotFound {
                    // Pode ser um symlink quebrado ou entrada removida durante a listagem
                    snapshot.skipped_non_candidates =
                        snapshot.skipped_non_candidates.saturating_add(1);
                } else if err.kind() == ErrorKind::PermissionDenied {
                    snapshot.complete = false;
                    snapshot.warnings.push(ScanWarning {
                        path: entry_path.to_string_lossy().into_owned(),
                        code: codes::PERMISSION_DENIED.0.to_string(),
                        message: String::from("Permissão negada ao inspecionar subpasta."),
                    });
                } else {
                    // Em caso de loop de symlink (ELOOP) ou não-diretório, ignora como não-candidato
                    let is_broken_symlink =
                        fs::symlink_metadata(&entry_path).is_ok_and(|m| m.file_type().is_symlink());
                    if is_broken_symlink {
                        snapshot.skipped_non_candidates =
                            snapshot.skipped_non_candidates.saturating_add(1);
                    } else {
                        snapshot.complete = false;
                        snapshot.warnings.push(ScanWarning {
                            path: entry_path.to_string_lossy().into_owned(),
                            code: codes::IO.0.to_string(),
                            message: String::from(
                                "Falha de leitura ao inspecionar subpasta da Pasta de Projetos.",
                            ),
                        });
                    }
                }
            }
        }
    }

    snapshot.entries.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.entry_path.cmp(&b.entry_path))
    });

    Ok(snapshot)
}

/// Calcula as entradas adicionadas/atualizadas e removidas em relação ao estado persistido anterior.
///
/// Quando `current.complete == false`, não remove origens que falharam por permissão/I/O; remove
/// apenas aquelas cuja ausência ou quebra de link simbólico seja comprovada.
#[must_use]
pub fn diff(current: &ScanSnapshot, previous: &[RootEntryRow]) -> ScanDiff {
    let current_set: HashSet<&str> = current
        .entries
        .iter()
        .filter_map(|e| e.entry_path.to_str())
        .collect();

    let mut removed_entry_paths = Vec::new();
    for prev in previous {
        if !current_set.contains(prev.entry_path.as_str()) {
            if current.complete {
                removed_entry_paths.push(prev.entry_path.clone());
            } else {
                // Em scan parcial, só remove se a entrada comprovadamente não existe ou é link quebrado
                let path = Path::new(&prev.entry_path);
                let confirmed_gone = match fs::metadata(path) {
                    Ok(meta) => !meta.is_dir(),
                    Err(err) => err.kind() == ErrorKind::NotFound,
                };
                if confirmed_gone {
                    removed_entry_paths.push(prev.entry_path.clone());
                }
            }
        }
    }

    ScanDiff {
        upserted_entries: current.entries.clone(),
        removed_entry_paths,
    }
}
