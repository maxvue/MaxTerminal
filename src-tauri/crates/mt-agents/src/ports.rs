//! Portas de domínio que desacoplam `mt-agents` de `mt-pty`, `mt-shell`, `mt-os` e Tauri.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use mt_core::{MtResult, PaneId, ProjectId, PtyId, SessionId, TabId};
use mt_git::{GitContext, WorktreeCreateRequest, WorktreeEntry};
use serde::{Deserialize, Serialize};

use crate::registry::AgentKind;

/// Identidade imutável de uma execução de agente dentro de um painel/sessão.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunBinding {
    /// Identificador da aba proprietária.
    pub tab_id: TabId,
    /// Identificador do painel onde o agente roda.
    pub pane_id: PaneId,
    /// Identificador do PTY ativo.
    pub pty_id: PtyId,
    /// Identificador da sessão SQLite.
    pub session_id: SessionId,
    /// Geração do owner da execução (Fase 07).
    pub generation: i64,
    /// Identificador do projeto.
    pub project_id: ProjectId,
    /// Identificador único desta execução do agente.
    pub run_id: String,
    /// Tipo de agente CLI.
    pub agent: AgentKind,
    /// Diretório de trabalho inicial da execução.
    pub cwd: PathBuf,
    /// Timestamp UNIX em ms do início da execução.
    pub started_at_ms: i64,
}

/// Informações sobre o processo em foreground de um PTY.
#[derive(Debug, Clone)]
pub struct ForegroundProc {
    /// PID do processo em foreground.
    pub pid: u32,
    /// Nome curto do executável (`comm`).
    pub comm: String,
    /// Caminho resolvido do executável, quando disponível.
    pub exe: Option<PathBuf>,
    /// Vetor de argumentos da linha de comando (`cmdline`).
    pub cmdline: Vec<String>,
}

/// Resumo de projeto consumido por `mt-agents`.
#[derive(Debug, Clone)]
pub struct ProjectRecord {
    /// Identificador do projeto.
    pub id: ProjectId,
    /// Título de exibição do projeto.
    pub title: String,
    /// Caminho raiz do projeto no sistema de arquivos.
    pub path: PathBuf,
    /// Stack detectada do projeto (ex.: `"laravel"`, `"vue"`, `"rust"`, `"tauri"`).
    pub stack: String,
}

/// Natureza da intervenção aguardada pelo agente CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AttentionKind {
    /// O agente aguarda aprovação de permissão/ferramenta.
    AwaitingApproval,
    /// O agente aguarda resposta textual ou input do usuário.
    AwaitingInput,
}

/// Porta para a máquina de estados da Fase 05 com suporte a overlay por episódio.
pub trait TabStatePort: Send + Sync {
    /// Aplica sobreposição `AttentionRequired` para `(run, episode)` e retorna `true` se o estado mudou.
    fn attention_required(&self, run: &RunBinding, episode: u64) -> bool;
    /// Remove apenas a sobreposição do episódio informado e recompõe o estado subjacente real do painel.
    fn attention_resolved(&self, run: &RunBinding, episode: u64);
}

/// Porta para o `AttentionRegistry` e `Notifier` da Fase 09.
#[async_trait]
pub trait NotifyPort: Send + Sync {
    /// Registra atenção pendente para a aba no serviço de OS/tray.
    fn set_attention(&self, tab: TabId, kind: AttentionKind);
    /// Limpa atenção pendente da aba no serviço de OS/tray.
    fn clear_attention(&self, tab: TabId);
    /// Verifica se a janela está focada e a aba é a ativa no momento.
    fn is_tab_focused_and_active(&self, tab: TabId) -> bool;
    /// Dispara notificação nativa do sistema operacional para a aba.
    async fn notify(&self, tab: TabId, title: &str, body: &str);
}

/// Porta para consulta de processo em foreground na Fase 02.
pub trait PtyPort: Send + Sync {
    /// Retorna o processo em foreground do PTY, se houver.
    fn foreground(&self, pty: PtyId) -> Option<ForegroundProc>;
    /// Verifica se há processos ativos em qualquer painel da aba.
    fn has_active_processes(&self, tab: TabId) -> bool;
}

/// Porta para operações de projeto, templates e abas (Fases 06 e 07).
#[async_trait]
pub trait WorkspacePort: Send + Sync {
    /// Obtém os dados de um projeto pelo ID.
    async fn project(&self, id: ProjectId) -> MtResult<ProjectRecord>;
    /// Retorna o projeto ao qual a aba pertence.
    fn project_of_tab(&self, tab: TabId) -> Option<ProjectId>;
    /// Lista todas as abas abertas cujo `cwd` está dentro de `dir`.
    fn tabs_using_cwd(&self, dir: &Path) -> Vec<TabId>;
    /// Garante a criação idempotente dos templates dedicados de agentes para o projeto.
    async fn ensure_agent_templates(&self, project: ProjectId) -> MtResult<u32>;
}

/// Porta para operações Git e Worktree (Fase 08).
#[async_trait]
pub trait GitPort: Send + Sync {
    /// Cria uma nova worktree Git conforme requisição.
    async fn create_worktree(&self, req: WorktreeCreateRequest) -> MtResult<WorktreeEntry>;
    /// Remove uma worktree Git existente.
    async fn remove_worktree(
        &self,
        repo_root: &Path,
        worktree_path: &Path,
        force: bool,
    ) -> MtResult<()>;
    /// Obtém o status Git atual do diretório informado.
    async fn status(&self, cwd: &Path) -> MtResult<Option<GitContext>>;
    /// Executa um subcomando `git` arbitrário no diretório informado e retorna o `stdout`.
    async fn run(&self, cwd: &Path, args: &[&str]) -> MtResult<String>;
}
