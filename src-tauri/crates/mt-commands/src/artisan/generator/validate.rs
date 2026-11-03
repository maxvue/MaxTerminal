//! Validação completa do formulário `ArtisanCommandForm` (regras de sintaxe PHP, Symfony Console, Cron e catálogo).

use std::collections::HashSet;

use crate::artisan::generator::form::{
    ArtisanCommandForm, FieldIssue, IssueSeverity, ParamKind, ScheduleFrequency, ScheduleSpec,
    ValidationReport,
};
use crate::artisan::model::ArtisanCommand;

const PHP_RESERVED_KEYWORDS: &[&str] = &[
    "__halt_compiler",
    "abstract",
    "and",
    "array",
    "as",
    "break",
    "callable",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "die",
    "do",
    "echo",
    "else",
    "elseif",
    "empty",
    "enddeclare",
    "endfor",
    "endforeach",
    "endif",
    "endswitch",
    "endwhile",
    "enum",
    "eval",
    "exit",
    "extends",
    "false",
    "final",
    "finally",
    "fn",
    "for",
    "foreach",
    "function",
    "global",
    "goto",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "insteadof",
    "int",
    "interface",
    "isset",
    "iterable",
    "list",
    "match",
    "mixed",
    "namespace",
    "never",
    "new",
    "null",
    "numeric",
    "object",
    "or",
    "parent",
    "print",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "resource",
    "return",
    "self",
    "static",
    "string",
    "switch",
    "throw",
    "trait",
    "true",
    "try",
    "unset",
    "use",
    "var",
    "void",
    "while",
    "xor",
    "yield",
];

const RESERVED_GLOBAL_OPTIONS: &[&str] = &[
    "help",
    "quiet",
    "verbose",
    "version",
    "ansi",
    "no-ansi",
    "no-interaction",
    "env",
];

const RESERVED_SHORTCUTS: &[char] = &['h', 'q', 'V', 'n', 'v'];
const MAX_PARAMS: usize = 100;
const MAX_OBJECTIVE_CHARS: usize = 8_000;

/// Valida o formulário `form` contra as regras estritas da Fase 14 e o catálogo existente.
#[must_use]
pub fn validate_form(
    form: &ArtisanCommandForm,
    existing_commands: &[ArtisanCommand],
    class_file_exists: bool,
    psr4_error: Option<&str>,
) -> ValidationReport {
    let mut issues = Vec::new();

    if let Some(msg) = psr4_error {
        issues.push(err("className", "psr4_unresolved", msg.to_string()));
    }

    validate_class_name(&form.class_name, class_file_exists, &mut issues);
    validate_signature_name(&form.signature_name, existing_commands, &mut issues);
    validate_description(&form.description, &mut issues);
    validate_params(form, &mut issues);
    validate_prompts_warning(form, &mut issues);

    if let Some(ref sched) = form.schedule {
        validate_schedule(form, sched, &mut issues);
    }

    if form.objective.chars().count() > MAX_OBJECTIVE_CHARS {
        issues.push(err(
            "objective",
            "objective_too_long",
            "Objetivo muito longo (máx. 8 000).".to_string(),
        ));
    }

    let valid = !issues.iter().any(|i| i.severity == IssueSeverity::Error);
    ValidationReport { valid, issues }
}

fn err(path: impl Into<String>, code: impl Into<String>, message: String) -> FieldIssue {
    FieldIssue {
        path: path.into(),
        severity: IssueSeverity::Error,
        code: code.into(),
        message,
    }
}

fn warn(path: impl Into<String>, code: impl Into<String>, message: String) -> FieldIssue {
    FieldIssue {
        path: path.into(),
        severity: IssueSeverity::Warning,
        code: code.into(),
        message,
    }
}

fn validate_class_name(raw: &str, file_exists: bool, issues: &mut Vec<FieldIssue>) {
    let name = raw.trim();
    let valid_syntax = !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_uppercase()
        && name.bytes().all(|b| b.is_ascii_alphanumeric());

    if !valid_syntax {
        issues.push(err(
            "className",
            "invalid_class_name",
            "Use PascalCase (ex.: SendWeeklyReport).".to_string(),
        ));
        return;
    }

    let lower = name.to_ascii_lowercase();
    if PHP_RESERVED_KEYWORDS.contains(&lower.as_str()) {
        issues.push(err(
            "className",
            "reserved_php_keyword",
            format!("\"{name}\" é palavra reservada do PHP."),
        ));
        return;
    }

    if file_exists {
        issues.push(err(
            "className",
            "class_file_exists",
            format!(
                "O arquivo app/Console/Commands/{name}.php já existe; escolha outro nome de classe."
            ),
        ));
    }
}

/// Verifica se uma signature segue `^[a-z][a-z0-9-]*(:[a-z0-9][a-z0-9-]*)*$` e `<= 100` chars.
#[must_use]
pub fn is_valid_signature_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 100 {
        return false;
    }
    let mut parts = name.split(':');
    let Some(first) = parts.next() else {
        return false;
    };
    if first.is_empty()
        || !first.as_bytes()[0].is_ascii_lowercase()
        || !first
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return false;
    }
    for seg in parts {
        if seg.is_empty()
            || !(seg.as_bytes()[0].is_ascii_lowercase() || seg.as_bytes()[0].is_ascii_digit())
            || !seg
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return false;
        }
    }
    true
}

fn validate_signature_name(
    raw: &str,
    existing_commands: &[ArtisanCommand],
    issues: &mut Vec<FieldIssue>,
) {
    let sig = raw.trim();
    if !is_valid_signature_name(sig) {
        issues.push(err(
            "signatureName",
            "invalid_signature_name",
            "Signature inválida: use minúsculas, `-` e `:`.".to_string(),
        ));
        return;
    }

    let collides = existing_commands
        .iter()
        .any(|c| c.name == sig || c.aliases.iter().any(|a| a == sig));
    if collides {
        issues.push(err(
            "signatureName",
            "signature_collision",
            format!("Já existe o comando \"{sig}\"."),
        ));
    }
}

fn validate_description(raw: &str, issues: &mut Vec<FieldIssue>) {
    let desc = raw.trim();
    if desc.is_empty() || desc.chars().count() > 255 {
        issues.push(err(
            "description",
            "invalid_description_length",
            "A descrição do comando deve ter entre 1 e 255 caracteres.".to_string(),
        ));
        return;
    }
    if desc.contains('{') || desc.contains('}') || desc.contains('\n') || desc.contains('\r') {
        issues.push(err(
            "description",
            "invalid_description_chars",
            "Descrições não podem conter { ou }.".to_string(),
        ));
    }
}

fn is_valid_param_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

fn is_valid_default_value(val: &str) -> bool {
    if val.is_empty() {
        return false;
    }
    let bytes = val.as_bytes();
    if bytes[0].is_ascii_whitespace() {
        return false;
    }
    if val.contains('{')
        || val.contains('}')
        || val.contains('\n')
        || val.contains('\r')
        || val.contains(" : ")
    {
        return false;
    }
    true
}

fn validate_params(form: &ArtisanCommandForm, issues: &mut Vec<FieldIssue>) {
    if form.params.len() > MAX_PARAMS {
        issues.push(err(
            "params",
            "too_many_params",
            format!("O comando pode ter no máximo {MAX_PARAMS} parâmetros."),
        ));
        return;
    }

    let mut seen_names = HashSet::new();
    let mut seen_shortcuts = HashSet::new();
    let mut seen_optional_arg = false;
    let mut seen_array_arg_index: Option<usize> = None;
    let mut last_arg_index: Option<usize> = None;

    for (idx, p) in form.params.iter().enumerate() {
        if p.kind == ParamKind::Argument {
            last_arg_index = Some(idx);
        }
    }

    for (idx, p) in form.params.iter().enumerate() {
        let name_path = format!("params[{idx}].name");
        let desc_path = format!("params[{idx}].description");
        let default_path = format!("params[{idx}].default");
        let shortcut_path = format!("params[{idx}].shortcut");

        let name = p.name.trim();
        if !is_valid_param_name(name) {
            issues.push(err(
                &name_path,
                "invalid_param_name",
                "Use apenas letras minúsculas, números, `-` ou `_` iniciando com letra."
                    .to_string(),
            ));
        } else if !seen_names.insert(name.to_string()) {
            issues.push(err(
                &name_path,
                "duplicate_param_name",
                format!("Nome duplicado: \"{name}\"."),
            ));
        }

        let p_desc = p.description.trim();
        if p_desc.chars().count() > 255
            || p_desc.contains('{')
            || p_desc.contains('}')
            || p_desc.contains('\n')
            || p_desc.contains('\r')
        {
            issues.push(err(
                &desc_path,
                "invalid_param_description",
                "Descrições não podem conter { ou }.".to_string(),
            ));
        }

        let has_default = p
            .default
            .as_deref()
            .map(str::trim)
            .is_some_and(|d| !d.is_empty());

        match p.kind {
            ParamKind::Argument => {
                if p.shortcut.as_deref().is_some_and(|s| !s.trim().is_empty()) {
                    issues.push(err(
                        &shortcut_path,
                        "shortcut_on_argument",
                        "Argumentos posicionais não possuem atalho curto.".to_string(),
                    ));
                }

                if p.optional {
                    seen_optional_arg = true;
                } else if seen_optional_arg {
                    issues.push(err(
                        &name_path,
                        "required_after_optional",
                        "Argumento obrigatório não pode vir após um opcional.".to_string(),
                    ));
                }

                if !p.optional && has_default {
                    issues.push(err(
                        &default_path,
                        "default_on_required_argument",
                        "Valor padrão inválido.".to_string(),
                    ));
                }

                if p.is_array {
                    if seen_array_arg_index.is_some() || last_arg_index != Some(idx) {
                        issues.push(err(
                            &name_path,
                            "array_not_last_argument",
                            "Apenas o último argumento pode ser array (*).".to_string(),
                        ));
                    }
                    seen_array_arg_index = Some(idx);
                    if has_default {
                        issues.push(err(
                            &default_path,
                            "default_on_array",
                            "Valor padrão inválido.".to_string(),
                        ));
                    }
                }
            }
            ParamKind::Option => {
                if RESERVED_GLOBAL_OPTIONS.contains(&name)
                    || (form.isolatable && name == "isolated")
                {
                    issues.push(err(
                        &name_path,
                        "reserved_option_name",
                        format!("\"--{name}\" é uma opção global reservada."),
                    ));
                }

                if p.is_array && !p.accepts_value {
                    issues.push(err(
                        &name_path,
                        "option_array_without_value",
                        "Opções de múltiplos valores precisam aceitar valor.".to_string(),
                    ));
                }

                if has_default && (!p.accepts_value || p.is_array) {
                    issues.push(err(
                        &default_path,
                        "invalid_option_default",
                        "Valor padrão inválido.".to_string(),
                    ));
                }

                if let Some(sc_raw) = p.shortcut.as_deref() {
                    let sc = sc_raw.trim();
                    if !sc.is_empty() {
                        let chars: Vec<char> = sc.chars().collect();
                        if chars.len() != 1 || !chars[0].is_ascii_alphabetic() {
                            issues.push(err(
                                &shortcut_path,
                                "invalid_shortcut_format",
                                "O atalho deve conter exatamente 1 letra ASCII.".to_string(),
                            ));
                        } else {
                            let ch = chars[0];
                            if RESERVED_SHORTCUTS.contains(&ch) {
                                issues.push(err(
                                    &shortcut_path,
                                    "reserved_shortcut",
                                    "Atalho reservado pelo Symfony Console.".to_string(),
                                ));
                            } else if !seen_shortcuts.insert(ch) {
                                issues.push(err(
                                    &shortcut_path,
                                    "duplicate_shortcut",
                                    format!("Atalho \"-{ch}\" duplicado."),
                                ));
                            }
                        }
                    }
                }
            }
        }

        if let Some(def_val) = p.default.as_deref() {
            let trimmed = def_val.trim();
            if !trimmed.is_empty() && !is_valid_default_value(def_val) {
                issues.push(err(
                    &default_path,
                    "invalid_default_value",
                    "Valor padrão inválido.".to_string(),
                ));
            }
        }
    }
}

fn validate_prompts_warning(form: &ArtisanCommandForm, issues: &mut Vec<FieldIssue>) {
    if !form.prompts_for_missing_input {
        return;
    }
    let has_required_arg = form.params.iter().any(|p| {
        p.kind == ParamKind::Argument
            && !p.optional
            && p.default.as_deref().map(str::trim).unwrap_or("").is_empty()
    });
    if !has_required_arg {
        issues.push(warn(
            "promptsForMissingInput",
            "prompts_without_required_args",
            "Sem argumentos obrigatórios: PromptsForMissingInput não terá efeito.".to_string(),
        ));
    }
}

fn validate_schedule(
    form: &ArtisanCommandForm,
    sched: &ScheduleSpec,
    issues: &mut Vec<FieldIssue>,
) {
    match &sched.frequency {
        ScheduleFrequency::EveryMinute
        | ScheduleFrequency::EveryFiveMinutes
        | ScheduleFrequency::EveryFifteenMinutes
        | ScheduleFrequency::EveryThirtyMinutes
        | ScheduleFrequency::Hourly
        | ScheduleFrequency::Daily
        | ScheduleFrequency::Weekly
        | ScheduleFrequency::Monthly => {}
        ScheduleFrequency::HourlyAt { minute } => {
            if *minute > 59 {
                issues.push(err(
                    "schedule.frequency.minute",
                    "invalid_schedule_minute",
                    "O minuto deve estar entre 0 e 59.".to_string(),
                ));
            }
        }
        ScheduleFrequency::DailyAt { time } => {
            if !is_valid_hh_mm(time) {
                issues.push(err(
                    "schedule.frequency.time",
                    "invalid_schedule_time",
                    "Horário inválido: use o formato HH:MM (00:00 a 23:59).".to_string(),
                ));
            }
        }
        ScheduleFrequency::TwiceDaily { first, second } => {
            if *first > 23 || *second > 23 || first == second {
                issues.push(err(
                    "schedule.frequency.first",
                    "invalid_twice_daily",
                    "Informe duas horas distintas entre 0 e 23.".to_string(),
                ));
            }
        }
        ScheduleFrequency::WeeklyOn { day, time } => {
            if *day > 6 {
                issues.push(err(
                    "schedule.frequency.day",
                    "invalid_weekly_day",
                    "O dia da semana deve estar entre 0 (domingo) e 6 (sábado).".to_string(),
                ));
            }
            if !is_valid_hh_mm(time) {
                issues.push(err(
                    "schedule.frequency.time",
                    "invalid_schedule_time",
                    "Horário inválido: use o formato HH:MM (00:00 a 23:59).".to_string(),
                ));
            }
        }
        ScheduleFrequency::MonthlyOn { day, time } => {
            if *day < 1 || *day > 31 {
                issues.push(err(
                    "schedule.frequency.day",
                    "invalid_monthly_day",
                    "O dia do mês deve estar entre 1 e 31.".to_string(),
                ));
            }
            if !is_valid_hh_mm(time) {
                issues.push(err(
                    "schedule.frequency.time",
                    "invalid_schedule_time",
                    "Horário inválido: use o formato HH:MM (00:00 a 23:59).".to_string(),
                ));
            }
        }
        ScheduleFrequency::Cron { expression } => {
            if let Err(field_name) = validate_cron_5(expression) {
                issues.push(err(
                    "schedule.frequency.expression",
                    "invalid_cron_expression",
                    format!("Expressão cron inválida no campo \"{field_name}\"."),
                ));
            }
        }
    }

    let required_args_count = form
        .params
        .iter()
        .filter(|p| {
            p.kind == ParamKind::Argument
                && !p.optional
                && p.default.as_deref().map(str::trim).unwrap_or("").is_empty()
        })
        .count();

    for req_idx in 0..required_args_count {
        let provided = sched.arguments.get(req_idx).map(|s| s.trim()).unwrap_or("");
        if provided.is_empty() || contains_forbidden_control(provided) {
            issues.push(err(
                format!("schedule.arguments[{req_idx}]"),
                "missing_schedule_argument",
                "Preencha o valor do argumento obrigatório para a execução agendada.".to_string(),
            ));
        }
    }

    for (idx, opt) in sched.options.iter().enumerate() {
        if !opt.enabled {
            continue;
        }
        let Some(param) = form
            .params
            .iter()
            .find(|p| p.kind == ParamKind::Option && p.name == opt.name)
        else {
            issues.push(err(
                format!("schedule.options[{idx}]"),
                "unknown_schedule_option",
                format!("A opção \"--{}\" não existe na signature.", opt.name),
            ));
            continue;
        };
        if param.accepts_value && opt.values.iter().all(|v| v.trim().is_empty()) {
            issues.push(err(
                format!("schedule.options[{idx}]"),
                "empty_schedule_option_value",
                format!(
                    "Informe o valor da opção \"--{}\" no agendamento.",
                    opt.name
                ),
            ));
        }
        if opt.values.iter().any(|v| contains_forbidden_control(v)) {
            issues.push(err(
                format!("schedule.options[{idx}]"),
                "invalid_schedule_option_chars",
                "Valores do agendamento não podem conter quebras de linha.".to_string(),
            ));
        }
    }
}

fn contains_forbidden_control(s: &str) -> bool {
    s.contains('\n') || s.contains('\r') || s.contains('\0')
}

fn is_valid_hh_mm(time: &str) -> bool {
    let trimmed = time.trim();
    if trimmed.len() != 5 || trimmed.as_bytes()[2] != b':' {
        return false;
    }
    let Ok(hh) = trimmed[0..2].parse::<u8>() else {
        return false;
    };
    let Ok(mm) = trimmed[3..5].parse::<u8>() else {
        return false;
    };
    hh <= 23 && mm <= 59
}

fn validate_cron_5(expr: &str) -> Result<(), &'static str> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err("formato");
    }
    let specs = [
        ("minuto", 0u8, 59u8),
        ("hora", 0u8, 23u8),
        ("dia do mês", 1u8, 31u8),
        ("mês", 1u8, 12u8),
        ("dia da semana", 0u8, 7u8),
    ];
    for (field, (name, min, max)) in fields.into_iter().zip(specs) {
        if !validate_cron_field(field, min, max) {
            return Err(name);
        }
    }
    Ok(())
}

fn validate_cron_field(field: &str, min: u8, max: u8) -> bool {
    if field.is_empty() {
        return false;
    }
    for part in field.split(',') {
        if part.is_empty() {
            return false;
        }
        let (range_part, step_opt) = match part.split_once('/') {
            Some((r, s)) => {
                let Ok(step) = s.parse::<u8>() else {
                    return false;
                };
                if step == 0 || step > max {
                    return false;
                }
                (r, Some(step))
            }
            None => (part, None),
        };
        let _ = step_opt;
        if range_part == "*" {
            continue;
        }
        if let Some((a_str, b_str)) = range_part.split_once('-') {
            let (Ok(a), Ok(b)) = (a_str.parse::<u8>(), b_str.parse::<u8>()) else {
                return false;
            };
            if a < min || b > max || a > b {
                return false;
            }
        } else {
            let Ok(n) = range_part.parse::<u8>() else {
                return false;
            };
            if n < min || n > max {
                return false;
            }
        }
    }
    true
}
