//! Parser resiliente da saída JSON de `php artisan list --format=json` (Symfony Console `JsonDescriptor`).

use std::collections::HashSet;
use std::fmt;

use indexmap::IndexMap;
use mt_core::MtResult;
use mt_db::sanitizer::sanitize;
use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::artisan::model::{ArtisanArgument, ArtisanCommand, ArtisanOption};
use crate::error;

#[derive(Deserialize)]
struct RawList {
    #[serde(default)]
    commands: Vec<RawCommand>,
}

#[derive(Deserialize)]
struct RawCommand {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    definition: Option<RawDefinition>,
}

#[derive(Default, Deserialize)]
struct RawDefinition {
    #[serde(default, deserialize_with = "map_or_empty_array")]
    arguments: IndexMap<String, RawArgument>,
    #[serde(default, deserialize_with = "map_or_empty_array")]
    options: IndexMap<String, RawOption>,
}

#[derive(Deserialize)]
struct RawArgument {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    is_required: Option<bool>,
    #[serde(default)]
    is_array: Option<bool>,
    #[serde(default)]
    default: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct RawOption {
    #[serde(default)]
    name: String,
    #[serde(default)]
    shortcut: String,
    #[serde(default)]
    accept_value: Option<bool>,
    #[serde(default)]
    is_value_required: Option<bool>,
    #[serde(default)]
    is_multiple: Option<bool>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    default: Option<serde_json::Value>,
}

fn map_or_empty_array<'de, D, V>(deserializer: D) -> Result<IndexMap<String, V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    struct MapOrEmptySeqVisitor<V>(std::marker::PhantomData<V>);

    impl<'de, V: Deserialize<'de>> Visitor<'de> for MapOrEmptySeqVisitor<V> {
        type Value = IndexMap<String, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("um objeto JSON ou um array vazio []")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            while (seq.next_element::<de::IgnoredAny>()?).is_some() {}
            Ok(IndexMap::new())
        }

        fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut map = IndexMap::with_capacity(access.size_hint().unwrap_or(0));
            while let Some((key, value)) = access.next_entry()? {
                map.insert(key, value);
            }
            Ok(map)
        }
    }

    deserializer.deserialize_any(MapOrEmptySeqVisitor(std::marker::PhantomData))
}

/// Verifica se um identificador de comando, argumento ou opção é seguro (sem controles ou metacaracteres shell).
#[must_use]
pub fn is_safe_command_token(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 128
        && trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'-' | b'_' | b'.'))
}

/// Faz o parse dos bytes de `php artisan list --format=json`, ignorando avisos anteriores ao JSON.
pub fn parse_artisan_list(bytes: &[u8]) -> MtResult<Vec<ArtisanCommand>> {
    let start = find_json_object_start(bytes)
        .ok_or_else(|| error::invalid_json("objeto JSON inicial '{' não encontrado"))?;

    let raw: RawList =
        serde_json::from_slice(&bytes[start..]).map_err(|e| error::invalid_json(e.to_string()))?;

    let visible: Vec<RawCommand> = raw
        .commands
        .into_iter()
        .filter(|c| !c.hidden && is_safe_command_token(&c.name))
        .collect();

    let globals = intersect_option_keys(&visible);
    let mut out: Vec<ArtisanCommand> = visible
        .into_iter()
        .map(|c| to_command(c, &globals))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn find_json_object_start(bytes: &[u8]) -> Option<usize> {
    for (idx, &b) in bytes.iter().enumerate() {
        if b == b'{' && serde_json::from_slice::<RawList>(&bytes[idx..]).is_ok() {
            return Some(idx);
        }
    }
    bytes.iter().position(|&b| b == b'{')
}

fn intersect_option_keys(commands: &[RawCommand]) -> HashSet<String> {
    if commands.is_empty() {
        return HashSet::new();
    }
    if commands.len() == 1 {
        return [
            "--help",
            "--quiet",
            "--verbose",
            "--version",
            "--ansi",
            "--no-ansi",
            "--no-interaction",
            "--env",
        ]
        .into_iter()
        .map(String::from)
        .collect();
    }

    let mut iter = commands.iter();
    let Some(first) = iter.next() else {
        return HashSet::new();
    };
    let mut common: HashSet<String> = first
        .definition
        .as_ref()
        .map(|d| {
            d.options
                .iter()
                .map(|(k, v)| normalize_option_name(if v.name.is_empty() { k } else { &v.name }))
                .collect()
        })
        .unwrap_or_default();

    for cmd in iter {
        let keys: HashSet<String> = cmd
            .definition
            .as_ref()
            .map(|d| {
                d.options
                    .iter()
                    .map(|(k, v)| {
                        normalize_option_name(if v.name.is_empty() { k } else { &v.name })
                    })
                    .collect()
            })
            .unwrap_or_default();
        common.retain(|k| keys.contains(k));
    }
    common
}

fn normalize_option_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("--") {
        trimmed.to_string()
    } else {
        format!("--{}", trimmed.trim_start_matches('-'))
    }
}

fn clean_default(val: Option<serde_json::Value>) -> Option<serde_json::Value> {
    match val {
        None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false)) => None,
        Some(serde_json::Value::Array(ref arr)) if arr.is_empty() => None,
        Some(other) => Some(other),
    }
}

fn clean_text(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(sanitize(trimmed).into_owned())
    }
}

fn to_command(raw: RawCommand, globals: &HashSet<String>) -> ArtisanCommand {
    let def = raw.definition.unwrap_or_default();

    let arguments = def
        .arguments
        .into_iter()
        .filter_map(|(key, arg)| {
            let name = if arg.name.trim().is_empty() {
                key
            } else {
                arg.name.trim().to_string()
            };
            if !is_safe_command_token(&name) {
                return None;
            }
            Some(ArtisanArgument {
                name,
                description: clean_text(&arg.description),
                required: arg.is_required,
                is_array: arg.is_array,
                default: clean_default(arg.default),
            })
        })
        .collect();

    let options = def
        .options
        .into_iter()
        .filter_map(|(key, opt)| {
            let base_name = if opt.name.trim().is_empty() {
                &key
            } else {
                opt.name.trim()
            };
            let normalized = normalize_option_name(base_name);
            let bare = normalized.trim_start_matches('-');
            if !is_safe_command_token(bare) {
                return None;
            }
            let is_global = if globals.contains(&normalized) {
                Some(true)
            } else {
                None
            };
            let shortcut = {
                let s = opt.shortcut.trim();
                if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                }
            };
            Some(ArtisanOption {
                name: normalized,
                description: clean_text(&opt.description),
                shortcut,
                accept_value: opt.accept_value,
                is_value_required: opt.is_value_required,
                is_multiple: opt.is_multiple,
                default: clean_default(opt.default),
                is_global,
            })
        })
        .collect();

    let aliases = raw
        .aliases
        .into_iter()
        .filter(|a| is_safe_command_token(a))
        .collect();

    ArtisanCommand {
        name: raw.name.trim().to_string(),
        description: clean_text(&raw.description),
        options,
        arguments,
        aliases,
    }
}
