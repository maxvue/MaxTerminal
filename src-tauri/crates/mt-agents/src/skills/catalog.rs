//! Descoberta do catálogo `MaxSkills` (`index.json` ou fallback `**/SKILL.md`) e cálculo de hash SHA-256 determinístico.

use std::collections::{BTreeMap, HashSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use mt_core::MtResult;
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::detect::current_unix_ms;
use crate::error;
use crate::ports::ProjectRecord;
use crate::skills::{SkillCatalog, SkillEntry, SkillRelevance};

/// Nome do arquivo de manifesto gravado em cópias gerenciadas.
pub const SKILL_MANIFEST_FILENAME: &str = ".maxterminal-skill.json";
/// Profundidade máxima permitida na árvore de uma skill.
pub const MAX_SKILL_DEPTH: usize = 12;
/// Número máximo de arquivos permitidos em uma única skill.
pub const MAX_SKILL_FILES: usize = 10_000;
/// Tamanho total máximo permitido para uma única skill (64 MiB).
pub const MAX_SKILL_BYTES: u64 = 64 * 1024 * 1024;

fn is_valid_skill_name(name: &str) -> bool {
    let re = Regex::new(r"^[a-z0-9][a-z0-9._-]{0,63}$").expect("regex estática válida");
    re.is_match(name)
}

/// Calcula o hash SHA-256 determinístico da árvore de uma skill.
///
/// Ignora apenas `.maxterminal-skill.json` e `.DS_Store`.
/// Rejeita symlinks que escapem da raiz da skill, loops cíclicos ou excesso de limites (`skills.limit_exceeded`).
pub fn compute_skill_tree_hash(dir: &Path) -> MtResult<String> {
    let canonical_root = std::fs::canonicalize(dir).map_err(|e| {
        error::skills_io(format!(
            "Falha ao resolver caminho canônico da skill {}: {e}",
            dir.display()
        ))
    })?;

    let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut file_count: usize = 0;
    let mut total_bytes: u64 = 0;

    walk_skill_dir(
        &canonical_root,
        &canonical_root,
        0,
        &mut file_count,
        &mut total_bytes,
        &mut entries,
    )?;

    let mut hasher = Sha256::new();
    for (rel_path, record_bytes) in entries {
        hasher.update(rel_path.as_bytes());
        hasher.update(b"\0");
        hasher.update(&record_bytes);
    }

    Ok(hex_encode(&hasher.finalize()))
}

fn walk_skill_dir(
    root: &Path,
    current: &Path,
    depth: usize,
    file_count: &mut usize,
    total_bytes: &mut u64,
    entries: &mut BTreeMap<String, Vec<u8>>,
) -> MtResult<()> {
    if depth > MAX_SKILL_DEPTH {
        return Err(error::skills_limit_exceeded(format!(
            "Skill em {} excede a profundidade máxima de {MAX_SKILL_DEPTH} níveis",
            root.display()
        )));
    }

    let read_dir = std::fs::read_dir(current).map_err(|e| {
        error::skills_io(format!(
            "Falha ao listar diretório {}: {e}",
            current.display()
        ))
    })?;

    for entry_res in read_dir {
        let entry = entry_res
            .map_err(|e| error::skills_io(format!("Erro ao ler entrada de diretório: {e}")))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == SKILL_MANIFEST_FILENAME || name_str == ".DS_Store" {
            continue;
        }

        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .map_err(|e| error::skills_io(format!("Prefixo inválido: {e}")))?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        let meta = std::fs::symlink_metadata(&path).map_err(|e| {
            error::skills_io(format!("Falha ao ler metadados de {}: {e}", path.display()))
        })?;
        let ftype = meta.file_type();

        if ftype.is_symlink() {
            let link_target = std::fs::read_link(&path).map_err(|e| {
                error::skills_io(format!("Falha ao ler symlink {}: {e}", path.display()))
            })?;
            let resolved = if link_target.is_absolute() {
                link_target.clone()
            } else {
                path.parent().unwrap_or(root).join(&link_target)
            };
            let canonical_target = std::fs::canonicalize(&resolved).map_err(|e| {
                error::skills_io(format!(
                    "Symlink quebrado ou inválido em {}: {e}",
                    path.display()
                ))
            })?;
            if !canonical_target.starts_with(root) {
                return Err(error::skills_io(format!(
                    "Symlink em {} escapa da árvore da skill para {}",
                    path.display(),
                    canonical_target.display()
                )));
            }
            if canonical_target.is_dir() && current.starts_with(&canonical_target) {
                return Err(error::skills_io(format!(
                    "Loop cíclico de symlink detectado em {}",
                    path.display()
                )));
            }
            *file_count += 1;
            if *file_count > MAX_SKILL_FILES {
                return Err(error::skills_limit_exceeded(format!(
                    "Skill excede o limite de {MAX_SKILL_FILES} arquivos"
                )));
            }
            let target_str = link_target.to_string_lossy();
            let mut rec = Vec::new();
            rec.extend_from_slice(b"L\x000\0");
            rec.extend_from_slice(target_str.len().to_string().as_bytes());
            rec.push(b'\0');
            rec.extend_from_slice(target_str.as_bytes());
            entries.insert(rel_str, rec);
        } else if ftype.is_dir() {
            let mut rec = Vec::new();
            rec.extend_from_slice(b"D\x000\x000\0");
            entries.insert(rel_str, rec);
            walk_skill_dir(root, &path, depth + 1, file_count, total_bytes, entries)?;
        } else if ftype.is_file() {
            *file_count += 1;
            if *file_count > MAX_SKILL_FILES {
                return Err(error::skills_limit_exceeded(format!(
                    "Skill excede o limite de {MAX_SKILL_FILES} arquivos"
                )));
            }
            let len = meta.len();
            *total_bytes = total_bytes.saturating_add(len);
            if *total_bytes > MAX_SKILL_BYTES {
                return Err(error::skills_limit_exceeded(format!(
                    "Skill excede o limite de {MAX_SKILL_BYTES} bytes"
                )));
            }
            let exec_bit = if meta.permissions().mode() & 0o111 != 0 {
                b"1"
            } else {
                b"0"
            };
            let bytes = std::fs::read(&path).map_err(|e| {
                error::skills_io(format!("Falha ao ler arquivo {}: {e}", path.display()))
            })?;
            let mut rec = Vec::with_capacity(16 + bytes.len());
            rec.extend_from_slice(b"F\0");
            rec.extend_from_slice(exec_bit);
            rec.push(b'\0');
            rec.extend_from_slice(bytes.len().to_string().as_bytes());
            rec.push(b'\0');
            rec.extend_from_slice(&bytes);
            entries.insert(rel_str, rec);
        } else {
            return Err(error::skills_io(format!(
                "Arquivo especial não suportado na skill: {}",
                path.display()
            )));
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct IndexFileSchema {
    version: Option<u32>,
    skills: Vec<IndexSkillItem>,
}

#[derive(Debug, Deserialize)]
struct IndexSkillItem {
    name: String,
    #[serde(default)]
    description: String,
    path: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    stacks: Vec<String>,
}

/// Escaneia o diretório fonte de `MaxSkills` (preferindo `index.json` quando válido e fazendo fallback para `**/SKILL.md`).
pub fn scan_catalog(source_dir: &Path, project: Option<&ProjectRecord>) -> MtResult<SkillCatalog> {
    if !source_dir.exists() || !source_dir.is_dir() {
        return Err(error::skills_source_missing(source_dir));
    }

    let canonical_source =
        std::fs::canonicalize(source_dir).map_err(|_| error::skills_source_missing(source_dir))?;
    let project_stack = project.map(|p| p.stack.to_lowercase());

    let index_path = canonical_source.join("index.json");
    if index_path.is_file()
        && let Ok(raw) = std::fs::read_to_string(&index_path)
        && let Ok(parsed) = serde_json::from_str::<IndexFileSchema>(&raw)
        && parsed.version.unwrap_or(1) == 1
    {
        let mut skills = Vec::new();
        let mut seen_names = HashSet::new();
        let mut valid_index = true;

        for item in parsed.skills {
            if !is_valid_skill_name(&item.name) || !seen_names.insert(item.name.clone()) {
                valid_index = false;
                break;
            }
            let rel_path = item.path.unwrap_or_else(|| item.name.clone());
            let skill_dir = canonical_source.join(&rel_path);
            if !skill_dir.is_dir() || !skill_dir.join("SKILL.md").is_file() {
                valid_index = false;
                break;
            }
            let Ok(canonical_skill_dir) = std::fs::canonicalize(&skill_dir) else {
                valid_index = false;
                break;
            };
            if !canonical_skill_dir.starts_with(&canonical_source) {
                valid_index = false;
                break;
            }
            let hash = compute_skill_tree_hash(&canonical_skill_dir)?;
            let stacks: Vec<String> = if item.stacks.is_empty() {
                infer_stacks(&item.name, &item.description, &item.tags)
            } else {
                item.stacks.iter().map(|s| s.to_lowercase()).collect()
            };
            let relevance = compute_relevance(&stacks, project_stack.as_deref());
            skills.push(SkillEntry {
                name: item.name,
                description: item.description,
                source_dir: canonical_skill_dir.to_string_lossy().to_string(),
                tags: item.tags,
                stacks,
                hash,
                relevance,
            });
        }

        if valid_index && !skills.is_empty() {
            skills.sort_by(|a, b| a.name.cmp(&b.name));
            return Ok(SkillCatalog {
                source_dir: canonical_source.to_string_lossy().to_string(),
                from_index: true,
                skills,
                scanned_at_ms: current_unix_ms(),
            });
        }
    }

    // Fallback: varredura recursiva de `**/SKILL.md`
    let mut skill_dirs: Vec<PathBuf> = Vec::new();
    collect_skill_md_dirs(&canonical_source, 0, &mut skill_dirs)?;

    let mut skills = Vec::new();
    let mut seen_names = HashSet::new();

    for skill_dir in skill_dirs {
        let dir_name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let skill_md_path = skill_dir.join("SKILL.md");
        let md_content = std::fs::read_to_string(&skill_md_path).unwrap_or_default();
        let (fm_name, fm_desc, fm_tags, fm_stacks) = parse_skill_frontmatter(&md_content);

        let candidate_name = fm_name
            .filter(|n| is_valid_skill_name(n))
            .unwrap_or_else(|| dir_name.to_lowercase());
        if !is_valid_skill_name(&candidate_name) {
            continue;
        }
        if !seen_names.insert(candidate_name.clone()) {
            return Err(error::skills_index_invalid(format!(
                "Nome de skill duplicado no catálogo: {candidate_name}"
            )));
        }

        let hash = compute_skill_tree_hash(&skill_dir)?;
        let stacks = if fm_stacks.is_empty() {
            infer_stacks(&candidate_name, &fm_desc, &fm_tags)
        } else {
            fm_stacks
        };
        let relevance = compute_relevance(&stacks, project_stack.as_deref());

        skills.push(SkillEntry {
            name: candidate_name,
            description: fm_desc,
            source_dir: skill_dir.to_string_lossy().to_string(),
            tags: fm_tags,
            stacks,
            hash,
            relevance,
        });
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(SkillCatalog {
        source_dir: canonical_source.to_string_lossy().to_string(),
        from_index: false,
        skills,
        scanned_at_ms: current_unix_ms(),
    })
}

fn collect_skill_md_dirs(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) -> MtResult<()> {
    if depth > 6 {
        return Ok(());
    }
    if dir.join("SKILL.md").is_file() && depth > 0 {
        out.push(dir.to_path_buf());
        return Ok(());
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.')
            || name_str == "node_modules"
            || name_str == "target"
            || name_str == ".worktrees"
            || name_str == ".max-code-worktrees"
        {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_dir() {
            collect_skill_md_dirs(&path, depth + 1, out)?;
        }
    }
    Ok(())
}

fn parse_skill_frontmatter(md: &str) -> (Option<String>, String, Vec<String>, Vec<String>) {
    let mut name = None;
    let mut description = String::new();
    let mut tags = Vec::new();
    let mut stacks = Vec::new();

    let trimmed = md.trim_start();
    if let Some(after_open) = trimmed.strip_prefix("---")
        && let Some(end_idx) = after_open.find("\n---")
    {
        let fm = &after_open[..end_idx];
        for line in fm.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("name:") {
                let clean = v.trim().trim_matches('"').trim_matches('\'').to_string();
                if !clean.is_empty() {
                    name = Some(clean);
                }
            } else if let Some(v) = line.strip_prefix("description:") {
                let clean = v.trim().trim_matches('"').trim_matches('\'').to_string();
                if !clean.is_empty() {
                    description = clean;
                }
            } else if let Some(v) = line.strip_prefix("tags:") {
                tags = parse_inline_list(v);
            } else if let Some(v) = line.strip_prefix("stacks:") {
                stacks = parse_inline_list(v)
                    .into_iter()
                    .map(|s| s.to_lowercase())
                    .collect();
            }
        }
    }

    if description.is_empty() {
        for line in md.lines() {
            let l = line.trim();
            if !l.is_empty() && !l.starts_with("---") && !l.starts_with('#') {
                description = l.chars().take(200).collect();
                break;
            }
        }
    }

    (name, description, tags, stacks)
}

fn parse_inline_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim().trim_start_matches('[').trim_end_matches(']');
    trimmed
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn infer_stacks(name: &str, description: &str, tags: &[String]) -> Vec<String> {
    let haystack = format!(
        "{} {} {}",
        name.to_lowercase(),
        description.to_lowercase(),
        tags.join(" ").to_lowercase()
    );
    let mut out = Vec::new();
    for (kw, stack) in [
        ("laravel", "laravel"),
        ("artisan", "laravel"),
        ("php", "laravel"),
        ("vue", "vue"),
        ("pinia", "vue"),
        ("unocss", "vue"),
        ("vitest", "vue"),
        ("tauri", "tauri"),
        ("rust", "rust"),
        ("cargo", "rust"),
        ("python", "python"),
        ("vscode", "vscode-extension"),
    ] {
        if haystack.contains(kw) && !out.iter().any(|s| s == stack) {
            out.push(stack.to_string());
        }
    }
    out
}

fn compute_relevance(stacks: &[String], project_stack: Option<&str>) -> SkillRelevance {
    let Some(ps) = project_stack else {
        return SkillRelevance::Neutral;
    };
    let ps_lower = ps.to_lowercase();
    let matches = stacks.iter().any(|s| {
        s == &ps_lower
            || (ps_lower == "tauri" && (s == "rust" || s == "vue"))
            || (ps_lower == "laravel" && s == "vue")
    });
    if matches {
        SkillRelevance::Recommended
    } else {
        SkillRelevance::Neutral
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}
