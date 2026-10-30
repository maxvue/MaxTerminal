# Implementação 01 — Sanitizador de Segredos, `StreamSanitizer` e `ExternalPayload` (`spec.md`)

> Escopo da Fase 03: **I-01** (setup do crate `mt-db`), **I-10** (`sanitizer`), **I-17** (testes/benchmarks do sanitizador) e **I-19** (sanitização nas fronteiras, decoder incremental, `ExternalPayload` e inspeção JSON).
> Critérios de aceite cobertos: **CA-27**, **CA-28**, **CA-29**, **CA-30**, **CA-31**, **CA-46**.

---

## 1. Contexto, Objetivo e Escopo

O `AGENTS.md` (§3 Sanitização de Segredos, Diretriz §4) determina que nenhum comando, scrollback, log ou payload externo do MaxTerminal seja gravado em disco ou enviado para inspeção externa sem redação prévia de segredos.

Esta implementação cria o crate `src-tauri/crates/mt-db` e entrega o módulo puro e independente de banco **`mt_db::sanitizer`**, fornecendo:
1. **`sanitize(&str) -> Cow<'_, str>`** e **`sanitize_with(&str, &SanitizeOptions) -> SanitizeOutcome<'_>`**: redação síncrona em passe único com pré-filtro `RegexSet` (`LazyLock`), parser contextual (headers, atribuições, flags CLI, URLs, aspas e blocos PEM), normalização de sequências ANSI quando presentes, retorno `Cow::Borrowed` zero-copy quando limpo e idempotência garantida.
2. **`SanitizedText`**: newtype imutável que implementa `rusqlite::ToSql` e possui `Debug` opaco (sem revelar o texto), atuando como barreira de tipo obrigatória para repositórios e escritores em lote.
3. **`StreamSanitizer`**: máquina de estados incremental por stream (`Utf8Decoder` + `TerminalNormalizer` + buffer de unidade lógica até 16 KiB) que retém bytes UTF-8 incompletos, escapes ANSI partidos, valores entre aspas abertas, blocos PEM multilinhas e prefixos suspeitos de credenciais através de múltiplos chunks, tratando lacunas (`on_gap()`) e EOF (`finish()`).
4. **`ExternalPayload` e inspeção recursiva de JSON**: fronteira tipada `prepare_external_payload` para envio externo (Fases 11/12/13) e `inspect_settings_json` para rejeitar chaves ou valores sensíveis antes de gravar na tabela `settings`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Modificações
- `src-tauri/Cargo.toml` — inclusão de `"crates/mt-db"` em `[workspace].members` e adição das dependências de workspace de `mt-db`.

### 2.2. Novos Arquivos
- `src-tauri/crates/mt-db/Cargo.toml`
- `src-tauri/crates/mt-db/src/lib.rs` (esqueleto inicial expondo `pub mod error; pub mod sanitizer; pub mod time;`)
- `src-tauri/crates/mt-db/src/time.rs`
- `src-tauri/crates/mt-db/src/error.rs` (base de `DbError` e `SanitizeError`)
- `src-tauri/crates/mt-db/src/sanitizer/mod.rs`
- `src-tauri/crates/mt-db/src/sanitizer/rules.rs`
- `src-tauri/crates/mt-db/src/sanitizer/stream.rs`
- `src-tauri/crates/mt-db/tests/fixtures/sanitizer_cases.toml`
- `src-tauri/crates/mt-db/tests/sanitizer_table.rs`
- `src-tauri/crates/mt-db/tests/sanitizer_props.rs`
- `src-tauri/crates/mt-db/tests/sanitizer_stream.rs`
- `src-tauri/crates/mt-db/benches/sanitizer.rs`

---

## 3. Manifesto Cargo e Utilitários Base

### 3.1. `src-tauri/Cargo.toml` (adições em `[workspace]`)

```toml
[workspace]
resolver = "3"
members = [".", "crates/mt-core", "crates/mt-db"]

[workspace.dependencies]
mt-core = { path = "crates/mt-core" }
mt-db = { path = "crates/mt-db" }
rusqlite = { version = "0.34", features = ["bundled", "backup", "functions", "serde_json"] }
r2d2 = "0.8"
r2d2_sqlite = "0.27"
crossbeam-channel = "0.5"
tokio = { version = "1", features = ["sync", "rt", "time", "macros"] }
regex = "1"
include_dir = "0.7"
sha2 = "0.10"
tempfile = "3"
proptest = "1"
rstest = "0.25"
criterion = "0.5"
```

> **Invariante de lockfile:** Confirmar que `r2d2_sqlite` resolve exatamente a mesma versão de `rusqlite` e `libsqlite3-sys` no `Cargo.lock` (sem duplicar `libsqlite3-sys`). O SQLite bundled deve ter versão ≥ `3.51.3` (ou backport validado para WAL-reset).

### 3.2. `src-tauri/crates/mt-db/Cargo.toml`

```toml
[package]
name = "mt-db"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
mt-core.workspace = true
rusqlite.workspace = true
r2d2.workspace = true
r2d2_sqlite.workspace = true
crossbeam-channel.workspace = true
tokio.workspace = true
regex.workspace = true
include_dir.workspace = true
sha2.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tracing.workspace = true
specta.workspace = true

[dev-dependencies]
tempfile.workspace = true
proptest.workspace = true
rstest.workspace = true
criterion.workspace = true
tokio = { workspace = true, features = ["rt-multi-thread", "macros", "test-util"] }

[[bench]]
name = "sanitizer"
harness = false

[[bench]]
name = "batch"
harness = false

[lints]
workspace = true
```

### 3.3. `src-tauri/crates/mt-db/src/time.rs`

```rust
use std::time::{SystemTime, UNIX_EPOCH};

/// Retorna o timestamp atual em milissegundos desde a época Unix (UTC).
#[must_use]
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
```

---

## 4. Assinaturas Exatas e Contratos do Módulo `sanitizer`

### 4.1. Tipos Públicos (`src-tauri/crates/mt-db/src/sanitizer/mod.rs`)

```rust
pub mod rules;
pub mod stream;

use std::{borrow::Cow, fmt, ops::Range};
use rusqlite::types::{ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

pub use stream::{StreamSanitizer, StreamState, TerminalNormalizer, Utf8Decoder};

/// Versão atual do catálogo de regras de sanitização.
pub const SANITIZER_CATALOG_VERSION: u16 = 1;

/// Famílias de segredos detectadas e mascaradas pelo sanitizador.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    UrlCredentials,
    AppKey,
    Bearer,
    Cookie,
    GithubToken,
    AwsAccessKeyId,
    AwsSecretKey,
    OpenAiKey,
    AnthropicKey,
    Jwt,
    EnvAssignment,
    CliFlag,
    PemPrivateKey,
}

impl SecretKind {
    /// Nome canônico em `snake_case` usado na máscara `[REDACTED:<kind>]`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UrlCredentials => "url_credentials",
            Self::AppKey => "app_key",
            Self::Bearer => "bearer",
            Self::Cookie => "cookie",
            Self::GithubToken => "github_token",
            Self::AwsAccessKeyId => "aws_access_key_id",
            Self::AwsSecretKey => "aws_secret_key",
            Self::OpenAiKey => "openai_key",
            Self::AnthropicKey => "anthropic_key",
            Self::Jwt => "jwt",
            Self::EnvAssignment => "env_assignment",
            Self::CliFlag => "cli_flag",
            Self::PemPrivateKey => "pem_private_key",
        }
    }

    /// Máscara estática correspondente: `[REDACTED:<kind>]`.
    #[must_use]
    pub const fn redaction_marker(self) -> &'static str {
        match self {
            Self::UrlCredentials => "[REDACTED:url_credentials]",
            Self::AppKey => "[REDACTED:app_key]",
            Self::Bearer => "[REDACTED:bearer]",
            Self::Cookie => "[REDACTED:cookie]",
            Self::GithubToken => "[REDACTED:github_token]",
            Self::AwsAccessKeyId => "[REDACTED:aws_access_key_id]",
            Self::AwsSecretKey => "[REDACTED:aws_secret_key]",
            Self::OpenAiKey => "[REDACTED:openai_key]",
            Self::AnthropicKey => "[REDACTED:anthropic_key]",
            Self::Jwt => "[REDACTED:jwt]",
            Self::EnvAssignment => "[REDACTED:env_assignment]",
            Self::CliFlag => "[REDACTED:cli_flag]",
            Self::PemPrivateKey => "[REDACTED:pem_private_key]",
        }
    }

    /// Todas as 13 famílias obrigatórias do catálogo.
    pub const ALL: [Self; 13] = [
        Self::PemPrivateKey,
        Self::Jwt,
        Self::GithubToken,
        Self::AnthropicKey,
        Self::OpenAiKey,
        Self::AwsAccessKeyId,
        Self::AwsSecretKey,
        Self::AppKey,
        Self::Bearer,
        Self::Cookie,
        Self::UrlCredentials,
        Self::CliFlag,
        Self::EnvAssignment,
    ];
}

/// Opções para `sanitize_with` (restrito a testes e auditoria; nunca desabilita regras na persistência).
#[derive(Debug, Clone, Default)]
pub struct SanitizeOptions {
    pub kinds: Option<&'static [SecretKind]>,
}

/// Ocorrência detectada no texto original (nunca armazena o valor secreto encontrado).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub kind: SecretKind,
    /// Intervalo em bytes no texto de entrada inspecionado.
    pub range: Range<usize>,
}

/// Resultado detalhado da sanitização.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizeOutcome<'a> {
    pub text: Cow<'a, str>,
    pub findings: Vec<Finding>,
}

/// Erros de fronteira do sanitizador incremental e de payloads estruturados.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SanitizeError {
    #[error("stream sanitizador já foi finalizado (sanitizer.finished)")]
    StreamFinished,
    #[error("profundidade máxima do JSON excedida (máximo: 16 níveis)")]
    JsonDepthExceeded,
    #[error("número máximo de nós do JSON excedido (máximo: 1024 nós)")]
    JsonNodesExceeded,
    #[error("tamanho máximo do payload externo excedido (máximo: 256 KiB)")]
    PayloadTooLarge,
    #[error("valor sensível detectado na chave ou valor de configuração")]
    SensitiveContentDetected,
}

/// Texto que passou obrigatoriamente pelo pipeline de sanitização vigente.
/// Não implementa `From<String>`, `From<&str>` nem `Deserialize` para impedir bypass.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SanitizedText(String);

impl SanitizedText {
    /// Sanitiza uma mensagem completa (com semântica de EOF, incluindo normalização ANSI se houver bytes ESC/controle).
    #[must_use]
    pub fn new(raw: &str) -> Self {
        Self(sanitize(raw).into_owned())
    }

    /// Construtor interno exclusivo para saídas comprovadamente sanitizadas por `StreamSanitizer`.
    pub(crate) fn from_sanitized_unchecked(s: String) -> Self {
        Self(s)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// `Debug` opaco obrigatório: nunca imprime o conteúdo textual em logs de erro ou panic.
impl fmt::Debug for SanitizedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SanitizedText")
            .field("catalog_version", &SANITIZER_CATALOG_VERSION)
            .field("byte_len", &self.0.len())
            .finish()
    }
}

impl ToSql for SanitizedText {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(self.0.as_bytes())))
    }
}

/// Sanitiza texto completo com o catálogo padrão.
/// Retorna `Cow::Borrowed` quando nenhuma normalização ANSI é necessária e nenhum segredo ou cauda suspeita é encontrado.
#[must_use]
pub fn sanitize(input: &str) -> Cow<'_, str>;

/// Sanitiza uma mensagem de terminal completa normalizando sequências ANSI antes da detecção.
#[must_use]
pub fn sanitize_terminal(input: &str) -> SanitizedText;

/// Variante com opções e lista de `Finding` para testes e diagnóstico de auditoria.
#[must_use]
pub fn sanitize_with<'a>(input: &'a str, opts: &SanitizeOptions) -> SanitizeOutcome<'a>;

/// Newtype privado de payload JSON pronto para envio externo (Fases 11, 12 e 13).
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ExternalPayload(serde_json::Value);

impl ExternalPayload {
    #[must_use]
    pub fn as_value(&self) -> &serde_json::Value {
        &self.0
    }

    /// Serializa o JSON já validado e sanitizado.
    pub fn to_json_string(&self) -> Result<String, SanitizeError>;
}

impl fmt::Debug for ExternalPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternalPayload")
            .field("catalog_version", &SANITIZER_CATALOG_VERSION)
            .finish()
    }
}

/// Prepara e valida um JSON para envio externo:
/// - Limite de 16 níveis de profundidade, 1024 nós e 256 KiB após serialização.
/// - Chaves com nomes sensíveis têm seus valores inteiramente substituídos pela máscara correspondente.
/// - Strings comuns passam por `sanitize_terminal`.
pub fn prepare_external_payload(value: serde_json::Value) -> Result<ExternalPayload, SanitizeError>;

/// Inspeciona recursivamente um valor JSON candidato à tabela `settings`:
/// - Limite de 16 níveis de profundidade e 1024 nós.
/// - Rejeita com `Err(SanitizeError::SensitiveContentDetected)` se qualquer chave do JSON casar com nome sensível
///   (`password`, `passwd`, `secret`, `token`, `api_key`, `apikey`, `private_key`, `credential`, `credentials`,
///   `app_key`, `authorization`, `cookie`, `set-cookie`, `aws_secret_access_key`) ou se qualquer string interna
///   sofrer redação por `sanitize()` ou contiver controles ANSI brutos.
pub fn inspect_settings_json(value: &serde_json::Value) -> Result<(), SanitizeError>;
```

---

## 5. Catálogo de Regras, Parser Contextual e Exclusões (`src/sanitizer/rules.rs`)

### 5.1. As 13 Famílias e Expressões Regulares (`LazyLock`)

Todas as expressões regulares são compiladas uma única vez via `std::sync::LazyLock` junto com um `regex::RegexSet` unificado usado como pré-filtro rápido em texto limpo:

| Ordem | `SecretKind` | Padrão Regex | Máscara / Preservação de Prefixo |
|-------|-------------|--------------|----------------------------------|
| 1 | `PemPrivateKey` | `(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----(?:.*?-----END [A-Z0-9 ]*PRIVATE KEY-----\|.*$)` | `[REDACTED:pem_private_key]` (cobre também PEM truncado em EOF) |
| 2 | `Jwt` | `\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}` | `[REDACTED:jwt]` |
| 3 | `GithubToken` | `\b(?:gh[pousr]_[A-Za-z0-9]{36,255}\|github_pat_[A-Za-z0-9_]{22,255})\b` | `[REDACTED:github_token]` |
| 4 | `AnthropicKey` | `\bsk-ant-[A-Za-z0-9_-]{20,}` | `[REDACTED:anthropic_key]` |
| 5 | `OpenAiKey` | `\bsk-(?:proj-\|svcacct-\|admin-)?[A-Za-z0-9_-]{20,}` | `[REDACTED:openai_key]` (exclui prefixo `sk-ant-` já coberto por `AnthropicKey`) |
| 6 | `AwsAccessKeyId` | `\b(?:AKIA\|ASIA\|AGPA\|AIDA\|AROA\|ANPA\|ANVA\|AIPA)[A-Z0-9]{16}\b` | `[REDACTED:aws_access_key_id]` |
| 7 | `AwsSecretKey` | `(?i)(\baws_secret_access_key\s*[=:]\s*)(?:"[^"\r\n]*"\|'[^'\r\n]*'\|[A-Za-z0-9/+=]{16,}\|[^\s"'$][^\s]*)` | `${1}[REDACTED:aws_secret_key]` |
| 8 | `AppKey` | `(?i)(\bAPP_KEY\s*[=:]\s*["']?)(base64:)?([^\s"'\r\n]+)` | Se tinha `base64:`: `${1}base64:[REDACTED:app_key]` (sem aspa final solta); caso contrário: `${1}[REDACTED:app_key]` |
| 9 | `Bearer` | `(?i)(\b(?:authorization\s*[:=]\s*)?bearer\s+)([^\s"'\r\n,;]+)` | `${1}[REDACTED:bearer]` |
| 10 | `Cookie` | `(?im)(^\s*(?:set-)?cookie\s*:\s*\|--cookie(?:=\|\s+)["']?\|(?:^\|[\s])(?:-b)\s+["'])([^\r\n"']+)` | `${1}[REDACTED:cookie]` |
| 11 | `UrlCredentials` | `(?i)\b([a-z][a-z0-9+.\-]{1,20}://[^\s:/@"']+):([^\s@/"']{1,256})@` | `${1}:[REDACTED:url_credentials]@` |
| 12 | `CliFlag` | `(?i)(--(?:password\|passwd\|token\|secret\|api-key\|access-token)(?:=\|\s+))("[^"\r\n]*"\|'[^'\r\n]*'\|[^\s"'$][^\s]*)` | `${1}[REDACTED:cli_flag]` |
| 13 | `EnvAssignment` | `(?i)\b((?:export\s+)?[A-Z0-9_]*(?:PASSWORD\|PASSWD\|SECRET\|TOKEN\|API_KEY\|APIKEY\|PRIVATE_KEY\|CREDENTIALS?)[A-Z0-9_]*\s*[=:]\s*)("[^"\r\n]*"\|'[^'\r\n]*'\|[^\s"'$][^\s]*)` | `${1}[REDACTED:env_assignment]` |

### 5.2. Parser Contextual de Sobreposições, Aspas Abertas e Valores Curtos
A tabela regex é complementada por um analisador contextual em `rules.rs`:
1. **Precedência de contexto externo sobre token interno:** Se um intervalo externo (ex.: `Authorization: Bearer <JWT>` ou `GITHUB_TOKEN=ghp_...` ou `OPENAI_API_KEY="sk-..."`) englobar um token específico (`Jwt`, `GithubToken`, `OpenAiKey`), o intervalo de valor completo é substituído uma única vez pelo `SecretKind` contextual externo (no caso de `Bearer <JWT>`, vira `Bearer [REDACTED:bearer]`; no caso de `APP_KEY` / `AwsSecretKey` têm precedência sobre `EnvAssignment` genérico).
   - **Ordem de prioridade entre regras contextuais na mesma posição de chave:**
     1. `PemPrivateKey`
     2. `AwsSecretKey` (quando a chave é `aws_secret_access_key`)
     3. `AppKey` (quando a chave é `APP_KEY`)
     4. `Bearer`
     5. `Cookie`
     6. `UrlCredentials`
     7. `CliFlag`
     8. `EnvAssignment`
     9. Tokens autônomos (`Jwt`, `GithubToken`, `AnthropicKey`, `OpenAiKey`, `AwsAccessKeyId`)
2. **Exclusões e Falsos Positivos (R-02):**
   - Qualquer valor que já comece com `[REDACTED:` e termine com `]` (ou seja exatamente `[REDACTED:<kind>]`) **não** é redigido novamente (garantia de idempotência: `sanitize(sanitize(x)) == sanitize(x)`).
   - Em `EnvAssignment`, `CliFlag`, `AppKey` e `AwsSecretKey`:
     - Valores vazios (`FOO_TOKEN=` ou `FOO_TOKEN=""` ou `FOO_TOKEN=''`) são preservados intactos.
     - Referências simples e **inteiras** a variáveis de ambiente (`$VAR` ou `${VAR}`, onde `VAR` casa `^[A-Za-z_][A-Za-z0-9_]*$`, inclusive entre aspas simples/duplas sem texto adicional) são preservadas (`export GITHUB_TOKEN=$TOKEN` permanece inalterado).
     - Expressões compostas (`$VAR-suffix`, `prefix$VAR`, `$(cat secret)`, `` `cmd` ``) **são redigidas**.
   - Variáveis `PWD` e `OLDPWD` nunca acionam `EnvAssignment` pelo nome da chave, mas se o caminho contiver uma URL com credencial ou token explícito, o token interno é redigido normalmente.
   - Em `UrlCredentials`, se a parte da senha for `$VAR` ou `${VAR}` inteira ou já for `[REDACTED:url_credentials]`, não redige novamente.
3. **Finalização Conservadora de EOF (Aspas Abertas / PEM Incompleto / Prefixo Truncado):**
   - Se o texto terminar com `-----BEGIN ... PRIVATE KEY-----` sem o `-----END ...` correspondente, todo o sufixo a partir de `-----BEGIN` é substituído por `[REDACTED:pem_private_key]`.
   - Se o texto terminar dentro de uma atribuição sensível (`DB_PASSWORD="minha senha sem fechar aspas`), flag CLI (`--token "abc`), `Bearer `, `Cookie: ` ou `https://user:pass` truncado antes de `@` ou espaço com prefixo contextual sensível, a cauda do valor aberto é redigida com a máscara da respectiva família.
4. **Caminho Rápido Zero-Copy (`Cow::Borrowed`):**
   - Se o texto não contém bytes de controle ANSI (`\x1b`, `\r`, `\x08`, `\x9b`), não aciona `PREFILTER` (`RegexSet`) e não possui prefixos contextuais pendentes no final, `sanitize(input)` retorna `Cow::Borrowed(input)` sem alocar memória.

---

## 6. `StreamSanitizer`, `Utf8Decoder` e `TerminalNormalizer` (`src/sanitizer/stream.rs`)

### 6.1. Estrutura e Assinaturas

```rust
/// Limite máximo de retenção (carry) de uma unidade lógica antes de acionar supressão por tamanho.
pub const MAX_STREAM_CARRY_BYTES: usize = 16 * 1024; // 16 KiB

/// Marcador emitido quando uma linha/unidade excede 16 KiB sem fronteira segura.
pub const OVERSIZE_REDACTION_MARKER: &str = "[REDACTED:oversize]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteKind {
    Single,
    Double,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamState {
    /// Fluxo normal de texto linha a linha.
    Text,
    /// Valor entre aspas aberto dentro de contexto sensível ou linha pendente.
    QuotedValue { quote: QuoteKind, escaped: bool },
    /// Dentro de um bloco `-----BEGIN ... PRIVATE KEY-----` aguardando `-----END ... PRIVATE KEY-----`.
    Pem { emitted_marker: bool, tail_window: String },
    /// Linha/unidade excedeu 16 KiB ou sofreu `on_gap()`; suprime até encontrar fronteira segura (`\n` fora de PEM).
    Suppressed { emitted_marker: bool },
    /// `finish()` já foi invocado; novas chamadas a `push()` retornam `Err(SanitizeError::StreamFinished)`.
    Finished,
}

/// Decodificador UTF-8 incremental que retém sequências multi-byte incompletas (1 a 3 bytes finais) entre chunks.
#[derive(Debug, Clone, Default)]
pub struct Utf8Decoder {
    incomplete: [u8; 4],
    incomplete_len: usize,
}

impl Utf8Decoder {
    #[must_use]
    pub fn new() -> Self;
    /// Decodifica bytes anexando ao buffer `out`. Se encontrar sequência UTF-8 inválida,
    /// sinaliza `had_invalid_utf8 = true` e insere U+FFFD para o avaliador decidir supressão.
    pub fn decode_chunk(&mut self, chunk: &[u8], out: &mut String) -> bool;
    /// Descarta bytes incompletos em caso de `on_gap()` ou `finish()`, retornando `true` se havia bytes pendentes.
    pub fn flush_incomplete(&mut self, out: &mut String) -> bool;
}

/// Normalizador incremental de sequências de controle de terminal (ANSI CSI/SGR, OSC, DCS, ESC, CR e Backspace).
#[derive(Debug, Clone, Default)]
pub struct TerminalNormalizer { /* estado interno da máquina ANSI */ }

impl TerminalNormalizer {
    #[must_use]
    pub fn new() -> Self;
    /// Processa texto UTF-8, removendo escapes CSI/SGR e descartando payloads OSC/DCS (ex.: `\x1b]0;...\x07` ou `\x1b\\`).
    /// Trata `\r` (sem `\n`) e `\x08` (backspace) de forma conservadora: se uma sobrescrita por `\r` ou `\x08`
    /// ocorrer após texto não vazio na mesma linha, marca a linha como ambígua para impedir ocultação visual de segredo.
    pub fn normalize_chunk(&mut self, input: &str, out: &mut String) -> bool;
    /// Finaliza no EOF, descartando qualquer sequência de escape incompleta.
    pub fn finish(&mut self);
    /// Reseta estado de escape em `on_gap()`.
    pub fn reset(&mut self);
}

pub struct StreamSanitizer {
    carry: String,
    state: StreamState,
    decoder: Utf8Decoder,
    terminal: TerminalNormalizer,
    max_carry: usize,
    had_corrupted_bytes: bool,
}

impl StreamSanitizer {
    #[must_use]
    pub fn new() -> Self;

    #[must_use]
    pub fn with_max_carry(max_carry: usize) -> Self;

    /// Consome bytes crus em ordem; retorna apenas unidades `SanitizedText` com fronteira comprovada.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SanitizedText>, SanitizeError>;

    /// Notifica perda de bytes crus antes do sanitizador (ex.: backpressure no produtor):
    /// descarta cauda ambígua e entra em `StreamState::Suppressed` até a próxima fronteira segura (`\n`).
    pub fn on_gap(&mut self);

    /// Finaliza o stream (EOF real, fechamento ou cancelamento).
    /// Conclui decoder/normalizador, mascara estados suspeitos abertos e retorna a cauda sanitizada.
    /// Idempotente: segunda chamada retorna `SanitizedText` vazio (`""`).
    pub fn finish(&mut self) -> SanitizedText;

    #[must_use]
    pub fn state(&self) -> &StreamState {
        &self.state
    }
}
```

### 6.2. Invariantes do `StreamSanitizer`
1. **Fronteira de emissão segura:**
   - Uma unidade em `StreamState::Text` só é emitida quando encontra `\n` **e**:
     - Não está dentro de um bloco `-----BEGIN ... PRIVATE KEY-----` (ou de um prefixo parcial de `-----BEGIN` no fim da linha).
     - Não possui aspas simples ou duplas abertas após um operador de atribuição/flag/header.
     - Não termina com barra invertida de continuação de linha de shell (`\\\n`).
     - A próxima linha (ou final do chunk) não indica continuação de header HTTP dobrado.
   - Enquanto não houver `\n` seguro, o texto permanece em `self.carry`. Isso garante que **qualquer token dividido em qualquer ponto entre 2 ou mais chunks** (dentro da mesma linha até 16 KiB) seja montado integralmente antes de passar por `sanitize()`.
2. **Tratamento de Blocos PEM (`StreamState::Pem`):**
   - Ao detectar `-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----` (mesmo dividido em 50 chunks byte a byte), o `StreamSanitizer` emite o prefixo anterior ao `-----BEGIN` junto com `[REDACTED:pem_private_key]`, entra em `StreamState::Pem` e descarta todos os bytes subsequentes mantendo apenas uma janela deslizante de 64 bytes (`tail_window`) para detectar `-----END [A-Z0-9 ]*PRIVATE KEY-----`.
   - Mesmo que um bloco PEM tenha 100 KiB em 50 chunks, ele nunca estoura `max_carry` nem vaza linhas intermediárias da chave privada. Quando `-----END ... PRIVATE KEY-----` chega, o estado volta a `Text` após o delimitador final. Se `finish()` chegar antes do `END`, nada do corpo do PEM é liberado.
3. **Overflow de 16 KiB (`StreamState::Suppressed`):**
   - Se `self.carry.len() > self.max_carry` (16 KiB) sem encontrar fronteira segura:
     - Descarta imediatamente `self.carry` (nunca emite o prefixo acumulado!).
     - Emite uma única unidade `SanitizedText("[REDACTED:oversize]\n")` (ou sem `\n` até o término da linha) e transita para `StreamState::Suppressed { emitted_marker: true }`.
     - Consome e descarta todos os bytes subsequentes sem armazenar até encontrar `\n` (fora de PEM), quando retorna para `StreamState::Text`.
4. **UTF-8 Inválido e Sobrescrita Ambígua (`\r` / `\x08`):**
   - Bytes UTF-8 incompletos no fim de um chunk são retidos em `Utf8Decoder` até o próximo chunk.
   - Se uma sequência UTF-8 for genuinamente inválida dentro de uma unidade com candidato sensível ou se `\r`/`\x08` reescrever caracteres no meio de um candidato na mesma linha, a unidade inteira é substituída conservadoramente por `[REDACTED:corrupted]` / sanitizada sobre a união antes e depois do controle.
5. **Comportamento de `on_gap()`, `finish()` e `Drop`:**
   - `on_gap()` limpa `decoder`, `terminal` e `carry`, e coloca o estado em `StreamState::Suppressed { emitted_marker: false }`, emitindo `[REDACTED:gap]` na retomada e descartando bytes até o próximo `\n` seguro.
   - `finish()` processa bytes pendentes no `decoder`/`terminal`: se `state` for `Finished`, retorna `SanitizedText("")`; se estiver em `Pem` ou `QuotedValue` ou tiver prefixo suspeito no fim, aplica a redação de EOF e marca `state = StreamState::Finished`.
   - `Drop` descarta `carry` silenciosamente sem nunca fazer flush ou logar o conteúdo pendente.

---

## 7. Testes e Benchmarks Obrigatórios da Implementação 01

1. **`tests/fixtures/sanitizer_cases.toml` (CA-27):**
   - Arquivo TOML estruturado com entradas `[[case]]` contendo `name`, `kind` (string do `SecretKind`), `positive` (`bool`), `input` e `expected`.
   - Mínimo estrito de **3 casos positivos** e **2 casos negativos** para **cada uma das 13 famílias** de `SecretKind` (total ≥ 65 casos catalogados), incluindo os casos canônicos da spec §15 (`git clone https://joao:s3nh4F0rte@...`, `APP_KEY=base64:...`, `Authorization: Bearer ...`, `DB_PASSWORD="minha senha"`, `cd $OLDPWD && export GITHUB_TOKEN=$TOKEN && git log 3f2a9c...`).
2. **`tests/sanitizer_table.rs` (CA-27, CA-28):**
   - Carrega `tests/fixtures/sanitizer_cases.toml`, valida que todas as 13 famílias de `SecretKind::ALL` possuem ≥ 3 positivos e ≥ 2 negativos, executa `sanitize(&case.input)` para cada caso, verifica `out == case.expected` e, para todos os casos negativos sem controles ANSI, verifica `matches!(out, Cow::Borrowed(_))` (CA-28).
3. **`tests/sanitizer_props.rs` (CA-29, CA-30):**
   - `proptest` com **10 000 casos** verificando idempotência (`sanitize(&sanitize(&s)) == sanitize(&s)`) e ausência de panic para qualquer string arbitrária (CA-29).
   - Teste determinístico exaustivo de **todos os pontos de corte** entre 2 chunks para um caso representativo de cada `SecretKind`, garantindo que a concatenação das saídas de `StreamSanitizer::push(c1)` + `push(c2)` + `finish()` é idêntica à versão redigida e nunca contém o segredo (CA-30).
   - Teste de bloco `PemPrivateKey` dividido em **50 chunks** confirmando redação integral para `[REDACTED:pem_private_key]` (CA-30).
4. **`tests/sanitizer_stream.rs` (CA-46):**
   - Cortes byte a byte no meio de caracteres UTF-8 multibyte (ex.: `ã`, `ç`, emojis) e no meio de sequências ANSI (`\x1b[31m`, `\x1b]0;titulo\x07`).
   - Partições em 3+ chunks, linha > 16 KiB emitindo `[REDACTED:oversize]` sem vazar prefixo, PEM sem `END` no EOF, aspas abertas no EOF, `finish()` chamado repetidas vezes (segunda retorna vazio e `push` subsequente retorna `Err(SanitizeError::StreamFinished)`), `on_gap()` descartando até `\n`, e inspeção recursiva de JSON (`inspect_settings_json` e `prepare_external_payload`).
5. **`benches/sanitizer.rs` (CA-31):**
   - Benchmark `criterion` medindo throughput em MiB/s para:
     - Corpus de 1 MiB de texto limpo de terminal (meta: **≥ 100 MiB/s** em release).
     - Corpus de 1 MiB com 1 segredo a cada 4 KiB (meta: **≥ 30 MiB/s** em release).
