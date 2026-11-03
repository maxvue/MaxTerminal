//! Planejamento e aplicação idempotente de blocos de agendamento em `routes/console.php` (Laravel >= 11).

use crate::artisan::generator::form::{ArtisanCommandForm, ConsoleRoutesPatch, ScheduleSpec};
use crate::artisan::generator::schedule::{render_chain, render_schedule_params_array};
use crate::artisan::model::LaravelLayout;
use crate::fs_util::sha256_hex;

/// Resultado interno do planejamento de patch em `routes/console.php`.
#[derive(Debug, Clone)]
pub struct PatchPlan {
    /// Patch serializável para o preview.
    pub patch: ConsoleRoutesPatch,
    /// Conteúdo final completo de `routes/console.php` quando `manual_only == false`.
    pub new_content: Option<String>,
    /// Passos manuais quando `manual_only == true` ou layout `Legacy`/`Unknown`.
    pub manual_steps: Vec<String>,
}

/// Planeja a inserção ou atualização do agendamento do comando em `routes/console.php`.
#[must_use]
pub fn plan_console_routes_patch(
    layout: LaravelLayout,
    existing_source: Option<&str>,
    namespace: &str,
    form: &ArtisanCommandForm,
    spec: &ScheduleSpec,
) -> PatchPlan {
    let class_name = form.class_name.trim();
    let fqcn = format!("\\{}\\{}", namespace.trim_matches('\\'), class_name);
    let chain = render_chain(spec);
    let params_opt = render_schedule_params_array(form, spec);

    let call_args = match params_opt {
        Some(ref arr) => format!("{fqcn}::class, {arr}"),
        None => format!("{fqcn}::class"),
    };

    if layout != LaravelLayout::Modern {
        let kernel_call = format!("$schedule->command({call_args}){chain};");
        let step =
            format!("Adicione em app/Console/Kernel.php, no método schedule():\n{kernel_call}");
        return PatchPlan {
            patch: ConsoleRoutesPatch {
                path: "routes/console.php".to_string(),
                will_create: false,
                added_lines: vec![kernel_call.clone()],
                manual_only: true,
                base_sha256: existing_source.map(|s| sha256_hex(s.as_bytes())),
                unified_diff: format!(
                    "// Layout Legacy/Customizado — registro manual:\n{kernel_call}\n"
                ),
            },
            new_content: None,
            manual_steps: vec![step],
        };
    }

    let marker_start = format!("// <maxterminal:schedule:{class_name}>");
    let marker_end = format!("// </maxterminal:schedule:{class_name}>");
    let schedule_line =
        format!("\\Illuminate\\Support\\Facades\\Schedule::command({call_args}){chain};");
    let block_lines = vec![
        marker_start.clone(),
        schedule_line.clone(),
        marker_end.clone(),
    ];
    let block_text = format!("{marker_start}\n{schedule_line}\n{marker_end}\n");

    match existing_source {
        None => {
            let new_file = format!("<?php\n\n{block_text}");
            let diff = build_unified_diff("routes/console.php", "", &new_file);
            PatchPlan {
                patch: ConsoleRoutesPatch {
                    path: "routes/console.php".to_string(),
                    will_create: true,
                    added_lines: block_lines,
                    manual_only: false,
                    base_sha256: None,
                    unified_diff: diff,
                },
                new_content: Some(new_file),
                manual_steps: Vec::new(),
            }
        }
        Some(src) => {
            let base_sha = sha256_hex(src.as_bytes());
            if !is_recognized_php_console_file(src) {
                let step = format!("Adicione manualmente em routes/console.php:\n{schedule_line}");
                return PatchPlan {
                    patch: ConsoleRoutesPatch {
                        path: "routes/console.php".to_string(),
                        will_create: false,
                        added_lines: block_lines,
                        manual_only: true,
                        base_sha256: Some(base_sha),
                        unified_diff: format!(
                            "// routes/console.php requer edição manual:\n+{schedule_line}\n"
                        ),
                    },
                    new_content: None,
                    manual_steps: vec![step],
                };
            }

            let start_count = src.matches(&marker_start).count();
            let end_count = src.matches(&marker_end).count();
            if start_count > 1 || end_count > 1 || start_count != end_count {
                let step = format!(
                    "Bloco de agendamento duplicado ou incompleto em routes/console.php. Revise manualmente:\n{schedule_line}"
                );
                return PatchPlan {
                    patch: ConsoleRoutesPatch {
                        path: "routes/console.php".to_string(),
                        will_create: false,
                        added_lines: block_lines,
                        manual_only: true,
                        base_sha256: Some(base_sha),
                        unified_diff: format!(
                            "// Conflito de marcadores em routes/console.php:\n+{schedule_line}\n"
                        ),
                    },
                    new_content: None,
                    manual_steps: vec![step],
                };
            }

            if start_count == 1 {
                if let (Some(s_idx), Some(e_idx)) = (src.find(&marker_start), src.find(&marker_end))
                {
                    if s_idx < e_idx {
                        let end_pos = e_idx + marker_end.len();
                        let existing_block = &src[s_idx..end_pos];
                        let target_block = format!("{marker_start}\n{schedule_line}\n{marker_end}");
                        if existing_block.trim() == target_block.trim() {
                            return PatchPlan {
                                patch: ConsoleRoutesPatch {
                                    path: "routes/console.php".to_string(),
                                    will_create: false,
                                    added_lines: Vec::new(),
                                    manual_only: false,
                                    base_sha256: Some(base_sha),
                                    unified_diff: String::new(),
                                },
                                new_content: Some(src.to_string()),
                                manual_steps: Vec::new(),
                            };
                        }
                    }
                }
                let step = format!(
                    "Já existe um agendamento diferente para {class_name} em routes/console.php. Atualize manualmente para:\n{schedule_line}"
                );
                return PatchPlan {
                    patch: ConsoleRoutesPatch {
                        path: "routes/console.php".to_string(),
                        will_create: false,
                        added_lines: block_lines,
                        manual_only: true,
                        base_sha256: Some(base_sha),
                        unified_diff: format!(
                            "// Agendamento divergente existente em routes/console.php:\n+{schedule_line}\n"
                        ),
                    },
                    new_content: None,
                    manual_steps: vec![step],
                };
            }

            // Verifica se já existe chamada equivalente fora do bloco delimitado (ignorando comentários)
            let code_only = strip_comments(src);
            if code_only.contains(&format!("{class_name}::class"))
                || code_only.contains(&format!("'{}'", form.signature_name.trim()))
                || code_only.contains(&format!("\"{}\"", form.signature_name.trim()))
            {
                if code_only.contains(&chain) {
                    return PatchPlan {
                        patch: ConsoleRoutesPatch {
                            path: "routes/console.php".to_string(),
                            will_create: false,
                            added_lines: Vec::new(),
                            manual_only: false,
                            base_sha256: Some(base_sha),
                            unified_diff: String::new(),
                        },
                        new_content: Some(src.to_string()),
                        manual_steps: Vec::new(),
                    };
                }
                let step = format!(
                    "O comando {class_name} já é referenciado em routes/console.php. Ajuste manualmente o agendamento:\n{schedule_line}"
                );
                return PatchPlan {
                    patch: ConsoleRoutesPatch {
                        path: "routes/console.php".to_string(),
                        will_create: false,
                        added_lines: block_lines,
                        manual_only: true,
                        base_sha256: Some(base_sha),
                        unified_diff: format!(
                            "// Referência existente em routes/console.php:\n+{schedule_line}\n"
                        ),
                    },
                    new_content: None,
                    manual_steps: vec![step],
                };
            }

            let updated = insert_before_closing_tag_or_eof(src, &block_text);
            let diff = build_unified_diff("routes/console.php", src, &updated);
            PatchPlan {
                patch: ConsoleRoutesPatch {
                    path: "routes/console.php".to_string(),
                    will_create: false,
                    added_lines: block_lines,
                    manual_only: false,
                    base_sha256: Some(base_sha),
                    unified_diff: diff,
                },
                new_content: Some(updated),
                manual_steps: Vec::new(),
            }
        }
    }
}

fn is_recognized_php_console_file(src: &str) -> bool {
    let trimmed = src.trim_start_matches('\u{feff}').trim_start();
    trimmed.starts_with("<?php")
}

fn insert_before_closing_tag_or_eof(src: &str, block_text: &str) -> String {
    let trimmed_end = src.trim_end();
    if let Some(without_close) = trimmed_end.strip_suffix("?>") {
        let prefix = without_close.trim_end();
        format!("{prefix}\n\n{block_text}?>\n")
    } else if src.ends_with('\n') {
        if src.ends_with("\n\n") {
            format!("{src}{block_text}")
        } else {
            format!("{src}\n{block_text}")
        }
    } else {
        format!("{src}\n\n{block_text}")
    }
}

fn strip_comments(src: &str) -> String {
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
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn build_unified_diff(path: &str, old: &str, new: &str) -> String {
    if old == new {
        return String::new();
    }
    let mut out = String::new();
    out.push_str(&format!("--- a/{path}\n+++ b/{path}\n"));
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    out.push_str(&format!(
        "@@ -1,{} +1,{} @@\n",
        old_lines.len(),
        new_lines.len()
    ));
    let common_prefix = old_lines
        .iter()
        .zip(new_lines.iter())
        .take_while(|(a, b)| a == b)
        .count();
    for line in &old_lines[..common_prefix] {
        out.push(' ');
        out.push_str(line);
        out.push('\n');
    }
    for line in &old_lines[common_prefix..] {
        out.push('-');
        out.push_str(line);
        out.push('\n');
    }
    for line in &new_lines[common_prefix..] {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}
