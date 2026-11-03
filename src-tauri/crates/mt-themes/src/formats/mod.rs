//! Formatos brutos intermediários, pré-validador de segurança YAML e roteamento para os 5 formatos suportados.

pub mod detect;
pub mod tabby_config;
pub mod tabby_scheme;
pub mod windows_terminal;
pub mod xterm_json;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;
use yaml_rust2::scanner::{Scanner, TokenType};

use crate::error::ThemeError;

/// Formato de arquivo de esquema de cores detectado ou solicitado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SchemeFileFormat {
    /// Arquivo `config.yaml` completo do Tabby Terminal.
    TabbyConfigYaml,
    /// Arquivo YAML avulso contendo um esquema (ou lista de esquemas) do Tabby.
    TabbySchemeYaml,
    /// Arquivo JSON avulso contendo um esquema (ou lista de esquemas) do Tabby.
    TabbySchemeJson,
    /// Arquivo JSON no formato Windows Terminal / iTerm2-Color-Schemes (`background`, `foreground`, `black`..`brightWhite`).
    WindowsTerminalJson,
    /// Arquivo JSON no formato `ITheme` do `@xterm/xterm`.
    XtermThemeJson,
}

impl SchemeFileFormat {
    /// Identificador canônico em `kebab-case` gravado em `ThemeMetadata.source_format`.
    #[must_use]
    pub const fn as_metadata_str(self) -> &'static str {
        match self {
            Self::TabbyConfigYaml => "tabby-config-yaml",
            Self::TabbySchemeYaml => "tabby-scheme-yaml",
            Self::TabbySchemeJson => "tabby-scheme-json",
            Self::WindowsTerminalJson => "windows-terminal-json",
            Self::XtermThemeJson => "xterm-theme-json",
        }
    }
}

/// Representação intermediária bruta extraída de qualquer um dos 5 formatos suportados,
/// antes da validação de cores, normalização de nomes e preenchimento de defaults.
#[derive(Debug, Clone, Default)]
pub struct RawColorScheme {
    /// Nome bruto do esquema (`name`).
    pub name: String,
    /// Autor opcional (`author`).
    pub author: Option<String>,
    /// Cor de texto principal (`foreground` / `fg`).
    pub foreground: String,
    /// Cor de fundo principal (`background` / `bg`).
    pub background: String,
    /// Cor do cursor (`cursor` / `cursorColor`).
    pub cursor: Option<String>,
    /// Cor do texto sob o cursor (`cursorAccent`).
    pub cursor_accent: Option<String>,
    /// Estilo do cursor (`cursorStyle`).
    pub cursor_style: Option<String>,
    /// Cor de fundo da seleção (`selection` / `selectionBackground`).
    pub selection_background: Option<String>,
    /// Cor de texto da seleção (`selectionForeground`).
    pub selection_foreground: Option<String>,
    /// Cor de destaque da aba ativa (`activeTabAccent`).
    pub active_tab_accent: Option<String>,
    /// Paleta ANSI bruta de 8 ou 16 cores.
    pub colors: Vec<String>,
    /// Cores indexadas extras (`16..=255`).
    pub indexed_colors: BTreeMap<u8, String>,
    /// Formato do qual o esquema foi extraído.
    pub source_format: Option<SchemeFileFormat>,
}

/// Pré-valida um documento YAML com o scanner léxico `yaml-rust2`, rejeitando:
/// - Âncoras (`&anchor`)
/// - Aliases (`*alias`)
/// - Merge keys (`<<`)
/// - Tags YAML explícitas (`!tag`, `!!type`)
/// - Mais de 3 documentos YAML (`---`) no mesmo arquivo
/// - Profundidade de aninhamento superior a 16 níveis
pub fn validate_safe_yaml(content: &str) -> Result<(), ThemeError> {
    let mut scanner = Scanner::new(content.chars());
    let mut doc_count = 0usize;
    let mut depth = 0usize;

    for token in &mut scanner {
        let mark = token.0;
        let line = mark.line() + 1;
        let column = mark.col() + 1;

        match token.1 {
            TokenType::Anchor(name) => {
                return Err(ThemeError::UnsafeYamlConstruct {
                    line,
                    column,
                    construct: format!("anchor (&{name})"),
                });
            }
            TokenType::Alias(name) => {
                return Err(ThemeError::UnsafeYamlConstruct {
                    line,
                    column,
                    construct: format!("alias (*{name})"),
                });
            }
            TokenType::Tag(handle, suffix) => {
                return Err(ThemeError::UnsafeYamlConstruct {
                    line,
                    column,
                    construct: format!("tag ({handle}{suffix})"),
                });
            }
            TokenType::DocumentStart => {
                doc_count += 1;
                if doc_count > 3 {
                    return Err(ThemeError::UnsafeYamlConstruct {
                        line,
                        column,
                        construct: "mais de 3 documentos YAML (---) no mesmo arquivo".to_string(),
                    });
                }
            }
            TokenType::BlockMappingStart
            | TokenType::BlockSequenceStart
            | TokenType::FlowMappingStart
            | TokenType::FlowSequenceStart => {
                depth += 1;
                if depth > 16 {
                    return Err(ThemeError::UnsafeYamlConstruct {
                        line,
                        column,
                        construct: "profundidade máxima de aninhamento (16) excedida".to_string(),
                    });
                }
            }
            TokenType::BlockEnd | TokenType::FlowMappingEnd | TokenType::FlowSequenceEnd => {
                depth = depth.saturating_sub(1);
            }
            TokenType::Scalar(_, ref val) if val == "<<" => {
                return Err(ThemeError::UnsafeYamlConstruct {
                    line,
                    column,
                    construct: "merge key (<<)".to_string(),
                });
            }
            _ => {}
        }
    }

    if let Some(err) = scanner.get_error() {
        return Err(ThemeError::UnsupportedFormat {
            reason: format!("Erro léxico YAML: {err}"),
        });
    }

    Ok(())
}

/// Faz o parsing de um arquivo nos 5 formatos suportados (detectando automaticamente quando `hint` é `None`).
pub fn parse_schemes_by_format(
    content: &str,
    filename: Option<&str>,
    hint: Option<SchemeFileFormat>,
) -> Result<(SchemeFileFormat, Vec<RawColorScheme>), ThemeError> {
    let format = match hint {
        Some(f) => f,
        None => detect::detect_format(content, filename)?,
    };

    let mut schemes = match format {
        SchemeFileFormat::TabbyConfigYaml => tabby_config::parse_tabby_config_yaml(content)?,
        SchemeFileFormat::TabbySchemeYaml => tabby_scheme::parse_tabby_scheme_yaml(content)?,
        SchemeFileFormat::TabbySchemeJson => tabby_scheme::parse_tabby_scheme_json(content)?,
        SchemeFileFormat::WindowsTerminalJson => {
            windows_terminal::parse_windows_terminal_json(content)?
        }
        SchemeFileFormat::XtermThemeJson => xterm_json::parse_xterm_theme_json(content)?,
    };

    for s in &mut schemes {
        s.source_format = Some(format);
    }

    if schemes.is_empty() {
        return Err(ThemeError::NoSchemesFound);
    }

    Ok((format, schemes))
}
