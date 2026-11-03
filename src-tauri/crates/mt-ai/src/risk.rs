//! Classificador determinístico de risco de comandos shell (`RiskClassifier`).

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Nível de risco atribuído a um comando shell.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum RiskLevel {
    /// Comando seguro para execução direta em 1 clique.
    Safe,
    /// Comando que requer atenção (confirmação em popover rápido).
    Caution,
    /// Comando potencialmente destrutivo (requer modal com digitação do token).
    Destructive,
}

impl RiskLevel {
    /// Retorna a representação textual em minúsculas (`"safe"`, `"caution"` ou `"destructive"`).
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Caution => "caution",
            Self::Destructive => "destructive",
        }
    }

    /// Converte uma string de banco/JSON em `RiskLevel`.
    #[must_use]
    pub fn from_str_opt(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "safe" => Some(Self::Safe),
            "caution" => Some(Self::Caution),
            "destructive" => Some(Self::Destructive),
            _ => None,
        }
    }
}

/// Motivo individual detectado pelo classificador de risco em um segmento do comando.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RiskReason {
    /// Identificador estável da regra acionada (ex.: `"rm-recursive-or-force"`).
    pub rule_id: String,
    /// Explicação em português do Brasil exibida ao usuário.
    pub message: String,
    /// Segmento do comando que acionou a regra.
    pub segment: String,
}

/// Avaliação consolidada de risco de um comando shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RiskAssessment {
    /// Nível final efetivo (`max(local_level, model_hint)`).
    pub level: RiskLevel,
    /// Nível determinado localmente pelo `RiskClassifier`.
    pub local_level: RiskLevel,
    /// Dica opcional sugerida pelo modelo de IA (nunca rebaixa `local_level`).
    pub model_hint: Option<RiskLevel>,
    /// Lista de motivos que justificam a classificação.
    pub reasons: Vec<RiskReason>,
    /// Indica se o comando exige confirmação destrutiva (`level == Destructive`).
    pub requires_confirmation: bool,
    /// Token exato que o usuário deve digitar no modal de confirmação destrutiva.
    pub confirm_token: Option<String>,
}

#[allow(clippy::expect_used)]
static FORK_BOMB_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r":\s*\(\s*\)\s*\{\s*:\s*\|\s*:\s*&\s*\}\s*;\s*:").expect("regex fork bomb válida")
});

#[allow(clippy::expect_used)]
static PIPE_TO_SHELL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:curl|wget|fetch)\b[^|]*\|\s*(?:sudo\s+|doas\s+|env\s+)*(?:sh|bash|zsh|dash|ksh|fish|python[23]?|perl|ruby|node)\b")
        .expect("regex pipe to shell válida")
});

#[allow(clippy::expect_used)]
static BLOCK_DEV_REDIRECT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r">{1,2}\s*/dev/(?:sd[a-z]\d*|nvme\d+n\d+(?:p\d+)?|vd[a-z]\d*|hd[a-z]\d*|loop\d+|mmcblk\d+(?:p\d+)?)")
        .expect("regex block device válida")
});

#[allow(clippy::expect_used)]
static SQL_DESTRUCTIVE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:DROP\s+DATABASE|DROP\s+TABLE|DROP\s+SCHEMA|TRUNCATE\s+TABLE|TRUNCATE)\b")
        .expect("regex sql destructive válida")
});

#[allow(clippy::expect_used)]
static LITERAL_DANGEROUS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:rm\s+-(?:r|f|rf|fr)|DROP\s+DATABASE|DROP\s+TABLE|mkfs|dd\s+if=|git\s+reset\s+--hard|git\s+push\s+--force)\b")
        .expect("regex literal dangerous válida")
});

/// Classificador determinístico de risco de comandos shell (`CA-16`, `CA-17`, `CA-19`).
#[derive(Clone, Debug, Default)]
pub struct RiskClassifier;

impl RiskClassifier {
    /// Cria uma instância com o catálogo padrão de regras determinísticas.
    #[must_use]
    pub fn default_rules() -> Self {
        Self
    }

    /// Classifica um comando shell completo inspecionando pipelines, operadores e subshells.
    #[must_use]
    pub fn classify(&self, command: &str) -> RiskAssessment {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return RiskAssessment {
                level: RiskLevel::Safe,
                local_level: RiskLevel::Safe,
                model_hint: None,
                reasons: Vec::new(),
                requires_confirmation: false,
                confirm_token: None,
            };
        }

        let mut reasons: Vec<RiskReason> = Vec::new();
        let mut max_level = RiskLevel::Safe;
        let mut destructive_token: Option<String> = None;

        // 1. Inspeção de padrões globais de linha inteira (fork bomb, pipe-to-shell, redirecionamento de bloco)
        if FORK_BOMB_RE.is_match(trimmed) {
            max_level = RiskLevel::Destructive;
            destructive_token.get_or_insert_with(|| first_token_fallback(trimmed));
            reasons.push(RiskReason {
                rule_id: "fork-bomb".to_string(),
                message: "Detectado padrão de fork bomb capaz de travar o sistema operacional."
                    .to_string(),
                segment: trimmed.to_string(),
            });
        }

        if PIPE_TO_SHELL_RE.is_match(trimmed) {
            max_level = RiskLevel::Destructive;
            destructive_token.get_or_insert_with(|| first_token_fallback(trimmed));
            reasons.push(RiskReason {
                rule_id: "pipe-to-shell".to_string(),
                message: "Execução direta de script remoto via pipe para interpretador shell."
                    .to_string(),
                segment: trimmed.to_string(),
            });
        }

        if BLOCK_DEV_REDIRECT_RE.is_match(trimmed) {
            max_level = RiskLevel::Destructive;
            destructive_token.get_or_insert_with(|| first_token_fallback(trimmed));
            reasons.push(RiskReason {
                rule_id: "block-device-redirect".to_string(),
                message: "Redirecionamento direto de escrita para dispositivo de bloco em /dev."
                    .to_string(),
                segment: trimmed.to_string(),
            });
        }

        // 2. Extração de segmentos principais e subshells $(...) / `...`
        let mut segments = split_shell_segments(trimmed);
        let subshells = extract_subshells(trimmed);
        segments.extend(subshells);

        for seg in segments {
            let seg_trimmed = seg.trim();
            if seg_trimmed.is_empty() {
                continue;
            }
            let (seg_level, seg_token, seg_reasons) = evaluate_segment(seg_trimmed);
            if seg_level > max_level {
                max_level = seg_level;
            }
            if seg_level == RiskLevel::Destructive && destructive_token.is_none() {
                destructive_token = seg_token.or_else(|| Some(first_token_fallback(seg_trimmed)));
            }
            for r in seg_reasons {
                if !reasons
                    .iter()
                    .any(|existing| existing.rule_id == r.rule_id && existing.segment == r.segment)
                {
                    reasons.push(r);
                }
            }
        }

        let confirm_token = if max_level == RiskLevel::Destructive {
            Some(destructive_token.unwrap_or_else(|| first_token_fallback(trimmed)))
        } else {
            None
        };

        RiskAssessment {
            level: max_level,
            local_level: max_level,
            model_hint: None,
            reasons,
            requires_confirmation: max_level == RiskLevel::Destructive,
            confirm_token,
        }
    }

    /// Combina a avaliação local com a dica do modelo (`level = max(local.level, hint)` — `CA-17`).
    #[must_use]
    pub fn combine(mut local: RiskAssessment, hint: Option<RiskLevel>) -> RiskAssessment {
        local.model_hint = hint;
        let final_level = std::cmp::max(local.local_level, hint.unwrap_or(RiskLevel::Safe));
        local.level = final_level;
        local.requires_confirmation = final_level == RiskLevel::Destructive;
        if final_level == RiskLevel::Destructive && local.confirm_token.is_none() {
            let fallback = local.reasons.first().map_or_else(
                || "confirmar".to_string(),
                |r| first_token_fallback(&r.segment),
            );
            local.confirm_token = Some(fallback);
        }
        if final_level > local.local_level {
            local.reasons.push(RiskReason {
                rule_id: "model-risk-hint".to_string(),
                message:
                    "O modelo de IA sinalizou um nível de risco mais elevado para este comando."
                        .to_string(),
                segment: String::new(),
            });
        }
        local
    }
}

fn first_token_fallback(cmd: &str) -> String {
    cmd.split_whitespace()
        .next()
        .unwrap_or("confirmar")
        .trim_matches(|c: char| c == '\'' || c == '"' || c == '(' || c == '$')
        .rsplit('/')
        .next()
        .unwrap_or("confirmar")
        .to_string()
}

fn split_shell_segments(input: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut paren_depth = 0usize;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' && !in_single {
            current.push(c);
            escaped = true;
            continue;
        }
        if c == '\'' && !in_double {
            in_single = !in_single;
            current.push(c);
            continue;
        }
        if c == '"' && !in_single {
            in_double = !in_double;
            current.push(c);
            continue;
        }
        if !in_single && !in_double {
            if c == '(' {
                paren_depth = paren_depth.saturating_add(1);
                current.push(c);
                continue;
            }
            if c == ')' {
                paren_depth = paren_depth.saturating_sub(1);
                current.push(c);
                continue;
            }
            if paren_depth == 0 {
                if c == ';' || c == '\n' {
                    if !current.trim().is_empty() {
                        segments.push(std::mem::take(&mut current));
                    }
                    continue;
                }
                if (c == '&' || c == '|') && chars.peek() == Some(&c) {
                    chars.next();
                    if !current.trim().is_empty() {
                        segments.push(std::mem::take(&mut current));
                    }
                    continue;
                }
                if c == '|' || c == '&' {
                    if !current.trim().is_empty() {
                        segments.push(std::mem::take(&mut current));
                    }
                    continue;
                }
            }
        }
        current.push(c);
    }

    if !current.trim().is_empty() {
        segments.push(current);
    }
    segments
}

fn extract_subshells(input: &str) -> Vec<String> {
    let mut subs = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            let start = i + 2;
            let mut depth = 1usize;
            let mut j = start;
            while j < bytes.len() && depth > 0 {
                if bytes[j] == b'(' {
                    depth += 1;
                } else if bytes[j] == b')' {
                    depth -= 1;
                }
                j += 1;
            }
            if depth == 0 && j > start {
                subs.push(input[start..j - 1].to_string());
            }
            i = j;
            continue;
        }
        if bytes[i] == b'`' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'`' {
                if bytes[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            if j < bytes.len() && j > start {
                subs.push(input[start..j].to_string());
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }
    subs
}

const READONLY_BINS: &[&str] = &[
    "grep", "rg", "egrep", "fgrep", "awk", "cat", "less", "more", "head", "tail", "ls", "stat",
    "file", "wc", "which", "type", "pwd", "whoami", "id", "date", "uname", "true", "false",
];

#[allow(clippy::too_many_lines)]
fn evaluate_segment(segment: &str) -> (RiskLevel, Option<String>, Vec<RiskReason>) {
    let mut reasons = Vec::new();
    let mut level = RiskLevel::Safe;
    let mut confirm_token = None;

    let Ok(tokens) = shell_words::split(segment) else {
        reasons.push(RiskReason {
            rule_id: "unparseable-shell".to_string(),
            message: "Sintaxe shell ambígua ou com aspas não balanceadas.".to_string(),
            segment: segment.to_string(),
        });
        return (RiskLevel::Caution, None, reasons);
    };

    if tokens.is_empty() {
        return (RiskLevel::Safe, None, reasons);
    }

    // Desempacota wrappers como sudo, doas, pkexec, env, VAR=val, nice, nohup, time, xargs, command
    let mut idx = 0usize;
    let mut saw_priv_wrapper = false;
    while idx < tokens.len() {
        let tok = tokens[idx].as_str();
        let base = tok.rsplit('/').next().unwrap_or(tok);
        if matches!(base, "sudo" | "doas" | "pkexec" | "su") {
            saw_priv_wrapper = true;
            idx += 1;
            // Pula flags do sudo (ex.: -u root, -E, -i, -s)
            while idx < tokens.len() && tokens[idx].starts_with('-') {
                let flag = tokens[idx].as_str();
                idx += 1;
                if matches!(
                    flag,
                    "-u" | "-g" | "-C" | "-D" | "-h" | "-p" | "-r" | "-t" | "-U"
                ) && idx < tokens.len()
                {
                    idx += 1;
                }
            }
            continue;
        }
        if matches!(
            base,
            "env" | "nice" | "nohup" | "time" | "command" | "builtin" | "xargs"
        ) {
            idx += 1;
            while idx < tokens.len() && (tokens[idx].starts_with('-') || tokens[idx].contains('='))
            {
                idx += 1;
            }
            continue;
        }
        if tok.contains('=') && !tok.starts_with('-') && !tok.contains('/') {
            idx += 1;
            continue;
        }
        break;
    }

    if saw_priv_wrapper {
        level = std::cmp::max(level, RiskLevel::Caution);
        reasons.push(RiskReason {
            rule_id: "privilege-escalation".to_string(),
            message: "Execução com elevação de privilégios (sudo/doas/pkexec).".to_string(),
            segment: segment.to_string(),
        });
    }

    if idx >= tokens.len() {
        return (level, confirm_token, reasons);
    }

    let raw_bin = &tokens[idx];
    let bin = raw_bin.rsplit('/').next().unwrap_or(raw_bin.as_str());
    let args = &tokens[idx + 1..];

    // Prevenção de falsos positivos (CA-19):
    // 1. echo / printf sem redirecionamento para dispositivo de bloco: no máximo Caution se contiver padrão perigoso literal
    if matches!(bin, "echo" | "printf") {
        let joined_args = args.join(" ");
        if LITERAL_DANGEROUS_RE.is_match(&joined_args) && !BLOCK_DEV_REDIRECT_RE.is_match(segment) {
            level = std::cmp::max(level, RiskLevel::Caution);
            reasons.push(RiskReason {
                rule_id: "literal-dangerous-pattern".to_string(),
                message:
                    "O texto literal impresso contém termos semelhantes a comandos destrutivos."
                        .to_string(),
                segment: segment.to_string(),
            });
        }
        return (level, confirm_token, reasons);
    }

    // 2. Utilitários puramente de leitura/busca sem redirecionamento de escrita
    let has_write_redirect = segment.contains('>');
    if READONLY_BINS.contains(&bin) && !has_write_redirect {
        return (level, confirm_token, reasons);
    }

    // 3. Regras Destructive por executável
    match bin {
        "rm" => {
            let has_recursive = args.iter().any(|a| {
                a == "--recursive"
                    || a == "--no-preserve-root"
                    || (a.starts_with('-')
                        && !a.starts_with("--")
                        && (a.contains('r') || a.contains('R')))
            });
            let has_critical_target = args.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "/" | "/*" | "~" | "~/" | "~/*" | "*" | ".." | "../" | "$HOME"
                ) || a.starts_with("/etc")
                    || a.starts_with("/usr")
                    || a.starts_with("/var")
                    || a.starts_with("/boot")
                    || a.starts_with("/home")
            });

            if has_recursive || has_critical_target {
                level = RiskLevel::Destructive;
                confirm_token = Some("rm".to_string());
                reasons.push(RiskReason {
                    rule_id: "rm-recursive-or-force".to_string(),
                    message: "Remoção recursiva ou forçada de arquivos/diretórios.".to_string(),
                    segment: segment.to_string(),
                });
            } else {
                level = std::cmp::max(level, RiskLevel::Caution);
                reasons.push(RiskReason {
                    rule_id: "file-remove".to_string(),
                    message: "Remoção de arquivo no sistema de arquivos.".to_string(),
                    segment: segment.to_string(),
                });
            }
        }
        "shred" | "wipefs" | "mkfs" | "fdisk" | "parted" | "sgdisk" | "cfdisk" | "sfdisk" => {
            level = RiskLevel::Destructive;
            confirm_token = Some(bin.to_string());
            reasons.push(RiskReason {
                rule_id: "filesystem-format".to_string(),
                message:
                    "Formatação, particionamento ou destruição irreversível de dados em disco."
                        .to_string(),
                segment: segment.to_string(),
            });
        }
        b if b.starts_with("mkfs.") => {
            level = RiskLevel::Destructive;
            confirm_token = Some(b.to_string());
            reasons.push(RiskReason {
                rule_id: "filesystem-format".to_string(),
                message: "Formatação de sistema de arquivos.".to_string(),
                segment: segment.to_string(),
            });
        }
        "dd" => {
            if args.iter().any(|a| a.starts_with("of=")) {
                level = RiskLevel::Destructive;
                confirm_token = Some("dd".to_string());
                reasons.push(RiskReason {
                    rule_id: "dd-disk-write".to_string(),
                    message:
                        "Escrita bruta com dd (of=...) capaz de sobrescrever imagens ou discos."
                            .to_string(),
                    segment: segment.to_string(),
                });
            } else {
                level = std::cmp::max(level, RiskLevel::Caution);
                reasons.push(RiskReason {
                    rule_id: "dd-invocation".to_string(),
                    message: "Execução do utilitário de cópia de baixo nível dd.".to_string(),
                    segment: segment.to_string(),
                });
            }
        }
        "git" => {
            let sub = args
                .iter()
                .find(|a| !a.starts_with('-'))
                .map(String::as_str);
            match sub {
                Some("push") => {
                    let is_force = args.iter().any(|a| {
                        a == "-f"
                            || a == "--force"
                            || a.starts_with("--force-with-lease")
                            || (a.starts_with('+') && a.len() > 1)
                    });
                    if is_force {
                        level = RiskLevel::Destructive;
                        confirm_token = Some("git".to_string());
                        reasons.push(RiskReason {
                            rule_id: "git-force-push".to_string(),
                            message:
                                "Envio forçado (git push --force) que pode sobrescrever histórico remoto."
                                    .to_string(),
                            segment: segment.to_string(),
                        });
                    } else {
                        level = std::cmp::max(level, RiskLevel::Caution);
                        reasons.push(RiskReason {
                            rule_id: "git-push".to_string(),
                            message: "Publicação de commits em repositório remoto (git push)."
                                .to_string(),
                            segment: segment.to_string(),
                        });
                    }
                }
                Some("reset") => {
                    if args.iter().any(|a| a == "--hard") {
                        level = RiskLevel::Destructive;
                        confirm_token = Some("git".to_string());
                        reasons.push(RiskReason {
                            rule_id: "git-hard-reset".to_string(),
                            message:
                                "Descarte irreversível de alterações não commitadas (git reset --hard)."
                                    .to_string(),
                            segment: segment.to_string(),
                        });
                    }
                }
                Some("clean") => {
                    let has_f = args.iter().any(|a| {
                        a == "--force"
                            || (a.starts_with('-') && !a.starts_with("--") && a.contains('f'))
                    });
                    if has_f {
                        level = RiskLevel::Destructive;
                        confirm_token = Some("git".to_string());
                        reasons.push(RiskReason {
                            rule_id: "git-clean-force".to_string(),
                            message:
                                "Remoção permanente de arquivos não rastreados (git clean -f)."
                                    .to_string(),
                            segment: segment.to_string(),
                        });
                    }
                }
                Some("branch") => {
                    if args.iter().any(|a| a == "-D") {
                        level = RiskLevel::Destructive;
                        confirm_token = Some("git".to_string());
                        reasons.push(RiskReason {
                            rule_id: "git-branch-force-delete".to_string(),
                            message: "Exclusão forçada de branch Git não mesclada (git branch -D)."
                                .to_string(),
                            segment: segment.to_string(),
                        });
                    }
                }
                Some("checkout" | "restore") => {
                    let discards_all = args.iter().any(|a| a == ".")
                        || (args.windows(2).any(|w| w[0] == "--" && w[1] == "."));
                    if discards_all {
                        level = RiskLevel::Destructive;
                        confirm_token = Some("git".to_string());
                        reasons.push(RiskReason {
                            rule_id: "git-discard-worktree".to_string(),
                            message:
                                "Descarte de todas as modificações locais na árvore de trabalho."
                                    .to_string(),
                            segment: segment.to_string(),
                        });
                    }
                }
                Some("rebase") => {
                    level = std::cmp::max(level, RiskLevel::Caution);
                    reasons.push(RiskReason {
                        rule_id: "git-rebase".to_string(),
                        message: "Reescrita de histórico de commits (git rebase).".to_string(),
                        segment: segment.to_string(),
                    });
                }
                _ => {}
            }
        }
        "chmod" | "chown" | "chgrp" => {
            let is_recursive = args.iter().any(|a| {
                a == "--recursive"
                    || (a.starts_with('-') && !a.starts_with("--") && a.contains('R'))
            });
            let is_777 = args
                .iter()
                .any(|a| a == "777" || a == "0777" || a == "a+rwx");
            let on_root_or_home = args
                .iter()
                .any(|a| matches!(a.as_str(), "/" | "/*" | "~" | "~/" | "$HOME"));
            if is_recursive || (is_777 && on_root_or_home) {
                level = RiskLevel::Destructive;
                confirm_token = Some(bin.to_string());
                reasons.push(RiskReason {
                    rule_id: "chmod-chown-recursive-root".to_string(),
                    message: format!("Alteração recursiva de permissões/propriedade com {bin}."),
                    segment: segment.to_string(),
                });
            } else {
                level = std::cmp::max(level, RiskLevel::Caution);
                reasons.push(RiskReason {
                    rule_id: "permission-change".to_string(),
                    message: format!("Alteração de permissões ou propriedade com {bin}."),
                    segment: segment.to_string(),
                });
            }
        }
        "php" | "artisan" => {
            let is_artisan =
                bin == "artisan" || args.first().is_some_and(|a| a.ends_with("artisan"));
            if is_artisan {
                let destructive_sub = args.iter().any(|a| {
                    matches!(
                        a.as_str(),
                        "migrate:fresh"
                            | "migrate:reset"
                            | "db:wipe"
                            | "migrate:refresh"
                            | "schema:drop"
                    )
                });
                if destructive_sub {
                    level = RiskLevel::Destructive;
                    confirm_token = Some(bin.to_string());
                    reasons.push(RiskReason {
                        rule_id: "artisan-destructive".to_string(),
                        message: "Comando Artisan que apaga ou recria tabelas do banco de dados."
                            .to_string(),
                        segment: segment.to_string(),
                    });
                }
            }
        }
        "docker" | "podman" => {
            let joined = args.join(" ");
            if joined.contains("system prune")
                || joined.contains("volume rm")
                || joined.contains("volume prune")
                || (joined.contains("rm") && args.iter().any(|a| a == "-f" || a == "--force"))
            {
                level = RiskLevel::Destructive;
                confirm_token = Some(bin.to_string());
                reasons.push(RiskReason {
                    rule_id: "docker-prune-volume".to_string(),
                    message: "Remoção destrutiva de containers, imagens ou volumes de dados."
                        .to_string(),
                    segment: segment.to_string(),
                });
            }
        }
        "kill" | "killall" | "pkill" => {
            let is_kill_all = bin == "killall"
                || args.iter().any(|a| a == "-1")
                || (bin == "pkill" && args.iter().any(|a| a == "-9" || a == "-KILL"));
            let has_sigkill = args
                .iter()
                .any(|a| a == "-9" || a == "-KILL" || a == "-SIGKILL");
            if is_kill_all && has_sigkill {
                level = RiskLevel::Destructive;
                confirm_token = Some(bin.to_string());
                reasons.push(RiskReason {
                    rule_id: "kill-all-processes".to_string(),
                    message: "Encerramento abrupto (SIGKILL) de múltiplos processos.".to_string(),
                    segment: segment.to_string(),
                });
            } else {
                level = std::cmp::max(level, RiskLevel::Caution);
                reasons.push(RiskReason {
                    rule_id: "process-kill".to_string(),
                    message: "Envio de sinal de encerramento a processos.".to_string(),
                    segment: segment.to_string(),
                });
            }
        }
        "systemctl" | "service" => {
            if args.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "stop" | "disable" | "mask" | "poweroff" | "reboot"
                )
            }) {
                level = RiskLevel::Destructive;
                confirm_token = Some(bin.to_string());
                reasons.push(RiskReason {
                    rule_id: "systemd-stop-disable".to_string(),
                    message: "Parada ou desativação de serviço do sistema (systemd).".to_string(),
                    segment: segment.to_string(),
                });
            } else if args
                .iter()
                .any(|a| matches!(a.as_str(), "restart" | "reload"))
            {
                level = std::cmp::max(level, RiskLevel::Caution);
                reasons.push(RiskReason {
                    rule_id: "systemd-restart".to_string(),
                    message: "Reinicialização de serviço do sistema.".to_string(),
                    segment: segment.to_string(),
                });
            }
        }
        "reboot" | "shutdown" | "poweroff" | "halt" => {
            level = RiskLevel::Destructive;
            confirm_token = Some(bin.to_string());
            reasons.push(RiskReason {
                rule_id: "systemd-stop-disable".to_string(),
                message: "Desligamento ou reinicialização imediata do sistema operacional."
                    .to_string(),
                segment: segment.to_string(),
            });
        }
        "find" => {
            let has_delete = args.iter().any(|a| a == "-delete");
            let has_exec_rm = args
                .windows(2)
                .any(|w| (w[0] == "-exec" || w[0] == "-execdir") && w[1] == "rm");
            if has_delete || has_exec_rm {
                level = RiskLevel::Destructive;
                confirm_token = Some("find".to_string());
                reasons.push(RiskReason {
                    rule_id: "find-delete".to_string(),
                    message: "Busca com exclusão em lote de arquivos (find -delete / -exec rm)."
                        .to_string(),
                    segment: segment.to_string(),
                });
            }
        }
        "mv" => {
            level = std::cmp::max(level, RiskLevel::Caution);
            reasons.push(RiskReason {
                rule_id: "file-move".to_string(),
                message: "Movimentação ou renomeação de arquivos (mv).".to_string(),
                segment: segment.to_string(),
            });
        }
        "sed" => {
            if args
                .iter()
                .any(|a| a == "-i" || a.starts_with("-i") || a == "--in-place")
            {
                level = std::cmp::max(level, RiskLevel::Caution);
                reasons.push(RiskReason {
                    rule_id: "sed-in-place".to_string(),
                    message: "Edição direta de arquivos no local (sed -i).".to_string(),
                    segment: segment.to_string(),
                });
            }
        }
        "truncate" => {
            level = RiskLevel::Destructive;
            confirm_token = Some("truncate".to_string());
            reasons.push(RiskReason {
                rule_id: "sql-destructive".to_string(),
                message: "Truncamento direto do tamanho/conteúdo de arquivo.".to_string(),
                segment: segment.to_string(),
            });
        }
        _ => {}
    }

    // 4. Inspeção de SQL destrutivo no segmento (ex.: mysql -e "DROP DATABASE ...", psql -c "TRUNCATE ...")
    if SQL_DESTRUCTIVE_RE.is_match(segment) && level < RiskLevel::Destructive {
        level = RiskLevel::Destructive;
        confirm_token.get_or_insert_with(|| bin.to_string());
        reasons.push(RiskReason {
            rule_id: "sql-destructive".to_string(),
            message: "Instrução SQL destrutiva detectada (DROP / TRUNCATE).".to_string(),
            segment: segment.to_string(),
        });
    }

    (level, confirm_token, reasons)
}
