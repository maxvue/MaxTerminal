//! Construção pura e execução segura de invocações de editor externo (`build_editor_invocation` e `EditorLauncher`).

use std::env;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use mt_core::error::MtResult;
use parking_lot::RwLock;
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::error::{codes, os_error};
use crate::links::ResolvedLink;
use crate::settings::{EditorSettings, OsSettings, validate_custom_argv};

/// Conjunto de caracteres que precisam de percent-encoding no caminho de `vscode://file/...` (preservando `/`).
const VSCODE_URI_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b':')
    .add(b'[')
    .add(b']');

/// Tipo de editor configurado para abertura de arquivos clicados no terminal.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EditorKind {
    /// Visual Studio Code (`code --goto <abs>:<l>:<c>`).
    #[default]
    VsCode,
    /// Visual Studio Code Insiders (`code-insiders --goto <abs>:<l>:<c>`).
    VsCodeInsiders,
    /// Integração `MaxCode` (`vscode://file/<abs>:<l>:<c>` ou ponte loopback da Fase 11).
    MaxCode,
    /// Comando personalizado com vetor `custom_argv` contendo `{file}`, `{line}`, `{column}`.
    Custom,
}

/// Disponibilidade detectada de um editor no sistema operacional.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EditorAvailability {
    /// Identificador do editor.
    pub kind: EditorKind,
    /// `true` se o binário ou handler de URI está disponível.
    pub available: bool,
    /// Caminho do executável ou handler detectado no `$PATH`, se houver.
    pub binary: Option<String>,
}

/// Invocação pura construída por `build_editor_invocation` (sem usar shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorInvocation {
    /// Execução direta de processo com argv vetorizado.
    Exec {
        /// Binário a ser invocado.
        program: PathBuf,
        /// Lista de argumentos individuais.
        args: Vec<String>,
    },
    /// Abertura via URI (`vscode://file/...`).
    OpenUri(String),
}

/// Constrói de forma pura a invocação do editor para um `ResolvedLink` (CA-02):
/// - `VsCode` → `code --goto <abs>:<l>:<c>` (`l`/`c` padrão `1`)
/// - `VsCodeInsiders` → `code-insiders --goto <abs>:<l>:<c>`
/// - `MaxCode` → `vscode://file/<abs percent-encoded, '/' preservado>:<l>:<c>`
/// - `Custom` → substitui `{file}`, `{line}`, `{column}` dentro de cada elemento de `custom_argv`
pub fn build_editor_invocation(
    settings: &EditorSettings,
    link: &ResolvedLink,
) -> MtResult<EditorInvocation> {
    if link.absolute_path.is_empty()
        || !link.absolute_path.starts_with('/')
        || link.absolute_path.starts_with('-')
        || link.absolute_path.contains('\0')
    {
        return Err(os_error(
            codes::LINK_REJECTED,
            "O caminho do arquivo para abertura no editor deve ser absoluto e válido.",
        ));
    }

    let line_num = link.line.unwrap_or(1).max(1);
    let column = link.column.unwrap_or(1).max(1);

    match settings.kind {
        EditorKind::VsCode => Ok(EditorInvocation::Exec {
            program: PathBuf::from("code"),
            args: vec![
                String::from("--goto"),
                format!("{}:{line_num}:{column}", link.absolute_path),
            ],
        }),
        EditorKind::VsCodeInsiders => Ok(EditorInvocation::Exec {
            program: PathBuf::from("code-insiders"),
            args: vec![
                String::from("--goto"),
                format!("{}:{line_num}:{column}", link.absolute_path),
            ],
        }),
        EditorKind::MaxCode => {
            let trimmed = link.absolute_path.trim_start_matches('/');
            let encoded_path = utf8_percent_encode(trimmed, VSCODE_URI_ENCODE_SET).to_string();
            let uri = format!("vscode://file/{encoded_path}:{line_num}:{column}");
            Ok(EditorInvocation::OpenUri(uri))
        }
        EditorKind::Custom => {
            validate_custom_argv(&settings.custom_argv)?;
            let program = PathBuf::from(&settings.custom_argv[0]);
            let line_str = line_num.to_string();
            let col_str = column.to_string();
            let mut expanded_args =
                Vec::with_capacity(settings.custom_argv.len().saturating_sub(1));

            for template_arg in &settings.custom_argv[1..] {
                let expanded = template_arg
                    .replace("{file}", &link.absolute_path)
                    .replace("{line}", &line_str)
                    .replace("{column}", &col_str);
                if expanded.len() > 4096 || expanded.contains('\0') {
                    return Err(os_error(
                        codes::SETTINGS_INVALID,
                        "Argumento expandido do editor personalizado excede o limite de segurança.",
                    ));
                }
                expanded_args.push(expanded);
            }

            Ok(EditorInvocation::Exec {
                program,
                args: expanded_args,
            })
        }
    }
}

/// Ponto de extensão para abertura customizada (ex.: Fase 11 registra opener para `MaxCode` via loopback).
#[async_trait]
pub trait EditorOpener: Send + Sync {
    /// Retorna `true` se este opener atende o `EditorKind` informado.
    fn handles(&self, kind: EditorKind) -> bool;

    /// Abre o `ResolvedLink` no editor externo.
    async fn open(&self, link: &ResolvedLink) -> MtResult<()>;
}

/// Lançador de editores externos com suporte a ponte (`bridge_opener`), openers extras e execução direta.
pub struct EditorLauncher {
    bridge_opener: RwLock<Option<Arc<dyn EditorOpener>>>,
    openers: RwLock<Vec<Arc<dyn EditorOpener>>>,
    settings: Arc<RwLock<OsSettings>>,
}

impl EditorLauncher {
    /// Cria um novo `EditorLauncher`.
    #[must_use]
    pub fn new(settings: Arc<RwLock<OsSettings>>) -> Self {
        Self {
            bridge_opener: RwLock::new(None),
            openers: RwLock::new(Vec::new()),
            settings,
        }
    }

    /// Registra um opener no início da fila de prioridade.
    pub fn register_opener(&self, opener: Arc<dyn EditorOpener>) {
        self.openers.write().insert(0, opener);
    }

    /// Define ou substitui idempotentemente o opener dedicado da ponte `MaxCode` (Fase 11).
    pub fn set_bridge_opener(&self, opener: Arc<dyn EditorOpener>) {
        *self.bridge_opener.write() = Some(opener);
    }

    /// Detecta a disponibilidade de `code`, `code-insiders`, `MaxCode` (`code` ou `xdg-open`) e `Custom`.
    #[must_use]
    pub fn detect() -> Vec<EditorAvailability> {
        let code_bin = find_in_path("code").map(|p| p.to_string_lossy().into_owned());
        let insiders_bin = find_in_path("code-insiders").map(|p| p.to_string_lossy().into_owned());
        let xdg_open_bin = find_in_path("xdg-open").map(|p| p.to_string_lossy().into_owned());

        let maxcode_bin = code_bin.clone().or(xdg_open_bin);

        vec![
            EditorAvailability {
                kind: EditorKind::VsCode,
                available: code_bin.is_some(),
                binary: code_bin,
            },
            EditorAvailability {
                kind: EditorKind::VsCodeInsiders,
                available: insiders_bin.is_some(),
                binary: insiders_bin,
            },
            EditorAvailability {
                kind: EditorKind::MaxCode,
                available: maxcode_bin.is_some(),
                binary: maxcode_bin,
            },
            EditorAvailability {
                kind: EditorKind::Custom,
                available: true,
                binary: None,
            },
        ]
    }

    /// Abre o `ResolvedLink` usando o opener registrado ou construindo a invocação nativa.
    pub async fn open(&self, link: &ResolvedLink) -> MtResult<()> {
        let editor_settings = self.settings.read().editor.clone();
        let bridge_opt = self.bridge_opener.read().clone();

        if let Some(bridge) = bridge_opt
            && bridge.handles(editor_settings.kind)
            && bridge.open(link).await.is_ok()
        {
            return Ok(());
        }

        let openers_snapshot = self.openers.read().clone();
        for opener in openers_snapshot {
            if opener.handles(editor_settings.kind) {
                return opener.open(link).await;
            }
        }

        let invocation = build_editor_invocation(&editor_settings, link)?;
        match invocation {
            EditorInvocation::Exec { program, args } => {
                let resolved_prog = resolve_executable(&program, editor_settings.kind)?;
                spawn_detached(&resolved_prog, &args)
            }
            EditorInvocation::OpenUri(uri) => {
                if !uri.starts_with("vscode://file/") {
                    return Err(os_error(
                        codes::EDITOR_LAUNCH_FAILED,
                        "Esquema de URI não autorizado para abertura de arquivo no editor.",
                    ));
                }
                let xdg_open = find_in_path("xdg-open").ok_or_else(|| {
                    os_error(
                        codes::EDITOR_NOT_FOUND,
                        "Utilitário 'xdg-open' não encontrado no PATH para abrir URI do editor.",
                    )
                })?;
                spawn_detached(&xdg_open, &[uri])
            }
        }
    }
}

fn resolve_executable(program: &Path, kind: EditorKind) -> MtResult<PathBuf> {
    if program.is_absolute() {
        if is_executable_file(program) {
            return Ok(program.to_path_buf());
        }
    } else if let Some(name) = program.to_str()
        && let Some(found) = find_in_path(name)
    {
        return Ok(found);
    }

    let msg = match kind {
        EditorKind::VsCode => {
            "VS Code (`code`) não encontrado no PATH. Configure o editor em Preferências > Sistema."
        }
        EditorKind::VsCodeInsiders => {
            "VS Code Insiders (`code-insiders`) não encontrado no PATH. Configure o editor em Preferências > Sistema."
        }
        EditorKind::MaxCode => {
            "Editor MaxCode / VS Code não encontrado no PATH. Configure o editor em Preferências > Sistema."
        }
        EditorKind::Custom => {
            "O executável do editor personalizado não foi encontrado no PATH. Verifique Preferências > Sistema."
        }
    };
    Err(os_error(codes::EDITOR_NOT_FOUND, msg))
}

fn spawn_detached(program: &Path, args: &[String]) -> MtResult<()> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let mut child = cmd.spawn().map_err(|e| {
        os_error(
            codes::EDITOR_LAUNCH_FAILED,
            format!("Falha ao iniciar o editor externo: {e}"),
        )
    })?;

    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    Ok(())
}

/// Procura um executável pelo nome nos diretórios da variável `$PATH`.
#[must_use]
pub fn find_in_path(bin_name: &str) -> Option<PathBuf> {
    if bin_name.is_empty() || bin_name.contains('/') || bin_name.contains('\0') {
        return None;
    }
    let path_os = env::var_os("PATH")?;
    for dir in env::split_paths(&path_os) {
        let candidate = dir.join(bin_name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    if let Ok(meta) = std::fs::metadata(path) {
        meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
    } else {
        false
    }
}
