//! Detecção de gerenciadores de pacotes (`Composer`, `JS`, `Cargo`), variantes de Yarn e conflitos de lockfiles.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::fs_util::{file_stamp, read_capped};
use crate::shortcuts::manifests::{
    ComposerJsonInfo, PackageJsonInfo, read_composer_json, read_package_json,
};
use crate::shortcuts::model::{DetectedManager, ManagerEvidence, PackageManager};

const MAX_LOCK_HEADER_BYTES: usize = 64 * 1024;

/// Variante detectada para projetos Yarn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YarnVariant {
    /// Yarn Classic 1.x (`yarn upgrade`, `yarn install --force`).
    Classic,
    /// Yarn Modern / Berry >= 2 (`yarn up -R '*'`, sem `--force`).
    Modern,
    /// Variante ambígua (ex.: `.yarnrc.yml` contradiz `packageManager` ou `yarn.lock`).
    Ambiguous,
}

/// Estado do ecossistema JS detectado no projeto.
#[derive(Debug, Clone)]
pub enum JsEcosystemState {
    /// Nenhum `package.json` presente.
    Absent,
    /// `package.json` contém JSON inválido.
    InvalidManifest(String),
    /// Campo `"packageManager"` contém valor inválido ou gerenciador desconhecido.
    InvalidDeclaration(String),
    /// Conflito entre múltiplos lockfiles de gerenciadores diferentes sem `"packageManager"` declarado.
    LockfileConflict {
        /// Caminhos dos lockfiles conflitantes.
        lockfiles: Vec<String>,
        /// Mensagem explicativa em `pt-BR`.
        message: String,
    },
    /// Gerenciador JS resolvido com sucesso.
    Resolved {
        /// Gerenciador selecionado (`npm`, `pnpm`, `yarn` ou `bun`).
        manager: PackageManager,
        /// Variante quando `manager == PackageManager::Yarn`.
        yarn_variant: Option<YarnVariant>,
        /// Metadados detalhados do gerenciador.
        detected: DetectedManager,
    },
}

/// Estado do Cargo detectado no projeto.
#[derive(Debug, Clone)]
pub struct CargoEcosystem {
    /// Caminho relativo do manifesto (`"Cargo.toml"` ou `"src-tauri/Cargo.toml"`).
    pub manifest_rel: String,
    /// Indica se o manifesto é válido.
    pub valid_manifest: bool,
    /// Metadados detalhados do gerenciador.
    pub detected: DetectedManager,
}

/// Snapshot completo de detecção na raiz do projeto.
#[derive(Debug, Clone)]
pub struct ProjectDetection {
    /// Caminho absoluto canônico da raiz de manifestos.
    pub root: PathBuf,
    /// Informações do `composer.json`.
    pub composer: ComposerJsonInfo,
    /// Gerenciador Composer detectado, quando presente.
    pub composer_detected: Option<DetectedManager>,
    /// Informações do `package.json`.
    pub package_json: PackageJsonInfo,
    /// Estado resolvido do ecossistema JS.
    pub js: JsEcosystemState,
    /// Estado resolvido do Cargo, quando presente.
    pub cargo: Option<CargoEcosystem>,
    /// Lista consolidada de gerenciadores detectados (`Composer` → `JS` → `Cargo`).
    pub managers: Vec<DetectedManager>,
    /// Avisos de conflito ou coexistência de lockfiles.
    pub warnings: Vec<String>,
    /// Hash de revisão das evidências.
    pub evidence_hash: String,
}

/// Localiza a raiz de manifestos subindo de `canonical_cwd` até `project_boundary` (sem atravessar `.git`).
#[must_use]
pub fn find_manifest_root(canonical_cwd: &Path, project_boundary: &Path) -> PathBuf {
    let mut dir = canonical_cwd;
    loop {
        if has_any_manifest(dir) {
            return dir.to_path_buf();
        }
        if dir == project_boundary || dir.join(".git").symlink_metadata().is_ok() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) if parent.starts_with(project_boundary) => dir = parent,
            _ => return dir.to_path_buf(),
        }
    }
}

fn has_any_manifest(dir: &Path) -> bool {
    is_regular_file(&dir.join("composer.json"))
        || is_regular_file(&dir.join("package.json"))
        || is_regular_file(&dir.join("Cargo.toml"))
        || is_regular_file(&dir.join("src-tauri/Cargo.toml"))
}

pub(crate) fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| !m.file_type().is_symlink() && m.is_file())
        .unwrap_or(false)
}

/// Executa a detecção determinística de gerenciadores e evidências em `root`.
#[must_use]
pub fn detect_project_managers(root: &Path) -> ProjectDetection {
    let mut managers = Vec::new();
    let mut warnings = Vec::new();

    // 1. Composer
    let composer = read_composer_json(root);
    let composer_detected = if composer.exists {
        let lockfile =
            is_regular_file(&root.join("composer.lock")).then(|| "composer.lock".to_string());
        let mut evidence = vec![ManagerEvidence {
            path: "composer.json".to_string(),
            field: None,
            value: if composer.valid_json {
                "presente".to_string()
            } else {
                "JSON inválido".to_string()
            },
        }];
        if let Some(ref lf) = lockfile {
            evidence.push(ManagerEvidence {
                path: lf.clone(),
                field: None,
                value: "presente".to_string(),
            });
        }
        let det = DetectedManager {
            manager: PackageManager::Composer,
            manifest: "composer.json".to_string(),
            lockfile,
            version: None,
            evidence,
            reason: "Detectado via composer.json".to_string(),
        };
        managers.push(det.clone());
        Some(det)
    } else {
        None
    };

    // 2. Ecossistema JS
    let package_json = read_package_json(root);
    let js = detect_js_ecosystem(root, &package_json, &mut warnings);
    if let JsEcosystemState::Resolved { ref detected, .. } = js {
        managers.push(detected.clone());
    }

    // 3. Cargo (Cargo.toml na raiz ou src-tauri/Cargo.toml)
    let cargo = detect_cargo_ecosystem(root);
    if let Some(ref c) = cargo {
        managers.push(c.detected.clone());
    }

    let evidence_hash = compute_evidence_hash(root, &warnings);

    ProjectDetection {
        root: root.to_path_buf(),
        composer,
        composer_detected,
        package_json,
        js,
        cargo,
        managers,
        warnings,
        evidence_hash,
    }
}

fn detect_js_ecosystem(
    root: &Path,
    pkg: &PackageJsonInfo,
    warnings: &mut Vec<String>,
) -> JsEcosystemState {
    if !pkg.exists {
        return JsEcosystemState::Absent;
    }
    if !pkg.valid_json {
        let msg = "O arquivo package.json contém JSON inválido.".to_string();
        warnings.push(msg.clone());
        return JsEcosystemState::InvalidManifest(msg);
    }

    let lockfiles = scan_js_lockfiles(root);

    if let Some(ref raw_pm) = pkg.package_manager_raw {
        let Some((pm, version)) = parse_package_manager_field(raw_pm) else {
            let msg = format!("Valor inválido em package.json#packageManager: \"{raw_pm}\".");
            warnings.push(msg.clone());
            return JsEcosystemState::InvalidDeclaration(msg);
        };

        let mut evidence = vec![ManagerEvidence {
            path: "package.json".to_string(),
            field: Some("packageManager".to_string()),
            value: raw_pm.clone(),
        }];

        let mut matching_lock: Option<String> = None;
        for (lf_pm, lf_name) in &lockfiles {
            evidence.push(ManagerEvidence {
                path: (*lf_name).to_string(),
                field: None,
                value: "presente".to_string(),
            });
            if *lf_pm == pm && matching_lock.is_none() {
                matching_lock = Some((*lf_name).to_string());
            } else if *lf_pm != pm {
                warnings.push(format!(
                    "Lockfile '{}' diverge de packageManager \"{raw_pm}\" (usando {}).",
                    lf_name,
                    pm.binary_name()
                ));
            }
        }

        let yarn_variant =
            (pm == PackageManager::Yarn).then(|| detect_yarn_variant(root, Some(&version)));

        let detected = DetectedManager {
            manager: pm,
            manifest: "package.json".to_string(),
            lockfile: matching_lock,
            version: Some(version),
            evidence,
            reason: format!("Declarado em package.json#packageManager ({raw_pm})"),
        };

        return JsEcosystemState::Resolved {
            manager: pm,
            yarn_variant,
            detected,
        };
    }

    // Sem packageManager declarado: verifica lockfiles distintos
    let mut distinct_managers: Vec<PackageManager> = Vec::new();
    for (pm, _) in &lockfiles {
        if !distinct_managers.contains(pm) {
            distinct_managers.push(*pm);
        }
    }

    if distinct_managers.len() > 1 {
        let lf_names: Vec<String> = lockfiles.iter().map(|(_, n)| (*n).to_string()).collect();
        let msg = format!(
            "Conflito de gerenciadores JS: múltiplos lockfiles encontrados ({}) sem campo \"packageManager\" em package.json.",
            lf_names.join(", ")
        );
        warnings.push(msg.clone());
        return JsEcosystemState::LockfileConflict {
            lockfiles: lf_names,
            message: msg,
        };
    }

    if let Some(&pm) = distinct_managers.first() {
        let primary_lock = lockfiles
            .iter()
            .find(|(m, _)| *m == pm)
            .map(|(_, n)| (*n).to_string());
        let mut evidence = vec![ManagerEvidence {
            path: "package.json".to_string(),
            field: None,
            value: "presente".to_string(),
        }];
        for (_, n) in &lockfiles {
            evidence.push(ManagerEvidence {
                path: (*n).to_string(),
                field: None,
                value: "presente".to_string(),
            });
        }
        let yarn_variant = (pm == PackageManager::Yarn).then(|| detect_yarn_variant(root, None));
        let reason = format!(
            "Detectado via lockfile {}",
            primary_lock.as_deref().unwrap_or("package.json")
        );
        let detected = DetectedManager {
            manager: pm,
            manifest: "package.json".to_string(),
            lockfile: primary_lock,
            version: None,
            evidence,
            reason,
        };
        return JsEcosystemState::Resolved {
            manager: pm,
            yarn_variant,
            detected,
        };
    }

    // Sem packageManager e sem lockfile → fallback para npm
    let detected = DetectedManager {
        manager: PackageManager::Npm,
        manifest: "package.json".to_string(),
        lockfile: None,
        version: None,
        evidence: vec![ManagerEvidence {
            path: "package.json".to_string(),
            field: None,
            value: "presente (sem lockfile)".to_string(),
        }],
        reason: "Padrão npm (package.json sem packageManager nem lockfile)".to_string(),
    };
    JsEcosystemState::Resolved {
        manager: PackageManager::Npm,
        yarn_variant: None,
        detected,
    }
}

fn scan_js_lockfiles(root: &Path) -> Vec<(PackageManager, &'static str)> {
    let mut out = Vec::new();
    if is_regular_file(&root.join("npm-shrinkwrap.json")) {
        out.push((PackageManager::Npm, "npm-shrinkwrap.json"));
    }
    if is_regular_file(&root.join("package-lock.json")) {
        out.push((PackageManager::Npm, "package-lock.json"));
    }
    if is_regular_file(&root.join("pnpm-lock.yaml")) {
        out.push((PackageManager::Pnpm, "pnpm-lock.yaml"));
    }
    if is_regular_file(&root.join("yarn.lock")) {
        out.push((PackageManager::Yarn, "yarn.lock"));
    }
    if is_regular_file(&root.join("bun.lock")) {
        out.push((PackageManager::Bun, "bun.lock"));
    }
    if is_regular_file(&root.join("bun.lockb")) {
        out.push((PackageManager::Bun, "bun.lockb"));
    }
    out
}

fn parse_package_manager_field(raw: &str) -> Option<(PackageManager, String)> {
    let trimmed = raw.trim();
    let (name, ver_with_hash) = trimmed.split_once('@')?;
    let ver = ver_with_hash
        .split_once('+')
        .map_or(ver_with_hash, |(v, _)| v)
        .trim();
    if ver.is_empty() || !ver.as_bytes()[0].is_ascii_digit() {
        return None;
    }
    let pm = match name {
        "npm" => PackageManager::Npm,
        "pnpm" => PackageManager::Pnpm,
        "yarn" => PackageManager::Yarn,
        "bun" => PackageManager::Bun,
        _ => return None,
    };
    Some((pm, ver.to_string()))
}

fn detect_yarn_variant(root: &Path, declared_version: Option<&str>) -> YarnVariant {
    let has_yarnrc_yml = is_regular_file(&root.join(".yarnrc.yml"));
    let lock_header = read_capped(&root.join("yarn.lock"), MAX_LOCK_HEADER_BYTES)
        .ok()
        .flatten();

    let lock_variant = lock_header.as_deref().and_then(|text| {
        if text.contains("__metadata:") {
            Some(YarnVariant::Modern)
        } else if text.contains("# yarn lockfile v1") {
            Some(YarnVariant::Classic)
        } else {
            None
        }
    });

    if let Some(ver) = declared_version {
        let major_str = ver.split('.').next().unwrap_or("");
        let Ok(major) = major_str.parse::<u32>() else {
            return YarnVariant::Ambiguous;
        };
        let declared_var = if major == 1 {
            YarnVariant::Classic
        } else if major >= 2 {
            YarnVariant::Modern
        } else {
            return YarnVariant::Ambiguous;
        };
        if let Some(lv) = lock_variant {
            if lv != declared_var {
                return YarnVariant::Ambiguous;
            }
        }
        if declared_var == YarnVariant::Classic && has_yarnrc_yml {
            return YarnVariant::Ambiguous;
        }
        return declared_var;
    }

    if has_yarnrc_yml {
        if lock_variant == Some(YarnVariant::Classic) {
            return YarnVariant::Ambiguous;
        }
        return YarnVariant::Modern;
    }

    lock_variant.unwrap_or(YarnVariant::Classic)
}

fn detect_cargo_ecosystem(root: &Path) -> Option<CargoEcosystem> {
    let root_cargo = root.join("Cargo.toml");
    let tauri_cargo = root.join("src-tauri/Cargo.toml");

    let (manifest_rel, full_path, lock_rel) = if is_regular_file(&root_cargo) {
        ("Cargo.toml".to_string(), root_cargo, "Cargo.lock")
    } else if is_regular_file(&tauri_cargo) {
        (
            "src-tauri/Cargo.toml".to_string(),
            tauri_cargo,
            "src-tauri/Cargo.lock",
        )
    } else {
        return None;
    };

    let content = read_capped(&full_path, 2 * 1024 * 1024).ok().flatten();
    let valid_manifest = content
        .as_deref()
        .is_some_and(|c| c.contains("[package]") || c.contains("[workspace]"));

    let lockfile = is_regular_file(&root.join(lock_rel)).then(|| lock_rel.to_string());
    let mut evidence = vec![ManagerEvidence {
        path: manifest_rel.clone(),
        field: None,
        value: if valid_manifest {
            "presente".to_string()
        } else {
            "inválido".to_string()
        },
    }];
    if let Some(ref lf) = lockfile {
        evidence.push(ManagerEvidence {
            path: lf.clone(),
            field: None,
            value: "presente".to_string(),
        });
    }

    let detected = DetectedManager {
        manager: PackageManager::Cargo,
        manifest: manifest_rel.clone(),
        lockfile,
        version: None,
        evidence,
        reason: format!("Detectado via {manifest_rel}"),
    };

    Some(CargoEcosystem {
        manifest_rel,
        valid_manifest,
        detected,
    })
}

fn compute_evidence_hash(root: &Path, warnings: &[String]) -> String {
    let tracked = [
        "composer.json",
        "composer.lock",
        "package.json",
        "npm-shrinkwrap.json",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        ".yarnrc.yml",
        "bun.lock",
        "bun.lockb",
        "Cargo.toml",
        "Cargo.lock",
        "src-tauri/Cargo.toml",
        "src-tauri/Cargo.lock",
        "artisan",
        "vendor/bin/pest",
        "vendor/bin/phpunit",
    ];

    let mut hasher = Sha256::new();
    for rel in tracked {
        hasher.update(rel.as_bytes());
        hasher.update(b":");
        if let Ok(Some(st)) = file_stamp(&root.join(rel), 4 * 1024 * 1024) {
            hasher.update(st.size.to_le_bytes());
            hasher.update(st.mtime_ns.to_le_bytes());
            hasher.update(st.content_sha256.as_bytes());
        } else if is_regular_file(&root.join(rel)) {
            hasher.update(b"PRESENT");
        } else {
            hasher.update(b"ABSENT");
        }
        hasher.update(b"\n");
    }
    for w in warnings {
        hasher.update(w.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}
