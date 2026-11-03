//! Parser e construtor estrito das URIs `maxterminal://...` e `vscode://johnattas.maxcode/...`
//! com proteção anti-command-injection, TTL de 60 s e fallback em arquivo `0600` quando o prompt excede 6 KiB.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use nix::libc;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::error::{BridgeError, BridgeErrorCode};
use crate::issues::IssueCommandPayload;
use crate::paths::{normalize_existing_project_path, validate_file_path_for_open};
use crate::protocol::{
    AgentKind, InspectCommandErrorPayload, OpenFilePayload, OpenTerminalForIssuePayload,
    PROTOCOL_VERSION, ProjectOpenedPayload, current_unix_ms,
};

/// Limite máximo total de uma URI `maxterminal://` ou `vscode://johnattas.maxcode/...` (8 192 bytes).
pub const MAX_URI_TOTAL_LEN: usize = 8192;
/// Limite máximo de prompt inline codificado em `Base64URL` dentro da URI (6 144 bytes).
pub const MAX_PROMPT_INLINE_BYTES: usize = 6144;
/// Janela máxima de validade de `ts` em uma URI (60 segundos).
pub const URI_TTL_MS: i64 = 60_000;

/// Rotas válidas recebidas pelo `MaxTerminal` via `maxterminal://...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedMaxTerminalUri {
    /// `maxterminal://open-project?v=1&req=<uuid>&ts=<ms>&path=<abs>&name=<opt>&origin=<id>`
    OpenProject {
        /// ID de correlação/idempotência.
        request_id: Uuid,
        /// Payload de abertura de projeto.
        payload: ProjectOpenedPayload,
    },
    /// `maxterminal://open-terminal?v=1&req=<uuid>&ts=<ms>&project=<abs>&worktree=<opt>&branch=<opt>&issue=<u64>&agent=<opt>`
    OpenTerminalForIssue {
        /// ID de correlação/idempotência.
        request_id: Uuid,
        /// Payload de abertura de aba para issue.
        payload: OpenTerminalForIssuePayload,
    },
    /// `maxterminal://focus?v=1&req=<uuid>&ts=<ms>`
    Focus {
        /// ID de correlação/idempotência.
        request_id: Uuid,
    },
}

/// Rotas geradas pelo `MaxTerminal` para o `MaxCode` via `vscode://johnattas.maxcode/...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedVsCodeMaxCodeUri {
    /// `vscode://johnattas.maxcode/inspect?...`
    Inspect {
        /// ID da requisição.
        request_id: Uuid,
        /// Caminho do projeto.
        project_path: String,
        /// CWD do comando.
        cwd: String,
        /// ID do bloco de comando.
        block_id: String,
        /// Código de saída.
        exit_code: i32,
        /// Modo (`"plan"`).
        mode: String,
        /// Prompt inline decodificado (quando `prompt_b64` está presente).
        prompt: Option<String>,
        /// Caminho do arquivo de artefato temporário `0600` (quando `payload_file` está presente).
        payload_file: Option<String>,
    },
    /// `vscode://johnattas.maxcode/open-file?...`
    OpenFile {
        /// ID da requisição.
        request_id: Uuid,
        /// Payload de abertura de arquivo.
        payload: OpenFilePayload,
    },
    /// `vscode://johnattas.maxcode/project-sync?...`
    ProjectSync {
        /// ID da requisição.
        request_id: Uuid,
        /// Caminho do projeto.
        project_path: String,
        /// Origem.
        origin_id: String,
    },
    /// `vscode://johnattas.maxcode/issue/<open|plan|fix>?...`
    IssueAction {
        /// ID da requisição.
        request_id: Uuid,
        /// Ação (`"open"`, `"plan"`, `"fix"`).
        action: String,
        /// Caminho do projeto.
        project_path: String,
        /// Repositório `"owner/repo"`.
        repo: String,
        /// Número da issue.
        issue_number: u64,
        /// Título opcional decodificado.
        title: Option<String>,
    },
}

/// Artefato JSON temporário gravado em `$XDG_RUNTIME_DIR/max-bridge/inspect-<uuid>.json` (`0600`)
/// quando o prompt de inspeção excede `MAX_PROMPT_INLINE_BYTES`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectFallbackArtifact {
    /// ID da requisição.
    pub request_id: Uuid,
    /// Timestamp UNIX em ms de criação.
    pub created_at_ms: i64,
    /// Payload completo de inspeção.
    pub payload: InspectCommandErrorPayload,
}

/// Analisa e valida uma URI `maxterminal://...` no instante `now_ms`.
#[allow(clippy::too_many_lines)]
pub fn parse_maxterminal_uri(
    raw_uri: &str,
    now_ms: i64,
) -> Result<ParsedMaxTerminalUri, BridgeError> {
    validate_uri_basics(raw_uri)?;
    let url = Url::parse(raw_uri).map_err(|_| {
        BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "Sintaxe de URI maxterminal:// inválida.",
        )
    })?;

    if url.scheme() != "maxterminal" {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "Esquema de URI inválido; esperado 'maxterminal://'.",
        ));
    }

    let action = url.host_str().unwrap_or_default();
    let params = extract_query_map(&url)?;
    let (request_id, _ts) = validate_common_query_params(&params, now_ms)?;

    match action {
        "open-project" => {
            ensure_allowed_query_keys(
                &params,
                &["v", "req", "ts", "path", "name", "origin", "branch"],
            )?;
            let raw_path = params.get("path").ok_or_else(|| {
                BridgeError::new(
                    BridgeErrorCode::InvalidUri,
                    "Parâmetro obrigatório 'path' ausente em maxterminal://open-project.",
                )
            })?;
            let canon = normalize_existing_project_path(raw_path)?;
            let name = params.get("name").cloned();
            let origin_id = params
                .get("origin")
                .cloned()
                .unwrap_or_else(|| "maxcode".to_string());

            Ok(ParsedMaxTerminalUri::OpenProject {
                request_id,
                payload: ProjectOpenedPayload {
                    project_path: canon.to_string_lossy().into_owned(),
                    project_name: name,
                    git_branch: params.get("branch").cloned(),
                    origin_id,
                },
            })
        }
        "open-terminal" => {
            ensure_allowed_query_keys(
                &params,
                &[
                    "v", "req", "ts", "project", "worktree", "branch", "issue", "title", "agent",
                ],
            )?;
            let raw_project = params.get("project").ok_or_else(|| {
                BridgeError::new(
                    BridgeErrorCode::InvalidUri,
                    "Parâmetro obrigatório 'project' ausente em maxterminal://open-terminal.",
                )
            })?;
            let canon_proj = normalize_existing_project_path(raw_project)?;
            let worktree_path = if let Some(wt) = params.get("worktree") {
                Some(
                    normalize_existing_project_path(wt)?
                        .to_string_lossy()
                        .into_owned(),
                )
            } else {
                None
            };
            let issue_number: u64 = params
                .get("issue")
                .ok_or_else(|| {
                    BridgeError::new(
                        BridgeErrorCode::InvalidUri,
                        "Parâmetro obrigatório 'issue' ausente em maxterminal://open-terminal.",
                    )
                })?
                .parse()
                .map_err(|_| {
                    BridgeError::new(
                        BridgeErrorCode::InvalidUri,
                        "Parâmetro 'issue' deve ser um inteiro positivo.",
                    )
                })?;

            let agent = match params.get("agent").map(String::as_str) {
                None | Some("") => None,
                Some("claude") => Some(AgentKind::Claude),
                Some("codex") => Some(AgentKind::Codex),
                Some("agy") => Some(AgentKind::Agy),
                Some("opencode") => Some(AgentKind::OpenCode),
                Some(_) => {
                    return Err(BridgeError::new(
                        BridgeErrorCode::InvalidUri,
                        "Agente CLI inválido na URI maxterminal://open-terminal.",
                    ));
                }
            };

            Ok(ParsedMaxTerminalUri::OpenTerminalForIssue {
                request_id,
                payload: OpenTerminalForIssuePayload {
                    project_path: canon_proj.to_string_lossy().into_owned(),
                    worktree_path,
                    branch: params.get("branch").cloned(),
                    issue_number,
                    issue_title: params.get("title").cloned(),
                    agent,
                    // URIs externas nunca podem injetar comando arbitrário de shell
                    initial_command: None,
                },
            })
        }
        "focus" => {
            ensure_allowed_query_keys(&params, &["v", "req", "ts"])?;
            Ok(ParsedMaxTerminalUri::Focus { request_id })
        }
        _ => Err(BridgeError::new(
            BridgeErrorCode::InvalidUri,
            format!("Rota maxterminal://{action} desconhecida."),
        )),
    }
}

/// Constrói a URI `vscode://johnattas.maxcode/inspect?...` para acordar o `MaxCode` em Plan Mode.
///
/// Se `prompt_b64` exceder `MAX_PROMPT_INLINE_BYTES` ou a URI total exceder `MAX_URI_TOTAL_LEN`,
/// grava um arquivo temporário `0600` em `artifact_dir` e referencia-o via `payload_file`.
pub fn build_vscode_inspect_uri(
    request_id: Uuid,
    payload: &InspectCommandErrorPayload,
    artifact_dir: &Path,
) -> Result<String, BridgeError> {
    let ts = current_unix_ms();
    let exit_code = match payload.exit_status {
        crate::protocol::InspectErrorCode::ExitCode { code } => code,
        crate::protocol::InspectErrorCode::Signal { signal, .. } => 128 + signal,
    };

    let prompt_b64 = URL_SAFE_NO_PAD.encode(payload.formatted_prompt.as_bytes());

    if prompt_b64.len() <= MAX_PROMPT_INLINE_BYTES {
        let mut url = Url::parse("vscode://johnattas.maxcode/inspect").map_err(|_| {
            BridgeError::new(BridgeErrorCode::InvalidUri, "Falha ao construir URI base.")
        })?;
        url.query_pairs_mut()
            .append_pair("v", &PROTOCOL_VERSION.to_string())
            .append_pair("req", &request_id.to_string())
            .append_pair("ts", &ts.to_string())
            .append_pair("mode", "plan")
            .append_pair("project", &payload.project_path)
            .append_pair("cwd", &payload.cwd)
            .append_pair("block_id", &payload.block_id)
            .append_pair("exit", &exit_code.to_string())
            .append_pair("prompt_b64", &prompt_b64);

        let built = String::from(url);
        if built.len() <= MAX_URI_TOTAL_LEN {
            return Ok(built);
        }
    }

    let artifact_path = write_inspect_artifact_file(artifact_dir, request_id, payload)?;
    let mut url = Url::parse("vscode://johnattas.maxcode/inspect").map_err(|_| {
        BridgeError::new(BridgeErrorCode::InvalidUri, "Falha ao construir URI base.")
    })?;
    url.query_pairs_mut()
        .append_pair("v", &PROTOCOL_VERSION.to_string())
        .append_pair("req", &request_id.to_string())
        .append_pair("ts", &ts.to_string())
        .append_pair("mode", "plan")
        .append_pair("project", &payload.project_path)
        .append_pair("cwd", &payload.cwd)
        .append_pair("block_id", &payload.block_id)
        .append_pair("exit", &exit_code.to_string())
        .append_pair("payload_file", &artifact_path.to_string_lossy());

    let built = String::from(url);
    if built.len() > MAX_URI_TOTAL_LEN {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "URI de fallback excede o limite de 8192 bytes.",
        ));
    }
    Ok(built)
}

/// Constrói a URI `vscode://johnattas.maxcode/open-file?...`.
pub fn build_vscode_open_file_uri(
    request_id: Uuid,
    payload: &OpenFilePayload,
) -> Result<String, BridgeError> {
    let _ = validate_file_path_for_open(&payload.file_path)?;
    let ts = current_unix_ms();
    let mut url = Url::parse("vscode://johnattas.maxcode/open-file").map_err(|_| {
        BridgeError::new(BridgeErrorCode::InvalidUri, "Falha ao construir URI base.")
    })?;
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("v", &PROTOCOL_VERSION.to_string())
            .append_pair("req", &request_id.to_string())
            .append_pair("ts", &ts.to_string())
            .append_pair("file", &payload.file_path)
            .append_pair("line", &payload.line.unwrap_or(1).max(1).to_string())
            .append_pair("col", &payload.column.unwrap_or(1).max(1).to_string());
        if let Some(ref proj) = payload.project_path {
            qp.append_pair("project", proj);
        }
    }
    let out = String::from(url);
    if out.len() > MAX_URI_TOTAL_LEN {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "URI open-file excede 8192 bytes.",
        ));
    }
    Ok(out)
}

/// Constrói a URI `vscode://johnattas.maxcode/project-sync?...`.
pub fn build_vscode_project_sync_uri(
    request_id: Uuid,
    project_path: &str,
    origin_id: &str,
) -> Result<String, BridgeError> {
    let ts = current_unix_ms();
    let mut url = Url::parse("vscode://johnattas.maxcode/project-sync").map_err(|_| {
        BridgeError::new(BridgeErrorCode::InvalidUri, "Falha ao construir URI base.")
    })?;
    url.query_pairs_mut()
        .append_pair("v", &PROTOCOL_VERSION.to_string())
        .append_pair("req", &request_id.to_string())
        .append_pair("ts", &ts.to_string())
        .append_pair("path", project_path)
        .append_pair("origin", origin_id);
    Ok(String::from(url))
}

/// Constrói a URI `vscode://johnattas.maxcode/issue/<open|plan|fix>?...` (Subetapa 11/06).
pub fn build_vscode_issue_action_uri(
    request_id: Uuid,
    payload: &IssueCommandPayload,
) -> Result<String, BridgeError> {
    let ts = current_unix_ms();
    let seg = payload.kind.uri_action_segment();
    let base = format!("vscode://johnattas.maxcode/issue/{seg}");
    let mut url = Url::parse(&base).map_err(|_| {
        BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "Falha ao construir URI de issue.",
        )
    })?;
    let title_b64 = URL_SAFE_NO_PAD.encode(payload.issue_title.as_bytes());
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("v", &PROTOCOL_VERSION.to_string())
            .append_pair("req", &request_id.to_string())
            .append_pair("ts", &ts.to_string())
            .append_pair("project", &payload.project_path)
            .append_pair("repo", &payload.repo_full_name)
            .append_pair("number", &payload.issue_number.to_string());
        if !title_b64.is_empty() && title_b64.len() <= 1024 {
            qp.append_pair("title_b64", &title_b64);
        }
    }
    let out = String::from(url);
    if out.len() > MAX_URI_TOTAL_LEN {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "URI de issue excede o limite de 8192 bytes.",
        ));
    }
    Ok(out)
}

/// Analisa e valida uma URI `vscode://johnattas.maxcode/...` (útil para round-trip e testes de conformidade).
#[allow(clippy::too_many_lines)]
pub fn parse_vscode_maxcode_uri(
    raw_uri: &str,
    now_ms: i64,
) -> Result<ParsedVsCodeMaxCodeUri, BridgeError> {
    validate_uri_basics(raw_uri)?;
    let url = Url::parse(raw_uri).map_err(|_| {
        BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "Sintaxe de URI vscode:// inválida.",
        )
    })?;
    if url.scheme() != "vscode" || url.host_str() != Some("johnattas.maxcode") {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "Autoridade inválida; esperado 'vscode://johnattas.maxcode/...'.",
        ));
    }

    let params = extract_query_map(&url)?;
    let (request_id, _ts) = validate_common_query_params(&params, now_ms)?;
    let path = url.path();

    match path {
        "/inspect" => {
            let project_path = params
                .get("project")
                .ok_or_else(|| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'project' ausente.")
                })?
                .clone();
            let cwd = params
                .get("cwd")
                .ok_or_else(|| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'cwd' ausente.")
                })?
                .clone();
            let block_id = params
                .get("block_id")
                .ok_or_else(|| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'block_id' ausente.")
                })?
                .clone();
            let exit_code: i32 = params
                .get("exit")
                .ok_or_else(|| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'exit' ausente.")
                })?
                .parse()
                .map_err(|_| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'exit' inválido.")
                })?;
            let mode = params
                .get("mode")
                .cloned()
                .unwrap_or_else(|| "plan".to_string());

            let prompt = if let Some(b64) = params.get("prompt_b64") {
                let decoded = URL_SAFE_NO_PAD.decode(b64.as_bytes()).map_err(|_| {
                    BridgeError::new(
                        BridgeErrorCode::InvalidUri,
                        "Base64URL inválido em prompt_b64.",
                    )
                })?;
                let text = String::from_utf8(decoded).map_err(|_| {
                    BridgeError::new(
                        BridgeErrorCode::InvalidUri,
                        "prompt_b64 não contém UTF-8 válido.",
                    )
                })?;
                Some(text)
            } else {
                None
            };
            let payload_file = params.get("payload_file").cloned();

            Ok(ParsedVsCodeMaxCodeUri::Inspect {
                request_id,
                project_path,
                cwd,
                block_id,
                exit_code,
                mode,
                prompt,
                payload_file,
            })
        }
        "/open-file" => {
            let file_path = params
                .get("file")
                .ok_or_else(|| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'file' ausente.")
                })?
                .clone();
            let _ = validate_file_path_for_open(&file_path)?;
            let line = params.get("line").and_then(|s| s.parse::<u32>().ok());
            let column = params.get("col").and_then(|s| s.parse::<u32>().ok());
            Ok(ParsedVsCodeMaxCodeUri::OpenFile {
                request_id,
                payload: OpenFilePayload {
                    file_path,
                    line,
                    column,
                    project_path: params.get("project").cloned(),
                    focus_editor: true,
                },
            })
        }
        "/project-sync" => {
            let project_path = params
                .get("path")
                .ok_or_else(|| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'path' ausente.")
                })?
                .clone();
            let origin_id = params
                .get("origin")
                .cloned()
                .unwrap_or_else(|| "maxterminal".to_string());
            Ok(ParsedVsCodeMaxCodeUri::ProjectSync {
                request_id,
                project_path,
                origin_id,
            })
        }
        "/issue/open" | "/issue/plan" | "/issue/fix" => {
            let action = path.trim_start_matches("/issue/").to_string();
            let project_path = params
                .get("project")
                .ok_or_else(|| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'project' ausente.")
                })?
                .clone();
            let repo = params
                .get("repo")
                .ok_or_else(|| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'repo' ausente.")
                })?
                .clone();
            let issue_number: u64 = params
                .get("number")
                .ok_or_else(|| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'number' ausente.")
                })?
                .parse()
                .map_err(|_| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "Campo 'number' inválido.")
                })?;
            let title = if let Some(b64) = params.get("title_b64") {
                let bytes = URL_SAFE_NO_PAD.decode(b64.as_bytes()).map_err(|_| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "title_b64 inválido.")
                })?;
                Some(String::from_utf8(bytes).map_err(|_| {
                    BridgeError::new(BridgeErrorCode::InvalidUri, "title_b64 não é UTF-8.")
                })?)
            } else {
                None
            };

            Ok(ParsedVsCodeMaxCodeUri::IssueAction {
                request_id,
                action,
                project_path,
                repo,
                issue_number,
                title,
            })
        }
        _ => Err(BridgeError::new(
            BridgeErrorCode::InvalidUri,
            format!("Caminho de URI vscode://johnattas.maxcode{path} desconhecido."),
        )),
    }
}

/// Grava com permissão `0600` (`O_NOFOLLOW`) o artefato JSON temporário de inspeção para que o `MaxCode` leia e remova.
pub fn write_inspect_artifact_file(
    dir: &Path,
    request_id: Uuid,
    payload: &InspectCommandErrorPayload,
) -> Result<PathBuf, BridgeError> {
    crate::discovery::ensure_secure_dir(dir)?;
    let path = dir.join(format!("inspect-{request_id}.json"));
    let artifact = InspectFallbackArtifact {
        request_id,
        created_at_ms: current_unix_ms(),
        payload: payload.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&artifact).map_err(|e| {
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao serializar artefato de inspeção: {e}"),
        )
    })?;
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                format!("Falha ao criar artefato de inspeção: {e}"),
            )
        })?;
    f.write_all(&bytes).map_err(|e| {
        let _ = fs::remove_file(&path);
        BridgeError::new(
            BridgeErrorCode::TransportIo,
            format!("Falha ao gravar artefato de inspeção: {e}"),
        )
    })?;
    Ok(path)
}

fn validate_uri_basics(raw_uri: &str) -> Result<(), BridgeError> {
    if raw_uri.is_empty() || raw_uri.len() > MAX_URI_TOTAL_LEN {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "URI vazia ou acima do limite máximo de 8192 bytes.",
        ));
    }
    if raw_uri.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "URI contém caracteres de controle inválidos.",
        ));
    }
    Ok(())
}

fn extract_query_map(url: &Url) -> Result<HashMap<String, String>, BridgeError> {
    let mut map = HashMap::new();
    for (k, v) in url.query_pairs() {
        if map.len() >= 16 {
            return Err(BridgeError::new(
                BridgeErrorCode::InvalidUri,
                "URI excede o limite de 16 parâmetros de consulta.",
            ));
        }
        let k_owned = k.into_owned();
        let v_owned = v.into_owned();
        if v_owned.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return Err(BridgeError::new(
                BridgeErrorCode::InvalidUri,
                "Parâmetro de URI contém caracteres de controle inválidos.",
            ));
        }
        if map.insert(k_owned, v_owned).is_some() {
            return Err(BridgeError::new(
                BridgeErrorCode::InvalidUri,
                "Parâmetro de consulta duplicado na URI.",
            ));
        }
    }
    Ok(map)
}

fn ensure_allowed_query_keys(
    params: &HashMap<String, String>,
    allowed: &[&str],
) -> Result<(), BridgeError> {
    for key in params.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(BridgeError::new(
                BridgeErrorCode::InvalidUri,
                format!("Parâmetro de consulta não permitido na URI: '{key}'."),
            ));
        }
    }
    Ok(())
}

fn validate_common_query_params(
    params: &HashMap<String, String>,
    now_ms: i64,
) -> Result<(Uuid, i64), BridgeError> {
    let v_str = params.get("v").ok_or_else(|| {
        BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "Parâmetro obrigatório 'v' ausente na URI.",
        )
    })?;
    let v: u32 = v_str.parse().map_err(|_| {
        BridgeError::new(BridgeErrorCode::InvalidUri, "Versão 'v' inválida na URI.")
    })?;
    if v != PROTOCOL_VERSION {
        return Err(BridgeError::new(
            BridgeErrorCode::UnsupportedProtocolVersion,
            format!("Versão de protocolo de URI não suportada: v={v}."),
        ));
    }

    let req_str = params.get("req").ok_or_else(|| {
        BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "Parâmetro obrigatório 'req' ausente na URI.",
        )
    })?;
    let request_id = Uuid::parse_str(req_str).map_err(|_| {
        BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "Parâmetro 'req' deve ser um UUID válido.",
        )
    })?;

    let ts_str = params.get("ts").ok_or_else(|| {
        BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "Parâmetro obrigatório 'ts' ausente na URI.",
        )
    })?;
    let ts: i64 = ts_str.parse().map_err(|_| {
        BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "Parâmetro 'ts' deve ser um inteiro UNIX em milissegundos.",
        )
    })?;

    if (now_ms - ts).abs() > URI_TTL_MS {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidUri,
            "A URI expirou (fora da janela de validade de 60 segundos).",
        ));
    }

    Ok((request_id, ts))
}
