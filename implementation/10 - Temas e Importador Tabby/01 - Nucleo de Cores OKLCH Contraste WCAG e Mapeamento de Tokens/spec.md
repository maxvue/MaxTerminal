# Implementação 01 — Núcleo de Cores, OKLCH, Contraste WCAG e Mapeamento de Tokens (`spec.md`)

> Fase 10 — Temas e Importador Tabby
> Referências: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§1.3, §3.1–§3.8, §3.11–§3.13, §12.1).

---

## 1. Contexto, Objetivo e Escopo

Esta primeira implementação estabelece a fundação algorítmica e matemática do crate `mt-themes` (sem dependência de `tauri`), cobrindo:

1. **Identificadores e Erros (`ids.rs`, `error.rs`)**: `ThemeId` determinístico UUID v5 para temas embutidos (`BUILTIN_NAMESPACE`, `DEFAULT_THEME_ID = b5907b11-f15c-5886-b803-f1fb94bc4c9d`) e UUID v7 para temas importados, `ContentHash` SHA-256 hex de 64 caracteres minúsculos e `ThemeError` com códigos canônicos `theme.*` e conversão para `MtError` em `pt-BR`.
2. **Parser de Cores e Espaço Perceptual OKLCH (`color.rs`, `oklch.rs`)**: validação e normalização de `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(...)` e `rgba(...)` (inteiros ou percentuais), conversão sRGB ⇄ linear ⇄ OKLab ⇄ OKLCH (matrizes de Björn Ottosson) e gamut mapping por bisseção em croma mantendo luminosidade e matiz.
3. **Contraste WCAG 2.x (`contrast.rs`)**: cálculo de luminância relativa, razão de contraste `(L1 + 0.05) / (L2 + 0.05)`, classificação `WcagLevel` (`AAA`, `AA`, `AA_LARGE`, `FAIL`), composição de alfa sobre o background para seleção e geração de `ContrastReport`.
4. **Modelo Canônico e Geração de Rampas (`scheme.rs`, `scale.rs`)**: `ColorScheme`, `AnsiPalette` (16 cores), `ThemeAppearance` (`Dark` quando `relative_luminance(background) <= 0.179`, senão `Light`), `generate_scale` (11 passos `50..950` + `-content` com contraste ≥ 4.5:1), `background_ramp` (39 passos `0..950` de 25 em 25, com `--background-500` acromático para o badge Parado) e proteção semântica estrita das escalas `success`, `warning`, `danger` e `info`.
5. **Mapeamento para xterm e Tokens CSS (`mapping/xterm.rs`, `mapping/tokens.rs`)**: conversão bidirecional `ColorScheme` ⇄ `XtermTheme` e derivação das exatas 102 variáveis CSS (`--background-*`, `--max-*`, `--mt-terminal-*`) em `ThemeTokens`, além da aproximação de exportação `scheme_from_tokens`.
6. **Temas Embutidos (`builtin/`)**: os 6 temas builtins (`max-navy-teal`, `max-light`, `dracula`, `nord`, `solarized-dark`, `solarized-light`) embutidos via `include_str!`, garantindo que "Max Navy/Teal" reproduza de forma idêntica os tokens de `@maxvue/max-components-ui` (modo escuro).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Modificados
- `src-tauri/Cargo.toml` — inclusão de `"crates/mt-themes"` em `[workspace].members` e declaração de `mt-themes = { path = "crates/mt-themes" }` em `[workspace.dependencies]`.

### 2.2. Arquivos Criados
- `src-tauri/crates/mt-themes/Cargo.toml`
- `src-tauri/crates/mt-themes/src/lib.rs`
- `src-tauri/crates/mt-themes/src/error.rs`
- `src-tauri/crates/mt-themes/src/ids.rs`
- `src-tauri/crates/mt-themes/src/color.rs`
- `src-tauri/crates/mt-themes/src/oklch.rs`
- `src-tauri/crates/mt-themes/src/contrast.rs`
- `src-tauri/crates/mt-themes/src/scheme.rs`
- `src-tauri/crates/mt-themes/src/scale.rs`
- `src-tauri/crates/mt-themes/src/mapping/mod.rs`
- `src-tauri/crates/mt-themes/src/mapping/xterm.rs`
- `src-tauri/crates/mt-themes/src/mapping/tokens.rs`
- `src-tauri/crates/mt-themes/src/builtin/mod.rs`
- `src-tauri/crates/mt-themes/src/builtin/max-navy-teal.yaml`
- `src-tauri/crates/mt-themes/src/builtin/max-light.yaml`
- `src-tauri/crates/mt-themes/src/builtin/dracula.yaml`
- `src-tauri/crates/mt-themes/src/builtin/nord.yaml`
- `src-tauri/crates/mt-themes/src/builtin/solarized-dark.yaml`
- `src-tauri/crates/mt-themes/src/builtin/solarized-light.yaml`
- `src-tauri/crates/mt-themes/tests/fixtures/max-components-ui/dark-tokens.json`
- `src-tauri/crates/mt-themes/tests/fixtures/max-components-ui/light-tokens.json`
- `src-tauri/crates/mt-themes/tests/mapping_roundtrip.rs`
- `src-tauri/crates/mt-themes/tests/scale_snapshots.rs`

---

## 3. Especificação Técnica e Assinaturas Rust

### 3.1. Manifesto `src-tauri/crates/mt-themes/Cargo.toml`

```toml
[package]
name = "mt-themes"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish = false

[dependencies]
mt-core.workspace = true
mt-db.workspace = true
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
serde_yaml_ng = "0.10"
yaml-rust2 = "0.10"
specta.workspace = true
thiserror.workspace = true
tracing.workspace = true
sha2 = "0.10"
walkdir = "2"
rustix = { version = "1", features = ["fs"] }
lru = "0.12"
uuid = { workspace = true, features = ["v5", "v7", "serde"] }
tokio = { workspace = true, features = ["rt", "sync"] }

[dev-dependencies]
insta = { version = "1", features = ["yaml", "json"] }
proptest = "1"
tempfile = "3"
rstest = "0.23"
tokio = { workspace = true, features = ["macros", "rt-multi-thread"] }

[lints]
workspace = true
```

### 3.2. Erros Canônicos (`src-tauri/crates/mt-themes/src/error.rs`)

```rust
use mt_core::{ErrorCode, MtError};
use serde_json::json;

pub mod codes {
    use mt_core::ErrorCode;
    pub const NOT_FOUND: ErrorCode = ErrorCode("theme.not_found");
    pub const INVALID_COLOR: ErrorCode = ErrorCode("theme.invalid_color");
    pub const INVALID_PALETTE: ErrorCode = ErrorCode("theme.invalid_palette");
    pub const INVALID_SCHEME: ErrorCode = ErrorCode("theme.invalid_scheme");
    pub const PARSE_FAILED: ErrorCode = ErrorCode("theme.parse_failed");
    pub const UNSUPPORTED_FORMAT: ErrorCode = ErrorCode("theme.unsupported_format");
    pub const FILE_TOO_LARGE: ErrorCode = ErrorCode("theme.file_too_large");
    pub const FILE_UNREADABLE: ErrorCode = ErrorCode("theme.file_unreadable");
    pub const PATH_INVALID: ErrorCode = ErrorCode("theme.path_invalid");
    pub const DIR_LIMIT_EXCEEDED: ErrorCode = ErrorCode("theme.dir_limit_exceeded");
    pub const BUILTIN_READONLY: ErrorCode = ErrorCode("theme.builtin_readonly");
    pub const EXPORT_FAILED: ErrorCode = ErrorCode("theme.export_failed");
    pub const INVALID_FONT: ErrorCode = ErrorCode("theme.invalid_font");
    pub const INVALID_APPEARANCE: ErrorCode = ErrorCode("theme.invalid_appearance");
    pub const INVALID_TOKEN: ErrorCode = ErrorCode("theme.invalid_token");
    pub const PROJECT_NOT_FOUND: ErrorCode = ErrorCode("theme.project_not_found");
}

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("cor inválida em {field}: \"{value}\"")]
    InvalidColor { field: String, value: String },
    #[error("paleta com {found} cores; são necessárias ao menos 8")]
    InvalidPalette { found: usize },
    #[error("esquema inválido: campo obrigatório \"{field}\" ausente")]
    InvalidScheme { field: &'static str },
    #[error("não foi possível interpretar o arquivo: {reason}")]
    ParseFailed { reason: String },
    #[error("formato de tema não reconhecido")]
    UnsupportedFormat,
    #[error("arquivo excede {limit} bytes")]
    FileTooLarge { limit: u64 },
    #[error("não foi possível ler o arquivo")]
    FileUnreadable(#[source] std::io::Error),
    #[error("caminho inválido: {reason}")]
    PathInvalid { reason: &'static str },
    #[error("temas embutidos não podem ser alterados ou excluídos")]
    BuiltinReadonly,
    #[error("falha ao exportar: {reason}")]
    ExportFailed { reason: String },
    #[error("fonte inválida: {reason}")]
    InvalidFont { reason: String },
    #[error("valor inválido para {field}: {reason}")]
    InvalidAppearance { field: &'static str, reason: String },
    #[error("token inválido: {key}")]
    InvalidToken { key: String },
    #[error("tema não encontrado")]
    NotFound,
    #[error("projeto não encontrado")]
    ProjectNotFound,
    #[error(transparent)]
    Db(#[from] mt_db::DbError),
}

impl ThemeError {
    pub fn invalid_color(field: impl Into<String>, raw_value: &str) -> Self {
        let truncated: String = raw_value.chars().take(64).collect();
        Self::InvalidColor {
            field: field.into(),
            value: truncated,
        }
    }

    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidColor { .. } => codes::INVALID_COLOR,
            Self::InvalidPalette { .. } => codes::INVALID_PALETTE,
            Self::InvalidScheme { .. } => codes::INVALID_SCHEME,
            Self::ParseFailed { .. } => codes::PARSE_FAILED,
            Self::UnsupportedFormat => codes::UNSUPPORTED_FORMAT,
            Self::FileTooLarge { .. } => codes::FILE_TOO_LARGE,
            Self::FileUnreadable(_) => codes::FILE_UNREADABLE,
            Self::PathInvalid { .. } => codes::PATH_INVALID,
            Self::BuiltinReadonly => codes::BUILTIN_READONLY,
            Self::ExportFailed { .. } => codes::EXPORT_FAILED,
            Self::InvalidFont { .. } => codes::INVALID_FONT,
            Self::InvalidAppearance { .. } => codes::INVALID_APPEARANCE,
            Self::InvalidToken { .. } => codes::INVALID_TOKEN,
            Self::NotFound => codes::NOT_FOUND,
            Self::ProjectNotFound => codes::PROJECT_NOT_FOUND,
            Self::Db(err) => MtError::from(err.clone_or_map()).code,
        }
    }
}

impl From<ThemeError> for MtError {
    fn from(err: ThemeError) -> Self {
        match err {
            ThemeError::InvalidColor { field, value } => MtError::with_details(
                codes::INVALID_COLOR,
                format!("A cor \"{value}\" em {field} é inválida."),
                json!({ "field": field }),
            ),
            ThemeError::InvalidPalette { found } => MtError::with_details(
                codes::INVALID_PALETTE,
                format!("A paleta possui {found} cores; são necessárias ao menos 8 cores."),
                json!({ "found": found }),
            ),
            ThemeError::InvalidScheme { field } => MtError::with_details(
                codes::INVALID_SCHEME,
                format!("Esquema de tema inválido: o campo obrigatório \"{field}\" está ausente."),
                json!({ "field": field }),
            ),
            ThemeError::ParseFailed { reason } => MtError::new(
                codes::PARSE_FAILED,
                format!("Não foi possível interpretar o arquivo de tema: {reason}"),
            ),
            ThemeError::UnsupportedFormat => MtError::new(
                codes::UNSUPPORTED_FORMAT,
                "Formato de tema não reconhecido (esperado Tabby, xterm ou Windows Terminal).",
            ),
            ThemeError::FileTooLarge { limit } => MtError::with_details(
                codes::FILE_TOO_LARGE,
                format!("O arquivo excede o tamanho máximo permitido de {limit} bytes."),
                json!({ "limit": limit }),
            ),
            ThemeError::FileUnreadable(_) => MtError::new(
                codes::FILE_UNREADABLE,
                "Não foi possível ler o arquivo de tema selecionado.",
            ),
            ThemeError::PathInvalid { reason } => MtError::new(
                codes::PATH_INVALID,
                format!("Caminho de arquivo ou diretório inválido: {reason}"),
            ),
            ThemeError::BuiltinReadonly => MtError::new(
                codes::BUILTIN_READONLY,
                "Temas embutidos não podem ser alterados ou excluídos.",
            ),
            ThemeError::ExportFailed { reason } => MtError::new(
                codes::EXPORT_FAILED,
                format!("Falha ao exportar o tema: {reason}"),
            ),
            ThemeError::InvalidFont { reason } => MtError::new(
                codes::INVALID_FONT,
                format!("Configuração de fonte inválida: {reason}"),
            ),
            ThemeError::InvalidAppearance { field, reason } => MtError::with_details(
                codes::INVALID_APPEARANCE,
                format!("Valor inválido para {field}: {reason}"),
                json!({ "field": field }),
            ),
            ThemeError::InvalidToken { key } => MtError::with_details(
                codes::INVALID_TOKEN,
                format!("Token CSS não permitido: {key}"),
                json!({ "key": key }),
            ),
            ThemeError::NotFound => MtError::new(codes::NOT_FOUND, "Tema não encontrado."),
            ThemeError::ProjectNotFound => MtError::new(codes::PROJECT_NOT_FOUND, "Projeto não encontrado."),
            ThemeError::Db(db_err) => MtError::from(db_err),
        }
    }
}
```

### 3.3. Identificadores (`src-tauri/crates/mt-themes/src/ids.rs`)

```rust
use serde::{Deserialize, Serialize};

/// Identificador de tema. Builtins usam UUID v5 determinístico por `slug`; importados usam UUID v7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct ThemeId(pub uuid::Uuid);

/// Namespace fixo para geração de UUID v5 dos temas embutidos (imutável).
pub const BUILTIN_NAMESPACE: uuid::Uuid = uuid::uuid!("6f1c2a4e-9b3d-4c57-8e21-5a7d0c9e4b13");

/// `ThemeId::builtin("max-navy-teal")` = `b5907b11-f15c-5886-b803-f1fb94bc4c9d`.
pub const DEFAULT_THEME_ID: ThemeId = ThemeId(uuid::uuid!("b5907b11-f15c-5886-b803-f1fb94bc4c9d"));

impl ThemeId {
    pub fn builtin(slug: &str) -> Self {
        Self(uuid::Uuid::new_v5(&BUILTIN_NAMESPACE, slug.as_bytes()))
    }

    pub fn new_imported() -> Self {
        Self(uuid::Uuid::now_v7())
    }
}

impl std::fmt::Display for ThemeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ThemeId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(uuid::Uuid::parse_str(s)?))
    }
}

/// SHA-256 hexadecimal (64 caracteres minúsculos) do JSON canônico das cores normalizadas (sem `name`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct ContentHash(pub String);
```

### 3.4. Parser e Tipos de Cor (`src-tauri/crates/mt-themes/src/color.rs`)

```rust
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rgba {
    pub rgb: Rgb,
    pub a: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedColor {
    pub rgb: Rgb,
    pub alpha: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorParseError {
    Empty,
    BadHex,
    OutOfRange,
    UnsupportedFunction,
    Malformed,
}

pub fn parse_color(input: &str) -> Result<ParsedColor, ColorParseError>;

impl Rgb {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255 };

    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn to_linear(self) -> [f64; 3];
    pub fn from_linear(c: [f64; 3]) -> Option<Self>;
}

impl Rgba {
    pub fn new(rgb: Rgb, a: u8) -> Self {
        Self { rgb, a }
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            self.rgb.to_hex()
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.rgb.r, self.rgb.g, self.rgb.b, self.a)
        }
    }

    /// Compõe `self` sobre `backdrop` em sRGB: `out = fg * (a/255) + bg * (1 - a/255)`.
    pub fn composite_over(self, backdrop: Rgb) -> Rgb;
}
```

Regras de `parse_color`:
- Trim de espaços ASCII ao redor da string e ao redor de vírgulas/parênteses.
- String vazia retorna `Err(ColorParseError::Empty)`.
- Se começar com `#`:
  - 3 dígitos hex (`#rgb`): duplica cada nibble (`r = rr`, `g = gg`, `b = bb`), `alpha = None`.
  - 4 dígitos hex (`#rgba`): duplica cada nibble (`r = rr`, `g = gg`, `b = bb`, `a = aa`), `alpha = Some(a)`.
  - 6 dígitos hex (`#rrggbb`): `alpha = None`.
  - 8 dígitos hex (`#rrggbbaa`): `alpha = Some(a)`.
  - Qualquer outro comprimento ou caractere não-hexadecimal retorna `Err(ColorParseError::BadHex)`.
- Se começar com `rgb(` ou `rgba(` (case-insensitive) e terminar com `)`:
  - `rgb(...)` deve ter exatamente 3 argumentos separados por vírgula; `rgba(...)` deve ter exatamente 4 argumentos separados por vírgula (rejeita sintaxe CSS Color 4 separada por espaços ou `/`, retornando `Malformed`).
  - Os 3 canais R, G, B devem ser **todos** inteiros `0..=255` **ou todos** percentuais `0.0%..=100.0%` (misturar inteiro e percentual retorna `Malformed`; valores fora de `0..=255` ou `0.0..=100.0%` retornam `OutOfRange`).
  - Percentuais RGB são convertidos via `(pct * 2.55).round() as u8`.
  - O canal alfa `A` em `rgba(...)` aceita decimal `0.0..=1.0` (`(a * 255.0).round() as u8`) ou percentual `0.0%..=100.0%` (`(pct * 2.55).round() as u8`); fora do intervalo retorna `OutOfRange`.
- Qualquer outra função (`hsl(`, `hsla(`, `hwb(`, `oklch(`, `lab(`) retorna `Err(ColorParseError::UnsupportedFunction)`.
- Nomes CSS (`red`, `transparent`, etc.) retornam `Err(ColorParseError::Malformed)`.
- `Serialize`, `Deserialize` e `specta::Type` de `Rgb` e `Rgba` serializam e tipam como `String` (`#rrggbb` / `#rrggbbaa`), garantindo que `bindings.ts` exponha `string`.

### 3.5. Conversão OKLab / OKLCH (`src-tauri/crates/mt-themes/src/oklch.rs`)

```rust
use crate::color::Rgb;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    pub l: f64, // [0.0, 1.0]
    pub c: f64, // >= 0.0
    pub h: f64, // [0.0, 360.0)
}

pub fn rgb_to_oklch(c: Rgb) -> Oklch;
pub fn oklch_to_rgb_clamped(c: Oklch) -> Rgb;
pub fn in_srgb_gamut(c: Oklch) -> bool;
pub fn mix_rgb_oklch(a: Rgb, b: Rgb, t: f64) -> Rgb;
```

Fórmulas de Björn Ottosson:
- sRGB linear `(r, g, b)`:
  - `l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b`
  - `m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b`
  - `s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b`
  - `l_ = l.cbrt()`, `m_ = m.cbrt()`, `s_ = s.cbrt()`
  - `L = 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_`
  - `a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_`
  - `b_lab = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_`
  - `C = (a * a + b_lab * b_lab).sqrt()`
  - `h = b_lab.atan2(a).to_degrees().rem_euclid(360.0)` (se `C < 1e-7`, `h = 0.0`).
- Inversa OKLab → linear sRGB:
  - `l_ = L + 0.3963377774 * a + 0.2158037573 * b_lab`
  - `m_ = L - 0.1055613458 * a - 0.0638541728 * b_lab`
  - `s_ = L - 0.0894841775 * a - 1.2914855480 * b_lab`
  - `l = l_ * l_ * l_`, `m = m_ * m_ * m_`, `s = s_ * s_ * s_`
  - `r = +4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s`
  - `g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s`
  - `b = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s`
- `in_srgb_gamut`: verdadeiro se `r, g, b ∈ [-1e-4, 1.0 + 1e-4]`.
- `oklch_to_rgb_clamped`: se `l <= 0.0`, retorna `Rgb::BLACK`; se `l >= 1.0`, retorna `Rgb::WHITE`. Caso `in_srgb_gamut` seja falso, executa 20 iterações de bisseção em `c ∈ [0.0, c0]` mantendo `l` e `h`, faz clamp de cada canal linear em `[0.0, 1.0]` e converte para 8 bits.

### 3.6. Contraste WCAG (`src-tauri/crates/mt-themes/src/contrast.rs`)

```rust
use crate::color::Rgb;
use crate::mapping::tokens::ThemeTokens;
use crate::scheme::ColorScheme;
use serde::{Deserialize, Serialize};

pub fn relative_luminance(c: Rgb) -> f64 {
    let [r, g, b] = c.to_linear();
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (l1, l2) = if la >= lb { (la, lb) } else { (lb, la) };
    (l1 + 0.05) / (l2 + 0.05)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WcagLevel {
    Aaa,
    Aa,
    AaLarge,
    Fail,
}

impl WcagLevel {
    pub fn from_ratio(ratio: f64) -> Self {
        if ratio >= 7.0 {
            Self::Aaa
        } else if ratio >= 4.5 {
            Self::Aa
        } else if ratio >= 3.0 {
            Self::AaLarge
        } else {
            Self::Fail
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContrastEntry {
    pub pair: String,
    pub ratio: f64,
    pub level: WcagLevel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContrastReport {
    pub foreground_background: ContrastEntry,
    pub ansi: Vec<ContrastEntry>,
    pub scales: Vec<ContrastEntry>,
    pub interaction: Vec<ContrastEntry>,
    pub status_surfaces: Vec<ContrastEntry>,
    pub low_contrast: bool,
}

pub fn build_report(scheme: &ColorScheme, tokens: &ThemeTokens) -> ContrastReport;
```

Detalhes de `build_report`:
- `foreground_background`: par `"foreground/background"`, `low_contrast = ratio < 4.5`.
- `ansi`: 16 entradas `"ansi[0]/background"` … `"ansi[15]/background"`.
- `scales`: 5 entradas `"--max-<escala>-500/--max-<escala>-content"` para `primary`, `success`, `warning`, `danger`, `info`.
- `interaction`:
  1. `"cursor/background"` (`scheme.cursor` vs `scheme.background`)
  2. `"cursorAccent/cursor"` (`scheme.cursor_accent` vs `scheme.cursor`)
  3. `"selection/selectionForeground"` (`scheme.selection_background.composite_over(scheme.background)` vs `scheme.selection_foreground.unwrap_or(scheme.foreground)`)
  4. `"selectionInactive/selectionForeground"` (seleção com `a = scheme.selection_background.a / 2` composta sobre `scheme.background` vs `scheme.selection_foreground.unwrap_or(scheme.foreground)`).
- `status_surfaces`: 5 entradas `"--max-<escala>-500/--background-0"` para `primary`, `success`, `warning`, `danger`, `info`.
- Na serialização JSON de `ContrastEntry.ratio`, arredondar para 2 casas decimais (`(ratio * 100.0).round() / 100.0`), enquanto `contrast_ratio(a, b)` retorna o `f64` exato sem arredondamento (CA-12).

### 3.7. Modelo Canônico (`src-tauri/crates/mt-themes/src/scheme.rs`)

```rust
use crate::color::{Rgb, Rgba};
use crate::contrast::relative_luminance;
use crate::ids::ContentHash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ANSI_NAMES: [&str; 16] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    "brightBlack", "brightRed", "brightGreen", "brightYellow", "brightBlue", "brightMagenta", "brightCyan", "brightWhite",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnsiPalette(pub [Rgb; 16]);

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ColorScheme {
    pub name: String,
    #[specta(type = String)]
    pub foreground: Rgb,
    #[specta(type = String)]
    pub background: Rgb,
    #[specta(type = String)]
    pub cursor: Rgb,
    #[specta(type = String)]
    pub cursor_accent: Rgb,
    #[specta(type = String)]
    pub selection_background: Rgba,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional, type = Option<String>)]
    pub selection_foreground: Option<Rgb>,
    #[specta(type = Vec<String>)]
    pub ansi: AnsiPalette,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeAppearance {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeOrigin {
    Builtin,
    TabbyConfig,
    TabbyScheme,
    XtermJson,
    WindowsTerminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeWarningCode {
    AlphaIgnored,
    PaletteExpanded,
    PaletteTruncated,
    LowContrast,
    ScaleBaseAdjusted,
    UnknownFieldsIgnored,
    NameMissing,
    NameTruncated,
    CursorDefaulted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeWarning {
    pub code: ThemeWarningCode,
    pub field: Option<String>,
    pub message: String,
}

impl ColorScheme {
    pub fn appearance(&self) -> ThemeAppearance {
        if relative_luminance(self.background) <= 0.179 {
            ThemeAppearance::Dark
        } else {
            ThemeAppearance::Light
        }
    }

    /// Calcula SHA-256 hex do JSON canônico das cores normalizadas (exclui `name`, ordena chaves).
    pub fn content_hash(&self) -> ContentHash {
        let canonical = serde_json::json!({
            "ansi": self.ansi.0.iter().map(|c| c.to_hex()).collect::<Vec<_>>(),
            "background": self.background.to_hex(),
            "cursor": self.cursor.to_hex(),
            "cursorAccent": self.cursor_accent.to_hex(),
            "foreground": self.foreground.to_hex(),
            "selectionBackground": self.selection_background.to_hex(),
            "selectionForeground": self.selection_foreground.map(|c| c.to_hex()),
        });
        let bytes = serde_json::to_vec(&canonical).expect("serialização JSON canônica infalível");
        let digest = Sha256::digest(&bytes);
        ContentHash(format!("{digest:x}"))
    }
}
```

### 3.8. Escalas e Rampa de Background (`src-tauri/crates/mt-themes/src/scale.rs`)

```rust
use crate::color::Rgb;
use crate::contrast::contrast_ratio;
use crate::oklch::{oklch_to_rgb_clamped, rgb_to_oklch, Oklch};
use crate::scheme::ThemeAppearance;

pub const SCALE_STEPS: [u16; 11] = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950];
pub const SEMANTIC_SCALES: [&str; 5] = ["primary", "success", "warning", "danger", "info"];
pub const PRIMARY_ANSI_INDEX: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scale {
    pub steps: [(u16, Rgb); 11],
    pub content: Rgb,
    pub base_adjusted: bool,
}

pub fn generate_scale(base: Rgb) -> Scale;
pub fn background_ramp(bg: Rgb, appearance: ThemeAppearance) -> Vec<(u16, Rgb)>;
pub fn pick_content(c500: Rgb, c950: Rgb) -> Rgb;
```

Regras de `generate_scale` e `background_ramp`:
- Em `generate_scale(base)`:
  - `b = rgb_to_oklch(base)`. Se `b.l < 0.45` ou `b.l > 0.75`, faz `b.l = b.l.clamp(0.45, 0.75)` e `base_adjusted = true`; caso contrário `base_adjusted = false` e o passo `500` preserva exatamente `base` (CA-13).
  - Passos claros `400, 300, 200, 100, 50`: `t = [0.18, 0.36, 0.56, 0.76, 0.88]`, `k = [0.92, 0.80, 0.62, 0.42, 0.28]`, `l = b.l + (0.985 - b.l) * t[i]`, `c = b.c * k[i]`.
  - Passos escuros `600, 700, 800, 900, 950`: `t = [0.16, 0.34, 0.52, 0.70, 0.82]`, `k = [0.95, 0.88, 0.78, 0.66, 0.58]`, `l = b.l - (b.l - 0.13) * t[i]`, `c = b.c * k[i]`.
  - Caso o arredondamento de 8 bits produza dois passos vizinhos com `l_recalculado` fora de ordem estrita, ajusta-se `l` em incrementos mínimos de `0.004` antes de `oklch_to_rgb_clamped` para garantir monotonicidade estrita `L(50) > L(100) > ... > L(950)` após conversão para 8 bits (CA-14).
- Em `pick_content(c500, c950)`:
  - Se `contrast_ratio(c500, Rgb::WHITE) >= 4.5`, retorna `Rgb::WHITE`.
  - Senão, se `contrast_ratio(c500, c950) >= 4.5`, retorna `c950`.
  - Senão, retorna `Rgb::BLACK`.
- Em `background_ramp(bg, appearance)`:
  - Gera os 39 passos `s = 0, 25, 50, ..., 950`.
  - Passo `0` é sempre `bg` exato (CA-15).
  - Para `ThemeAppearance::Dark`: `l(s) = l0 + (1.0 - l0) * (s as f64 / 850.0).min(1.0)` (850..950 saturam em `#ffffff`).
  - Para `ThemeAppearance::Light`: `l(s) = (l0 - (l0 - 0.16) * (s as f64 / 900.0)).max(0.12)`.
  - Croma `c(s) = c0 * (1.0 - 0.85 * (s as f64 / 950.0))`, exceto no passo `s == 500`, onde `c(500) = 0.0` (neutro acromático para o badge Parado, conforme `spec.md` §3.8).

### 3.9. Mapeamento xterm e Tokens CSS (`src-tauri/crates/mt-themes/src/mapping/`)

#### `mapping/xterm.rs`
```rust
use crate::color::{parse_color, Rgb, Rgba};
use crate::error::ThemeError;
use crate::scheme::{AnsiPalette, ColorScheme};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct XtermTheme {
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    pub cursor_accent: String,
    pub selection_background: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub selection_foreground: Option<String>,
    pub selection_inactive_background: String,
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_magenta: String,
    pub bright_cyan: String,
    pub bright_white: String,
}

impl From<&ColorScheme> for XtermTheme;
impl TryFrom<&XtermTheme> for ColorScheme {
    type Error = ThemeError;
}
```
- `selection_inactive_background` recebe `Rgba { rgb: scheme.selection_background.rgb, a: scheme.selection_background.a / 2 }.to_hex()`.

#### `mapping/tokens.rs`
```rust
use crate::color::Rgb;
use crate::error::ThemeError;
use crate::scheme::{ColorScheme, ThemeAppearance, ThemeWarning};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeTokens {
    pub appearance: ThemeAppearance,
    pub vars: BTreeMap<String, String>,
}

pub type TokenOverrides = BTreeMap<String, Rgb>;

pub const MAPPING_VERSION: u32 = 1;
pub const CANONICAL_TOKENS_VERSION: u32 = 1;

pub fn allowed_token_keys() -> &'static BTreeSet<String>;
pub fn is_allowed_token_key(key: &str) -> bool;
pub fn tokens_from_scheme(
    scheme: &ColorScheme,
    overrides: Option<&TokenOverrides>,
) -> Result<(ThemeTokens, Vec<ThemeWarning>), ThemeError>;
pub fn scheme_from_tokens(name: &str, tokens: &ThemeTokens) -> Result<ColorScheme, ThemeError>;
```
- `tokens_from_scheme` produz exatamente as **102 chaves** permitidas:
  - 39 chaves `--background-0`, `--background-25`, ..., `--background-950`.
  - 60 chaves das 5 escalas (`primary`, `success`, `warning`, `danger`, `info`): 11 passos (`50..950`) + `-content` por escala.
  - 3 chaves de terminal: `--mt-terminal-background`, `--mt-terminal-foreground`, `--mt-terminal-selection`.
- A escala `primary` é derivada de `scheme.ansi.0[6]` (`cyan`) via `generate_scale`. Se `base_adjusted == true`, adiciona `ThemeWarning { code: ThemeWarningCode::ScaleBaseAdjusted, field: Some("--max-primary-500".into()), message: "Luminosidade da cor primária ajustada para preservar legibilidade da escala.".into() }`.
- As escalas `success`, `warning`, `danger` e `info` são copiadas integralmente das tabelas canônicas do `@maxvue/max-components-ui` para o modo correspondente (`Dark` ou `Light`), protegendo o significado semântico dos badges (CA-13).
- Se `overrides` for fornecido (exclusivo para builtins), valida cada chave contra `is_allowed_token_key(key)` (retornando `ThemeError::InvalidToken` se fora das 102 chaves) e sobrescreve o valor correspondente em `vars`.

### 3.10. Temas Builtins (`src-tauri/crates/mt-themes/src/builtin/`)

```rust
use crate::color::{parse_color, Rgb};
use crate::error::ThemeError;
use crate::mapping::tokens::{is_allowed_token_key, TokenOverrides};
use crate::scheme::{ColorScheme, ThemeOrigin, ThemeWarning};
use serde::Deserialize;
use std::collections::BTreeMap;

pub struct BuiltinTheme {
    pub slug: &'static str,
    pub source: &'static str,
}

pub const BUILTINS: [BuiltinTheme; 6] = [
    BuiltinTheme { slug: "max-navy-teal", source: include_str!("max-navy-teal.yaml") },
    BuiltinTheme { slug: "max-light", source: include_str!("max-light.yaml") },
    BuiltinTheme { slug: "dracula", source: include_str!("dracula.yaml") },
    BuiltinTheme { slug: "nord", source: include_str!("nord.yaml") },
    BuiltinTheme { slug: "solarized-dark", source: include_str!("solarized-dark.yaml") },
    BuiltinTheme { slug: "solarized-light", source: include_str!("solarized-light.yaml") },
];

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuiltinDocument {
    pub slug: String,
    pub builtin_version: u32,
    pub name: String,
    pub license: String,
    pub scheme: serde_json::Value,
    #[serde(default)]
    pub token_overrides: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct LoadedBuiltin {
    pub doc: BuiltinDocument,
    pub scheme: ColorScheme,
    pub warnings: Vec<ThemeWarning>,
    pub overrides: TokenOverrides,
}

pub fn load_builtins() -> Result<Vec<LoadedBuiltin>, ThemeError>;
```

---

## 4. Regras de Negócio, Segurança e Casos de Borda

1. **Proibição de Strings Cruas (R-08)**: Nenhuma cor vinda de YAML/JSON ou `TokenOverrides` chega a `ThemeTokens` ou `XtermTheme` sem passar por `parse_color` e ser convertida em `Rgb`/`Rgba`.
2. **Invariante do Tema Padrão (CA-16, R-04)**: `ThemeId::builtin("max-navy-teal")` deve ser igual a `DEFAULT_THEME_ID` (`b5907b11-f15c-5886-b803-f1fb94bc4c9d`), e `tokens_from_scheme` com os `tokenOverrides` de `max-navy-teal.yaml` deve produzir valores 100% idênticos a `tests/fixtures/max-components-ui/dark-tokens.json` (`--max-primary-500 = #00768e`, `--max-primary-950 = #00152a`, `--background-0 = #17293d`, etc.).
3. **Proteção Semântica dos Estados (CA-13, D-07)**: Mesmo que um tema importado inverta `red` e `green` na paleta ANSI (ex.: `ansi[1] = #00ff00` e `ansi[2] = #ff0000`), `XtermTheme` preserva essas cores para o terminal, mas `ThemeTokens` mantém `--max-success-*`, `--max-warning-*`, `--max-danger-*` e `--max-info-*` inalterados conforme a fixture Max do modo efetivo.
4. **Linting Estrito**: Respeitar `unsafe_code = "forbid"` e `unwrap_used = "deny"` do workspace em todo o código de produção de `mt-themes`.
