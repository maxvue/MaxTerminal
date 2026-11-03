//! Classificação dry-run de conflitos (`ForeignSymlink`, `Unmanaged`, `LocallyModified`) e cálculo de `fingerprint`.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::skills::catalog::{SKILL_MANIFEST_FILENAME, compute_skill_tree_hash};
use crate::skills::targets::canonical_or_lexical_path;
use crate::skills::{
    ConflictKind, SkillCopyManifest, SkillEntry, SkillRelevance, SkillSelection, SyncAction,
    SyncMode,
};

/// Registro persistido em `skill_sync_entries` para um alvo.
#[derive(Debug, Clone)]
pub struct ManagedEntryRecord {
    /// Nome da skill.
    pub skill_name: String,
    /// Caminho instalado no destino.
    pub dest_path: String,
    /// Modo em que foi sincronizada (`Symlink` ou `Copy`).
    pub mode: SyncMode,
    /// Hash da fonte quando sincronizada.
    pub source_hash: String,
    /// Caminho da fonte quando sincronizada.
    pub source_path: String,
    /// Hash sincronizado no destino.
    pub synced_hash: String,
}

/// Filtra as skills do catálogo de acordo com `SkillSelection`.
#[must_use]
pub fn select_skills<'a>(
    skills: &'a [SkillEntry],
    selection: &SkillSelection,
) -> Vec<&'a SkillEntry> {
    match selection {
        SkillSelection::All => skills.iter().collect(),
        SkillSelection::Recommended => skills
            .iter()
            .filter(|s| s.relevance == SkillRelevance::Recommended)
            .collect(),
        SkillSelection::Explicit { names } => {
            let set: HashSet<&str> = names.iter().map(String::as_str).collect();
            skills
                .iter()
                .filter(|s| set.contains(s.name.as_str()))
                .collect()
        }
    }
}

/// Planeja todas as ações (`Create`, `Update`, `Unchanged`, `Conflict`, `Prune`) para um destino sem escrever no disco (`CA-27`).
pub fn classify_target_actions(
    dest_dir: &Path,
    mode: SyncMode,
    selected_skills: &[&SkillEntry],
    managed_entries: &BTreeMap<String, ManagedEntryRecord>,
) -> Vec<SyncAction> {
    let mut actions = Vec::new();
    let mut selected_names = HashSet::new();

    for skill in selected_skills {
        selected_names.insert(skill.name.clone());
        let dest_skill_path = dest_dir.join(&skill.name);
        let dest_str = dest_skill_path.to_string_lossy().to_string();
        let managed = managed_entries.get(&skill.name);

        let Ok(meta) = std::fs::symlink_metadata(&dest_skill_path) else {
            actions.push(SyncAction::Create {
                skill: skill.name.clone(),
                dest: dest_str,
            });
            continue;
        };

        let ftype = meta.file_type();
        if ftype.is_symlink() {
            let link_target = std::fs::read_link(&dest_skill_path).unwrap_or_default();
            let resolved_target = if link_target.is_absolute() {
                canonical_or_lexical_path(&link_target)
            } else {
                canonical_or_lexical_path(&dest_dir.join(&link_target))
            };
            let canonical_source = canonical_or_lexical_path(Path::new(&skill.source_dir));

            if resolved_target == canonical_source {
                if let Some(m) = managed {
                    if m.mode == SyncMode::Symlink && mode == SyncMode::Symlink {
                        if m.synced_hash == skill.hash {
                            actions.push(SyncAction::Unchanged {
                                skill: skill.name.clone(),
                                dest: dest_str,
                            });
                        } else {
                            actions.push(SyncAction::Update {
                                skill: skill.name.clone(),
                                dest: dest_str,
                            });
                        }
                    } else {
                        // Mudança de Symlink -> Copy de item gerenciado
                        actions.push(SyncAction::Update {
                            skill: skill.name.clone(),
                            dest: dest_str,
                        });
                    }
                } else {
                    // Symlink não registrado em skill_sync_entries (passível de Adopt por apontar para a mesma fonte)
                    actions.push(SyncAction::Conflict {
                        skill: skill.name.clone(),
                        dest: dest_str,
                        conflict: ConflictKind::ForeignSymlink {
                            target: resolved_target.to_string_lossy().to_string(),
                        },
                    });
                }
            } else {
                actions.push(SyncAction::Conflict {
                    skill: skill.name.clone(),
                    dest: dest_str,
                    conflict: ConflictKind::ForeignSymlink {
                        target: link_target.to_string_lossy().to_string(),
                    },
                });
            }
        } else if ftype.is_dir() {
            let manifest_path = dest_skill_path.join(SKILL_MANIFEST_FILENAME);
            let manifest_opt: Option<SkillCopyManifest> = std::fs::read_to_string(&manifest_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());

            let expected_hash = managed
                .map(|m| m.synced_hash.clone())
                .or_else(|| manifest_opt.as_ref().map(|mf| mf.synced_hash.clone()));

            if let Some(expected) = expected_hash {
                match compute_skill_tree_hash(&dest_skill_path) {
                    Ok(actual_hash) => {
                        if actual_hash != expected {
                            actions.push(SyncAction::Conflict {
                                skill: skill.name.clone(),
                                dest: dest_str,
                                conflict: ConflictKind::LocallyModified {
                                    expected,
                                    actual: actual_hash,
                                },
                            });
                        } else if mode == SyncMode::Copy && actual_hash == skill.hash {
                            actions.push(SyncAction::Unchanged {
                                skill: skill.name.clone(),
                                dest: dest_str,
                            });
                        } else {
                            actions.push(SyncAction::Update {
                                skill: skill.name.clone(),
                                dest: dest_str,
                            });
                        }
                    }
                    Err(_) => {
                        actions.push(SyncAction::Conflict {
                            skill: skill.name.clone(),
                            dest: dest_str,
                            conflict: ConflictKind::Unmanaged,
                        });
                    }
                }
            } else {
                actions.push(SyncAction::Conflict {
                    skill: skill.name.clone(),
                    dest: dest_str,
                    conflict: ConflictKind::Unmanaged,
                });
            }
        } else {
            actions.push(SyncAction::Conflict {
                skill: skill.name.clone(),
                dest: dest_str,
                conflict: ConflictKind::Unmanaged,
            });
        }
    }

    // Verifica entradas anteriormente gerenciadas que saíram da seleção -> Prune (ou Conflict se modificadas localmente)
    for (skill_name, managed) in managed_entries {
        if selected_names.contains(skill_name) {
            continue;
        }
        let dest_skill_path = dest_dir.join(skill_name);
        let dest_str = dest_skill_path.to_string_lossy().to_string();
        let Ok(meta) = std::fs::symlink_metadata(&dest_skill_path) else {
            continue;
        };
        let ftype = meta.file_type();
        if ftype.is_symlink() {
            let link_target = std::fs::read_link(&dest_skill_path).unwrap_or_default();
            let resolved_target = if link_target.is_absolute() {
                canonical_or_lexical_path(&link_target)
            } else {
                canonical_or_lexical_path(&dest_dir.join(&link_target))
            };
            let expected_source = canonical_or_lexical_path(Path::new(&managed.source_path));
            if resolved_target == expected_source {
                actions.push(SyncAction::Prune {
                    skill: skill_name.clone(),
                    dest: dest_str,
                });
            } else {
                actions.push(SyncAction::Conflict {
                    skill: skill_name.clone(),
                    dest: dest_str,
                    conflict: ConflictKind::ForeignSymlink {
                        target: link_target.to_string_lossy().to_string(),
                    },
                });
            }
        } else if ftype.is_dir() {
            match compute_skill_tree_hash(&dest_skill_path) {
                Ok(actual_hash) if actual_hash == managed.synced_hash => {
                    actions.push(SyncAction::Prune {
                        skill: skill_name.clone(),
                        dest: dest_str,
                    });
                }
                Ok(actual_hash) => {
                    actions.push(SyncAction::Conflict {
                        skill: skill_name.clone(),
                        dest: dest_str,
                        conflict: ConflictKind::LocallyModified {
                            expected: managed.synced_hash.clone(),
                            actual: actual_hash,
                        },
                    });
                }
                Err(_) => {
                    actions.push(SyncAction::Conflict {
                        skill: skill_name.clone(),
                        dest: dest_str,
                        conflict: ConflictKind::Unmanaged,
                    });
                }
            }
        }
    }

    actions
}

/// Calcula o fingerprint SHA-256 do estado observado (fonte + destino + modo + ações) para detecção de plano obsoleto (`skills.plan_stale`).
#[must_use]
pub fn compute_plan_fingerprint(
    dest_dir: &Path,
    mode: SyncMode,
    selected_skills: &[&SkillEntry],
    actions: &[SyncAction],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(dest_dir.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(mode.as_db_str().as_bytes());
    hasher.update(b"\0");

    for skill in selected_skills {
        hasher.update(skill.name.as_bytes());
        hasher.update(b":");
        hasher.update(skill.hash.as_bytes());
        hasher.update(b"\0");
    }

    if let Ok(rd) = std::fs::read_dir(dest_dir) {
        let mut dest_states: BTreeMap<String, String> = BTreeMap::new();
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".staging-") {
                continue;
            }
            let p = entry.path();
            let state_desc = match std::fs::symlink_metadata(&p) {
                Ok(m) if m.file_type().is_symlink() => {
                    let t = std::fs::read_link(&p).unwrap_or_default();
                    format!("L:{}", t.to_string_lossy())
                }
                Ok(m) if m.file_type().is_dir() => {
                    let h = compute_skill_tree_hash(&p).unwrap_or_else(|_| "err".to_string());
                    format!("D:{h}")
                }
                Ok(m) => format!("F:{}", m.len()),
                Err(_) => "E".to_string(),
            };
            dest_states.insert(name, state_desc);
        }
        for (k, v) in dest_states {
            hasher.update(k.as_bytes());
            hasher.update(b"=");
            hasher.update(v.as_bytes());
            hasher.update(b"\0");
        }
    }

    if let Ok(actions_json) = serde_json::to_string(actions) {
        hasher.update(actions_json.as_bytes());
    }

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{b:02x}");
    }
    hex
}
