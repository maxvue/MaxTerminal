//! Tipos do formulário "Criar comando", relatório de validação, preview e resultado de criação.

use serde::{Deserialize, Serialize};

/// Formulário completo do modal "Criar comando Artisan" (schema `artisan.command.v1`).
#[derive(
    Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtisanCommandForm {
    /// Nome da classe PHP em PascalCase (ex.: `"SendWeeklyReport"`).
    pub class_name: String,
    /// Nome do comando na signature (ex.: `"reports:send-weekly"`).
    pub signature_name: String,
    /// Descrição curta exibida em `php artisan list`.
    pub description: String,
    /// Lista ordenada de argumentos e opções do comando.
    #[serde(default)]
    pub params: Vec<ArtisanParam>,
    /// Indica se o comando implementa `Illuminate\Contracts\Console\Isolatable`.
    #[serde(default)]
    pub isolatable: bool,
    /// Indica se o comando implementa `Illuminate\Contracts\Console\PromptsForMissingInput`.
    #[serde(default)]
    pub prompts_for_missing_input: bool,
    /// Configuração opcional de agendamento (`Schedule`).
    #[serde(default)]
    pub schedule: Option<ScheduleSpec>,
    /// Objetivo e regras de negócio do comando (documentado no PHPDoc e enviado ao `MaxCode`).
    #[serde(default)]
    pub objective: String,
}

/// Natureza do parâmetro do comando Artisan (`argument` ou `option`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    /// Argumento posicional (`{user}`).
    Argument,
    /// Opção com flag (`{--queue=}`).
    Option,
}

/// Especificação de um argumento ou opção no construtor visual do modal.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtisanParam {
    /// Tipo do parâmetro (`argument` ou `option`).
    pub kind: ParamKind,
    /// Nome do parâmetro sem prefixo `--`.
    pub name: String,
    /// Descrição curta do parâmetro.
    #[serde(default)]
    pub description: String,
    /// Apenas para argumento: indica se é opcional (`?`).
    #[serde(default)]
    pub optional: bool,
    /// Indica se o argumento ou opção recebe múltiplos valores (`*`).
    #[serde(default)]
    pub is_array: bool,
    /// Valor padrão opcional.
    #[serde(default)]
    pub default: Option<String>,
    /// Apenas para opção: atalho de 1 letra ASCII (`-Q`).
    #[serde(default)]
    pub shortcut: Option<String>,
    /// Apenas para opção: indica se aceita valor (`--queue=`).
    #[serde(default)]
    pub accepts_value: bool,
}

/// Especificação de agendamento (`Schedule`) para o comando Artisan.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleSpec {
    /// Frequência de execução do agendamento.
    pub frequency: ScheduleFrequency,
    /// Valores posicionais para cada argumento da signature na execução agendada.
    #[serde(default)]
    pub arguments: Vec<String>,
    /// Opções passadas ao comando na execução agendada.
    #[serde(default)]
    pub options: Vec<ScheduleOption>,
    /// Adiciona `->withoutOverlapping()`.
    #[serde(default)]
    pub without_overlapping: bool,
    /// Adiciona `->onOneServer()`.
    #[serde(default)]
    pub on_one_server: bool,
    /// Adiciona `->runInBackground()`.
    #[serde(default)]
    pub run_in_background: bool,
}

/// Opção individual configurada para a execução agendada do comando.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleOption {
    /// Nome da opção sem prefixo `--`.
    pub name: String,
    /// Valores fornecidos para a opção (quando aceita valor ou é array).
    #[serde(default)]
    pub values: Vec<String>,
    /// Indica se a opção está habilitada no agendamento.
    #[serde(default)]
    pub enabled: bool,
}

/// Frequências suportadas pelo agendador do Laravel (`Illuminate\Console\Scheduling\Schedule`).
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScheduleFrequency {
    /// `->everyMinute()`
    EveryMinute,
    /// `->everyFiveMinutes()`
    EveryFiveMinutes,
    /// `->everyFifteenMinutes()`
    EveryFifteenMinutes,
    /// `->everyThirtyMinutes()`
    EveryThirtyMinutes,
    /// `->hourly()`
    Hourly,
    /// `->hourlyAt(minute)`
    HourlyAt {
        /// Minuto da hora (`0..=59`).
        minute: u8,
    },
    /// `->daily()`
    Daily,
    /// `->dailyAt('HH:MM')`
    DailyAt {
        /// Horário no formato `"HH:MM"`.
        time: String,
    },
    /// `->twiceDaily(first, second)`
    TwiceDaily {
        /// Primeira hora (`0..=23`).
        first: u8,
        /// Segunda hora (`0..=23`, diferente de `first`).
        second: u8,
    },
    /// `->weekly()`
    Weekly,
    /// `->weeklyOn(day, 'HH:MM')`
    WeeklyOn {
        /// Dia da semana (`0` = domingo .. `6` = sábado).
        day: u8,
        /// Horário no formato `"HH:MM"`.
        time: String,
    },
    /// `->monthly()`
    Monthly,
    /// `->monthlyOn(day, 'HH:MM')`
    MonthlyOn {
        /// Dia do mês (`1..=31`).
        day: u8,
        /// Horário no formato `"HH:MM"`.
        time: String,
    },
    /// `->cron('...')`
    Cron {
        /// Expressão cron de 5 campos.
        expression: String,
    },
}

/// Severidade de um apontamento de validação de formulário.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    /// Erro bloqueante que impede a criação do comando.
    Error,
    /// Aviso informativo que não bloqueia a criação.
    Warning,
}

/// Apontamento individual de validação associado a um caminho de campo (`path`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FieldIssue {
    /// Caminho do campo no formulário (`"className"`, `"params[0].name"`, `"schedule.frequency.time"`).
    pub path: String,
    /// Severidade (`error` ou `warning`).
    pub severity: IssueSeverity,
    /// Código estável da regra de validação.
    pub code: String,
    /// Mensagem amigável em `pt-BR`.
    pub message: String,
}

/// Relatório consolidado de validação do formulário `ArtisanCommandForm`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    /// `true` quando não há nenhum apontamento com `severity == Error`.
    pub valid: bool,
    /// Lista de apontamentos de erro e aviso.
    pub issues: Vec<FieldIssue>,
}

/// Prévia de modificação em `routes/console.php`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleRoutesPatch {
    /// Caminho relativo (`"routes/console.php"`).
    pub path: String,
    /// Indica se o arquivo será criado do zero.
    pub will_create: bool,
    /// Linhas adicionadas pelo bloco de agendamento.
    pub added_lines: Vec<String>,
    /// Indica se a alteração exige aplicação manual pelo desenvolvedor.
    pub manual_only: bool,
    /// Hash SHA-256 do conteúdo original de `routes/console.php` quando existente.
    pub base_sha256: Option<String>,
    /// Diff unificado legível para exibição na aba de preview.
    pub unified_diff: String,
}

/// Resultado de `artisan_preview_command`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanPreview {
    /// Hash SHA-256 combinando formulário, catálogo, layout e arquivos-base.
    pub revision: String,
    /// Relatório de validação do formulário.
    pub report: ValidationReport,
    /// Caminho relativo onde o comando será salvo (`"app/Console/Commands/..."`).
    pub path: String,
    /// Assinatura completa montada (`protected $signature`).
    pub signature: String,
    /// Código-fonte PHP completo (ou `None` se `report.valid == false`).
    pub php: Option<String>,
    /// Patch previsto para `routes/console.php` quando há agendamento.
    pub console_patch: Option<ConsoleRoutesPatch>,
    /// Passos manuais necessários (ex.: Laravel 10 `Kernel.php` ou `routes/console.php` ambíguo).
    pub manual_steps: Vec<String>,
}

/// Resultado de `artisan_create_command`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanCreateResult {
    /// Caminho relativo do arquivo PHP criado.
    pub path: String,
    /// Indica se `routes/console.php` foi atualizado ou criado automaticamente.
    pub console_routes_patched: bool,
    /// Passos manuais pendentes para o usuário concluir.
    pub manual_steps: Vec<String>,
    /// Indica que a atualização do catálogo em segundo plano foi disparada.
    pub catalog_refresh_pending: bool,
}

/// Gera o JSON Schema Draft 2020-12 para [`ArtisanCommandForm`].
#[must_use]
pub fn form_json_schema() -> serde_json::Value {
    let schema = schemars::schema_for!(ArtisanCommandForm);
    serde_json::to_value(&schema).unwrap_or_else(|_| serde_json::json!({ "type": "object" }))
}
