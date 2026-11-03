//! Tabela de resolução e composição multi-gerenciador para os 7 Atalhos Universais (`/`).

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::artisan::runner::quote_shell_token;
use crate::shortcuts::detect::{
    CargoEcosystem, JsEcosystemState, ProjectDetection, YarnVariant, is_regular_file,
};
use crate::shortcuts::maxcode_config::MaxCodeProjectOverrides;
use crate::shortcuts::model::{
    PackageManager, PartSource, ResolvedShortcut, ShortcutId, ShortcutPart,
};

/// Contexto adicional de comandos Artisan para resolução do atalho `/test`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArtisanTestContext {
    /// Indica se o projeto possui o comando `php artisan test` disponível.
    pub has_artisan_test: bool,
}

/// Resolve todos os 7 [`ResolvedShortcut`] para a detecção e o `cwd` informados.
#[must_use]
pub fn resolve_all_shortcuts(
    detection: &ProjectDetection,
    cwd: &Path,
    overrides: &MaxCodeProjectOverrides,
    artisan_ctx: ArtisanTestContext,
) -> Vec<ResolvedShortcut> {
    let evidence_revision =
        compute_combined_revision(&detection.evidence_hash, cwd, overrides, artisan_ctx);
    ShortcutId::ALL
        .into_iter()
        .map(|id| {
            resolve_single_shortcut(
                id,
                detection,
                cwd,
                overrides,
                artisan_ctx,
                &evidence_revision,
            )
        })
        .collect()
}

/// Resolve um único [`ShortcutId`] para a detecção e o `cwd` informados.
#[must_use]
pub fn resolve_single_shortcut(
    id: ShortcutId,
    detection: &ProjectDetection,
    cwd: &Path,
    overrides: &MaxCodeProjectOverrides,
    artisan_ctx: ArtisanTestContext,
    evidence_revision: &str,
) -> ResolvedShortcut {
    let root_str = detection.root.display().to_string();

    let outcome = match id {
        ShortcutId::Install => resolve_lifecycle(id, detection, &root_str),
        ShortcutId::InstallForce => resolve_lifecycle(id, detection, &root_str),
        ShortcutId::Update => resolve_lifecycle(id, detection, &root_str),
        ShortcutId::UpdateForce => resolve_lifecycle(id, detection, &root_str),
        ShortcutId::Build => resolve_build(detection, &root_str, overrides),
        ShortcutId::Deploy => resolve_deploy(detection, &root_str, overrides),
        ShortcutId::Test => resolve_test(detection, &root_str, artisan_ctx),
    };

    match outcome {
        Ok(parts) if !parts.is_empty() => {
            let joined = parts
                .iter()
                .map(|p| p.line.as_str())
                .collect::<Vec<_>>()
                .join(" && ");
            let final_line = wrap_for_cwd(&detection.root, cwd, &joined);
            ResolvedShortcut {
                id,
                label: id.label().to_string(),
                description: id.description_pt_br().to_string(),
                line: Some(final_line),
                parts,
                disabled_reason: None,
                requires_confirmation: id.requires_confirmation(),
                evidence_revision: evidence_revision.to_string(),
            }
        }
        Ok(_) => ResolvedShortcut {
            id,
            label: id.label().to_string(),
            description: id.description_pt_br().to_string(),
            line: None,
            parts: Vec::new(),
            disabled_reason: Some("Nenhum gerenciador de pacotes detectado.".to_string()),
            requires_confirmation: id.requires_confirmation(),
            evidence_revision: evidence_revision.to_string(),
        },
        Err(reason) => ResolvedShortcut {
            id,
            label: id.label().to_string(),
            description: id.description_pt_br().to_string(),
            line: None,
            parts: Vec::new(),
            disabled_reason: Some(reason),
            requires_confirmation: id.requires_confirmation(),
            evidence_revision: evidence_revision.to_string(),
        },
    }
}

fn resolve_lifecycle(
    id: ShortcutId,
    detection: &ProjectDetection,
    root_str: &str,
) -> Result<Vec<ShortcutPart>, String> {
    let mut parts = Vec::new();

    // 1. Composer
    if detection.composer.exists {
        if !detection.composer.valid_json {
            return Err("O arquivo composer.json contém JSON inválido.".to_string());
        }
        let args: Vec<&str> = match id {
            ShortcutId::Install => vec!["install"],
            ShortcutId::InstallForce => vec!["install", "--no-cache"],
            ShortcutId::Update => vec!["update"],
            ShortcutId::UpdateForce => vec!["update", "--no-cache"],
            _ => unreachable!(),
        };
        parts.push(make_table_part(
            PackageManager::Composer,
            root_str,
            "composer",
            &args,
        ));
    }

    // 2. JS
    match &detection.js {
        JsEcosystemState::Absent => {}
        JsEcosystemState::InvalidManifest(msg) | JsEcosystemState::InvalidDeclaration(msg) => {
            return Err(msg.clone());
        }
        JsEcosystemState::LockfileConflict { message, .. } => return Err(message.clone()),
        JsEcosystemState::Resolved {
            manager,
            yarn_variant,
            ..
        } => {
            let js_part = resolve_js_lifecycle_part(id, *manager, *yarn_variant, root_str)?;
            parts.push(js_part);
        }
    }

    // 3. Cargo
    if let Some(ref cargo) = detection.cargo {
        if !cargo.valid_manifest {
            return Err(format!("O arquivo {} é inválido.", cargo.manifest_rel));
        }
        parts.push(resolve_cargo_lifecycle_part(id, cargo, root_str));
    }

    if parts.is_empty() {
        return Err("Nenhum gerenciador de pacotes detectado neste projeto.".to_string());
    }
    Ok(parts)
}

fn resolve_js_lifecycle_part(
    id: ShortcutId,
    pm: PackageManager,
    yarn_variant: Option<YarnVariant>,
    root_str: &str,
) -> Result<ShortcutPart, String> {
    match (pm, id) {
        (PackageManager::Npm, ShortcutId::Install) => {
            Ok(make_table_part(pm, root_str, "npm", &["install"]))
        }
        (PackageManager::Npm, ShortcutId::InstallForce) => {
            Ok(make_table_part(pm, root_str, "npm", &["install", "--force"]))
        }
        (PackageManager::Npm, ShortcutId::Update) => {
            Ok(make_table_part(pm, root_str, "npm", &["update"]))
        }
        (PackageManager::Npm, ShortcutId::UpdateForce) => {
            Ok(make_table_part(pm, root_str, "npm", &["update", "--force"]))
        }

        (PackageManager::Pnpm, ShortcutId::Install) => {
            Ok(make_table_part(pm, root_str, "pnpm", &["install"]))
        }
        (PackageManager::Pnpm, ShortcutId::InstallForce) => {
            Ok(make_table_part(pm, root_str, "pnpm", &["install", "--force"]))
        }
        (PackageManager::Pnpm, ShortcutId::Update) => {
            Ok(make_table_part(pm, root_str, "pnpm", &["update"]))
        }
        (PackageManager::Pnpm, ShortcutId::UpdateForce) => Err(
            "O gerenciador pnpm não possui suporte validado para /update-force.".to_string(),
        ),

        (PackageManager::Bun, ShortcutId::Install) => {
            Ok(make_table_part(pm, root_str, "bun", &["install"]))
        }
        (PackageManager::Bun, ShortcutId::InstallForce) => {
            Ok(make_table_part(pm, root_str, "bun", &["install", "--force"]))
        }
        (PackageManager::Bun, ShortcutId::Update) => {
            Ok(make_table_part(pm, root_str, "bun", &["update"]))
        }
        (PackageManager::Bun, ShortcutId::UpdateForce) => Err(
            "O gerenciador bun não possui suporte validado para /update-force.".to_string(),
        ),

        (PackageManager::Yarn, ShortcutId::Install) => match yarn_variant {
            Some(YarnVariant::Ambiguous) => Err(
                "Variante do Yarn ambígua (.yarnrc.yml / yarn.lock / packageManager conflitantes)."
                    .to_string(),
            ),
            _ => Ok(make_table_part(pm, root_str, "yarn", &["install"])),
        },
        (PackageManager::Yarn, ShortcutId::InstallForce) => match yarn_variant {
            Some(YarnVariant::Classic) | None => {
                Ok(make_table_part(pm, root_str, "yarn", &["install", "--force"]))
            }
            Some(YarnVariant::Modern) => Err(
                "Yarn Modern (Berry) não suporta `yarn install --force`.".to_string(),
            ),
            Some(YarnVariant::Ambiguous) => Err(
                "Variante do Yarn ambígua; não é possível determinar a flag de /install-force."
                    .to_string(),
            ),
        },
        (PackageManager::Yarn, ShortcutId::Update) => match yarn_variant {
            Some(YarnVariant::Classic) | None => {
                Ok(make_table_part(pm, root_str, "yarn", &["upgrade"]))
            }
            Some(YarnVariant::Modern) => Ok(ShortcutPart {
                manager: Some(PackageManager::Yarn),
                cwd: root_str.to_string(),
                program: Some("yarn".to_string()),
                args: vec!["up".to_string(), "-R".to_string(), "*".to_string()],
                configured_shell: None,
                line: "yarn up -R '*'".to_string(),
                source: PartSource::Table,
            }),
            Some(YarnVariant::Ambiguous) => Err(
                "Variante do Yarn ambígua; não é possível escolher entre `yarn upgrade` e `yarn up -R '*'`."
                    .to_string(),
            ),
        },
        (PackageManager::Yarn, ShortcutId::UpdateForce) => Err(
            "O gerenciador Yarn não possui suporte validado para /update-force.".to_string(),
        ),
        _ => Err("Combinação de gerenciador e atalho não suportada.".to_string()),
    }
}

fn resolve_cargo_lifecycle_part(
    id: ShortcutId,
    cargo: &CargoEcosystem,
    root_str: &str,
) -> ShortcutPart {
    let base_sub = match id {
        ShortcutId::Install | ShortcutId::InstallForce => "build",
        ShortcutId::Update | ShortcutId::UpdateForce => "update",
        _ => unreachable!(),
    };
    let mut args = vec![base_sub];
    if cargo.manifest_rel != "Cargo.toml" {
        args.push("--manifest-path");
        args.push(&cargo.manifest_rel);
    }
    make_table_part(PackageManager::Cargo, root_str, "cargo", &args)
}

fn resolve_build(
    detection: &ProjectDetection,
    root_str: &str,
    overrides: &MaxCodeProjectOverrides,
) -> Result<Vec<ShortcutPart>, String> {
    if overrides.ambiguous_conflict {
        return Err(
            "Múltiplos arquivos config-trabalho.json do MaxCode divergem para este projeto."
                .to_string(),
        );
    }
    if let Some(ref custom) = overrides.build_command {
        return Ok(vec![ShortcutPart {
            manager: None,
            cwd: root_str.to_string(),
            program: None,
            args: Vec::new(),
            configured_shell: Some(custom.clone()),
            line: custom.clone(),
            source: PartSource::MaxCodeConfig,
        }]);
    }

    match &detection.js {
        JsEcosystemState::InvalidManifest(msg) | JsEcosystemState::InvalidDeclaration(msg) => {
            return Err(msg.clone());
        }
        JsEcosystemState::LockfileConflict { message, .. } => return Err(message.clone()),
        JsEcosystemState::Resolved { manager, .. } => {
            if detection.package_json.has_usable_script("build") {
                return Ok(vec![make_js_script_part(*manager, root_str, "build")]);
            }
        }
        JsEcosystemState::Absent => {}
    }

    if detection.composer.exists {
        if !detection.composer.valid_json {
            return Err("O arquivo composer.json contém JSON inválido.".to_string());
        }
        if detection.composer.has_usable_script("build") {
            return Ok(vec![make_composer_script_part(root_str, "build")]);
        }
    }

    if let Some(ref cargo) = detection.cargo {
        if !cargo.valid_manifest {
            return Err(format!("O arquivo {} é inválido.", cargo.manifest_rel));
        }
        let mut args = vec!["build", "--release"];
        if cargo.manifest_rel != "Cargo.toml" {
            args.push("--manifest-path");
            args.push(&cargo.manifest_rel);
        }
        return Ok(vec![make_table_part(
            PackageManager::Cargo,
            root_str,
            "cargo",
            &args,
        )]);
    }

    Err("Nenhum comando de build encontrado".to_string())
}

fn resolve_deploy(
    detection: &ProjectDetection,
    root_str: &str,
    overrides: &MaxCodeProjectOverrides,
) -> Result<Vec<ShortcutPart>, String> {
    if overrides.ambiguous_conflict {
        return Err(
            "Múltiplos arquivos config-trabalho.json do MaxCode divergem para este projeto."
                .to_string(),
        );
    }
    if let Some(ref custom) = overrides.deploy_command {
        return Ok(vec![ShortcutPart {
            manager: None,
            cwd: root_str.to_string(),
            program: None,
            args: Vec::new(),
            configured_shell: Some(custom.clone()),
            line: custom.clone(),
            source: PartSource::MaxCodeConfig,
        }]);
    }

    match &detection.js {
        JsEcosystemState::InvalidManifest(msg) | JsEcosystemState::InvalidDeclaration(msg) => {
            return Err(msg.clone());
        }
        JsEcosystemState::LockfileConflict { message, .. } => return Err(message.clone()),
        JsEcosystemState::Resolved { manager, .. } => {
            if detection.package_json.has_usable_script("deploy") {
                return Ok(vec![make_js_script_part(*manager, root_str, "deploy")]);
            }
        }
        JsEcosystemState::Absent => {}
    }

    if detection.composer.exists {
        if !detection.composer.valid_json {
            return Err("O arquivo composer.json contém JSON inválido.".to_string());
        }
        if detection.composer.has_usable_script("deploy") {
            return Ok(vec![make_composer_script_part(root_str, "deploy")]);
        }
    }

    Err("Nenhum comando de deploy encontrado".to_string())
}

fn resolve_test(
    detection: &ProjectDetection,
    root_str: &str,
    artisan_ctx: ArtisanTestContext,
) -> Result<Vec<ShortcutPart>, String> {
    let mut parts = Vec::new();

    // 1. Parte PHP
    if detection.composer.exists {
        if !detection.composer.valid_json {
            return Err("O arquivo composer.json contém JSON inválido.".to_string());
        }
        if detection.composer.is_laravel
            && is_regular_file(&detection.root.join("artisan"))
            && artisan_ctx.has_artisan_test
        {
            parts.push(ShortcutPart {
                manager: Some(PackageManager::Composer),
                cwd: root_str.to_string(),
                program: Some("php".to_string()),
                args: vec!["artisan".to_string(), "test".to_string()],
                configured_shell: None,
                line: "php artisan test".to_string(),
                source: PartSource::Artisan,
            });
        } else if detection.composer.has_usable_script("test") {
            parts.push(make_composer_script_part(root_str, "test"));
        } else if is_regular_file(&detection.root.join("vendor/bin/pest")) {
            parts.push(ShortcutPart {
                manager: Some(PackageManager::Composer),
                cwd: root_str.to_string(),
                program: Some("./vendor/bin/pest".to_string()),
                args: Vec::new(),
                configured_shell: None,
                line: "./vendor/bin/pest".to_string(),
                source: PartSource::VendorBin,
            });
        } else if is_regular_file(&detection.root.join("vendor/bin/phpunit")) {
            parts.push(ShortcutPart {
                manager: Some(PackageManager::Composer),
                cwd: root_str.to_string(),
                program: Some("./vendor/bin/phpunit".to_string()),
                args: Vec::new(),
                configured_shell: None,
                line: "./vendor/bin/phpunit".to_string(),
                source: PartSource::VendorBin,
            });
        }
    }

    // 2. Parte JS
    match &detection.js {
        JsEcosystemState::Absent => {}
        JsEcosystemState::InvalidManifest(msg) | JsEcosystemState::InvalidDeclaration(msg) => {
            if detection.package_json.exists {
                return Err(msg.clone());
            }
        }
        JsEcosystemState::LockfileConflict { message, .. } => {
            if detection.package_json.has_usable_script("test") {
                return Err(message.clone());
            }
        }
        JsEcosystemState::Resolved { manager, .. } => {
            if detection.package_json.has_usable_script("test") {
                parts.push(make_js_script_part(*manager, root_str, "test"));
            }
        }
    }

    // 3. Parte Cargo
    if let Some(ref cargo) = detection.cargo {
        if !cargo.valid_manifest {
            return Err(format!("O arquivo {} é inválido.", cargo.manifest_rel));
        }
        let mut args = vec!["test"];
        if cargo.manifest_rel != "Cargo.toml" {
            args.push("--manifest-path");
            args.push(&cargo.manifest_rel);
        }
        parts.push(make_table_part(
            PackageManager::Cargo,
            root_str,
            "cargo",
            &args,
        ));
    }

    if parts.is_empty() {
        return Err("Nenhum comando de teste encontrado".to_string());
    }
    Ok(parts)
}

fn make_table_part(pm: PackageManager, cwd: &str, prog: &str, args: &[&str]) -> ShortcutPart {
    let arg_vec: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let line = if arg_vec.is_empty() {
        prog.to_string()
    } else {
        format!("{prog} {}", arg_vec.join(" "))
    };
    ShortcutPart {
        manager: Some(pm),
        cwd: cwd.to_string(),
        program: Some(prog.to_string()),
        args: arg_vec,
        configured_shell: None,
        line,
        source: PartSource::Table,
    }
}

fn make_js_script_part(pm: PackageManager, cwd: &str, script: &str) -> ShortcutPart {
    let bin = pm.binary_name();
    ShortcutPart {
        manager: Some(pm),
        cwd: cwd.to_string(),
        program: Some(bin.to_string()),
        args: vec!["run".to_string(), script.to_string()],
        configured_shell: None,
        line: format!("{bin} run {script}"),
        source: PartSource::PackageScript,
    }
}

fn make_composer_script_part(cwd: &str, script: &str) -> ShortcutPart {
    ShortcutPart {
        manager: Some(PackageManager::Composer),
        cwd: cwd.to_string(),
        program: Some("composer".to_string()),
        args: vec!["run-script".to_string(), script.to_string()],
        configured_shell: None,
        line: format!("composer run-script {script}"),
        source: PartSource::ComposerScript,
    }
}

fn wrap_for_cwd(root: &Path, cwd: &Path, joined_line: &str) -> String {
    if root == cwd {
        return joined_line.to_string();
    }
    let quoted_root = quote_shell_token(&root.display().to_string());
    let inner = format!("cd -- {quoted_root} && {joined_line}");
    format!("sh -c {}", quote_shell_token(&inner))
}

fn compute_combined_revision(
    evidence_hash: &str,
    cwd: &Path,
    overrides: &MaxCodeProjectOverrides,
    artisan_ctx: ArtisanTestContext,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(evidence_hash.as_bytes());
    hasher.update(b"|");
    hasher.update(cwd.to_string_lossy().as_bytes());
    hasher.update(b"|");
    hasher.update(overrides.build_command.as_deref().unwrap_or("").as_bytes());
    hasher.update(b"|");
    hasher.update(overrides.deploy_command.as_deref().unwrap_or("").as_bytes());
    hasher.update(b"|");
    hasher.update(if overrides.ambiguous_conflict {
        b"1"
    } else {
        b"0"
    });
    hasher.update(if artisan_ctx.has_artisan_test {
        b"1"
    } else {
        b"0"
    });
    hex::encode(hasher.finalize())
}
