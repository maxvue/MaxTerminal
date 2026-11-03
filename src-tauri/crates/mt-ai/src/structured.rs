//! Motor de Geração Assistida Estruturada (`StructuredGenerator`) com validação JSON Schema local,
//! bloqueio de `$ref` externos e até 1 tentativa automática de reparo (`CA-27`, `CA-36`).

use crate::AiService;
use crate::client::dialect::ChatRequest;
use crate::client::http::{Op, now_iso8601};
use crate::config::AiSelection;
use crate::error::{ai_error, ai_error_with_details, codes};
use crate::ids::AiRequestId;
use crate::prompts::structured::{build_repair_messages, build_structured_messages};
use crate::repo::{AiHistoryItem, AiHistoryKind};
use crate::suggest::extract_strict_json_object;
use mt_core::ids::{PaneId, ProjectId};
use mt_core::{ErrorCode, MtError, MtResult};
use parking_lot::RwLock;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Tamanho máximo em bytes de um JSON Schema aceito (`32 KiB`).
pub const MAX_SCHEMA_BYTES: usize = 32 * 1024;
/// Profundidade máxima permitida na árvore de um JSON Schema (`16`).
pub const MAX_SCHEMA_DEPTH: usize = 16;
/// Limite máximo de caracteres no prompt de geração estruturada (`8000`).
pub const MAX_STRUCTURED_PROMPT_CHARS: usize = 8000;
/// Quantidade máxima de schemas compilados mantidos no cache em memória (`64`).
pub const MAX_CACHED_VALIDATORS: usize = 64;
/// Identificadores de schemas builtin consumidos pelas Fases 14 e 15 (D-05 / C-05 / C-06).
pub const BUILTIN_SCHEMA_IDS: [&str; 2] = ["artisan.command.v1", "command-group.suggestion.v1"];

#[allow(clippy::expect_used)]
static SCHEMA_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z][a-z0-9]*([.-][a-z0-9]+)*\.v[0-9]+$").expect("regex schema_id válida")
});

/// Requisição IPC para geração assistida estruturada (`ai_generate_structured`).
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StructuredRequest {
    /// Identificador único da requisição.
    pub request_id: AiRequestId,
    /// Identificador versionado do schema (ex.: `"artisan.command.v1"`, `"command-group.suggestion.v1"`).
    pub schema_id: String,
    /// Definição JSON Schema Draft 2020-12.
    pub schema: Value,
    /// Instrução/prompt em português do Brasil.
    pub prompt: String,
    /// Instrução de sistema adicional opcional.
    pub system: Option<String>,
    /// Seleção opcional de Harness/Modelo/Raciocínio.
    pub selection: Option<AiSelection>,
    /// Painel associado opcional.
    pub pane_id: Option<PaneId>,
    /// Projeto associado opcional.
    pub project_id: Option<ProjectId>,
}

/// Resposta validada de `ai_generate_structured`.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StructuredResponse {
    /// Identificador da requisição.
    pub request_id: AiRequestId,
    /// Objeto JSON validado contra o schema e sanitizado.
    pub value: Value,
    /// Indica se foi necessária a 2ª tentativa de reparo automático (`true`).
    pub repaired: bool,
    /// Latência total observada em milissegundos.
    pub latency_ms: u32,
}

/// Validador JSON Schema pré-validado armazenado no cache em memória.
#[derive(Clone, Debug)]
pub struct CompiledSchemaValidator {
    root: Value,
}

impl CompiledSchemaValidator {
    fn compile(schema: &Value) -> MtResult<Self> {
        if !schema.is_object() {
            return Err(ai_error(
                codes::AI_SCHEMA_VALIDATION,
                "O JSON Schema fornecido deve ser um objeto JSON.",
            ));
        }
        Ok(Self {
            root: schema.clone(),
        })
    }

    fn validate(&self, instance: &Value) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        validate_node(&self.root, &self.root, instance, "$", &mut errors, 0);
        if errors.is_empty() {
            Ok(())
        } else {
            errors.truncate(10);
            Err(errors)
        }
    }
}

type ValidatorKey = (String, [u8; 32]);

/// Gerador estruturado com validação JSON Schema e reparo único (`CA-27`, `CA-36`).
pub struct StructuredGenerator {
    compiled: RwLock<HashMap<ValidatorKey, Arc<CompiledSchemaValidator>>>,
}

impl Default for StructuredGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl StructuredGenerator {
    /// Cria uma nova instância do `StructuredGenerator`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            compiled: RwLock::new(HashMap::new()),
        }
    }

    /// Valida o `schema_id` contra o formato versionado obrigatório (`^[a-z][a-z0-9]*([.-][a-z0-9]+)*\.v[0-9]+$`).
    pub fn validate_schema_id(schema_id: &str) -> MtResult<()> {
        if !SCHEMA_ID_RE.is_match(schema_id.trim()) {
            return Err(MtError::new(
                ErrorCode("core.invalid_argument"),
                format!(
                    "O identificador de schema '{schema_id}' deve seguir o padrão versionado '<dominio>.<nome>.v<N>'."
                ),
            )
            .with_details(json!({ "field": "schemaId", "schemaId": schema_id })));
        }
        Ok(())
    }

    /// Valida segurança estrutural do JSON Schema: tamanho `<= 32 KiB`, profundidade `<= 16`
    /// e ausência de `$ref` externos (`http://`, `https://`, `file://` ou caminhos relativos fora de `#`).
    pub fn validate_schema_security(schema: &Value) -> MtResult<()> {
        let serialized = serde_json::to_vec(schema).map_err(|e| {
            ai_error(
                codes::AI_SCHEMA_VALIDATION,
                format!("JSON Schema inválido: {e}"),
            )
        })?;
        if serialized.len() > MAX_SCHEMA_BYTES {
            return Err(ai_error(
                codes::AI_SCHEMA_VALIDATION,
                "O JSON Schema excede o tamanho máximo permitido de 32 KiB.",
            ));
        }
        check_schema_node(schema, 0)
    }

    /// Executa a geração estruturada com validação JSON Schema e até 1 tentativa de reparo dentro do deadline total.
    #[allow(clippy::too_many_lines)]
    pub async fn generate(
        &self,
        svc: &AiService,
        req: StructuredRequest,
        cancel: &CancellationToken,
    ) -> MtResult<StructuredResponse> {
        Self::validate_schema_id(&req.schema_id)?;
        Self::validate_schema_security(&req.schema)?;

        let prompt_trimmed = req.prompt.trim();
        if prompt_trimmed.is_empty() {
            return Err(ai_error(
                codes::AI_EMPTY_COMMAND,
                "O prompt para geração estruturada não pode ser vazio.",
            ));
        }

        let validator = self.get_or_compile(&req.schema_id, &req.schema)?;
        let settings = svc.get_settings();
        let catalog = svc.catalog_get().await;

        let raw_sel = req
            .selection
            .clone()
            .or_else(|| settings.default_selection.clone())
            .ok_or_else(|| {
                ai_error(
                    codes::AI_NO_DEFAULT_SELECTION,
                    "Configure uma seleção padrão em Preferências > IA antes de usar a geração assistida.",
                )
            })?;
        let resolved_sel = svc.catalog.resolve(&catalog, &raw_sel)?.selection;

        let sanitized_prompt: String = mt_db::sanitizer::sanitize(prompt_trimmed)
            .chars()
            .take(MAX_STRUCTURED_PROMPT_CHARS)
            .collect();
        let sanitized_sys = req
            .system
            .as_deref()
            .map(|s| mt_db::sanitizer::sanitize(s.trim()).into_owned());

        let initial_msgs = build_structured_messages(
            &req.schema_id,
            &req.schema,
            sanitized_sys.as_deref(),
            &sanitized_prompt,
        );

        let total_deadline = Duration::from_millis(u64::from(settings.timeouts.structured_ms));
        let start = Instant::now();
        let history_gen = svc.repo.current_history_generation();

        let run_fut = async {
            let mut noop_delta = |_: &str| {};
            let first_req = ChatRequest {
                selection: resolved_sel.clone(),
                messages: initial_msgs.clone(),
                json_mode: true,
                stream: false,
                max_tokens: None,
            };
            let first_resp = svc
                .client
                .chat(&first_req, Op::Structured, cancel, &mut noop_delta)
                .await?;

            match parse_and_validate(&first_resp.content, &validator) {
                Ok(valid_val) => Ok((valid_val, false)),
                Err(first_errors) => {
                    let repair_msgs =
                        build_repair_messages(&initial_msgs, &first_resp.content, &first_errors);
                    let second_req = ChatRequest {
                        selection: resolved_sel.clone(),
                        messages: repair_msgs,
                        json_mode: true,
                        stream: false,
                        max_tokens: None,
                    };
                    let second_resp = svc
                        .client
                        .chat(&second_req, Op::Structured, cancel, &mut noop_delta)
                        .await?;

                    match parse_and_validate(&second_resp.content, &validator) {
                        Ok(valid_val) => Ok((valid_val, true)),
                        Err(second_errors) => {
                            let sanitized_errors: Vec<String> = second_errors
                                .into_iter()
                                .take(10)
                                .map(|e| mt_db::sanitizer::sanitize(&e).into_owned())
                                .collect();
                            Err(ai_error_with_details(
                                codes::AI_SCHEMA_VALIDATION,
                                "A resposta da IA não segue o formato esperado.",
                                json!({
                                    "schemaId": req.schema_id,
                                    "errors": sanitized_errors,
                                }),
                            ))
                        }
                    }
                }
            }
        };

        let outcome = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                Err(ai_error(codes::AI_CANCELLED, "Geração estruturada cancelada."))
            }
            res = tokio::time::timeout(total_deadline, run_fut) => {
                match res {
                    Ok(inner) => inner,
                    Err(_) => Err(crate::error::timeout_error(settings.timeouts.structured_ms)),
                }
            }
        };

        let latency_ms = u32::try_from(start.elapsed().as_millis()).unwrap_or(u32::MAX);

        match outcome {
            Ok((raw_val, repaired)) => {
                let sanitized_val = sanitize_json_value(raw_val);
                let resp_str = serde_json::to_string(&sanitized_val).ok();
                let _ = svc
                    .repo
                    .insert_history(
                        history_gen,
                        settings.save_history,
                        AiHistoryItem {
                            id: req.request_id,
                            kind: AiHistoryKind::Structured,
                            trigger: None,
                            project_id: req.project_id,
                            tab_id: req
                                .pane_id
                                .and_then(|p| svc.context.pane_snapshot(p).map(|s| s.tab_id)),
                            pane_id: req.pane_id,
                            session_id: req
                                .pane_id
                                .and_then(|p| svc.context.pane_snapshot(p).map(|s| s.session_id)),
                            block_id: None,
                            harness_id: Some(resolved_sel.harness_id),
                            model_id: Some(resolved_sel.model_id),
                            reasoning_id: resolved_sel.reasoning_id,
                            rag_slug: None,
                            schema_id: Some(req.schema_id),
                            prompt_sanitized: sanitized_prompt,
                            response_sanitized: resp_str,
                            risk_level: None,
                            status: "completed".to_string(),
                            error_code: None,
                            latency_ms: Some(latency_ms),
                            created_at: now_iso8601(),
                        },
                    )
                    .await;

                Ok(StructuredResponse {
                    request_id: req.request_id,
                    value: sanitized_val,
                    repaired,
                    latency_ms,
                })
            }
            Err(err) => {
                let status_str = if err.code == codes::AI_CANCELLED {
                    "cancelled"
                } else {
                    "failed"
                };
                let _ = svc
                    .repo
                    .insert_history(
                        history_gen,
                        settings.save_history,
                        AiHistoryItem {
                            id: req.request_id,
                            kind: AiHistoryKind::Structured,
                            trigger: None,
                            project_id: req.project_id,
                            tab_id: req
                                .pane_id
                                .and_then(|p| svc.context.pane_snapshot(p).map(|s| s.tab_id)),
                            pane_id: req.pane_id,
                            session_id: req
                                .pane_id
                                .and_then(|p| svc.context.pane_snapshot(p).map(|s| s.session_id)),
                            block_id: None,
                            harness_id: Some(resolved_sel.harness_id),
                            model_id: Some(resolved_sel.model_id),
                            reasoning_id: resolved_sel.reasoning_id,
                            rag_slug: None,
                            schema_id: Some(req.schema_id),
                            prompt_sanitized: sanitized_prompt,
                            response_sanitized: None,
                            risk_level: None,
                            status: status_str.to_string(),
                            error_code: Some(err.code.to_string()),
                            latency_ms: Some(latency_ms),
                            created_at: now_iso8601(),
                        },
                    )
                    .await;
                Err(err)
            }
        }
    }

    fn get_or_compile(
        &self,
        schema_id: &str,
        schema: &Value,
    ) -> MtResult<Arc<CompiledSchemaValidator>> {
        let mut hasher = Sha256::new();
        hasher.update(serde_json::to_vec(schema).unwrap_or_default());
        let digest: [u8; 32] = hasher.finalize().into();
        let key = (schema_id.to_string(), digest);

        if let Some(found) = self.compiled.read().get(&key).cloned() {
            return Ok(found);
        }

        let compiled = Arc::new(CompiledSchemaValidator::compile(schema)?);
        let mut map = self.compiled.write();
        if map.len() >= MAX_CACHED_VALIDATORS
            && let Some(first_key) = map.keys().next().cloned()
        {
            map.remove(&first_key);
        }
        map.insert(key, Arc::clone(&compiled));
        Ok(compiled)
    }
}

fn check_schema_node(node: &Value, depth: usize) -> MtResult<()> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(ai_error(
            codes::AI_SCHEMA_VALIDATION,
            format!("O JSON Schema excede a profundidade máxima permitida ({MAX_SCHEMA_DEPTH})."),
        ));
    }
    match node {
        Value::Object(map) => {
            if let Some(ref_val) = map.get("$ref") {
                let Some(ref_str) = ref_val.as_str() else {
                    return Err(ai_error(
                        codes::AI_SCHEMA_VALIDATION,
                        "A propriedade '$ref' no JSON Schema deve ser uma string.",
                    ));
                };
                if !ref_str.starts_with('#') {
                    return Err(ai_error_with_details(
                        codes::AI_SCHEMA_VALIDATION,
                        "Referências externas ($ref fora do documento '#') são bloqueadas por segurança.",
                        json!({ "ref": ref_str }),
                    ));
                }
            }
            for v in map.values() {
                check_schema_node(v, depth + 1)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                check_schema_node(item, depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn parse_and_validate(
    raw_content: &str,
    validator: &CompiledSchemaValidator,
) -> Result<Value, Vec<String>> {
    let json_slice = match extract_strict_json_object(raw_content) {
        Ok(s) => s,
        Err(e) => return Err(vec![e.message.to_string()]),
    };
    let val: Value = match serde_json::from_str(json_slice) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("JSON sintaticamente inválido: {e}")]),
    };
    validator.validate(&val)?;
    Ok(val)
}

fn sanitize_json_value(val: Value) -> Value {
    match val {
        Value::String(s) => Value::String(mt_db::sanitizer::sanitize(&s).into_owned()),
        Value::Array(arr) => Value::Array(arr.into_iter().map(sanitize_json_value).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    (
                        mt_db::sanitizer::sanitize(&k).into_owned(),
                        sanitize_json_value(v),
                    )
                })
                .collect(),
        ),
        other => other,
    }
}

#[allow(clippy::too_many_lines)]
fn validate_node(
    root: &Value,
    schema: &Value,
    instance: &Value,
    path: &str,
    errors: &mut Vec<String>,
    depth: usize,
) {
    if depth > 32 {
        errors.push(format!("{path}: profundidade máxima de validação excedida"));
        return;
    }

    let Some(schema_obj) = schema.as_object() else {
        return;
    };

    // Resolução de $ref local (#/...)
    if let Some(Value::String(ref_str)) = schema_obj.get("$ref") {
        if let Some(pointer) = ref_str.strip_prefix('#') {
            if let Some(target) = root.pointer(pointer) {
                validate_node(root, target, instance, path, errors, depth + 1);
            } else {
                errors.push(format!(
                    "{path}: referência local '{ref_str}' não encontrada"
                ));
            }
        }
        return;
    }

    // Validação de const
    if let Some(const_val) = schema_obj.get("const")
        && instance != const_val
    {
        errors.push(format!("{path}: valor difere da constante esperada"));
        return;
    }

    // Validação de enum
    if let Some(Value::Array(enum_vals)) = schema_obj.get("enum")
        && !enum_vals.contains(instance)
    {
        errors.push(format!(
            "{path}: valor não pertence ao conjunto permitido (enum)"
        ));
        return;
    }

    // Validação de type (string ou array de tipos)
    if let Some(type_val) = schema_obj.get("type") {
        let matches = match type_val {
            Value::String(t) => matches_json_type(instance, t),
            Value::Array(types) => types
                .iter()
                .filter_map(Value::as_str)
                .any(|t| matches_json_type(instance, t)),
            _ => true,
        };
        if !matches {
            errors.push(format!(
                "{path}: tipo inválido (esperado {type_val}, recebido {})",
                json_type_name(instance)
            ));
            return;
        }
    }

    // Validações específicas para String
    if let Some(s) = instance.as_str() {
        let char_count = s.chars().count();
        if let Some(min_len) = schema_obj.get("minLength").and_then(Value::as_u64)
            && (char_count as u64) < min_len
        {
            errors.push(format!(
                "{path}: comprimento ({char_count}) menor que minLength ({min_len})"
            ));
        }
        if let Some(max_len) = schema_obj.get("maxLength").and_then(Value::as_u64)
            && (char_count as u64) > max_len
        {
            errors.push(format!(
                "{path}: comprimento ({char_count}) maior que maxLength ({max_len})"
            ));
        }
        if let Some(pat) = schema_obj.get("pattern").and_then(Value::as_str)
            && let Ok(re) = Regex::new(pat)
            && !re.is_match(s)
        {
            errors.push(format!("{path}: string não casa com o padrão '{pat}'"));
        }
    }

    // Validações específicas para Number / Integer
    if let Some(num) = instance.as_f64() {
        if let Some(min) = schema_obj.get("minimum").and_then(Value::as_f64)
            && num < min
        {
            errors.push(format!("{path}: valor ({num}) menor que minimum ({min})"));
        }
        if let Some(max) = schema_obj.get("maximum").and_then(Value::as_f64)
            && num > max
        {
            errors.push(format!("{path}: valor ({num}) maior que maximum ({max})"));
        }
    }

    // Validações específicas para Array
    if let Some(arr) = instance.as_array() {
        if let Some(min_items) = schema_obj.get("minItems").and_then(Value::as_u64)
            && (arr.len() as u64) < min_items
        {
            errors.push(format!(
                "{path}: array possui {} itens, mínimo exigido é {min_items}",
                arr.len()
            ));
        }
        if let Some(max_items) = schema_obj.get("maxItems").and_then(Value::as_u64)
            && (arr.len() as u64) > max_items
        {
            errors.push(format!(
                "{path}: array possui {} itens, máximo permitido é {max_items}",
                arr.len()
            ));
        }
        if let Some(items_schema) = schema_obj.get("items") {
            for (idx, item) in arr.iter().enumerate() {
                let child_path = format!("{path}[{idx}]");
                validate_node(root, items_schema, item, &child_path, errors, depth + 1);
            }
        }
    }

    // Validações específicas para Object
    if let Some(obj) = instance.as_object() {
        if let Some(Value::Array(req_fields)) = schema_obj.get("required") {
            for req in req_fields.iter().filter_map(Value::as_str) {
                if !obj.contains_key(req) {
                    errors.push(format!("{path}: propriedade obrigatória '{req}' ausente"));
                }
            }
        }

        let props_map = schema_obj.get("properties").and_then(Value::as_object);
        if let Some(props) = props_map {
            for (prop_name, prop_schema) in props {
                if let Some(prop_val) = obj.get(prop_name) {
                    let child_path = format!("{path}.{prop_name}");
                    validate_node(root, prop_schema, prop_val, &child_path, errors, depth + 1);
                }
            }
        }

        if let Some(add_props) = schema_obj.get("additionalProperties") {
            for (key, val) in obj {
                let is_declared = props_map.is_some_and(|p| p.contains_key(key));
                if !is_declared {
                    match add_props {
                        Value::Bool(false) => {
                            errors.push(format!(
                                "{path}: propriedade adicional não permitida '{key}'"
                            ));
                        }
                        Value::Object(_) => {
                            let child_path = format!("{path}.{key}");
                            validate_node(root, add_props, val, &child_path, errors, depth + 1);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // Combinadores allOf / anyOf / oneOf
    if let Some(Value::Array(all_of)) = schema_obj.get("allOf") {
        for sub in all_of {
            validate_node(root, sub, instance, path, errors, depth + 1);
        }
    }
    if let Some(Value::Array(any_of)) = schema_obj.get("anyOf") {
        let mut any_ok = false;
        for sub in any_of {
            let mut sub_errs = Vec::new();
            validate_node(root, sub, instance, path, &mut sub_errs, depth + 1);
            if sub_errs.is_empty() {
                any_ok = true;
                break;
            }
        }
        if !any_ok {
            errors.push(format!(
                "{path}: valor não satisfez nenhuma alternativa de anyOf"
            ));
        }
    }
    if let Some(Value::Array(one_of)) = schema_obj.get("oneOf") {
        let mut match_count = 0usize;
        for sub in one_of {
            let mut sub_errs = Vec::new();
            validate_node(root, sub, instance, path, &mut sub_errs, depth + 1);
            if sub_errs.is_empty() {
                match_count += 1;
            }
        }
        if match_count != 1 {
            errors.push(format!(
                "{path}: valor satisfez {match_count} alternativas de oneOf (esperado exatamente 1)"
            ));
        }
    }
}

fn matches_json_type(val: &Value, expected: &str) -> bool {
    match expected {
        "string" => val.is_string(),
        "number" => val.is_number(),
        "integer" => val.is_i64() || val.is_u64() || val.as_f64().is_some_and(|f| f.fract() == 0.0),
        "boolean" => val.is_boolean(),
        "array" => val.is_array(),
        "object" => val.is_object(),
        "null" => val.is_null(),
        _ => true,
    }
}

const fn json_type_name(val: &Value) -> &'static str {
    match val {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
