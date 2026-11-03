//! Coleta, truncamento seguro e sanitização dupla do contexto de terminal enviado ao `MaxAiManager`.

use crate::config::ContextSettings;
use mt_core::MtResult;
use mt_core::ids::{CommandBlockId, PaneId, ProjectId, PtyId, SessionId, TabId};
use serde::{Deserialize, Serialize};

/// Limite máximo em bytes de cada comando recente incluído no contexto (`1 KiB`).
pub const MAX_COMMAND_BYTES: usize = 1024;
/// Versão do sanitizador aplicada ao contexto e à chave de cache de diagnóstico.
pub const SANITIZER_VERSION: u32 = 1;

/// Informações do sistema operacional Linux em execução.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    /// Família do SO (ex.: `"linux"`).
    pub family: String,
    /// Distribuição detectada em `/etc/os-release` (ex.: `"CachyOS"`).
    pub distro: String,
    /// Versão do kernel Linux.
    pub kernel: String,
}

/// Comando executado recentemente na sessão atual do painel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecentCommand {
    /// Texto do comando já sanitizado e truncado em 1 KiB.
    pub command: String,
    /// Código de saída (`None` se interrompido sem código).
    pub exit_code: Option<i32>,
    /// Diretório de trabalho abreviado (`~`).
    pub cwd: Option<String>,
}

/// Snapshot sanitizado de contexto de terminal enviado ao `MaxAiManager` (`CA-18`, `CA-31`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiContext {
    /// Nome do shell ativo (ex.: `"zsh"`, `"bash"`, `"fish"`).
    pub shell: String,
    /// Informações do sistema operacional.
    pub os: OsInfo,
    /// Diretório de trabalho atual abreviado e sanitizado.
    pub cwd: String,
    /// Stack detectada do projeto (ex.: `"laravel"`, `"vue"`, `"tauri"`, `"rust"`).
    pub project_stack: Option<String>,
    /// Nome do projeto ativo.
    pub project_name: Option<String>,
    /// Últimos comandos finalizados da sessão atual.
    pub recent_commands: Vec<RecentCommand>,
    /// Cauda opcional de saída do último comando (sanitizada e truncada).
    pub output_tail: Option<String>,
}

/// Vínculo autoritativo atual de um painel (`PaneId`) para verificação de obsolescência (`CA-33`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneBindingSnapshot {
    /// Projeto proprietário da aba.
    pub project_id: Option<ProjectId>,
    /// Aba proprietária do painel.
    pub tab_id: TabId,
    /// Identificador do painel.
    pub pane_id: PaneId,
    /// Sessão de painel ativa.
    pub session_id: SessionId,
    /// PTY real associado ao painel.
    pub pty_id: PtyId,
}

/// Dados de falha de um bloco de comando para diagnóstico rápido inline.
#[derive(Clone, Debug)]
pub struct BlockFailure {
    /// Painel onde o bloco executou.
    pub pane_id: PaneId,
    /// Sessão em que o bloco executou.
    pub session_id: SessionId,
    /// PTY em que o bloco executou.
    pub pty_id: PtyId,
    /// Projeto associado à aba.
    pub project_id: Option<ProjectId>,
    /// Comando executado (sanitizado).
    pub command: String,
    /// Código de saída observado.
    pub exit_code: Option<i32>,
    /// Cauda de saída/erro sanitizada.
    pub output_tail: String,
    /// Indica se o bloco rodou em modo interativo/TUI ou aba de Agente CLI.
    pub is_interactive: bool,
}

/// Fonte de contexto desacoplada de `mt-pty`, `mt-shell` e `tauri` (`CA-15`).
#[async_trait::async_trait]
pub trait AiContextSource: Send + Sync {
    /// Monta o contexto sanitizado para o painel informado.
    async fn for_pane(&self, pane: PaneId, opts: &ContextSettings) -> MtResult<AiContext>;
    /// Monta o contexto e os detalhes de falha de um bloco de comando específico.
    async fn for_block(
        &self,
        block: CommandBlockId,
        tail_bytes: u32,
    ) -> MtResult<(AiContext, BlockFailure)>;
    /// Retorna síncronamente o snapshot atual de vínculo do painel.
    fn pane_snapshot(&self, pane: PaneId) -> Option<PaneBindingSnapshot>;
    /// Retorna o projeto associado ao painel, se existir.
    fn project_of_pane(&self, pane: PaneId) -> Option<ProjectId> {
        self.pane_snapshot(pane).and_then(|s| s.project_id)
    }
}

impl AiContext {
    /// Remove códigos ANSI, abrevia o diretório `$HOME` para `~`, trunca comandos em 1 KiB e `output_tail`
    /// mantendo os últimos `tail_bytes` em fronteira UTF-8 válida, e reaplica `mt_db::sanitizer::sanitize()`
    /// em todos os campos textuais (defesa em profundidade — CA-18, CA-31).
    #[must_use]
    pub fn sanitize_and_bound(mut self, max_recent: u8, tail_bytes: u32) -> Self {
        let home = std::env::var("MAXCODE_REAL_HOME")
            .ok()
            .or_else(|| std::env::var("HOME").ok());
        let home_ref = home.as_deref();

        self.shell = strip_ansi_and_sanitize(&self.shell);
        self.os.family = strip_ansi_and_sanitize(&self.os.family);
        self.os.distro = strip_ansi_and_sanitize(&self.os.distro);
        self.os.kernel = strip_ansi_and_sanitize(&self.os.kernel);
        self.cwd = abbreviate_home(&strip_ansi_and_sanitize(&self.cwd), home_ref);
        self.project_stack = self
            .project_stack
            .as_deref()
            .map(strip_ansi_and_sanitize)
            .filter(|s| !s.is_empty());
        self.project_name = self
            .project_name
            .as_deref()
            .map(strip_ansi_and_sanitize)
            .filter(|s| !s.is_empty());

        let max_len = usize::from(max_recent);
        if self.recent_commands.len() > max_len {
            let drain_count = self.recent_commands.len() - max_len;
            self.recent_commands.drain(0..drain_count);
        }

        for cmd in &mut self.recent_commands {
            let clean_cmd = strip_ansi_and_sanitize(&cmd.command);
            cmd.command = truncate_utf8_head(&clean_cmd, MAX_COMMAND_BYTES);
            cmd.cwd = cmd
                .cwd
                .as_deref()
                .map(|c| abbreviate_home(&strip_ansi_and_sanitize(c), home_ref));
        }

        if tail_bytes == 0 {
            self.output_tail = None;
        } else if let Some(tail) = &self.output_tail {
            let clean_tail = strip_ansi_and_sanitize(tail);
            let bounded = truncate_utf8_tail(&clean_tail, tail_bytes as usize);
            self.output_tail = Some(mt_db::sanitizer::sanitize(&bounded).into_owned());
        }

        self
    }
}

/// Remove sequências de escape ANSI (CSI, OSC, DCS e sequências de 2 bytes) e aplica `mt_db::sanitizer::sanitize()`.
#[must_use]
pub fn strip_ansi_and_sanitize(input: &str) -> String {
    let stripped = strip_ansi_codes(input);
    mt_db::sanitizer::sanitize(&stripped).into_owned()
}

fn strip_ansi_codes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                Some('P' | 'X' | '^' | '_') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
        } else if ch == '\r' {
            // Normaliza CR isolado ou CRLF preservando apenas LF
            if chars.peek() != Some(&'\n') {
                out.push('\n');
            }
        } else if ch != '\x00' && ch != '\x07' && ch != '\x08' {
            out.push(ch);
        }
    }

    out
}

/// Trunca `input` mantendo no máximo os primeiros `max_bytes` em fronteira válida UTF-8.
#[must_use]
pub fn truncate_utf8_head(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    input[..end].to_string()
}

/// Trunca `input` mantendo no máximo os últimos `max_bytes` em fronteira válida UTF-8.
#[must_use]
pub fn truncate_utf8_tail(input: &str, max_bytes: usize) -> String {
    if input.len() <= max_bytes {
        return input.to_string();
    }
    let mut start = input.len() - max_bytes;
    while start < input.len() && !input.is_char_boundary(start) {
        start += 1;
    }
    input[start..].to_string()
}

/// Abrevia o diretório `$HOME` no início de `path` para `~` e aplica sanitização.
#[must_use]
pub fn abbreviate_home(path: &str, home_dir: Option<&str>) -> String {
    let trimmed = path.trim();
    let abbreviated = if let Some(home) = home_dir.map(|h| h.trim_end_matches('/'))
        && !home.is_empty()
    {
        if trimmed == home {
            "~".to_string()
        } else if let Some(rest) = trimmed.strip_prefix(home)
            && rest.starts_with('/')
        {
            format!("~{rest}")
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    };
    mt_db::sanitizer::sanitize(&abbreviated).into_owned()
}

/// Detecta informações da distribuição Linux e kernel a partir de `/etc/os-release` e `/proc/sys/kernel/osrelease`.
#[must_use]
pub fn detect_linux_os_info() -> OsInfo {
    let distro = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| {
            let mut pretty = None;
            let mut name = None;
            for line in content.lines() {
                if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
                    pretty = Some(val.trim_matches('"').trim().to_string());
                } else if let Some(val) = line.strip_prefix("NAME=") {
                    name = Some(val.trim_matches('"').trim().to_string());
                }
            }
            pretty.or(name)
        })
        .unwrap_or_else(|| "Linux".to_string());

    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Linux".to_string());

    OsInfo {
        family: "linux".to_string(),
        distro: strip_ansi_and_sanitize(&distro),
        kernel: strip_ansi_and_sanitize(&kernel),
    }
}
