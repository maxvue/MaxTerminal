# Implementação 02 — Parsers Tabby, Segurança YAML/JSON, Normalização, Importador e Exportador (`spec.md`)

> Fase 10 — Temas e Importador Tabby
> Referências: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§3.9, §3.10, §3.14, §3.15, §11, §12.1).
> Pré-requisito: Implementação 01 concluída.

---

## 1. Contexto, Objetivo e Escopo

Esta segunda implementação constrói o pipeline completo de entrada/saída de arquivos de temas do `mt-themes`, com foco em segurança contra arquivos maliciosos ou malformados, compatibilidade com variações reais do Tabby Terminal e operações de sistema de arquivos seguras no Linux:

1. **Scanner Sintático Limitado YAML e Visitor JSON (`formats/mod.rs`)**: validação estrutural antes de materializar `serde_json::Value` — tamanho ≤ 1 MiB (`take`), UTF-8 estrito (BOM opcional), profundidade ≤ 32, ≤ 65.536 nós, escalares ≤ 16 KiB, rejeição imediata de âncoras (`&`), aliases (`*`), merge keys (`<<`), tags customizadas YAML, múltiplos documentos e chaves duplicadas em um mesmo mapa, limitando a 256 candidatos por arquivo.
2. **Detecção e Parsers de Formatos (`formats/{detect.rs,tabby_config.rs,tabby_scheme.rs,windows_terminal.rs,xterm_json.rs}`)**: arquitetura extensível via trait `SchemeParser` e `ParserRegistry`, suportando `config.yaml` do Tabby (`colorScheme`, `lightColorScheme`, `customColorSchemes[]`, inclusive seleção por nome e mapas nomeados `ANSI_NAMES`), esquemas Tabby individuais ou em coleção, Windows Terminal (`purple`/`brightPurple`, `cursorColor`, `selectionBackground`, `{ "schemes": [...] }`) e `ITheme` do xterm.js.
3. **Normalização de Esquemas (`normalize.rs`)**: conversão de `RawScheme` em `NormalizedScheme` com caminhos de campo exatos nos erros (`terminal.customColorSchemes[1].colors[3]`), avisos de alfa ignorado fora de seleção (`alpha_ignored`), expansão de paletas de 8 e 9–15 cores para 16 (`palette_expanded`), truncamento > 16 (`palette_truncated`), sanitização de nome (`name_missing`, `name_truncated`), default de cursor (`cursor_defaulted`) e aviso de baixo contraste (`low_contrast`).
4. **Importador de Arquivo e Diretório (`import.rs`)**: abertura segura no Linux via `rustix::fs::openat` com `OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK` e verificação `fstat` de arquivo regular (rejeitando symlinks, FIFOs, sockets e dispositivos), `parse_file` e `scan_dir` com `ImportLimits` (profundidade 4, 500 arquivos, 64 MiB acumulados, 10.000 entradas visitadas, 2.000 esquemas/lote e teto de 500 detalhes com contadores exatos).
5. **Exportador (`export.rs`)**: renderização nos 4 formatos (`TabbyYaml`, `TabbyJson`, `XtermJson`, `CssTokens`) e escrita atômica `write_atomic` via arquivo temporário `.tmp-<uuid>` com permissão `0644` e `rename`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Modificados
- `src-tauri/crates/mt-themes/src/lib.rs` — exposição dos módulos `formats`, `normalize`, `import`, `export`.
- `src-tauri/crates/mt-themes/src/builtin/mod.rs` — uso de `TabbySchemeParser` e `normalize` no `load_builtins()`.
- `src-tauri/crates/mt-themes/tests/mapping_roundtrip.rs` — adição do teste de roundtrip Tabby YAML/JSON (preservando nome, cores e `ContentHash`).
- `src-tauri/crates/mt-themes/tests/scale_snapshots.rs` — inclusão das 20 fixtures de esquemas Tabby nos testes de monotonicidade e snapshots.

### 2.2. Arquivos Criados
- `src-tauri/crates/mt-themes/src/formats/mod.rs`
- `src-tauri/crates/mt-themes/src/formats/detect.rs`
- `src-tauri/crates/mt-themes/src/formats/tabby_config.rs`
- `src-tauri/crates/mt-themes/src/formats/tabby_scheme.rs`
- `src-tauri/crates/mt-themes/src/formats/windows_terminal.rs`
- `src-tauri/crates/mt-themes/src/formats/xterm_json.rs`
- `src-tauri/crates/mt-themes/src/normalize.rs`
- `src-tauri/crates/mt-themes/src/import.rs`
- `src-tauri/crates/mt-themes/src/export.rs`
- `src-tauri/crates/mt-themes/tests/import_tabby.rs`
- `src-tauri/crates/mt-themes/tests/fixtures/tabby/config.yaml`
- `src-tauri/crates/mt-themes/tests/fixtures/tabby/config-light.yaml`
- `src-tauri/crates/mt-themes/tests/fixtures/tabby/config-by-name.yaml`
- `src-tauri/crates/mt-themes/tests/fixtures/tabby/config-with-secrets.yaml`
- `src-tauri/crates/mt-themes/tests/fixtures/tabby/schemes/` (20 esquemas MIT/domínio público em `.yaml` e `.json`, incluindo mapa nomeado, paleta de 8 cores, paleta de 12 cores e paleta monocromática/invertida)
- `src-tauri/crates/mt-themes/tests/fixtures/xterm/itheme.json`
- `src-tauri/crates/mt-themes/tests/fixtures/windows-terminal/campbell.json`
- `src-tauri/crates/mt-themes/tests/fixtures/windows-terminal/settings.json`
- `src-tauri/crates/mt-themes/tests/fixtures/invalid/{bad-color.yaml,short-palette.yaml,billion-laughs.yaml,duplicate-keys.yaml,deep-nesting.yaml,custom-tag.yaml,arbitrary.json}`

---

## 3. Especificação Técnica e Assinaturas Rust

### 3.1. Scanner Estrutural e `ParserRegistry` (`src-tauri/crates/mt-themes/src/formats/mod.rs`)

```rust
use crate::error::ThemeError;
use crate::scheme::ThemeOrigin;

pub mod detect;
pub mod tabby_config;
pub mod tabby_scheme;
pub mod windows_terminal;
pub mod xterm_json;

pub const MAX_DOC_DEPTH: usize = 32;
pub const MAX_DOC_NODES: usize = 65_536;
pub const MAX_SCALAR_BYTES: usize = 16 * 1024;
pub const MAX_CANDIDATES_PER_FILE: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawScheme {
    pub name: Option<String>,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub cursor: Option<String>,
    pub cursor_accent: Option<String>,
    pub selection: Option<String>,
    pub selection_foreground: Option<String>,
    pub colors: Vec<String>,
    pub origin: ThemeOrigin,
    pub field_prefix: String,
    pub unknown_fields: Vec<String>,
}

pub trait SchemeParser: Send + Sync {
    fn id(&self) -> ThemeOrigin;
    fn detect(&self, doc: &serde_json::Value) -> bool;
    fn parse(
        &self,
        doc: &serde_json::Value,
        file_stem: &str,
    ) -> Result<Vec<Result<RawScheme, ThemeError>>, ThemeError>;
}

pub struct ParserRegistry {
    parsers: Vec<Box<dyn SchemeParser>>,
}

impl ParserRegistry {
    pub fn default_set() -> Self;
    pub fn register(&mut self, parser: Box<dyn SchemeParser>);
    pub fn detect(&self, doc: &serde_json::Value) -> Option<&dyn SchemeParser>;
    pub fn parse_document(
        &self,
        doc: &serde_json::Value,
        file_stem: &str,
    ) -> Result<(ThemeOrigin, Vec<Result<RawScheme, ThemeError>>), ThemeError>;
}

/// Converte texto YAML ou JSON em `serde_json::Value` aplicando limites estruturais ANTES de materializar a árvore.
pub fn parse_structured_document(content: &str, is_json_hint: bool) -> Result<serde_json::Value, ThemeError>;
```

Regras de `parse_structured_document`:
- Remove BOM UTF-8 (`\u{FEFF}`) inicial se presente.
- Para YAML (via `yaml_rust2::parser::Parser` de eventos):
  - Conta profundidade de `SequenceStart`/`MappingStart` (erro `ParseFailed` se `> 32`).
  - Conta total de eventos de valor/coleção (erro `ParseFailed` se `> 65_536`).
  - Rejeita `Event::Alias` e qualquer `Anchor` (`usize` / `Option<NonZeroUsize>` presente em `Scalar`, `SequenceStart` ou `MappingStart`) imediatamente com `ThemeError::ParseFailed { reason: "âncoras e aliases YAML não são permitidos".into() }`.
  - Rejeita tags customizadas (`Tag` diferente das tags padrão YAML `!!str`, `!!int`, `!!float`, `!!bool`, `!!null`, `!!seq`, `!!map`) e chave de merge `<<`.
  - Rejeita múltiplos documentos (`DocumentStart` > 1).
  - Rejeita escalares com `.len() > 16_384` bytes.
  - Em cada mapa, rastreia as chaves em um `HashSet<String>` e rejeita chaves duplicadas com `ThemeError::ParseFailed { reason: format!("chave duplicada: {key}") }`.
  - Constrói o `serde_json::Value` incrementalmente a partir da pilha de eventos validados.
- Para JSON (via `serde::de::Deserializer` customizado / `serde_json::Deserializer::from_str` com `Visitor` recursivo limitado):
  - Mesmos limites de profundidade (`<= 32`), nós (`<= 65_536`), strings (`<= 16_384` bytes) e rejeição de chaves duplicadas em objetos antes de inserir no `serde_json::Map`.

### 3.2. Regras dos 4 Parsers (`formats/*.rs`)

1. **`TabbyConfigParser` (`tabby_config.rs`)**:
   - `detect`: `doc` é um objeto contendo chave `"terminal"` cujo valor é um objeto contendo ao menos uma das chaves `"colorScheme"`, `"lightColorScheme"` ou `"customColorSchemes"`. Outras seções do `config.yaml` (`ssh`, `Hotkeys`, `vault`, etc.) são completamente ignoradas e nunca lidas/logadas (CA-35).
   - `parse`:
     - Primeiro processa `terminal.customColorSchemes` (se presente; deve ser array com `<= MAX_CANDIDATES_PER_FILE` itens). Cada item `i` usa `field_prefix = format!("terminal.customColorSchemes[{i}].")`.
     - Em seguida processa `terminal.colorScheme` (`field_prefix = "terminal.colorScheme."`) e `terminal.lightColorScheme` (`field_prefix = "terminal.lightColorScheme."`):
       - Se `null` ou ausente: ignora.
       - Se objeto: extrai `RawScheme`; caso `name` esteja ausente/vazio, usa `format!("{file_stem} — esquema ativo")` (ou `"— esquema claro"` para `lightColorScheme`).
       - Se string (referência por nome): busca correspondência exata de `name` entre os esquemas válidos extraídos de `terminal.customColorSchemes`. Se não encontrar nenhum, retorna `Err(ThemeError::ParseFailed { reason: ... })` naquele item; se encontrar múltiplos com cores diferentes, retorna erro de referência ambígua; se encontrar, emite o `RawScheme` correspondente (que será contabilizado como duplicata na normalização/importação se o custom já tiver sido incluído).
2. **`TabbySchemeParser` (`tabby_scheme.rs`)**:
   - `detect`: objeto contendo `"colors"` (array ou objeto) e (`"foreground"` ou `"background"`), **ou** array não vazio cujo primeiro elemento case com essa forma.
   - Suporta `colors` como array de strings ou como mapa objeto contendo exatamente as 16 chaves de `ANSI_NAMES` (ordenadas na ordem canônica de `ANSI_NAMES`, nunca na ordem do mapa; se faltar alguma das 16 chaves no mapa ou houver chave estranha, retorna `Err(ThemeError::InvalidPalette { found })` ou `InvalidColor`).
   - Aceita `selection` e `selectionBackground` como aliases; se ambos estiverem presentes e resultarem em cores normalizadas diferentes, retorna `Err(ThemeError::InvalidColor { field: format!("{prefix}selectionBackground"), ... })`.
3. **`WindowsTerminalParser` (`windows_terminal.rs`)**:
   - `detect`: objeto contendo (`"purple"` e `"brightPurple"`) ou ((`"cursorColor"` ou `"selectionBackground"`) e `"brightBlack"`), ou `{ "schemes": [ ... ] }` onde o primeiro item case.
   - Mapeia `purple` → `colors[5]` (`magenta`), `brightPurple` → `colors[13]` (`brightMagenta`), `cursorColor` → `cursor`, `selectionBackground` → `selection`.
4. **`XtermJsonParser` (`xterm_json.rs`)**:
   - `detect`: objeto contendo as 16 chaves de `ANSI_NAMES` (incluindo `"magenta"` e `"brightMagenta"`).
   - Mapeia 1:1 para `RawScheme`; se `extendedAnsi` ou outras chaves desconhecidas existirem, adiciona a `unknown_fields` (gerando aviso `unknown_fields_ignored`).

### 3.3. Normalização (`src-tauri/crates/mt-themes/src/normalize.rs`)

```rust
use crate::error::ThemeError;
use crate::formats::RawScheme;
use crate::scheme::{ColorScheme, ThemeOrigin, ThemeWarning};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedScheme {
    pub scheme: ColorScheme,
    pub origin: ThemeOrigin,
    pub warnings: Vec<ThemeWarning>,
}

pub fn normalize(raw: RawScheme) -> Result<NormalizedScheme, ThemeError>;
```

Sequência exata de `normalize`:
1. `foreground` ausente → `Err(ThemeError::InvalidScheme { field: "foreground" })`; `background` ausente → `Err(ThemeError::InvalidScheme { field: "background" })`.
2. `parse_color` em `foreground` e `background` com caminho `format!("{prefix}foreground")` / `format!("{prefix}background")`. Se `alpha.is_some()`, emite aviso `ThemeWarningCode::AlphaIgnored`.
3. `cursor`: se ausente, usa `foreground` e emite `ThemeWarningCode::CursorDefaulted` (`field = Some(format!("{prefix}cursor"))`). Se presente, valida com `parse_color` (alfa ignorado com `AlphaIgnored`).
4. `cursor_accent`: se ausente, usa `background` (sem aviso); se presente, valida com `parse_color` (alfa ignorado com `AlphaIgnored`).
5. `selection`: se ausente, usa `Rgba { rgb: foreground, a: 0x4d }`; se presente, valida com `parse_color` preservando `alpha.unwrap_or(0xff)`.
6. `selection_foreground`: se `Some(s)` não vazio, valida com `parse_color` (alfa ignorado com `AlphaIgnored`).
7. `colors`:
   - Se `raw.colors.len() < 8`, retorna `Err(ThemeError::InvalidPalette { found: raw.colors.len() })`.
   - Faz parse das primeiras `min(raw.colors.len(), 16)` cores; erro no índice `i` reporta `field = format!("{prefix}colors[{i}]")` (CA-07). Se alguma tiver alfa, emite `AlphaIgnored` para `colors[i]`.
   - Se `len == 8..=15`, expande os índices faltantes `i ∈ len..16` a partir de `normal = colors[i - 8]`:
     - Para `i == 8` (`brightBlack`): `mix_rgb_oklch(colors[0], foreground, 0.35)`.
     - Para `i ∈ 9..16`: `oklch_to_rgb_clamped(Oklch { l: (b.l + 0.08).min(0.95), c: b.c * 1.05, h: b.h })`.
     - Emite aviso `ThemeWarningCode::PaletteExpanded` (CA-08).
   - Se `len > 16`, trunca em 16 e emite `ThemeWarningCode::PaletteTruncated` (CA-08).
8. `name`: remove caracteres de controle (`c.is_control()`), aplica `.trim()`. Se vazio, usa `"Tema importado"` e emite `ThemeWarningCode::NameMissing`. Se `.chars().count() > 120`, trunca em 120 caracteres e emite `ThemeWarningCode::NameTruncated`.
9. Se `!raw.unknown_fields.is_empty()`, emite `ThemeWarningCode::UnknownFieldsIgnored`.
10. Se `contrast_ratio(foreground, background) < 4.5`, emite `ThemeWarningCode::LowContrast`.

### 3.4. Importação Segura (`src-tauri/crates/mt-themes/src/import.rs`)

```rust
use crate::error::ThemeError;
use crate::formats::ParserRegistry;
use crate::ids::ThemeId;
use crate::normalize::NormalizedScheme;
use crate::scheme::ThemeWarning;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ImportLimits {
    pub max_file_bytes: u64,
    pub max_depth: usize,
    pub max_files: usize,
    pub max_total_bytes: u64,
    pub max_entries: usize,
    pub max_schemes: usize,
    pub max_details: usize,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 1_048_576,      // 1 MiB
            max_depth: 4,
            max_files: 500,
            max_total_bytes: 64 * 1_048_576, // 64 MiB
            max_entries: 10_000,
            max_schemes: 2_000,
            max_details: 500,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    pub file: String,
    pub code: String,
    pub message: String,
    pub field: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportDuplicate {
    pub file: String,
    pub name: String,
    pub existing_id: ThemeId,
    pub existing_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportWarning {
    pub file: String,
    pub theme_name: String,
    pub warning: ThemeWarning,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportLimitHit {
    pub code: String,
    pub limit: u64,
}

pub struct ParsedFile {
    pub file: String,
    pub bytes_read: u64,
    pub schemes: Vec<NormalizedScheme>,
    pub failures: Vec<ImportFailure>,
}

pub struct DirScanResult {
    pub files: Vec<(PathBuf, String)>, // (caminho completo, caminho relativo à raiz)
    pub scanned_files: u32,
    pub skipped_files: u32,
    pub limits_hit: Vec<ImportLimitHit>,
}

pub fn read_regular_file_bounded(path: &Path, max_bytes: u64) -> Result<String, ThemeError>;
pub fn parse_file(
    path: &Path,
    display_name: &str,
    limits: &ImportLimits,
    reg: &ParserRegistry,
) -> Result<ParsedFile, ThemeError>;
pub fn scan_dir(root: &Path, limits: &ImportLimits) -> Result<DirScanResult, ThemeError>;
```

Segurança de `read_regular_file_bounded`:
- Usa `rustix::fs::openat(CWD, path, OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK, Mode::empty())`.
- Se falhar com `ELOOP`, retorna `ThemeError::PathInvalid { reason: "links simbólicos não são permitidos" }`.
- Chama `rustix::fs::fstat(&fd)`: verifica se `(stat.st_mode & S_IFMT) == S_IFREG` (rejeita diretórios, FIFOs, sockets, devices com `ThemeError::PathInvalid { reason: "o caminho não é um arquivo regular" }`).
- Se `stat.st_size as u64 > max_bytes`, retorna `Err(ThemeError::FileTooLarge { limit: max_bytes })` sem ler o arquivo (CA-09).
- Converte o `OwnedFd` em `std::fs::File`, lê com `.take(max_bytes + 1)` para um `Vec<u8>`; se `buf.len() as u64 > max_bytes`, retorna `Err(ThemeError::FileTooLarge { limit: max_bytes })`.
- Converte para UTF-8 com `String::from_utf8(buf)` (erro → `ThemeError::ParseFailed { reason: "arquivo não está em UTF-8 válido".into() }`).

### 3.5. Exportação (`src-tauri/crates/mt-themes/src/export.rs`)

```rust
use crate::error::ThemeError;
use crate::mapping::tokens::ThemeTokens;
use crate::mapping::xterm::XtermTheme;
use crate::scheme::ColorScheme;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeExportFormat {
    TabbyYaml,
    TabbyJson,
    XtermJson,
    CssTokens,
}

impl ThemeExportFormat {
    pub fn allowed_extensions(self) -> &'static [&'static str] {
        match self {
            Self::TabbyYaml => &["yaml", "yml"],
            Self::TabbyJson | Self::XtermJson => &["json"],
            Self::CssTokens => &["css"],
        }
    }

    pub fn default_extension(self) -> &'static str {
        match self {
            Self::TabbyYaml => "yaml",
            Self::TabbyJson | Self::XtermJson => "json",
            Self::CssTokens => "css",
        }
    }
}

pub fn render_scheme(
    scheme: &ColorScheme,
    xterm: &XtermTheme,
    tokens: &ThemeTokens,
    format: ThemeExportFormat,
) -> Result<String, ThemeError>;

pub fn write_atomic(dest: &Path, format: ThemeExportFormat, content: &str) -> Result<PathBuf, ThemeError>;
```

---

## 4. Regras de Negócio, Segurança e Casos de Borda

1. **Zero Vazamento de Segredos de `config.yaml` (CA-35)**: O `config.yaml` do Tabby frequentemente contém chaves SSH, senhas ou tokens em outras seções (`ssh`, `vault`, `profiles`). `TabbyConfigParser` lê exclusivamente `doc["terminal"]["colorScheme" | "lightColorScheme" | "customColorSchemes"]`, e `ThemeError` nunca inclui o documento bruto na mensagem.
2. **Defesa contra YAML Billion Laughs e Deep Nesting (CA-09, R-02)**: O scanner de eventos `yaml-rust2` interrompe no primeiro evento `Alias` ou `Anchor`, sem depender de contagem textual de `&`/`*` (que quebraria cores ou strings legítimas contendo `&` ou `*`).
3. **Proteção contra Varredura Infinita e FIFOs (CA-22, R-09)**: `scan_dir` não segue symlinks (`follow_links(false)`), ignora diretórios ocultos (`.git`, `.worktrees`, `.max-code-worktrees`) e `node_modules`, contabiliza cada entrada visitada em `max_entries` (10.000) e abre arquivos com `O_NOFOLLOW | O_NONBLOCK` + checagem `S_IFREG`.
4. **Fidelidade de Roundtrip Tabby (CA-11, CA-23)**: `TabbyYaml` e `TabbyJson` exportam `name`, `foreground`, `background`, `cursor`, `cursorAccent`, `selection` (com alfa se `< 255`), `selectionForeground` e os 16 itens de `colors`, garantindo que reimportar o arquivo exportado gere o mesmo `ContentHash`.
