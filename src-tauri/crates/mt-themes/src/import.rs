//! Importador seguro de arquivos e diretórios com proteção `O_NOFOLLOW`, limites de tamanho/contagem
//! e políticas de resolução de colisão (`OverwritePolicy`).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use specta::Type;
use walkdir::WalkDir;

use crate::error::ThemeError;
use crate::formats::{SchemeFileFormat, parse_schemes_by_format};
use crate::ids::{ThemeId, ThemeOrigin, ThemePolarity};
use crate::normalize::{ImportWarning, normalize_raw_scheme};
use crate::scheme::ColorScheme;

/// Limite máximo de tamanho de um único arquivo de esquema (512 KiB).
pub const MAX_THEME_FILE_BYTES: u64 = 512 * 1024;
/// Limite máximo de esquemas extraídos de um único arquivo.
pub const MAX_SCHEMES_PER_FILE: usize = 64;
/// Limite máximo de arquivos inspecionados em uma importação de diretório.
pub const MAX_FILES_PER_DIR_IMPORT: usize = 256;
/// Profundidade máxima de recursão ao varrer diretórios.
pub const MAX_DIR_DEPTH: usize = 3;

/// Política de resolução de colisão de ID/nome durante a importação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum OverwritePolicy {
    /// Gera sufixo determinístico `-<short_hash>` quando o slug colide com tema diferente;
    /// caso o SHA-256 seja idêntico, ignora como duplicata.
    #[default]
    RenameWithSuffix,
    /// Sobrescreve temas `imported`/`custom` existentes (nunca sobrescreve `builtin`).
    OverwriteImported,
    /// Ignora silenciosamente esquemas conflitantes registrando-os em `skipped`.
    SkipExisting,
    /// Aborta a importação do esquema retornando `ThemeError::Conflict`.
    Reject,
}

/// Opções de importação de arquivo de tema.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportFileOptions {
    /// Dica opcional de formato; se `None`, usa auto-detecção.
    pub format_hint: Option<SchemeFileFormat>,
    /// Se `true`, apenas valida e calcula o relatório sem persistir no SQLite.
    #[serde(default)]
    pub dry_run: bool,
    /// Política de resolução de conflitos.
    #[serde(default)]
    pub overwrite_policy: OverwritePolicy,
    /// Se `true` e exatamente 1 esquema for importado (e `!dry_run`), ativa-o imediatamente.
    #[serde(default)]
    pub activate_if_single: bool,
}

/// Opções de importação de diretório de temas.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportDirOptions {
    /// Se `true`, apenas simula e retorna o relatório sem gravar no banco.
    #[serde(default)]
    pub dry_run: bool,
    /// Política de resolução de conflitos.
    #[serde(default)]
    pub overwrite_policy: OverwritePolicy,
}

/// Motivo pelo qual um esquema foi ignorado durante a importação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Conteúdo idêntico (mesmo SHA-256) já existe no catálogo.
    IdenticalAlreadyExists,
    /// ID/nome já existe e `OverwritePolicy::SkipExisting` estava ativa.
    ExistingIdProtected,
    /// Colisão com tema `builtin` imutável quando `OverwriteImported` estava ativa.
    BuiltinProtected,
}

/// Item ignorado durante a importação.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SkippedEntry {
    /// Nome do esquema ou arquivo ignorado.
    pub name: String,
    /// ID existente no catálogo.
    pub existing_id: ThemeId,
    /// Motivo estruturado.
    pub reason: SkipReason,
}

/// Erro isolado ocorrido em um arquivo ou esquema específico durante importação em lote.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    /// Nome do arquivo ou esquema onde ocorreu o erro.
    pub source: String,
    /// Erro detalhado.
    pub error: ThemeError,
}

/// Resumo leve de um tema para listagens na UI e relatórios de importação.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeSummary {
    /// Identificador único.
    pub id: ThemeId,
    /// Nome exibido.
    pub name: String,
    /// Origem (`builtin`, `imported`, `custom`).
    pub origin: ThemeOrigin,
    /// Polaridade (`dark`, `light`).
    pub polarity: ThemePolarity,
    /// Cor de fundo principal (`#rrggbb`).
    pub background: String,
    /// Cor de texto principal (`#rrggbb`).
    pub foreground: String,
    /// Cor do cursor (`#rrggbb`).
    pub cursor: String,
    /// Amostra das 8 cores ANSI base (`0..=7`) para renderização de swatches.
    pub accent_swatches: [String; 8],
    /// Autor opcional.
    pub author: Option<String>,
    /// Formato de origem opcional.
    pub source_format: Option<String>,
}

impl ThemeSummary {
    /// Constrói um `ThemeSummary` a partir de um `ColorScheme`.
    #[must_use]
    pub fn from_scheme(s: &ColorScheme) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            origin: s.origin,
            polarity: s.polarity,
            background: s.background.to_hex_string(),
            foreground: s.foreground.to_hex_string(),
            cursor: s.cursor.to_hex_string(),
            accent_swatches: [
                s.ansi.black.to_hex_string(),
                s.ansi.red.to_hex_string(),
                s.ansi.green.to_hex_string(),
                s.ansi.yellow.to_hex_string(),
                s.ansi.blue.to_hex_string(),
                s.ansi.magenta.to_hex_string(),
                s.ansi.cyan.to_hex_string(),
                s.ansi.white.to_hex_string(),
            ],
            author: s.metadata.author.clone(),
            source_format: s.metadata.source_format.clone(),
        }
    }
}

/// Relatório detalhado retornado por `theme_import_file` e `theme_import_dir`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// Esquemas importados (ou validados em `dry_run`).
    pub imported: Vec<ThemeSummary>,
    /// Esquemas ignorados por duplicação ou política.
    pub skipped: Vec<SkippedEntry>,
    /// Avisos não fatais emitidos durante normalização.
    pub warnings: Vec<ImportWarning>,
    /// Falhas isoladas por arquivo/esquema.
    pub errors: Vec<ImportFailure>,
}

/// Resultado interno da leitura e normalização de um arquivo antes da resolução de conflitos no banco.
#[derive(Debug)]
pub struct ParsedFileBatch {
    /// Esquemas normalizados prontos para deduplicação/persistência.
    pub schemes: Vec<ColorScheme>,
    /// Avisos coletados.
    pub warnings: Vec<ImportWarning>,
    /// Falhas isoladas em esquemas individuais do arquivo.
    pub errors: Vec<ImportFailure>,
}

/// Lê um arquivo de tema de forma segura (rejeitando symlinks via `O_NOFOLLOW` no Linux,
/// FIFOs, sockets, devices e arquivos maiores que `MAX_THEME_FILE_BYTES`).
pub fn read_safe_theme_file(path: &Path) -> Result<String, ThemeError> {
    let symlink_meta = fs::symlink_metadata(path).map_err(|e| ThemeError::Io {
        message: format!(
            "Não foi possível inspecionar '{}': {e}",
            display_basename(path)
        ),
    })?;

    if symlink_meta.file_type().is_symlink() {
        return Err(ThemeError::UnsafePath {
            reason: format!(
                "Links simbólicos não são permitidos na importação de temas ({})",
                display_basename(path)
            ),
        });
    }

    if !symlink_meta.file_type().is_file() {
        return Err(ThemeError::UnsafePath {
            reason: format!(
                "O caminho informado não é um arquivo regular ({})",
                display_basename(path)
            ),
        });
    }

    if symlink_meta.len() > MAX_THEME_FILE_BYTES {
        return Err(ThemeError::FileTooLarge {
            actual_bytes: symlink_meta.len(),
            max_bytes: MAX_THEME_FILE_BYTES,
        });
    }

    let file = open_regular_file_nofollow(path)?;
    let mut reader = file.take(MAX_THEME_FILE_BYTES + 1);
    let mut buf = Vec::with_capacity(usize::try_from(symlink_meta.len()).unwrap_or(4096));
    reader.read_to_end(&mut buf)?;

    if (buf.len() as u64) > MAX_THEME_FILE_BYTES {
        return Err(ThemeError::FileTooLarge {
            actual_bytes: buf.len() as u64,
            max_bytes: MAX_THEME_FILE_BYTES,
        });
    }

    String::from_utf8(buf).map_err(|_| ThemeError::UnsupportedFormat {
        reason: "Arquivo não está codificado em UTF-8 válido".to_string(),
    })
}

#[cfg(unix)]
#[allow(clippy::cast_possible_wrap)]
fn open_regular_file_nofollow(path: &Path) -> Result<fs::File, ThemeError> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)
        .map_err(|e| ThemeError::UnsafePath {
            reason: format!(
                "Falha ao abrir arquivo com O_NOFOLLOW ({}): {e}",
                display_basename(path)
            ),
        })
}

#[cfg(not(unix))]
fn open_regular_file_nofollow(path: &Path) -> Result<fs::File, ThemeError> {
    fs::File::open(path).map_err(ThemeError::from)
}

/// Faz o parsing e normalização de todos os esquemas contidos em um arquivo no disco.
pub fn parse_and_normalize_file(
    path: &Path,
    format_hint: Option<SchemeFileFormat>,
    now_ms: i64,
) -> Result<ParsedFileBatch, ThemeError> {
    let content = read_safe_theme_file(path)?;
    let basename = display_basename(path);
    let (_fmt, raws) = parse_schemes_by_format(&content, Some(&basename), format_hint)?;

    if raws.len() > MAX_SCHEMES_PER_FILE {
        return Err(ThemeError::InvalidField {
            field: "schemes".to_string(),
            reason: format!(
                "Arquivo contém {} esquemas, acima do limite de {MAX_SCHEMES_PER_FILE}",
                raws.len()
            ),
        });
    }

    let mut schemes = Vec::new();
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for (idx, raw) in raws.into_iter().enumerate() {
        let label = if raw.name.trim().is_empty() {
            format!("{basename}#{idx}")
        } else {
            format!("{basename}: {}", raw.name.trim())
        };
        match normalize_raw_scheme(
            raw,
            ThemeOrigin::Imported,
            None,
            Some(basename.clone()),
            now_ms,
        ) {
            Ok(norm) => {
                schemes.push(norm.scheme);
                warnings.extend(norm.warnings);
            }
            Err(err) => {
                errors.push(ImportFailure {
                    source: label,
                    error: err,
                });
            }
        }
    }

    if schemes.is_empty() && !errors.is_empty() && errors.len() == 1 {
        return Err(errors.remove(0).error);
    }

    Ok(ParsedFileBatch {
        schemes,
        warnings,
        errors,
    })
}

/// Coleta até `MAX_FILES_PER_DIR_IMPORT` arquivos candidatos (`.yaml`, `.yml`, `.json`) em `dir`
/// com profundidade máxima `MAX_DIR_DEPTH`, sem seguir links simbólicos.
pub fn collect_candidate_theme_files(dir: &Path) -> Result<Vec<PathBuf>, ThemeError> {
    let meta = fs::symlink_metadata(dir).map_err(|e| ThemeError::Io {
        message: format!("Não foi possível acessar o diretório: {e}"),
    })?;
    if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
        return Err(ThemeError::UnsafePath {
            reason: "O diretório de importação deve ser uma pasta real (não symlink)".to_string(),
        });
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(dir)
        .follow_links(false)
        .max_depth(MAX_DIR_DEPTH)
        .sort_by_file_name()
    {
        let Ok(entry) = entry else {
            continue;
        };
        if entry.path() == dir {
            continue;
        }
        if entry.file_type().is_symlink() || !entry.file_type().is_file() {
            continue;
        }
        let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !(ext.eq_ignore_ascii_case("yaml")
            || ext.eq_ignore_ascii_case("yml")
            || ext.eq_ignore_ascii_case("json"))
        {
            continue;
        }
        files.push(entry.into_path());
        if files.len() >= MAX_FILES_PER_DIR_IMPORT {
            break;
        }
    }

    Ok(files)
}

fn display_basename(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("theme")
        .to_string()
}
