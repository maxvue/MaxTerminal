//! Detecção de comandos de execução contínua (`Continuous` / daemons) por catálogo de padrões,
//! política explícita (`ContinuousPolicy`) e promoção temporal após `15_000 ms`.

use mt_core::shell::ContinuousPolicy;

/// Limiar temporal em milissegundos para promover um comando em execução para `Continuous` sob política `Auto` (15 s).
pub const CONTINUOUS_PROMOTION_THRESHOLD_MS: i64 = 15_000;

/// Janela de histerese em milissegundos antes de anunciar visualmente `Running` para comandos ultrarrápidos (80 ms).
pub const RUNNING_HYSTERESIS_MS: i64 = 80;

/// Determina se um comando deve iniciar imediatamente como `Continuous` dado o `ContinuousPolicy` e o texto do comando.
#[must_use]
pub fn is_continuous_by_policy_or_command(
    policy: ContinuousPolicy,
    command_text: Option<&str>,
) -> bool {
    match policy {
        ContinuousPolicy::Always => true,
        ContinuousPolicy::Never => false,
        ContinuousPolicy::Auto => command_text.is_some_and(matches_daemon_command),
    }
}

/// Avalia se um comando em execução deve ser promovido para `Continuous` após `elapsed_ms`.
#[must_use]
pub fn should_promote_to_continuous(
    policy: ContinuousPolicy,
    command_text: Option<&str>,
    elapsed_ms: i64,
) -> bool {
    match policy {
        ContinuousPolicy::Always => true,
        ContinuousPolicy::Never => false,
        ContinuousPolicy::Auto => {
            elapsed_ms >= CONTINUOUS_PROMOTION_THRESHOLD_MS
                || command_text.is_some_and(matches_daemon_command)
        }
    }
}

/// Verifica de forma conservadora se a linha de comando corresponde a um servidor de desenvolvimento,
/// watcher ou daemon conhecido.
///
/// Rejeita comandos com flags de consulta rápida (`--help`, `--version`), `build` estático ou
/// execução destacada (`docker compose up -d`).
#[must_use]
pub fn matches_daemon_command(raw_command: &str) -> bool {
    let trimmed = raw_command.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Considera o primeiro segmento principal (antes de redirecionamentos, mantendo tokens simples)
    let first_line = trimmed.lines().next().unwrap_or(trimmed).trim();
    let tokens: Vec<String> = first_line
        .split_whitespace()
        .filter(|t| !t.contains('=') || t.starts_with('-'))
        .map(|t| {
            t.trim_matches(|c| c == '"' || c == '\'')
                .to_ascii_lowercase()
        })
        .collect();

    if tokens.is_empty() {
        return false;
    }

    // Filtros negativos globais: flags de ajuda/versão não são daemons
    if tokens.iter().any(|t| {
        matches!(
            t.as_str(),
            "--help" | "-h" | "--version" | "-v" | "help" | "version"
        )
    }) {
        return false;
    }

    let bin = basename(&tokens[0]);
    let args = &tokens[1..];

    match bin {
        "npm" | "pnpm" | "yarn" | "bun" => matches_js_package_runner(args),
        "npx" | "pnpx" | "bunx" => matches_js_exec_runner(args),
        "vite" => {
            // `vite`, `vite dev`, `vite preview`, `vite serve` são contínuos; `vite build` não é
            !args.iter().any(|a| a == "build" || a == "optimize")
        }
        "php" => matches_php_daemon(args),
        "artisan" => matches_artisan_subcommand(args),
        "cargo" => matches_cargo_daemon(args),
        "docker" | "podman" => matches_container_up_attached(args),
        "docker-compose" => matches_compose_up_attached(args),
        "tail" => args
            .iter()
            .any(|a| a == "-f" || a == "-F" || a.starts_with("--follow")),
        "journalctl" => args
            .iter()
            .any(|a| a == "-f" || a == "--follow" || a.starts_with("-f")),
        "watch" | "nodemon" | "watchexec" | "entr" | "air" => true,
        "uvicorn" | "gunicorn" | "hypercorn" | "flask" => {
            if bin == "flask" {
                args.iter().any(|a| a == "run")
            } else {
                true
            }
        }
        "python" | "python3" => {
            // `python -m http.server` ou `python manage.py runserver`
            (args.len() >= 2 && args[0] == "-m" && args[1] == "http.server")
                || args.iter().any(|a| a == "runserver")
        }
        "tauri" => args.first().is_some_and(|a| a == "dev"),
        _ => false,
    }
}

fn basename(cmd: &str) -> &str {
    cmd.rsplit('/').next().unwrap_or(cmd)
}

fn matches_js_package_runner(args: &[String]) -> bool {
    // Ex.: `npm run dev`, `npm run watch`, `pnpm dev`, `yarn start`, `bun run serve`
    let filtered: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with('-'))
        .collect();

    match filtered.as_slice() {
        ["start", ..] => true,
        ["run", script, ..] | [script, ..] => is_daemon_script_name(script),
        _ => false,
    }
}

fn matches_js_exec_runner(args: &[String]) -> bool {
    let filtered: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with('-'))
        .collect();

    matches!(
        filtered.as_slice(),
        ["vite"]
            | ["vite", "dev" | "serve" | "preview", ..]
            | ["tauri", "dev", ..]
            | ["nodemon" | "concurrently" | "turbo", "dev" | "watch", ..]
    )
}

fn is_daemon_script_name(script: &str) -> bool {
    if script.contains("build") || script.contains("test") || script.contains("lint") {
        return false;
    }
    matches!(
        script,
        "dev"
            | "serve"
            | "watch"
            | "start"
            | "preview"
            | "tauri:dev"
            | "dev:server"
            | "dev:client"
            | "storybook"
    ) || script.starts_with("dev:")
        || script.starts_with("watch:")
        || script.ends_with(":dev")
        || script.ends_with(":watch")
}

fn matches_php_daemon(args: &[String]) -> bool {
    // `php -S localhost:8000`
    if args.iter().any(|a| a == "-s") {
        return true;
    }
    // `php artisan ...` ou `./artisan ...`
    if let Some(pos) = args.iter().position(|a| basename(a) == "artisan") {
        return matches_artisan_subcommand(&args[pos + 1..]);
    }
    false
}

fn matches_artisan_subcommand(args: &[String]) -> bool {
    let sub = args.iter().find(|a| !a.starts_with('-'));
    let Some(cmd) = sub.map(String::as_str) else {
        return false;
    };
    if matches!(
        cmd,
        "serve" | "horizon" | "queue:listen" | "reverb:start" | "octane:start" | "pail"
    ) {
        return true;
    }
    if cmd == "queue:work" {
        // `queue:work --once` ou `--stop-when-empty` termina rápido
        return !args
            .iter()
            .any(|a| a == "--once" || a == "--stop-when-empty");
    }
    false
}

fn matches_cargo_daemon(args: &[String]) -> bool {
    let filtered: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with('-'))
        .collect();
    matches!(
        filtered.as_slice(),
        ["watch", ..] | ["tauri", "dev", ..] | ["trunk", "serve", ..]
    )
}

fn matches_container_up_attached(args: &[String]) -> bool {
    let Some(first) = args.first().map(String::as_str) else {
        return false;
    };
    if first == "compose" {
        return matches_compose_up_attached(&args[1..]);
    }
    false
}

fn matches_compose_up_attached(args: &[String]) -> bool {
    let has_up = args.iter().any(|a| a == "up");
    let detached = args.iter().any(|a| {
        a == "-d"
            || a == "--detach"
            || (a.starts_with('-') && !a.starts_with("--") && a.contains('d'))
    });
    has_up && !detached
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_common_dev_servers_and_rejects_builds_or_detached() {
        assert!(matches_daemon_command("npm run dev"));
        assert!(matches_daemon_command("pnpm dev --host"));
        assert!(matches_daemon_command("vite"));
        assert!(matches_daemon_command("vite dev"));
        assert!(matches_daemon_command("php artisan horizon"));
        assert!(matches_daemon_command("php artisan serve --port=8000"));
        assert!(matches_daemon_command("php artisan queue:work redis"));
        assert!(matches_daemon_command("cargo watch -x check"));
        assert!(matches_daemon_command("tail -f storage/logs/laravel.log"));
        assert!(matches_daemon_command("docker compose up"));
        assert!(matches_daemon_command("python3 -m http.server 8080"));

        assert!(!matches_daemon_command("vite build"));
        assert!(!matches_daemon_command("npm run build"));
        assert!(!matches_daemon_command("npm run dev -- --help"));
        assert!(!matches_daemon_command("docker compose up -d"));
        assert!(!matches_daemon_command("php artisan queue:work --once"));
        assert!(!matches_daemon_command("ls -la"));
    }

    #[test]
    fn policy_overrides_heuristics() {
        assert!(is_continuous_by_policy_or_command(
            ContinuousPolicy::Always,
            Some("ls -la")
        ));
        assert!(!is_continuous_by_policy_or_command(
            ContinuousPolicy::Never,
            Some("npm run dev")
        ));
        assert!(should_promote_to_continuous(
            ContinuousPolicy::Auto,
            Some("sleep 20"),
            15_000
        ));
        assert!(!should_promote_to_continuous(
            ContinuousPolicy::Never,
            Some("sleep 20"),
            30_000
        ));
    }
}
