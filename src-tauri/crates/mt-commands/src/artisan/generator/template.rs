//! Renderizador determinístico de classes PHP de comandos Artisan (coberto por snapshots `insta`).

use std::collections::HashSet;
use std::fmt::Write as _;

use crate::artisan::generator::form::{ArtisanCommandForm, ArtisanParam, ParamKind};
use crate::artisan::generator::signature::{build_signature, ordered_params};

/// Escapa caracteres especiais para literal PHP entre aspas simples (`\` → `\\`, `'` → `\'`).
#[must_use]
pub fn escape_php_single_quoted(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            _ => out.push(ch),
        }
    }
    out
}

/// Sanitiza texto inserido em bloco PHPDoc escapando `*/` para `*\/`.
#[must_use]
pub fn escape_phpdoc_line(input: &str) -> String {
    input.replace("*/", "*\\/")
}

/// Converte um nome `kebab-case` ou `snake_case` em `camelCase` para variáveis locais do `handle()`.
#[must_use]
pub fn to_camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper_next = false;
    for ch in name.chars() {
        if ch == '-' || ch == '_' {
            upper_next = true;
        } else if upper_next {
            for up in ch.to_uppercase() {
                out.push(up);
            }
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Renderiza o arquivo PHP completo para `form` sob o `namespace` informado.
#[must_use]
pub fn render_php_command(form: &ArtisanCommandForm, namespace: &str) -> String {
    let mut out = String::with_capacity(2048);
    let class_name = form.class_name.trim();

    let use_command_alias = class_name.eq_ignore_ascii_case("Command");
    let use_isolatable_alias = class_name.eq_ignore_ascii_case("Isolatable");
    let use_prompts_alias = class_name.eq_ignore_ascii_case("PromptsForMissingInput");

    let command_type = if use_command_alias {
        "BaseCommand"
    } else {
        "Command"
    };
    let isolatable_type = if use_isolatable_alias {
        "IsolatableContract"
    } else {
        "Isolatable"
    };
    let prompts_type = if use_prompts_alias {
        "PromptsForMissingInputContract"
    } else {
        "PromptsForMissingInput"
    };

    let _ = writeln!(out, "<?php\n");
    let _ = writeln!(out, "namespace {namespace};\n");
    if use_command_alias {
        let _ = writeln!(out, "use Illuminate\\Console\\Command as BaseCommand;");
    } else {
        let _ = writeln!(out, "use Illuminate\\Console\\Command;");
    }
    if form.isolatable {
        if use_isolatable_alias {
            let _ = writeln!(
                out,
                "use Illuminate\\Contracts\\Console\\Isolatable as IsolatableContract;"
            );
        } else {
            let _ = writeln!(out, "use Illuminate\\Contracts\\Console\\Isolatable;");
        }
    }
    if form.prompts_for_missing_input {
        if use_prompts_alias {
            let _ = writeln!(
                out,
                "use Illuminate\\Contracts\\Console\\PromptsForMissingInput as PromptsForMissingInputContract;"
            );
        } else {
            let _ = writeln!(
                out,
                "use Illuminate\\Contracts\\Console\\PromptsForMissingInput;"
            );
        }
    }

    out.push('\n');
    let _ = writeln!(out, "/**");
    let _ = writeln!(out, " * {}", escape_phpdoc_line(form.description.trim()));
    let objective = form.objective.trim();
    if !objective.is_empty() {
        let _ = writeln!(out, " *");
        let _ = writeln!(out, " * Objetivo / Regras:");
        for line in objective.lines() {
            let trimmed_end = line.trim_end();
            if trimmed_end.is_empty() {
                let _ = writeln!(out, " *");
            } else {
                let _ = writeln!(out, " * {}", escape_phpdoc_line(trimmed_end));
            }
        }
    }
    let _ = writeln!(out, " *");
    let _ = writeln!(out, " * Gerado pelo MaxTerminal.");
    let _ = writeln!(out, " */");

    let mut interfaces = Vec::new();
    if form.isolatable {
        interfaces.push(isolatable_type);
    }
    if form.prompts_for_missing_input {
        interfaces.push(prompts_type);
    }

    if interfaces.is_empty() {
        let _ = writeln!(out, "class {class_name} extends {command_type}");
    } else {
        let _ = writeln!(
            out,
            "class {class_name} extends {command_type} implements {}",
            interfaces.join(", ")
        );
    }
    let _ = writeln!(out, "{{");

    let sig = build_signature(form, true);
    let sig_escaped = escape_php_single_quoted(&sig);
    let _ = writeln!(out, "    /**");
    let _ = writeln!(out, "     * Nome e assinatura do comando no console.");
    let _ = writeln!(out, "     *");
    let _ = writeln!(out, "     * @var string");
    let _ = writeln!(out, "     */");
    let _ = writeln!(out, "    protected $signature = '{sig_escaped}';\n");

    let desc_escaped = escape_php_single_quoted(form.description.trim());
    let _ = writeln!(out, "    /**");
    let _ = writeln!(out, "     * Descrição do comando no console.");
    let _ = writeln!(out, "     *");
    let _ = writeln!(out, "     * @var string");
    let _ = writeln!(out, "     */");
    let _ = writeln!(out, "    protected $description = '{desc_escaped}';\n");

    let _ = writeln!(out, "    /**");
    let _ = writeln!(out, "     * Executa o comando.");
    let _ = writeln!(out, "     */");
    let _ = writeln!(out, "    public function handle(): int");
    let _ = writeln!(out, "    {{");

    let ordered = ordered_params(&form.params);
    if !ordered.is_empty() {
        render_handle_bindings(&mut out, &ordered);
        out.push('\n');
    }

    let _ = writeln!(
        out,
        "        // TODO: implementar conforme \"Objetivo / Regras\" descrito acima.\n"
    );
    let _ = writeln!(
        out,
        "        $this->warn('Implemente o objetivo do comando antes de executá-lo.');"
    );
    let _ = writeln!(out, "        return self::FAILURE;");
    let _ = writeln!(out, "    }}");

    let required_args: Vec<&ArtisanParam> = ordered
        .iter()
        .copied()
        .filter(|p| {
            p.kind == ParamKind::Argument
                && !p.optional
                && p.default.as_deref().map(str::trim).unwrap_or("").is_empty()
        })
        .collect();

    if form.prompts_for_missing_input && !required_args.is_empty() {
        out.push('\n');
        let _ = writeln!(out, "    /**");
        let _ = writeln!(
            out,
            "     * Perguntas exibidas quando argumentos obrigatórios não forem informados."
        );
        let _ = writeln!(out, "     *");
        let _ = writeln!(out, "     * @return array<string, string>");
        let _ = writeln!(out, "     */");
        let _ = writeln!(
            out,
            "    protected function promptForMissingArgumentsUsing(): array"
        );
        let _ = writeln!(out, "    {{");
        let _ = writeln!(out, "        return [");
        for arg in required_args {
            let name_esc = escape_php_single_quoted(arg.name.trim());
            let prompt_text = if arg.description.trim().is_empty() {
                format!("Informe o valor de \"{}\"", arg.name.trim())
            } else {
                format!("Informe: {}", arg.description.trim())
            };
            let prompt_esc = escape_php_single_quoted(&prompt_text);
            let _ = writeln!(out, "            '{name_esc}' => '{prompt_esc}',");
        }
        let _ = writeln!(out, "        ];");
        let _ = writeln!(out, "    }}");
    }

    let _ = writeln!(out, "}}");
    out
}

fn render_handle_bindings(out: &mut String, ordered: &[&ArtisanParam]) {
    let mut camel_counts = std::collections::HashMap::<String, usize>::new();
    for p in ordered {
        let camel = to_camel_case(p.name.trim());
        *camel_counts.entry(camel).or_insert(0) += 1;
    }

    let mut used_vars = HashSet::<String>::new();
    for p in ordered {
        let raw_name = p.name.trim();
        let name_esc = escape_php_single_quoted(raw_name);
        let camel = to_camel_case(raw_name);
        let has_collision = camel == "this"
            || camel.is_empty()
            || camel.as_bytes()[0].is_ascii_digit()
            || camel_counts.get(&camel).copied().unwrap_or(0) > 1
            || !used_vars.insert(camel.clone());

        match p.kind {
            ParamKind::Argument => {
                let comment = if p.is_array {
                    " // array<int, string>"
                } else {
                    ""
                };
                if has_collision {
                    let _ = writeln!(
                        out,
                        "        $arguments['{name_esc}'] = $this->argument('{name_esc}');{comment}"
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "        ${camel} = $this->argument('{name_esc}');{comment}"
                    );
                }
            }
            ParamKind::Option => {
                let comment = if p.is_array {
                    " // array<int, string>"
                } else {
                    ""
                };
                let rhs = if p.accepts_value || p.is_array {
                    format!("$this->option('{name_esc}')")
                } else {
                    format!("(bool) $this->option('{name_esc}')")
                };
                if has_collision {
                    let _ = writeln!(out, "        $options['{name_esc}'] = {rhs};{comment}");
                } else {
                    let _ = writeln!(out, "        ${camel} = {rhs};{comment}");
                }
            }
        }
    }
}
