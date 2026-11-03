//! Validação determinística de `CommandGroupInput`, normalização Unicode NFC,
//! validação de variáveis de ambiente POSIX e bloqueio de segredos literais.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mt_core::MtResult;
use mt_db::sanitizer::sanitize;
use unicode_normalization::UnicodeNormalization;

use crate::groups::model::{
    CommandGroupInput, ExecutionMode, GroupErrorCode, GroupItemId, GroupScope, SequentialSeparator,
};
use crate::groups::resolve::CwdResolver;

/// Limite máximo de caracteres (escalares Unicode) no nome do grupo.
pub const MAX_GROUP_NAME_CHARS: usize = 80;
/// Limite máximo de caracteres na descrição do grupo.
pub const MAX_GROUP_DESC_CHARS: usize = 2048;
/// Limite máximo de itens por grupo.
pub const MAX_GROUP_ITEMS: usize = 50;
/// Limite máximo de caracteres no título de um item.
pub const MAX_ITEM_TITLE_CHARS: usize = 80;
/// Limite máximo em bytes de um comando shell.
pub const MAX_COMMAND_BYTES: usize = 8192;
/// Limite máximo de labels por grupo.
pub const MAX_LABELS: usize = 20;
/// Limite máximo de caracteres por label.
pub const MAX_LABEL_CHARS: usize = 32;
/// Limite máximo de variáveis de ambiente por nível (grupo ou item).
pub const MAX_ENV_ENTRIES_PER_LEVEL: usize = 64;
/// Limite máximo em bytes do ambiente mesclado (`grupo + item`).
pub const MAX_MERGED_ENV_BYTES: usize = 65_536;

/// Resultado validado e normalizado pronto para persistência no repositório.
#[derive(Debug, Clone)]
pub struct ValidatedGroupInput {
    /// Escopo validado.
    pub scope: GroupScope,
    /// Projeto proprietário validado.
    pub project_id: Option<mt_core::ProjectId>,
    /// Nome aparado e sanitizado.
    pub name: String,
    /// Chave normalizada Unicode NFC + lowercase.
    pub name_key: String,
    /// Descrição sanitizada.
    pub description: String,
    /// Pares `(label_key, label_sanitized)` sem duplicatas por `label_key`.
    pub labels: Vec<(String, String)>,
    /// Modo de execução padrão.
    pub mode: ExecutionMode,
    /// Separador sequencial.
    pub separator: SequentialSeparator,
    /// Diretório relativo validado no nível do grupo.
    pub cwd: Option<String>,
    /// Variáveis de ambiente validadas no nível do grupo.
    pub env: BTreeMap<String, String>,
    /// Itens validados na ordem definida.
    pub items: Vec<ValidatedGroupItemInput>,
}

/// Item validado pronto para persistência.
#[derive(Debug, Clone)]
pub struct ValidatedGroupItemInput {
    /// ID existente opcional.
    pub id: Option<GroupItemId>,
    /// Título aparado e sanitizado.
    pub title: String,
    /// Comando shell validado (sem segredos literais).
    pub command: String,
    /// Diretório relativo validado.
    pub cwd: Option<String>,
    /// Variáveis de ambiente validadas (sem segredos literais).
    pub env: BTreeMap<String, String>,
    /// Flag de execução contínua.
    pub continuous: bool,
}

/// Normaliza uma chave textual aplicando `trim`, Unicode NFC e `to_lowercase()` determinístico.
#[must_use]
pub fn normalize_key(raw: &str) -> String {
    raw.trim().nfc().collect::<String>().to_lowercase()
}

/// Verifica se uma chave de variável de ambiente obedece ao padrão POSIX `[A-Za-z_][A-Za-z0-9_]*`
/// e não utiliza o prefixo reservado `MAXTERMINAL_`.
pub fn validate_env_key(key: &str) -> Result<(), &'static str> {
    if key.is_empty() {
        return Err("A chave da variável de ambiente não pode ser vazia.");
    }
    if key.starts_with("MAXTERMINAL_") {
        return Err("O prefixo MAXTERMINAL_ é reservado pelo sistema.");
    }
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return Err("A chave da variável de ambiente não pode ser vazia.");
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(
            "A chave da variável de ambiente deve iniciar com letra ASCII ou sublinhado (_).",
        );
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(
            "A chave da variável de ambiente deve conter apenas letras ASCII, dígitos ou sublinhado (_).",
        );
    }
    Ok(())
}

/// Valida sintaticamente um caminho relativo `cwd` na etapa de edição/persistência
/// (rejeita NUL, caminhos absolutos e componentes `..` que escapem da raiz do projeto).
pub fn validate_relative_cwd_syntax(raw_cwd: &str, field_path: &str) -> MtResult<String> {
    let trimmed = raw_cwd.trim();
    if trimmed.is_empty() {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            "O diretório de trabalho relativo não pode conter apenas espaços.",
            vec![field_path.to_string()],
        ));
    }
    if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            "O diretório de trabalho contém caracteres de controle inválidos.",
            vec![field_path.to_string()],
        ));
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(GroupErrorCode::CwdOutsideProject.into_error_with_fields(
            "O diretório de trabalho deve ser relativo à pasta raiz do projeto.",
            vec![field_path.to_string()],
        ));
    }
    let mut depth: i32 = 0;
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) => depth += 1,
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(GroupErrorCode::CwdOutsideProject.into_error_with_fields(
                        "O diretório de trabalho não pode escapar da pasta raiz do projeto.",
                        vec![field_path.to_string()],
                    ));
                }
            }
            _ => {
                return Err(GroupErrorCode::CwdOutsideProject.into_error_with_fields(
                    "Caminho relativo inválido para o diretório de trabalho.",
                    vec![field_path.to_string()],
                ));
            }
        }
    }
    Ok(trimmed.to_string())
}

fn validate_env_map(
    env: &BTreeMap<String, String>,
    prefix_path: &str,
    secret_fields: &mut Vec<String>,
) -> MtResult<BTreeMap<String, String>> {
    if env.len() > MAX_ENV_ENTRIES_PER_LEVEL {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            format!(
                "O número máximo de variáveis de ambiente por nível é {MAX_ENV_ENTRIES_PER_LEVEL}."
            ),
            vec![prefix_path.to_string()],
        ));
    }

    let mut out = BTreeMap::new();
    for (k, v) in env {
        let field_path = format!("{prefix_path}.{k}");
        if let Err(reason) = validate_env_key(k) {
            return Err(GroupErrorCode::Validation.into_error_with_fields(reason, vec![field_path]));
        }
        if v.contains('\0') {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                "O valor da variável de ambiente não pode conter o caractere NUL.",
                vec![field_path],
            ));
        }
        let kv_probe = format!("{k}={v}");
        let sanitized_val = sanitize(v);
        let sanitized_kv = sanitize(&kv_probe);
        if sanitized_val.as_ref() != v.as_str() || sanitized_kv.as_ref() != kv_probe.as_str() {
            secret_fields.push(field_path);
        } else {
            out.insert(k.clone(), v.clone());
        }
    }
    Ok(out)
}

/// Valida integralmente um `CommandGroupInput`.
/// Caso `mt_db::sanitizer::sanitize()` altere qualquer comando ou valor de `env`, rejeita a operação
/// com `commands.group.secret_detected` listando todos os caminhos dos campos afetados em `details.fields`
/// sem jamais ecoar os valores secretos na mensagem ou nos detalhes.
#[allow(clippy::too_many_lines)]
pub fn validate_group_input(
    input: &CommandGroupInput,
    project_root: Option<&Path>,
) -> MtResult<ValidatedGroupInput> {
    match input.scope {
        GroupScope::Local => {
            if input.project_id.is_none() {
                return Err(GroupErrorCode::ProjectRequired.into_error_with_fields(
                    "Grupos locais exigem um projeto válido.",
                    vec!["projectId".to_string()],
                ));
            }
        }
        GroupScope::Global => {
            if input.project_id.is_some() {
                return Err(GroupErrorCode::ScopeMismatch.into_error_with_fields(
                    "Grupos globais não podem estar vinculados a um projeto específico.",
                    vec!["projectId".to_string()],
                ));
            }
        }
    }

    let trimmed_name = input.name.trim();
    let name_chars = trimmed_name.chars().count();
    if name_chars == 0 || name_chars > MAX_GROUP_NAME_CHARS {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            format!("O nome do grupo deve ter entre 1 e {MAX_GROUP_NAME_CHARS} caracteres."),
            vec!["name".to_string()],
        ));
    }
    if trimmed_name.contains('\0') || trimmed_name.contains('\n') || trimmed_name.contains('\r') {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            "O nome do grupo contém caracteres de controle inválidos.",
            vec!["name".to_string()],
        ));
    }
    let sanitized_name = sanitize(trimmed_name).into_owned();
    let name_key = normalize_key(&sanitized_name);
    if name_key.is_empty() {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            "O nome do grupo não pode ser vazio.",
            vec!["name".to_string()],
        ));
    }

    let desc_trimmed = input.description.trim();
    if desc_trimmed.chars().count() > MAX_GROUP_DESC_CHARS {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            format!("A descrição do grupo deve ter no máximo {MAX_GROUP_DESC_CHARS} caracteres."),
            vec!["description".to_string()],
        ));
    }
    let sanitized_description = sanitize(desc_trimmed).into_owned();

    if input.labels.len() > MAX_LABELS {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            format!("O grupo pode ter no máximo {MAX_LABELS} etiquetas."),
            vec!["labels".to_string()],
        ));
    }

    let mut labels = Vec::with_capacity(input.labels.len());
    let mut seen_label_keys = BTreeSet::new();
    for (idx, raw_label) in input.labels.iter().enumerate() {
        let lbl = raw_label.trim();
        let chars_count = lbl.chars().count();
        if chars_count == 0 || chars_count > MAX_LABEL_CHARS {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                format!("Cada etiqueta deve ter entre 1 e {MAX_LABEL_CHARS} caracteres."),
                vec![format!("labels[{idx}]")],
            ));
        }
        if lbl.contains('\0') || lbl.contains('\n') || lbl.contains('\r') {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                "A etiqueta contém caracteres de controle inválidos.",
                vec![format!("labels[{idx}]")],
            ));
        }
        let clean_lbl = sanitize(lbl).into_owned();
        let lbl_key = normalize_key(&clean_lbl);
        if seen_label_keys.insert(lbl_key.clone()) {
            labels.push((lbl_key, clean_lbl));
        }
    }

    let group_cwd = match &input.cwd {
        Some(raw) if !raw.trim().is_empty() => {
            let valid_rel = validate_relative_cwd_syntax(raw, "cwd")?;
            if let Some(root) = project_root {
                let _ = CwdResolver::resolve_effective_cwd(root, Some(&valid_rel), None, "cwd")?;
            }
            Some(valid_rel)
        }
        Some(_) => {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                "O diretório de trabalho relativo não pode conter apenas espaços.",
                vec!["cwd".to_string()],
            ));
        }
        None => None,
    };

    if input.items.is_empty() || input.items.len() > MAX_GROUP_ITEMS {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            format!("O grupo deve conter entre 1 e {MAX_GROUP_ITEMS} itens."),
            vec!["items".to_string()],
        ));
    }

    let mut secret_fields = Vec::new();
    let group_env = validate_env_map(&input.env, "env", &mut secret_fields)?;

    let mut validated_items = Vec::with_capacity(input.items.len());
    let mut seen_item_ids = BTreeSet::new();

    for (idx, item) in input.items.iter().enumerate() {
        if let Some(ref item_id) = item.id {
            if item_id.0.trim().is_empty() || !seen_item_ids.insert(item_id.0.clone()) {
                return Err(GroupErrorCode::Validation.into_error_with_fields(
                    "Identificador de item inválido ou duplicado.",
                    vec![format!("items[{idx}].id")],
                ));
            }
        }

        let title_trimmed = item.title.trim();
        let title_chars = title_trimmed.chars().count();
        if title_chars == 0 || title_chars > MAX_ITEM_TITLE_CHARS {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                format!("O título do item deve ter entre 1 e {MAX_ITEM_TITLE_CHARS} caracteres."),
                vec![format!("items[{idx}].title")],
            ));
        }
        if title_trimmed.contains('\0')
            || title_trimmed.contains('\n')
            || title_trimmed.contains('\r')
        {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                "O título do item contém caracteres de controle inválidos.",
                vec![format!("items[{idx}].title")],
            ));
        }
        let clean_title = sanitize(title_trimmed).into_owned();

        let cmd_trimmed = item.command.trim();
        if cmd_trimmed.is_empty() || item.command.len() > MAX_COMMAND_BYTES {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                format!("O comando deve ter entre 1 e {MAX_COMMAND_BYTES} bytes."),
                vec![format!("items[{idx}].command")],
            ));
        }
        if item.command.contains('\0') || item.command.contains('\n') || item.command.contains('\r')
        {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                "O comando de um item não pode conter quebras de linha (CR/LF) ou caractere NUL.",
                vec![format!("items[{idx}].command")],
            ));
        }

        let sanitized_cmd = sanitize(&item.command);
        if sanitized_cmd.as_ref() != item.command.as_str() {
            secret_fields.push(format!("items[{idx}].command"));
        }

        let item_cwd_field = format!("items[{idx}].cwd");
        let item_cwd = match &item.cwd {
            Some(raw) if !raw.trim().is_empty() => {
                let valid_rel = validate_relative_cwd_syntax(raw, &item_cwd_field)?;
                if let Some(root) = project_root {
                    let _ = CwdResolver::resolve_effective_cwd(
                        root,
                        group_cwd.as_deref(),
                        Some(&valid_rel),
                        &item_cwd_field,
                    )?;
                }
                Some(valid_rel)
            }
            Some(_) => {
                return Err(GroupErrorCode::Validation.into_error_with_fields(
                    "O diretório de trabalho do item não pode conter apenas espaços.",
                    vec![item_cwd_field],
                ));
            }
            None => None,
        };

        let item_env_prefix = format!("items[{idx}].env");
        let item_env = validate_env_map(&item.env, &item_env_prefix, &mut secret_fields)?;

        // Verifica o tamanho mesclado (grupo + item)
        let _ = CwdResolver::merge_env(&BTreeMap::new(), &group_env, &item_env)?;

        validated_items.push(ValidatedGroupItemInput {
            id: item.id.clone(),
            title: clean_title,
            command: item.command.clone(),
            cwd: item_cwd,
            env: item_env,
            continuous: item.continuous,
        });
    }

    if !secret_fields.is_empty() {
        return Err(GroupErrorCode::SecretDetected.into_error_with_fields(
            "Foram detectados segredos literais nos comandos ou variáveis de ambiente. Utilize referências a variáveis já exportadas no ambiente (ex.: $TOKEN).",
            secret_fields,
        ));
    }

    Ok(ValidatedGroupInput {
        scope: input.scope,
        project_id: input.project_id.clone(),
        name: sanitized_name,
        name_key,
        description: sanitized_description,
        labels,
        mode: input.mode,
        separator: input.separator,
        cwd: group_cwd,
        env: group_env,
        items: validated_items,
    })
}
