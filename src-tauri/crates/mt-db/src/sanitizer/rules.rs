//! Catálogo de regras compiladas (`LazyLock`), pré-filtro `RegexSet` e avaliador contextual
//! para as 13 famílias de segredos (`SecretKind`).

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::{Captures, Regex, RegexSet};
use serde::{Deserialize, Serialize};

/// Famílias de segredos detectadas e mascaradas pelo sanitizador do `mt-db`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    /// Bloco de chave privada PEM (`RSA`, `OPENSSH`, `EC`, `DSA`, `PGP`, `PKCS#8`).
    PemBlock,
    /// JSON Web Token (`eyJ...`).
    JwtToken,
    /// Token de acesso do GitHub (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`, `github_pat_`).
    GitHubToken,
    /// Token de acesso do GitLab (`glpat-`, `glptt-`, `glrt-`, `gldt-`, `glsoat-`, `glimt-`).
    GitLabToken,
    /// Token do Slack (`xoxb-`, `xoxp-`, `xoxa-`, `xoxr-`, `xoxs-`, `xoxe-`, `xapp-`).
    SlackToken,
    /// Chave secreta ou restrita do Stripe (`sk_live_`, `sk_test_`, `rk_live_`, `rk_test_`, `whsec_`).
    StripeKey,
    /// Chave de API da Anthropic (`sk-ant-...`).
    AnthropicKey,
    /// Chave de API da `OpenAI` (`sk-...`, `sk-proj-...`, `sk-svcacct-...`, `sk-admin-...`).
    OpenAiKey,
    /// Chave de API do Google Cloud / Gemini / Firebase (`AIza...`).
    GoogleApiKey,
    /// Identificador de chave de acesso da AWS (`AKIA...`, `ASIA...`, etc.).
    AwsAccessKeyId,
    /// Cabeçalho de autenticação HTTP (`Authorization: Bearer ...`, `Basic ...`, `Token ...`).
    AuthHeader,
    /// Credencial/senha embutida em URL (`scheme://user:password@host`).
    UrlUserinfo,
    /// Par chave-valor sensível (`.env`, `APP_KEY`, `AWS_SECRET_ACCESS_KEY`, JSON/YAML, `--password`, `-p`).
    SensitiveKeyValue,
}

impl SecretKind {
    /// Lista canônica com todas as 13 famílias obrigatórias do catálogo.
    pub const ALL: [Self; 13] = [
        Self::PemBlock,
        Self::JwtToken,
        Self::GitHubToken,
        Self::GitLabToken,
        Self::SlackToken,
        Self::StripeKey,
        Self::AnthropicKey,
        Self::OpenAiKey,
        Self::GoogleApiKey,
        Self::AwsAccessKeyId,
        Self::AuthHeader,
        Self::UrlUserinfo,
        Self::SensitiveKeyValue,
    ];

    /// Retorna o marcador estático de redação correspondente à família.
    #[must_use]
    pub const fn marker(&self) -> &'static str {
        match self {
            Self::PemBlock => "[REDACTED:PEM]",
            Self::JwtToken => "[REDACTED:JWT]",
            Self::GitHubToken => "[REDACTED:GITHUB_TOKEN]",
            Self::GitLabToken => "[REDACTED:GITLAB_TOKEN]",
            Self::SlackToken => "[REDACTED:SLACK_TOKEN]",
            Self::StripeKey => "[REDACTED:STRIPE_KEY]",
            Self::AnthropicKey => "[REDACTED:ANTHROPIC_KEY]",
            Self::OpenAiKey => "[REDACTED:OPENAI_KEY]",
            Self::GoogleApiKey => "[REDACTED:GOOGLE_API_KEY]",
            Self::AwsAccessKeyId => "[REDACTED:AWS_ACCESS_KEY_ID]",
            Self::AuthHeader => "[REDACTED:AUTH_HEADER]",
            Self::UrlUserinfo => "[REDACTED:URL_PASSWORD]",
            Self::SensitiveKeyValue => "[REDACTED:SECRET]",
        }
    }

    /// Retorna o identificador textual da variante para fixtures e relatórios.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::PemBlock => "PemBlock",
            Self::JwtToken => "JwtToken",
            Self::GitHubToken => "GitHubToken",
            Self::GitLabToken => "GitLabToken",
            Self::SlackToken => "SlackToken",
            Self::StripeKey => "StripeKey",
            Self::AnthropicKey => "AnthropicKey",
            Self::OpenAiKey => "OpenAiKey",
            Self::GoogleApiKey => "GoogleApiKey",
            Self::AwsAccessKeyId => "AwsAccessKeyId",
            Self::AuthHeader => "AuthHeader",
            Self::UrlUserinfo => "UrlUserinfo",
            Self::SensitiveKeyValue => "SensitiveKeyValue",
        }
    }

    /// Converte o nome textual de uma família (em `PascalCase` ou `snake_case`) em `SecretKind`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim() {
            "PemBlock" | "pem_block" | "pem_private_key" | "PemPrivateKey" => Some(Self::PemBlock),
            "JwtToken" | "jwt_token" | "jwt" | "Jwt" => Some(Self::JwtToken),
            "GitHubToken" | "github_token" | "GithubToken" => Some(Self::GitHubToken),
            "GitLabToken" | "gitlab_token" | "GitlabToken" => Some(Self::GitLabToken),
            "SlackToken" | "slack_token" => Some(Self::SlackToken),
            "StripeKey" | "stripe_key" => Some(Self::StripeKey),
            "AnthropicKey" | "anthropic_key" => Some(Self::AnthropicKey),
            "OpenAiKey" | "openai_key" | "OpenAIKey" => Some(Self::OpenAiKey),
            "GoogleApiKey" | "google_api_key" => Some(Self::GoogleApiKey),
            "AwsAccessKeyId" | "aws_access_key_id" => Some(Self::AwsAccessKeyId),
            "AuthHeader" | "auth_header" | "bearer" | "Bearer" => Some(Self::AuthHeader),
            "UrlUserinfo" | "url_userinfo" | "url_credentials" | "UrlCredentials" => {
                Some(Self::UrlUserinfo)
            }
            "SensitiveKeyValue" | "sensitive_key_value" | "env_assignment" | "cli_flag" => {
                Some(Self::SensitiveKeyValue)
            }
            _ => None,
        }
    }
}

/// Relatório quantitativo de redações aplicadas durante uma execução do sanitizador.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizeReport {
    /// Quantidade total de segredos redigidos.
    pub total_redactions: usize,
    /// Contagem de redações discriminada por `SecretKind`.
    pub by_kind: Vec<(SecretKind, usize)>,
}

impl SanitizeReport {
    /// Registra `count` redações da família `kind`.
    pub fn record(&mut self, kind: SecretKind, count: usize) {
        if count == 0 {
            return;
        }
        self.total_redactions += count;
        if let Some(entry) = self.by_kind.iter_mut().find(|(k, _)| *k == kind) {
            entry.1 += count;
        } else {
            self.by_kind.push((kind, count));
        }
    }

    /// Mescla outro `SanitizeReport` neste relatório.
    pub fn merge(&mut self, other: &Self) {
        for &(kind, count) in &other.by_kind {
            self.record(kind, count);
        }
    }

    /// Retorna a contagem registrada para uma família específica.
    #[must_use]
    pub fn count_for(&self, kind: SecretKind) -> usize {
        self.by_kind
            .iter()
            .find_map(|&(k, c)| (k == kind).then_some(c))
            .unwrap_or(0)
    }
}

const PATTERN_PEM_BLOCK: &str = r"(?s)-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY(?: BLOCK)?-----(?:.*?-----END (?:[A-Z0-9 ]+ )?PRIVATE KEY(?: BLOCK)?-----|.*$)";
const PATTERN_URL_USERINFO: &str =
    r#"(?i)\b([a-z][a-z0-9+.\-]{1,20}://[^\s:/@"']+:)(?:([^\s/"'?#]{1,512}))@"#;
const PATTERN_AUTH_HEADER: &str = r#"(?i)(\b(?:proxy-)?authorization\s*[:=]\s*(?:(?:bearer|basic|token|digest)\s+)?|\b(?:bearer|basic)\s+)([^\s"'\r\n,;]{8,})"#;
const PATTERN_JWT_TOKEN: &str =
    r"\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b";
const PATTERN_GITHUB_TOKEN: &str =
    r"\b(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{22,255})\b";
const PATTERN_GITLAB_TOKEN: &str = r"\bgl(?:pat|ptt|rt|dt|soat|imt)-[A-Za-z0-9_=-]{20,255}\b";
const PATTERN_SLACK_TOKEN: &str =
    r"\b(?:xox[bparsed]-[A-Za-z0-9-]{20,255}|xapp-[0-9]-[A-Za-z0-9-]{20,255})\b";
const PATTERN_STRIPE_KEY: &str =
    r"\b(?:(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{20,255}|whsec_[A-Za-z0-9]{24,255})\b";
const PATTERN_ANTHROPIC_KEY: &str = r"\bsk-ant-[A-Za-z0-9_-]{20,255}\b";
const PATTERN_OPENAI_KEY: &str = r"\bsk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_-]{20,255}\b";
const PATTERN_GOOGLE_API_KEY: &str = r"\bAIza[0-9A-Za-z_-]{35}\b";
const PATTERN_AWS_ACCESS_KEY_ID: &str =
    r"\b(?:AKIA|ASIA|AGPA|AIDA|AROA|ANPA|ANVA|AIPA)[A-Z0-9]{16}\b";
const PATTERN_SENSITIVE_KV: &str = r#"(?i)((?:^|[\s,;({\[])(?:export\s+)?["']?[a-z0-9_.\-]*(?:password|passwd|db_pwd|redis_pwd|mail_pwd|secret|token|api_key|apikey|access_key|private_key|client_secret|auth_token|refresh_token|app_key|database_url|aws_secret_access_key|credentials?|cookie|laravel_session)[a-z0-9_.\-]*["']?)(\s*[:=]\s*)("[^"\r\n]*"|'[^'\r\n]*'|[^\s"',;)}\]\r\n]+)"#;
const PATTERN_CLI_FLAG: &str = r#"(?i)((?:^|\s)(?:--(?:password|passwd|secret|token|api-key|apikey|access-token|client-secret|private-key)|-p))(=|\s+)("[^"\r\n]*"|'[^'\r\n]*'|[^\s"'\r\n]+)"#;

struct CompiledRules {
    prefilter: RegexSet,
    pem_block: Regex,
    url_userinfo: Regex,
    auth_header: Regex,
    jwt_token: Regex,
    github_token: Regex,
    gitlab_token: Regex,
    slack_token: Regex,
    stripe_key: Regex,
    anthropic_key: Regex,
    openai_key: Regex,
    google_api_key: Regex,
    aws_access_key_id: Regex,
    sensitive_kv: Regex,
    cli_flag: Regex,
}

static RULES: LazyLock<CompiledRules> = LazyLock::new(|| {
    let patterns = [
        PATTERN_PEM_BLOCK,
        PATTERN_URL_USERINFO,
        PATTERN_AUTH_HEADER,
        PATTERN_JWT_TOKEN,
        PATTERN_GITHUB_TOKEN,
        PATTERN_GITLAB_TOKEN,
        PATTERN_SLACK_TOKEN,
        PATTERN_STRIPE_KEY,
        PATTERN_ANTHROPIC_KEY,
        PATTERN_OPENAI_KEY,
        PATTERN_GOOGLE_API_KEY,
        PATTERN_AWS_ACCESS_KEY_ID,
        PATTERN_SENSITIVE_KV,
        PATTERN_CLI_FLAG,
    ];

    let Ok(prefilter) = RegexSet::new(patterns) else {
        unreachable!("Catálogo estático de RegexSet inválido");
    };
    let Ok(pem_block) = Regex::new(PATTERN_PEM_BLOCK) else {
        unreachable!("Regex PemBlock inválida");
    };
    let Ok(url_userinfo) = Regex::new(PATTERN_URL_USERINFO) else {
        unreachable!("Regex UrlUserinfo inválida");
    };
    let Ok(auth_header) = Regex::new(PATTERN_AUTH_HEADER) else {
        unreachable!("Regex AuthHeader inválida");
    };
    let Ok(jwt_token) = Regex::new(PATTERN_JWT_TOKEN) else {
        unreachable!("Regex JwtToken inválida");
    };
    let Ok(github_token) = Regex::new(PATTERN_GITHUB_TOKEN) else {
        unreachable!("Regex GitHubToken inválida");
    };
    let Ok(gitlab_token) = Regex::new(PATTERN_GITLAB_TOKEN) else {
        unreachable!("Regex GitLabToken inválida");
    };
    let Ok(slack_token) = Regex::new(PATTERN_SLACK_TOKEN) else {
        unreachable!("Regex SlackToken inválida");
    };
    let Ok(stripe_key) = Regex::new(PATTERN_STRIPE_KEY) else {
        unreachable!("Regex StripeKey inválida");
    };
    let Ok(anthropic_key) = Regex::new(PATTERN_ANTHROPIC_KEY) else {
        unreachable!("Regex AnthropicKey inválida");
    };
    let Ok(openai_key) = Regex::new(PATTERN_OPENAI_KEY) else {
        unreachable!("Regex OpenAiKey inválida");
    };
    let Ok(google_api_key) = Regex::new(PATTERN_GOOGLE_API_KEY) else {
        unreachable!("Regex GoogleApiKey inválida");
    };
    let Ok(aws_access_key_id) = Regex::new(PATTERN_AWS_ACCESS_KEY_ID) else {
        unreachable!("Regex AwsAccessKeyId inválida");
    };
    let Ok(sensitive_kv) = Regex::new(PATTERN_SENSITIVE_KV) else {
        unreachable!("Regex SensitiveKeyValue inválida");
    };
    let Ok(cli_flag) = Regex::new(PATTERN_CLI_FLAG) else {
        unreachable!("Regex CliFlag inválida");
    };

    CompiledRules {
        prefilter,
        pem_block,
        url_userinfo,
        auth_header,
        jwt_token,
        github_token,
        gitlab_token,
        slack_token,
        stripe_key,
        anthropic_key,
        openai_key,
        google_api_key,
        aws_access_key_id,
        sensitive_kv,
        cli_flag,
    }
});

/// Verifica se um fragmento de valor já é um marcador `[REDACTED:...]` ou está isento de redação.
fn is_exempt_value(raw_val: &str) -> bool {
    let trimmed = raw_val.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })
        .unwrap_or(trimmed)
        .trim();

    if unquoted.is_empty() {
        return true;
    }

    if unquoted.contains("[REDACTED:") {
        return true;
    }

    if unquoted.eq_ignore_ascii_case("null")
        || unquoted.eq_ignore_ascii_case("nil")
        || unquoted.eq_ignore_ascii_case("none")
        || unquoted.eq_ignore_ascii_case("false")
        || unquoted.eq_ignore_ascii_case("true")
        || unquoted.eq_ignore_ascii_case("undefined")
    {
        return true;
    }

    if unquoted.starts_with('<') && unquoted.ends_with('>') {
        return true;
    }

    if unquoted.chars().all(|c| c == '*') {
        return true;
    }

    // Referência simples e inteira a variável de ambiente ($VAR ou ${VAR})
    if let Some(rest) = unquoted.strip_prefix('$') {
        let var_name = rest
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .unwrap_or(rest);
        if !var_name.is_empty()
            && var_name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && var_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return true;
        }
    }

    false
}

/// Verifica se uma flag curta `-p` deve ser ignorada (ex.: `mkdir -p /path`, `ssh -p 2222`, `cargo test -p mt-db`).
fn is_exempt_short_p_flag(full_input: &str, flag_raw: &str, val_raw: &str) -> bool {
    let flag_trimmed = flag_raw.trim_start();
    if flag_trimmed != "-p" {
        return false;
    }

    let unquoted = val_raw
        .trim()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            val_raw
                .trim()
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })
        .unwrap_or(val_raw.trim());

    if unquoted.starts_with('/')
        || unquoted.starts_with("./")
        || unquoted.starts_with("../")
        || unquoted.starts_with('~')
        || unquoted.starts_with('-')
    {
        return true;
    }

    // Porta numérica pura (ex.: 2222) ou mapeamento de porta (ex.: 8080:80)
    if unquoted.chars().all(|c| c.is_ascii_digit() || c == ':') {
        return true;
    }

    // Comandos comuns em que `-p` significa package/parents/port/patch
    let lower_line = full_input.to_ascii_lowercase();
    for non_auth_cmd in [
        "mkdir ", "cargo ", "rmdir ", "cp ", "mv ", "install ", "patch ", "git ", "ssh ",
    ] {
        if lower_line.contains(non_auth_cmd) {
            return true;
        }
    }

    false
}

/// Executa a sanitização completa em passe sobre `input`, retornando `Cow::Borrowed` quando nenhuma redação ocorre.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn apply_rules(input: &str) -> (Cow<'_, str>, SanitizeReport) {
    let mut report = SanitizeReport::default();
    if input.is_empty() {
        return (Cow::Borrowed(input), report);
    }

    let rules = &*RULES;
    let matches = rules.prefilter.matches(input);
    if !matches.matched_any() {
        return (Cow::Borrowed(input), report);
    }

    let mut current: Cow<'_, str> = Cow::Borrowed(input);

    // 1. PemBlock
    if matches.matched(0) {
        let mut count = 0usize;
        let replaced = rules
            .pem_block
            .replace_all(&current, |_caps: &Captures<'_>| {
                count += 1;
                SecretKind::PemBlock.marker()
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::PemBlock, count);
        }
    }

    // 2. UrlUserinfo
    if matches.matched(1) {
        let mut count = 0usize;
        let replaced = rules
            .url_userinfo
            .replace_all(&current, |caps: &Captures<'_>| {
                let prefix = &caps[1];
                let pass = &caps[2];
                if is_exempt_value(pass) {
                    caps[0].to_string()
                } else {
                    count += 1;
                    format!("{prefix}{}@", SecretKind::UrlUserinfo.marker())
                }
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::UrlUserinfo, count);
        }
    }

    // 3. AuthHeader
    if matches.matched(2) {
        let mut count = 0usize;
        let replaced = rules
            .auth_header
            .replace_all(&current, |caps: &Captures<'_>| {
                let prefix = &caps[1];
                let token = &caps[2];
                let prefix_lower = prefix.to_ascii_lowercase();
                let has_auth_header_name = prefix_lower.contains("authorization");

                if is_exempt_value(token)
                    || token.eq_ignore_ascii_case("bearer")
                    || token.eq_ignore_ascii_case("basic")
                    || token.eq_ignore_ascii_case("token")
                    || token.eq_ignore_ascii_case("digest")
                {
                    return caps[0].to_string();
                }

                // Se for apenas "Bearer <palavra>" sem "Authorization:", exige comprimento >= 20
                if !has_auth_header_name && token.len() < 20 {
                    return caps[0].to_string();
                }

                count += 1;
                format!("{prefix}{}", SecretKind::AuthHeader.marker())
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::AuthHeader, count);
        }
    }

    // 4. JwtToken
    if matches.matched(3) {
        let mut count = 0usize;
        let replaced = rules
            .jwt_token
            .replace_all(&current, |_caps: &Captures<'_>| {
                count += 1;
                SecretKind::JwtToken.marker()
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::JwtToken, count);
        }
    }

    // 5. GitHubToken
    if matches.matched(4) {
        let mut count = 0usize;
        let replaced = rules
            .github_token
            .replace_all(&current, |_caps: &Captures<'_>| {
                count += 1;
                SecretKind::GitHubToken.marker()
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::GitHubToken, count);
        }
    }

    // 6. GitLabToken
    if matches.matched(5) {
        let mut count = 0usize;
        let replaced = rules
            .gitlab_token
            .replace_all(&current, |_caps: &Captures<'_>| {
                count += 1;
                SecretKind::GitLabToken.marker()
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::GitLabToken, count);
        }
    }

    // 7. SlackToken
    if matches.matched(6) {
        let mut count = 0usize;
        let replaced = rules
            .slack_token
            .replace_all(&current, |_caps: &Captures<'_>| {
                count += 1;
                SecretKind::SlackToken.marker()
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::SlackToken, count);
        }
    }

    // 8. StripeKey
    if matches.matched(7) {
        let mut count = 0usize;
        let replaced = rules
            .stripe_key
            .replace_all(&current, |_caps: &Captures<'_>| {
                count += 1;
                SecretKind::StripeKey.marker()
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::StripeKey, count);
        }
    }

    // 9. AnthropicKey (antes de OpenAiKey pois sk-ant- tem prefixo sk-)
    if matches.matched(8) {
        let mut count = 0usize;
        let replaced = rules
            .anthropic_key
            .replace_all(&current, |_caps: &Captures<'_>| {
                count += 1;
                SecretKind::AnthropicKey.marker()
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::AnthropicKey, count);
        }
    }

    // 10. OpenAiKey
    if matches.matched(9) {
        let mut count = 0usize;
        let replaced = rules
            .openai_key
            .replace_all(&current, |caps: &Captures<'_>| {
                let full = &caps[0];
                if full.starts_with("sk-ant-") {
                    full.to_string()
                } else {
                    count += 1;
                    SecretKind::OpenAiKey.marker().to_string()
                }
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::OpenAiKey, count);
        }
    }

    // 11. GoogleApiKey
    if matches.matched(10) {
        let mut count = 0usize;
        let replaced = rules
            .google_api_key
            .replace_all(&current, |_caps: &Captures<'_>| {
                count += 1;
                SecretKind::GoogleApiKey.marker()
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::GoogleApiKey, count);
        }
    }

    // 12. AwsAccessKeyId
    if matches.matched(11) {
        let mut count = 0usize;
        let replaced = rules
            .aws_access_key_id
            .replace_all(&current, |_caps: &Captures<'_>| {
                count += 1;
                SecretKind::AwsAccessKeyId.marker()
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::AwsAccessKeyId, count);
        }
    }

    // 13a. SensitiveKeyValue (atribuições e pares chave=valor / chave: valor)
    if matches.matched(12) {
        let mut count = 0usize;
        let replaced = rules
            .sensitive_kv
            .replace_all(&current, |caps: &Captures<'_>| {
                let key_part = &caps[1];
                let sep_part = &caps[2];
                let val_part = &caps[3];

                if is_exempt_value(val_part) {
                    caps[0].to_string()
                } else {
                    count += 1;
                    format!(
                        "{key_part}{sep_part}{}",
                        SecretKind::SensitiveKeyValue.marker()
                    )
                }
            });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::SensitiveKeyValue, count);
        }
    }

    // 13b. SensitiveKeyValue (flags CLI --password, --token, -p, etc.)
    if matches.matched(13) {
        let mut count = 0usize;
        let snapshot = current.clone();
        let replaced = rules.cli_flag.replace_all(&current, |caps: &Captures<'_>| {
            let flag_part = &caps[1];
            let sep_part = &caps[2];
            let val_part = &caps[3];

            if is_exempt_value(val_part) || is_exempt_short_p_flag(&snapshot, flag_part, val_part) {
                caps[0].to_string()
            } else {
                count += 1;
                format!(
                    "{flag_part}{sep_part}{}",
                    SecretKind::SensitiveKeyValue.marker()
                )
            }
        });
        if count > 0 {
            current = Cow::Owned(replaced.into_owned());
            report.record(SecretKind::SensitiveKeyValue, count);
        }
    }

    (current, report)
}
