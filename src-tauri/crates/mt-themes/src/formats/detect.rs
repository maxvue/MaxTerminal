//! Heurística determinística de auto-detecção entre os 5 formatos suportados.

use serde_json::Value as JsonValue;

use crate::error::ThemeError;
use crate::formats::{SchemeFileFormat, validate_safe_yaml};

/// Detecta automaticamente o `SchemeFileFormat` de um conteúdo textual.
pub fn detect_format(
    content: &str,
    filename: Option<&str>,
) -> Result<SchemeFileFormat, ThemeError> {
    let trimmed = content.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        return Err(ThemeError::UnsupportedFormat {
            reason: "Arquivo vazio".to_string(),
        });
    }

    let is_json_ext = filename
        .and_then(|f| f.rsplit('.').next())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"));

    if trimmed.starts_with('{') || trimmed.starts_with('[') || is_json_ext {
        if let Ok(json) = serde_json::from_str::<JsonValue>(trimmed) {
            return classify_json_value(&json);
        }
        if is_json_ext {
            return Err(ThemeError::UnsupportedFormat {
                reason: "JSON sintaticamente inválido".to_string(),
            });
        }
    }

    validate_safe_yaml(trimmed)?;
    let yaml_val: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(trimmed).map_err(|e| ThemeError::UnsupportedFormat {
            reason: format!("YAML inválido: {e}"),
        })?;

    if let Some(map) = yaml_val.as_mapping() {
        if map.contains_key(serde_yaml_ng::Value::String("terminal".to_string())) {
            return Ok(SchemeFileFormat::TabbyConfigYaml);
        }
        if map.contains_key(serde_yaml_ng::Value::String("schemes".to_string()))
            || map.contains_key(serde_yaml_ng::Value::String("colors".to_string()))
            || map.contains_key(serde_yaml_ng::Value::String("fg".to_string()))
            || map.contains_key(serde_yaml_ng::Value::String("foreground".to_string()))
        {
            return Ok(SchemeFileFormat::TabbySchemeYaml);
        }
    } else if yaml_val.as_sequence().is_some() {
        return Ok(SchemeFileFormat::TabbySchemeYaml);
    }

    Err(ThemeError::UnsupportedFormat {
        reason: "Estrutura YAML não reconhecida como esquema Tabby".to_string(),
    })
}

fn classify_json_value(val: &JsonValue) -> Result<SchemeFileFormat, ThemeError> {
    let sample = match val {
        JsonValue::Array(arr) => arr.first().ok_or(ThemeError::NoSchemesFound)?,
        JsonValue::Object(map) => {
            if let Some(JsonValue::Array(schemes)) = map.get("schemes") {
                let first = schemes.first().ok_or(ThemeError::NoSchemesFound)?;
                if first.get("colors").is_some() || first.get("fg").is_some() {
                    return Ok(SchemeFileFormat::TabbySchemeJson);
                }
                return Ok(SchemeFileFormat::WindowsTerminalJson);
            }
            val
        }
        _ => {
            return Err(ThemeError::UnsupportedFormat {
                reason: "Raiz JSON deve ser um objeto ou lista de objetos".to_string(),
            });
        }
    };

    let Some(obj) = sample.as_object() else {
        return Err(ThemeError::UnsupportedFormat {
            reason: "Item JSON não é um objeto".to_string(),
        });
    };

    if obj.contains_key("colors") || obj.contains_key("fg") || obj.contains_key("bg") {
        return Ok(SchemeFileFormat::TabbySchemeJson);
    }

    if obj.contains_key("purple")
        || obj.contains_key("brightPurple")
        || obj.contains_key("cursorColor")
    {
        return Ok(SchemeFileFormat::WindowsTerminalJson);
    }

    if obj.contains_key("magenta")
        || obj.contains_key("brightMagenta")
        || obj.contains_key("cursorAccent")
    {
        if obj.contains_key("name") && !obj.contains_key("cursorAccent") {
            return Ok(SchemeFileFormat::WindowsTerminalJson);
        }
        return Ok(SchemeFileFormat::XtermThemeJson);
    }

    Err(ThemeError::UnsupportedFormat {
        reason: "Objeto JSON não corresponde a Tabby, Windows Terminal ou Xterm ITheme".to_string(),
    })
}
