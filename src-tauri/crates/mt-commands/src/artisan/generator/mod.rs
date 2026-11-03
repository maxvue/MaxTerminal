//! Orquestrador `CommandGenerator` para validação, preview determinístico e criação exclusiva (`O_EXCL`)
//! de comandos Artisan com atualização supervisionada de `routes/console.php`.

pub mod console_routes;
pub mod form;
pub mod maxcode_prompt;
pub mod psr4;
pub mod schedule;
pub mod signature;
pub mod template;
pub mod validate;

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use mt_core::{MtError, MtResult};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::artisan::generator::console_routes::plan_console_routes_patch;
use crate::artisan::generator::form::{
    ArtisanCommandForm, ArtisanCreateResult, ArtisanPreview, ValidationReport,
};
use crate::artisan::generator::psr4::{Psr4Resolution, resolve_commands_namespace};
use crate::artisan::generator::signature::build_signature;
use crate::artisan::generator::template::render_php_command;
use crate::artisan::generator::validate::validate_form;
use crate::artisan::model::{ArtisanCommand, CommandCreatedPayload, LaravelLayout};
use crate::artisan::root::{detect_layout, laravel_version};
use crate::artisan::service::ArtisanService;
use crate::error::{self, codes};
use crate::fs_util::{
    atomic_write, create_dirs_safe, create_new_file, ensure_safe_under_root, read_capped,
    sha256_hex,
};
use crate::ports::CommandsEventSink;

const MAX_CONSOLE_ROUTES_BYTES: usize = 256 * 1024;

/// Gerador transacional de comandos Artisan.
pub struct CommandGenerator {
    artisan: Arc<ArtisanService>,
    sink: Arc<dyn CommandsEventSink>,
    locks: DashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>,
}

impl CommandGenerator {
    /// Cria uma nova instância de [`CommandGenerator`].
    #[must_use]
    pub fn new(artisan: Arc<ArtisanService>, sink: Arc<dyn CommandsEventSink>) -> Self {
        Self {
            artisan,
            sink,
            locks: DashMap::new(),
        }
    }

    /// Valida `form` no contexto de `root` e dos comandos atualmente conhecidos.
    pub fn validate(
        &self,
        root: &Path,
        form: &ArtisanCommandForm,
        existing_commands: &[ArtisanCommand],
    ) -> MtResult<ValidationReport> {
        let class_rel = format!("app/Console/Commands/{}.php", form.class_name.trim());
        let class_exists = ensure_safe_under_root(root, Path::new(&class_rel))
            .ok()
            .is_some_and(|p| p.symlink_metadata().is_ok());

        let psr4_err = match resolve_commands_namespace(root) {
            Psr4Resolution::Resolved(_) => None,
            Psr4Resolution::Unresolved(msg) => Some(msg),
        };

        Ok(validate_form(
            form,
            existing_commands,
            class_exists,
            psr4_err.as_deref(),
        ))
    }

    /// Gera o [`ArtisanPreview`] completo (código PHP, patch de `routes/console.php`, `manual_steps` e `revision`).
    pub fn preview(
        &self,
        root: &Path,
        form: &ArtisanCommandForm,
        existing_commands: &[ArtisanCommand],
    ) -> MtResult<ArtisanPreview> {
        let class_trimmed = form.class_name.trim();
        let rel_path = if class_trimmed.is_empty() {
            "app/Console/Commands/Comando.php".to_string()
        } else {
            format!("app/Console/Commands/{class_trimmed}.php")
        };

        // Garante que o caminho relativo não possui symlinks nem escapa da raiz
        let target_check = ensure_safe_under_root(root, Path::new(&rel_path))?;
        let class_exists = target_check.symlink_metadata().is_ok();

        let (namespace, psr4_err) = match resolve_commands_namespace(root) {
            Psr4Resolution::Resolved(ns) => (ns, None),
            Psr4Resolution::Unresolved(msg) => ("App\\Console\\Commands".to_string(), Some(msg)),
        };

        let report = validate_form(form, existing_commands, class_exists, psr4_err.as_deref());

        let signature = build_signature(form, true);
        let layout = detect_layout(root);

        let console_routes_path = ensure_safe_under_root(root, Path::new("routes/console.php"))?;
        let existing_console = read_capped(&console_routes_path, MAX_CONSOLE_ROUTES_BYTES)
            .map_err(|e| {
                error::console_routes_failed(format!("Falha ao ler routes/console.php: {e}"))
            })?;

        let mut manual_steps = Vec::new();
        if let Some(ref msg) = psr4_err {
            manual_steps.push(msg.clone());
        }

        let console_patch = if let Some(ref sched) = form.schedule {
            let plan = plan_console_routes_patch(
                layout,
                existing_console.as_deref(),
                &namespace,
                form,
                sched,
            );
            manual_steps.extend(plan.manual_steps);
            Some(plan.patch)
        } else {
            None
        };

        let php = if report.valid {
            Some(render_php_command(form, &namespace))
        } else {
            None
        };

        let revision = compute_preview_revision(
            form,
            &layout,
            &namespace,
            class_exists,
            existing_console.as_deref(),
            existing_commands,
        );

        Ok(ArtisanPreview {
            revision,
            report,
            path: rel_path,
            signature,
            php,
            console_patch,
            manual_steps,
        })
    }

    /// Cria exclusivamente o arquivo PHP do comando e aplica o patch em `routes/console.php` quando aprovado.
    pub async fn create(
        &self,
        root: &Path,
        form: &ArtisanCommandForm,
        expected_revision: &str,
        existing_commands: &[ArtisanCommand],
    ) -> MtResult<ArtisanCreateResult> {
        let canon_root = fs::canonicalize(root).map_err(|e| {
            error::root_forbidden(format!("Raiz inválida '{}': {e}", root.display()))
        })?;

        let lock = self.locks.entry(canon_root.clone()).or_default().clone();
        let _guard = lock.lock().await;

        let class_name = form.class_name.trim();
        let rel_path = format!("app/Console/Commands/{class_name}.php");
        let target_path = ensure_safe_under_root(&canon_root, Path::new(&rel_path))?;

        if target_path.symlink_metadata().is_ok() {
            return Err(error::file_exists(&rel_path));
        }

        let fresh_preview = self.preview(&canon_root, form, existing_commands)?;
        if !fresh_preview.report.valid {
            return Err(MtError::new(
                codes::INVALID_FORM,
                "O formulário possui erros de validação; corrija os campos destacados.",
            )
            .with_details(serde_json::json!({
                "issues": fresh_preview.report.issues,
            })));
        }

        if fresh_preview.revision != expected_revision {
            return Err(error::preview_stale());
        }

        let php_code = fresh_preview
            .php
            .ok_or_else(|| error::write_failed("Código PHP ausente no preview válido."))?;

        let commands_dir = create_dirs_safe(&canon_root, Path::new("app/Console/Commands"))?;
        let final_php_path = commands_dir.join(format!("{class_name}.php"));

        let (created_ino, created_sha) =
            create_new_file(&final_php_path, php_code.as_bytes(), &rel_path)?;

        let mut console_routes_patched = false;
        let mut manual_steps = fresh_preview.manual_steps.clone();

        if let Some(ref sched) = form.schedule {
            let layout = detect_layout(&canon_root);
            let namespace = match resolve_commands_namespace(&canon_root) {
                Psr4Resolution::Resolved(ns) => ns,
                Psr4Resolution::Unresolved(_) => "App\\Console\\Commands".to_string(),
            };
            let routes_path = ensure_safe_under_root(&canon_root, Path::new("routes/console.php"))?;
            let existing_console = match read_capped(&routes_path, MAX_CONSOLE_ROUTES_BYTES) {
                Ok(c) => c,
                Err(e) => {
                    return rollback_or_partial(
                        &final_php_path,
                        &rel_path,
                        created_ino,
                        &created_sha,
                        format!("Falha ao ler routes/console.php: {e}"),
                    );
                }
            };

            let plan = plan_console_routes_patch(
                layout,
                existing_console.as_deref(),
                &namespace,
                form,
                sched,
            );

            if !plan.patch.manual_only {
                if let Some(ref new_routes_content) = plan.new_content {
                    if existing_console.as_deref() != Some(new_routes_content.as_str()) {
                        let routes_dir = create_dirs_safe(&canon_root, Path::new("routes"))?;
                        let safe_routes_file = routes_dir.join("console.php");
                        let write_res = if plan.patch.will_create {
                            create_new_file(
                                &safe_routes_file,
                                new_routes_content.as_bytes(),
                                "routes/console.php",
                            )
                            .map(|_| ())
                        } else {
                            atomic_write(&safe_routes_file, new_routes_content.as_bytes()).map_err(
                                |e| {
                                    error::console_routes_failed(format!(
                                        "Falha ao atualizar routes/console.php: {e}"
                                    ))
                                },
                            )
                        };

                        if let Err(err) = write_res {
                            return rollback_or_partial(
                                &final_php_path,
                                &rel_path,
                                created_ino,
                                &created_sha,
                                err.message.to_string(),
                            );
                        }
                        console_routes_patched = true;
                    }
                }
            } else {
                manual_steps = plan.manual_steps;
            }
        }

        self.artisan.invalidate(&canon_root);
        self.artisan.spawn_revalidate(canon_root.clone());

        let rev = self.artisan.next_revision(&canon_root);
        self.sink.command_created(CommandCreatedPayload {
            root: canon_root.display().to_string(),
            project_id: None,
            path: rel_path.clone(),
            class_name: class_name.to_string(),
            signature_name: form.signature_name.trim().to_string(),
            revision: rev,
            request_id: Uuid::now_v7().to_string(),
        });

        Ok(ArtisanCreateResult {
            path: rel_path,
            console_routes_patched,
            manual_steps,
            catalog_refresh_pending: true,
        })
    }

    /// Formata o prompt de criação para o Plan Mode do `MaxCode`.
    pub fn format_maxcode_prompt(
        &self,
        root: &Path,
        form: &ArtisanCommandForm,
    ) -> MtResult<String> {
        let layout = detect_layout(root);
        let lv = laravel_version(root);
        maxcode_prompt::format_maxcode_prompt(form, layout, lv.as_deref())
    }
}

fn compute_preview_revision(
    form: &ArtisanCommandForm,
    layout: &LaravelLayout,
    namespace: &str,
    class_exists: bool,
    existing_console: Option<&str>,
    existing_commands: &[ArtisanCommand],
) -> String {
    let mut hasher = Sha256::new();
    if let Ok(form_bytes) = serde_json::to_vec(form) {
        hasher.update(&form_bytes);
    }
    hasher.update(format!("{layout:?}|{namespace}|{class_exists}|").as_bytes());
    if let Some(console) = existing_console {
        hasher.update(sha256_hex(console.as_bytes()).as_bytes());
    } else {
        hasher.update(b"NONE");
    }
    hasher.update(b"|");
    for cmd in existing_commands {
        hasher.update(cmd.name.as_bytes());
        hasher.update(b",");
    }
    hex::encode(hasher.finalize())
}

fn rollback_or_partial(
    php_path: &Path,
    rel_path: &str,
    expected_ino: u64,
    expected_sha: &str,
    reason: String,
) -> MtResult<ArtisanCreateResult> {
    let can_remove = (|| -> bool {
        let Ok(meta) = fs::symlink_metadata(php_path) else {
            return false;
        };
        if meta.file_type().is_symlink() || meta.ino() != expected_ino {
            return false;
        }
        let Ok(bytes) = fs::read(php_path) else {
            return false;
        };
        sha256_hex(&bytes) == expected_sha
    })();

    if can_remove && fs::remove_file(php_path).is_ok() {
        return Err(error::console_routes_failed(reason));
    }

    Err(MtError::new(
        codes::PARTIAL_WRITE,
        format!(
            "O comando foi gravado em {rel_path}, mas a atualização de routes/console.php falhou e o arquivo foi preservado: {reason}"
        ),
    )
    .with_details(serde_json::json!({
        "createdPaths": [rel_path],
        "preservedPaths": [rel_path],
    })))
}
