//! Testes do `CommandGenerator`: validação (§7.2), renderização PHP (§7.6), prompt MaxCode (§7.3),
//! `routes/console.php` idempotente, layout Legacy e criação exclusiva (`O_EXCL`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::fs;
use std::path::Path;
use std::sync::Arc;

use mt_commands::artisan::generator::CommandGenerator;
use mt_commands::artisan::generator::form::{
    ArtisanCommandForm, ArtisanParam, ParamKind, ScheduleFrequency, ScheduleOption, ScheduleSpec,
};
use mt_commands::artisan::model::{ArtisanCommand, LaravelLayout};
use mt_commands::artisan::runner::RunnerSelector;
use mt_commands::artisan::service::ArtisanService;
use mt_commands::error::codes;
use mt_commands::ports::{NoopCommandsEventSink, SystemClock};
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn setup_modern_laravel(dir: &Path) {
    fs::create_dir_all(dir.join("app/Console/Commands")).expect("mkdir");
    fs::create_dir_all(dir.join("bootstrap")).expect("mkdir");
    fs::create_dir_all(dir.join("routes")).expect("mkdir");
    fs::write(dir.join("artisan"), "#!/usr/bin/env php\n<?php\n").expect("artisan");
    fs::write(
        dir.join("composer.json"),
        r#"{
            "name": "laravel/laravel",
            "require": { "laravel/framework": "^11.0" },
            "autoload": { "psr-4": { "App\\": "app/" } }
        }"#,
    )
    .expect("composer.json");
    fs::write(
        dir.join("composer.lock"),
        r#"{
            "packages": [
                { "name": "laravel/framework", "version": "v11.31.0" }
            ]
        }"#,
    )
    .expect("composer.lock");
    fs::write(
        dir.join("bootstrap/app.php"),
        "<?php\nuse Illuminate\\Foundation\\Application;\nreturn Application::configure(basePath: dirname(__DIR__))->withRouting(commands: __DIR__.'/../routes/console.php')->create();\n",
    )
    .expect("bootstrap");
}

fn sample_full_form() -> ArtisanCommandForm {
    ArtisanCommandForm {
        class_name: "SendWeeklyReport".to_string(),
        signature_name: "reports:send-weekly".to_string(),
        description: "Envia o relatório semanal por e-mail".to_string(),
        params: vec![
            ArtisanParam {
                kind: ParamKind::Argument,
                name: "user".to_string(),
                description: "ID do usuário".to_string(),
                optional: false,
                is_array: false,
                default: None,
                shortcut: None,
                accepts_value: false,
            },
            ArtisanParam {
                kind: ParamKind::Option,
                name: "queue".to_string(),
                description: "Fila de processamento".to_string(),
                optional: false,
                is_array: false,
                default: Some("default".to_string()),
                shortcut: Some("Q".to_string()),
                accepts_value: true,
            },
            ArtisanParam {
                kind: ParamKind::Option,
                name: "force".to_string(),
                description: "Ignora a confirmação".to_string(),
                optional: false,
                is_array: false,
                default: None,
                shortcut: None,
                accepts_value: false,
            },
        ],
        isolatable: true,
        prompts_for_missing_input: true,
        schedule: Some(ScheduleSpec {
            frequency: ScheduleFrequency::WeeklyOn {
                day: 1,
                time: "08:00".to_string(),
            },
            arguments: vec!["42".to_string()],
            options: Vec::new(),
            without_overlapping: true,
            on_one_server: false,
            run_in_background: false,
        }),
        objective: "Enviar para cada usuário ativo um resumo das vendas da semana anterior.\nNunca enviar duas vezes no mesmo dia.".to_string(),
    }
}

#[test]
fn template_and_maxcode_prompt_match_canonical_spec() {
    let form = sample_full_form();
    let php = mt_commands::artisan::generator::template::render_php_command(
        &form,
        "App\\Console\\Commands",
    );

    let expected_php = r#"<?php

namespace App\Console\Commands;

use Illuminate\Console\Command;
use Illuminate\Contracts\Console\Isolatable;
use Illuminate\Contracts\Console\PromptsForMissingInput;

/**
 * Envia o relatório semanal por e-mail
 *
 * Objetivo / Regras:
 * Enviar para cada usuário ativo um resumo das vendas da semana anterior.
 * Nunca enviar duas vezes no mesmo dia.
 *
 * Gerado pelo MaxTerminal.
 */
class SendWeeklyReport extends Command implements Isolatable, PromptsForMissingInput
{
    /**
     * Nome e assinatura do comando no console.
     *
     * @var string
     */
    protected $signature = 'reports:send-weekly
                            {user : ID do usuário}
                            {--Q|queue=default : Fila de processamento}
                            {--force : Ignora a confirmação}';

    /**
     * Descrição do comando no console.
     *
     * @var string
     */
    protected $description = 'Envia o relatório semanal por e-mail';

    /**
     * Executa o comando.
     */
    public function handle(): int
    {
        $user = $this->argument('user');
        $queue = $this->option('queue');
        $force = (bool) $this->option('force');

        // TODO: implementar conforme "Objetivo / Regras" descrito acima.

        $this->warn('Implemente o objetivo do comando antes de executá-lo.');
        return self::FAILURE;
    }

    /**
     * Perguntas exibidas quando argumentos obrigatórios não forem informados.
     *
     * @return array<string, string>
     */
    protected function promptForMissingArgumentsUsing(): array
    {
        return [
            'user' => 'Informe: ID do usuário',
        ];
    }
}
"#;
    assert_eq!(php, expected_php);

    let prompt = mt_commands::artisan::generator::maxcode_prompt::format_maxcode_prompt(
        &form,
        LaravelLayout::Modern,
        Some("11.31.0"),
    )
    .expect("prompt");

    let expected_prompt = r#"Criar comando artisan:
Nome da Classe: SendWeeklyReport
Signature: reports:send-weekly {user : ID do usuário} {--Q|queue=default : Fila de processamento} {--force : Ignora a confirmação}
Descrição: Envia o relatório semanal por e-mail
Arquivo: app/Console/Commands/SendWeeklyReport.php
Laravel: 11.31.0 (schedule em routes/console.php)
Argumentos:
- user (obrigatório) — ID do usuário
Opções:
- --queue=, atalho -Q, aceita valor, padrão "default" — Fila de processamento
- --force (flag) — Ignora a confirmação
Isolatable: sim
PromptsForMissingInput: sim
Agendamento: semanal, segunda-feira às 08:00 — ->weeklyOn(1, '08:00')->withoutOverlapping()
Parâmetros do agendamento: user=42
Objetivo / Regras:
Enviar para cada usuário ativo um resumo das vendas da semana anterior.
Nunca enviar duas vezes no mesmo dia."#;
    assert_eq!(prompt, expected_prompt);
}

#[test]
fn validate_catches_all_rule_violations() {
    let existing = vec![ArtisanCommand {
        name: "migrate".to_string(),
        description: None,
        options: Vec::new(),
        arguments: Vec::new(),
        aliases: vec!["mig".to_string()],
    }];

    // Palavra reservada PHP + colisão de signature + atalho reservado + argumento obrigatório após opcional + cron inválido
    let bad = ArtisanCommandForm {
        class_name: "List".to_string(),
        signature_name: "migrate".to_string(),
        description: "Descrição inválida com {chave}".to_string(),
        params: vec![
            ArtisanParam {
                kind: ParamKind::Argument,
                name: "first".to_string(),
                description: String::new(),
                optional: true,
                is_array: true,
                default: None,
                shortcut: None,
                accepts_value: false,
            },
            ArtisanParam {
                kind: ParamKind::Argument,
                name: "first".to_string(),
                description: String::new(),
                optional: false,
                is_array: false,
                default: Some("invalido".to_string()),
                shortcut: None,
                accepts_value: false,
            },
            ArtisanParam {
                kind: ParamKind::Option,
                name: "env".to_string(),
                description: String::new(),
                optional: false,
                is_array: false,
                default: None,
                shortcut: Some("h".to_string()),
                accepts_value: false,
            },
        ],
        isolatable: true,
        prompts_for_missing_input: true,
        schedule: Some(ScheduleSpec {
            frequency: ScheduleFrequency::Cron {
                expression: "0 25 * * 1".to_string(),
            },
            arguments: Vec::new(),
            options: vec![ScheduleOption {
                name: "unknown".to_string(),
                values: Vec::new(),
                enabled: true,
            }],
            without_overlapping: false,
            on_one_server: false,
            run_in_background: false,
        }),
        objective: String::new(),
    };

    let report =
        mt_commands::artisan::generator::validate::validate_form(&bad, &existing, false, None);
    assert!(!report.valid);
    let codes_found: Vec<&str> = report.issues.iter().map(|i| i.code.as_str()).collect();
    assert!(codes_found.contains(&"reserved_php_keyword"));
    assert!(codes_found.contains(&"signature_collision"));
    assert!(codes_found.contains(&"invalid_description_chars"));
    assert!(codes_found.contains(&"array_not_last_argument"));
    assert!(codes_found.contains(&"duplicate_param_name"));
    assert!(codes_found.contains(&"required_after_optional"));
    assert!(codes_found.contains(&"reserved_option_name"));
    assert!(codes_found.contains(&"reserved_shortcut"));
    assert!(codes_found.contains(&"invalid_cron_expression"));
}

#[tokio::test]
async fn create_writes_exact_preview_bytes_patches_console_routes_and_rejects_duplicate() {
    let tmp = TempDir::new().expect("tempdir");
    setup_modern_laravel(tmp.path());

    let sink = Arc::new(NoopCommandsEventSink);
    let svc = ArtisanService::new(
        RunnerSelector::default(),
        sink.clone(),
        Arc::new(SystemClock),
    );
    let generator = CommandGenerator::new(svc, sink);

    let form = sample_full_form();
    let preview = generator.preview(tmp.path(), &form, &[]).expect("preview");
    assert!(preview.report.valid);
    let expected_php = preview.php.clone().expect("php");

    let res = generator
        .create(tmp.path(), &form, &preview.revision, &[])
        .await
        .expect("create");
    assert_eq!(res.path, "app/Console/Commands/SendWeeklyReport.php");
    assert!(res.console_routes_patched);
    assert!(res.manual_steps.is_empty());

    // CA-19: bytes gravados == preview.php
    let disk_php = fs::read_to_string(tmp.path().join(&res.path)).expect("read php");
    assert_eq!(disk_php, expected_php);

    let routes_php =
        fs::read_to_string(tmp.path().join("routes/console.php")).expect("read routes");
    assert!(routes_php.contains("// <maxterminal:schedule:SendWeeklyReport>"));
    assert!(routes_php.contains("Schedule::command(\\App\\Console\\Commands\\SendWeeklyReport::class, ['42'])->weeklyOn(1, '08:00')->withoutOverlapping();"));

    // Segunda chamada com o mesmo nome falha com artisan.file_exists sem alterar routes/console.php
    let err = generator
        .create(tmp.path(), &form, &preview.revision, &[])
        .await
        .expect_err("duplicate must fail");
    assert_eq!(err.code, codes::FILE_EXISTS);
}
