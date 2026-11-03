//! Formatação canônica do prompt `"Criar comando artisan:\n..."` enviado ao Plan Mode do `MaxCode`.

use std::fmt::Write as _;

use mt_core::{MtError, MtResult};
use mt_db::sanitizer::sanitize;

use crate::artisan::generator::form::{ArtisanCommandForm, ArtisanParam, ParamKind};
use crate::artisan::generator::schedule::{human_frequency_pt_br, render_chain};
use crate::artisan::generator::signature::build_signature;
use crate::artisan::generator::validate::is_valid_signature_name;
use crate::artisan::model::LaravelLayout;
use crate::error::codes;

/// Formata e sanitiza o prompt determinístico para a ação `"Criar com MaxCode"` (CA-24).
pub fn format_maxcode_prompt(
    form: &ArtisanCommandForm,
    layout: LaravelLayout,
    laravel_version: Option<&str>,
) -> MtResult<String> {
    let class_name = form.class_name.trim();
    let sig_name = form.signature_name.trim();
    if class_name.is_empty() || !is_valid_signature_name(sig_name) {
        return Err(MtError::new(
            codes::INVALID_FORM,
            "Informe ao menos um Nome da Classe e uma Signature válidos antes de enviar ao MaxCode.",
        ));
    }

    let mut out = String::with_capacity(1024);
    let full_sig = build_signature(form, false);
    let desc = {
        let d = form.description.trim();
        if d.is_empty() { "Não informada" } else { d }
    };

    let laravel_line = match (laravel_version, layout) {
        (Some(ver), LaravelLayout::Modern) => {
            format!("{ver} (schedule em routes/console.php)")
        }
        (Some(ver), LaravelLayout::Legacy) => {
            format!("{ver} (schedule manual em app/Console/Kernel.php)")
        }
        (Some(ver), LaravelLayout::Unknown) => ver.to_string(),
        (None, LaravelLayout::Modern) => {
            "desconhecido (schedule em routes/console.php)".to_string()
        }
        (None, LaravelLayout::Legacy) => {
            "desconhecido (schedule manual em app/Console/Kernel.php)".to_string()
        }
        (None, LaravelLayout::Unknown) => "desconhecido".to_string(),
    };

    let _ = writeln!(out, "Criar comando artisan:");
    let _ = writeln!(out, "Nome da Classe: {class_name}");
    let _ = writeln!(out, "Signature: {full_sig}");
    let _ = writeln!(out, "Descrição: {desc}");
    let _ = writeln!(out, "Arquivo: app/Console/Commands/{class_name}.php");
    let _ = writeln!(out, "Laravel: {laravel_line}");

    let args: Vec<&ArtisanParam> = form
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Argument)
        .collect();
    let _ = writeln!(out, "Argumentos:");
    if args.is_empty() {
        let _ = writeln!(out, "- Nenhum");
    } else {
        for arg in &args {
            let _ = writeln!(out, "- {}", format_argument_bullet(arg));
        }
    }

    let opts: Vec<&ArtisanParam> = form
        .params
        .iter()
        .filter(|p| p.kind == ParamKind::Option)
        .collect();
    let _ = writeln!(out, "Opções:");
    if opts.is_empty() {
        let _ = writeln!(out, "- Nenhum");
    } else {
        for opt in &opts {
            let _ = writeln!(out, "- {}", format_option_bullet(opt));
        }
    }

    let _ = writeln!(
        out,
        "Isolatable: {}",
        if form.isolatable { "sim" } else { "não" }
    );
    let _ = writeln!(
        out,
        "PromptsForMissingInput: {}",
        if form.prompts_for_missing_input {
            "sim"
        } else {
            "não"
        }
    );

    if let Some(ref sched) = form.schedule {
        let human = human_frequency_pt_br(&sched.frequency);
        let chain = render_chain(sched);
        let _ = writeln!(out, "Agendamento: {human} — {chain}");
        let sched_params = format_schedule_params_summary(&args, sched);
        if !sched_params.is_empty() {
            let _ = writeln!(out, "Parâmetros do agendamento: {sched_params}");
        }
    } else {
        let _ = writeln!(out, "Agendamento: Nenhum");
    }

    let _ = writeln!(out, "Objetivo / Regras:");
    let objective = form.objective.trim();
    if objective.is_empty() {
        out.push_str("Não informado");
    } else {
        out.push_str(objective);
    }

    Ok(sanitize(&out).into_owned())
}

fn format_argument_bullet(arg: &ArtisanParam) -> String {
    let mut qualifiers = Vec::new();
    let default = arg
        .default
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty());
    if arg.optional || default.is_some() {
        qualifiers.push("opcional".to_string());
    } else {
        qualifiers.push("obrigatório".to_string());
    }
    if arg.is_array {
        qualifiers.push("array".to_string());
    }
    if let Some(d) = default {
        qualifiers.push(format!("padrão \"{d}\""));
    }
    let qual_str = qualifiers.join(", ");
    let desc = arg.description.trim();
    if desc.is_empty() {
        format!("{} ({qual_str})", arg.name.trim())
    } else {
        format!("{} ({qual_str}) — {desc}", arg.name.trim())
    }
}

fn format_option_bullet(opt: &ArtisanParam) -> String {
    let name = opt.name.trim();
    let default = opt
        .default
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty());
    let shortcut = opt
        .shortcut
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let head = if opt.accepts_value || opt.is_array {
        let mut parts = vec![format!("--{name}=")];
        if let Some(sc) = shortcut {
            parts.push(format!("atalho -{sc}"));
        }
        if opt.is_array {
            parts.push("múltiplos valores".to_string());
        } else {
            parts.push("aceita valor".to_string());
        }
        if let Some(d) = default {
            parts.push(format!("padrão \"{d}\""));
        }
        parts.join(", ")
    } else if let Some(sc) = shortcut {
        format!("--{name} (flag, atalho -{sc})")
    } else {
        format!("--{name} (flag)")
    };

    let desc = opt.description.trim();
    if desc.is_empty() {
        head
    } else {
        format!("{head} — {desc}")
    }
}

fn format_schedule_params_summary(
    args: &[&ArtisanParam],
    sched: &crate::artisan::generator::form::ScheduleSpec,
) -> String {
    let mut parts = Vec::new();
    for (idx, val) in sched.arguments.iter().enumerate() {
        let v = val.trim();
        if v.is_empty() {
            continue;
        }
        if let Some(arg) = args.get(idx) {
            parts.push(format!("{}={v}", arg.name.trim()));
        } else {
            parts.push(v.to_string());
        }
    }
    for opt in &sched.options {
        if !opt.enabled {
            continue;
        }
        if opt.values.is_empty() {
            parts.push(format!("--{}", opt.name.trim()));
        } else {
            for val in &opt.values {
                let v = val.trim();
                if !v.is_empty() {
                    parts.push(format!("--{}={v}", opt.name.trim()));
                }
            }
        }
    }
    parts.join(", ")
}
