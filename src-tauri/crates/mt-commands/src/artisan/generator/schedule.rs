//! Renderização da cadeia fluente de agendamento (`->weeklyOn(...)`) e resumo humano em `pt-BR`.

use crate::artisan::generator::form::{
    ArtisanCommandForm, ParamKind, ScheduleFrequency, ScheduleSpec,
};
use crate::artisan::generator::template::escape_php_single_quoted;

/// Renderiza o sufixo fluente do agendamento (ex.: `"->weeklyOn(1, '08:00')->withoutOverlapping()"`).
#[must_use]
pub fn render_chain(spec: &ScheduleSpec) -> String {
    let mut out = match &spec.frequency {
        ScheduleFrequency::EveryMinute => "->everyMinute()".to_string(),
        ScheduleFrequency::EveryFiveMinutes => "->everyFiveMinutes()".to_string(),
        ScheduleFrequency::EveryFifteenMinutes => "->everyFifteenMinutes()".to_string(),
        ScheduleFrequency::EveryThirtyMinutes => "->everyThirtyMinutes()".to_string(),
        ScheduleFrequency::Hourly => "->hourly()".to_string(),
        ScheduleFrequency::HourlyAt { minute } => format!("->hourlyAt({minute})"),
        ScheduleFrequency::Daily => "->daily()".to_string(),
        ScheduleFrequency::DailyAt { time } => {
            format!("->dailyAt('{}')", escape_php_single_quoted(time.trim()))
        }
        ScheduleFrequency::TwiceDaily { first, second } => {
            format!("->twiceDaily({first}, {second})")
        }
        ScheduleFrequency::Weekly => "->weekly()".to_string(),
        ScheduleFrequency::WeeklyOn { day, time } => {
            format!(
                "->weeklyOn({day}, '{}')",
                escape_php_single_quoted(time.trim())
            )
        }
        ScheduleFrequency::Monthly => "->monthly()".to_string(),
        ScheduleFrequency::MonthlyOn { day, time } => {
            format!(
                "->monthlyOn({day}, '{}')",
                escape_php_single_quoted(time.trim())
            )
        }
        ScheduleFrequency::Cron { expression } => {
            format!("->cron('{}')", escape_php_single_quoted(expression.trim()))
        }
    };

    if spec.without_overlapping {
        out.push_str("->withoutOverlapping()");
    }
    if spec.on_one_server {
        out.push_str("->onOneServer()");
    }
    if spec.run_in_background {
        out.push_str("->runInBackground()");
    }
    out
}

/// Constrói a lista de tokens passados como 2º argumento de `Schedule::command(Class::class, [...])`.
#[must_use]
pub fn build_schedule_tokens(form: &ArtisanCommandForm, spec: &ScheduleSpec) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for arg_val in &spec.arguments {
        let trimmed = arg_val.trim();
        if !trimmed.is_empty() {
            tokens.push(trimmed.to_string());
        }
    }
    for opt in &spec.options {
        if !opt.enabled {
            continue;
        }
        let param_def = form
            .params
            .iter()
            .find(|p| p.kind == ParamKind::Option && p.name == opt.name);
        let accepts = param_def.is_some_and(|p| p.accepts_value || p.is_array);
        if accepts {
            for val in &opt.values {
                let v = val.trim();
                if !v.is_empty() {
                    tokens.push(format!("--{}={v}", opt.name));
                }
            }
        } else {
            tokens.push(format!("--{}", opt.name));
        }
    }
    tokens
}

/// Renderiza o array PHP de parâmetros (`"['42', '--queue=default']"`) ou `None` quando vazio.
#[must_use]
pub fn render_schedule_params_array(
    form: &ArtisanCommandForm,
    spec: &ScheduleSpec,
) -> Option<String> {
    let tokens = build_schedule_tokens(form, spec);
    if tokens.is_empty() {
        return None;
    }
    let quoted: Vec<String> = tokens
        .into_iter()
        .map(|t| format!("'{}'", escape_php_single_quoted(&t)))
        .collect();
    Some(format!("[{}]", quoted.join(", ")))
}

/// Descreve a frequência do agendamento em português do Brasil para o prompt do `MaxCode`.
#[must_use]
pub fn human_frequency_pt_br(freq: &ScheduleFrequency) -> String {
    match freq {
        ScheduleFrequency::EveryMinute => "a cada minuto".to_string(),
        ScheduleFrequency::EveryFiveMinutes => "a cada 5 minutos".to_string(),
        ScheduleFrequency::EveryFifteenMinutes => "a cada 15 minutos".to_string(),
        ScheduleFrequency::EveryThirtyMinutes => "a cada 30 minutos".to_string(),
        ScheduleFrequency::Hourly => "a cada hora".to_string(),
        ScheduleFrequency::HourlyAt { minute } => format!("a cada hora no minuto {minute}"),
        ScheduleFrequency::Daily => "diariamente à meia-noite".to_string(),
        ScheduleFrequency::DailyAt { time } => format!("diariamente às {}", time.trim()),
        ScheduleFrequency::TwiceDaily { first, second } => {
            format!("duas vezes ao dia ({first}h e {second}h)")
        }
        ScheduleFrequency::Weekly => "semanalmente".to_string(),
        ScheduleFrequency::WeeklyOn { day, time } => {
            let day_label = weekday_pt_br(*day);
            format!("semanal, {day_label} às {}", time.trim())
        }
        ScheduleFrequency::Monthly => "mensalmente".to_string(),
        ScheduleFrequency::MonthlyOn { day, time } => {
            format!("mensal, dia {day} às {}", time.trim())
        }
        ScheduleFrequency::Cron { expression } => format!("cron ({})", expression.trim()),
    }
}

fn weekday_pt_br(day: u8) -> &'static str {
    match day {
        0 => "domingo",
        1 => "segunda-feira",
        2 => "terça-feira",
        3 => "quarta-feira",
        4 => "quinta-feira",
        5 => "sexta-feira",
        6 => "sábado",
        _ => "dia inválido",
    }
}
