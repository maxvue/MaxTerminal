//! Portas de infraestrutura (`GroupPtyPort`, `GroupShellPort`, `GroupTabPort`, `GroupAiPort`,
//! `RecurringCommandsSource`) e contratos de supervisão da Fase 15.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use mt_core::{CommandBlockId, MtResult, PaneId, ProjectId, PtyId, SessionId, TabId, TemplateId};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::groups::model::{CommandGroupInput, GroupScope, ShellDialect};

/// Especificação de spawn de um PTY headless em background (espelha `mt_pty::SpawnSpec` com `PtyOwner` vazio).
#[derive(Debug, Clone)]
pub struct HeadlessSpawnSpec {
    /// Caminho do executável do shell.
    pub program: PathBuf,
    /// Argumentos (ex.: `["-c", "<command>"]`).
    pub args: Vec<String>,
    /// Diretório de trabalho canonicalizado.
    pub cwd: PathBuf,
    /// Variáveis de ambiente mescladas.
    pub env: BTreeMap<String, String>,
    /// Se deve iniciar como login shell (`false` para PTYs headless).
    pub login: bool,
}

/// Informação de encerramento de processo supervisionado (espelha `mt_pty::ExitInfo` após limpeza da árvore).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisedExitInfo {
    /// Identificador do PTY encerrado.
    pub pty_id: PtyId,
    /// Código de saída, quando aplicável.
    pub exit_code: Option<i32>,
    /// Sinal que encerrou o processo, quando aplicável.
    pub signal: Option<i32>,
    /// Confirma que toda a árvore de processos em `/proc` foi limpa pela Fase 02.
    pub tree_cleaned: bool,
}

/// Sink de saída bruta anexado ao PTY antes/durante o spawn.
pub trait RawOutputTap: Send + Sync {
    /// Recebe um chunk bruto de bytes produzidos pelo PTY.
    fn on_bytes(&self, chunk: &[u8]);
    /// Notifica o fechamento do stream de saída (EOF).
    fn on_eof(&self);
}

/// Porta para `mt_pty::PtyManager` (Fase 02).
#[async_trait]
pub trait GroupPtyPort: Send + Sync {
    /// Cria um PTY headless (`PtyOwner { tab_id: None, pane_id: None }`) com `output_tap` instalado
    /// desde o primeiro byte e retorna o `PtyId`.
    async fn spawn_headless(
        &self,
        spec: HeadlessSpawnSpec,
        output_tap: Arc<dyn RawOutputTap>,
    ) -> MtResult<PtyId>;

    /// Aguarda o encerramento completo do processo líder E a limpeza supervisionada de todos os
    /// processos descendentes em `/proc` pela Fase 02.
    async fn wait_exit(&self, pty_id: &PtyId) -> MtResult<SupervisedExitInfo>;

    /// Escreve bytes no PTY de um painel existente (usado pelo modo Sequencial).
    async fn write_to_pty(&self, pty_id: &PtyId, data: &[u8]) -> MtResult<()>;

    /// Encerra um PTY headless (`KillMode::Graceful` com escalonamento para `SIGKILL` na árvore).
    async fn kill_graceful(&self, pty_id: &PtyId) -> MtResult<SupervisedExitInfo>;

    /// Anexa um tap temporário de leitura aos bytes produzidos por um PTY existente (Sequencial/Multi-Abas)
    /// durante a execução do item correlacionado.
    async fn attach_item_tap(
        &self,
        pty_id: &PtyId,
        output_tap: Arc<dyn RawOutputTap>,
    ) -> MtResult<Box<dyn Send + Sync>>;
}

/// Estado instantâneo verificado no backend para um painel de terminal (Fase 05).
#[derive(Debug, Clone)]
pub struct PaneShellState {
    /// Aba à qual o painel pertence.
    pub tab_id: TabId,
    /// Identificador do painel.
    pub pane_id: PaneId,
    /// Identificador do PTY ativo no painel.
    pub pty_id: PtyId,
    /// Identificador da sessão SQLite ativa no painel.
    pub session_id: SessionId,
    /// Caminho do executável do shell do painel.
    pub shell_path: PathBuf,
    /// Geração monotônica do painel/PTY.
    pub generation: u64,
    /// Indica se o shell está ocioso aguardando comando no prompt (OSC 133 A/B).
    pub prompt_ready: bool,
    /// Indica se o terminal está em buffer alternativo (TUI como `vim`/`htop`).
    pub in_alt_screen: bool,
    /// Indica se há processo em foreground ocupando o PTY.
    pub foreground_busy: bool,
    /// Indica se o Shell Integration (OSC 133) está operacional neste painel.
    pub tracking_available: bool,
}

/// Evento de ciclo de vida de bloco OSC 133 e prompt emitido pela Fase 05.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellLifecycleEvent {
    /// Início de execução de bloco OSC 133 (`133;C`).
    BlockStarted {
        /// PTY onde o bloco iniciou.
        pty_id: PtyId,
        /// Sessão associada.
        session_id: SessionId,
        /// Identificador do bloco criado.
        block_id: CommandBlockId,
        /// Geração do painel/PTY.
        generation: u64,
    },
    /// Fim de execução de bloco OSC 133 (`133;D`).
    BlockFinished {
        /// PTY onde o bloco terminou.
        pty_id: PtyId,
        /// Sessão associada.
        session_id: SessionId,
        /// Identificador do bloco finalizado.
        block_id: CommandBlockId,
        /// Geração do painel/PTY.
        generation: u64,
        /// Código de saída do comando.
        exit_code: i32,
        /// Sinal opcional de encerramento.
        signal: Option<i32>,
    },
    /// Prompt pronto para o próximo comando (`133;A`/`133;B`).
    PromptReady {
        /// PTY pronto.
        pty_id: PtyId,
        /// Sessão associada.
        session_id: SessionId,
        /// Geração do painel/PTY.
        generation: u64,
    },
    /// A geração do painel foi invalidada (ex.: "Iniciar nova Sessão", fechamento de aba ou queda de PTY).
    GenerationInvalidated {
        /// Painel afetado.
        pane_id: PaneId,
        /// PTY afetado.
        pty_id: PtyId,
        /// Motivo da invalidação.
        reason: String,
    },
}

/// Lease exclusivo de submissão de comandos sobre um painel durante uma fila Sequencial.
#[async_trait]
pub trait PaneExecutionLease: Send + Sync {
    /// Retorna o estado inicial capturado na aquisição do lease.
    fn state(&self) -> &PaneShellState;
    /// Aguarda o próximo evento de ciclo de vida OSC 133 deste painel.
    async fn next_event(&mut self) -> Option<ShellLifecycleEvent>;
    /// Libera explicitamente o lease sobre o painel.
    async fn release(self: Box<Self>);
}

/// Porta para integração com Shell Integration (Fase 05) e coordenação de input (Fase 04).
#[async_trait]
pub trait GroupShellPort: Send + Sync {
    /// Inspeciona o estado atual de um painel sem adquirir lease.
    async fn inspect_pane(&self, pane_id: &PaneId) -> MtResult<PaneShellState>;

    /// Adquire lease exclusivo `(pane_id, pty_id, session_id, generation)` se e somente se:
    /// - `tracking_available == true` (senão retorna `commands.group.tracking_unavailable`)
    /// - `prompt_ready == true && !in_alt_screen && !foreground_busy` e nenhum outro lease ativo
    ///   (senão retorna `commands.group.pane_busy`).
    async fn acquire_pane_lease(&self, pane_id: &PaneId) -> MtResult<Box<dyn PaneExecutionLease>>;

    /// Assina eventos de ciclo de vida OSC 133 de uma sessão/aba recém-aberta pelo modo Multi-Abas.
    async fn subscribe_session_events(
        &self,
        session_id: &SessionId,
    ) -> MtResult<tokio::sync::mpsc::UnboundedReceiver<ShellLifecycleEvent>>;

    /// Verifica se o binário `env` do sistema suporta a flag `-C` (GNU coreutils >= 8.28).
    fn supports_env_chdir(&self) -> bool;
}

/// Descritor de template retornado pela Fase 07 para validação de preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectShellTemplate {
    /// Identificador do template.
    pub id: TemplateId,
    /// Projeto ao qual pertence.
    pub project_id: ProjectId,
    /// Nome do template.
    pub name: String,
    /// Indica se é o template padrão do projeto.
    pub is_default: bool,
    /// Indica se é um template de ambiente Shell (não Agente CLI).
    pub is_shell_environment: bool,
    /// Indica se está marcado como Execução Contínua.
    pub continuous: bool,
    /// Executável do shell configurado.
    pub shell_path: PathBuf,
    /// Variáveis de ambiente base do template.
    pub base_env: BTreeMap<String, String>,
}

/// Requisição real de abertura de aba enviada para `WorkspaceManager::open_tab` (Fase 07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupTabOpenSpec {
    /// Projeto alvo.
    pub project_id: ProjectId,
    /// Template selecionado.
    pub template_id: TemplateId,
    /// Se a aba deve receber foco imediato (`true` apenas na primeira aba aberta pelo grupo).
    pub focus: bool,
    /// Título personalizado da aba (`"{group.name} · {item.title}"`).
    pub title: String,
    /// Diretório de trabalho resolvido.
    pub cwd: PathBuf,
    /// Comando inicial a ser disparado uma única vez pela Fase 07.
    pub initial_command: String,
    /// Variáveis de ambiente extras (`group.env + item.env`).
    pub extra_env: BTreeMap<String, String>,
    /// Desativa fallback silencioso de `cwd` (`false`).
    pub cwd_fallback_to_project: bool,
}

/// Identificadores retornados por `WorkspaceManager::open_tab` (Fase 07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedGroupTab {
    /// Aba criada.
    pub tab_id: TabId,
    /// Painel raiz da aba.
    pub pane_id: PaneId,
    /// PTY associado ao painel.
    pub pty_id: PtyId,
    /// Sessão SQLite criada.
    pub session_id: SessionId,
    /// Bloco de comando inicial caso já tenha sido aberto sincronamente.
    pub initial_block_id: Option<CommandBlockId>,
}

/// Porta para o `WorkspaceManager` (Fase 07) e informações de projeto (Fase 06).
#[async_trait]
pub trait GroupTabPort: Send + Sync {
    /// Resolve o diretório raiz absoluto do projeto.
    async fn resolve_project_root(&self, project_id: &ProjectId) -> MtResult<PathBuf>;

    /// Lista os templates do projeto para seleção no modo Multi-Abas.
    async fn list_project_templates(
        &self,
        project_id: &ProjectId,
    ) -> MtResult<Vec<ProjectShellTemplate>>;

    /// Abre uma nova aba no `WorkspaceManager`.
    async fn open_tab(&self, spec: GroupTabOpenSpec) -> MtResult<OpenedGroupTab>;

    /// Fecha uma aba aberta pelo modo Multi-Abas somente se ela ainda estiver executando o comando
    /// inicial do item (`session_id` e `pty_id` inalterados e comando inicial não finalizado).
    async fn close_tab_if_running_initial(
        &self,
        tab_id: &TabId,
        expected_session_id: &SessionId,
        expected_pty_id: &PtyId,
    ) -> MtResult<bool>;
}

/// Item de comando recorrente retornado pela memória contextual opcional (Fase 13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecurringCommandEntry {
    /// Comando sanitizado.
    pub command: String,
    /// Último diretório relativo observado.
    pub cwd: Option<String>,
    /// Frequência de uso acumulada.
    pub frequency: u32,
    /// Timestamp da última execução (epoch ms).
    pub last_used_at: i64,
}

/// Porta opcional para comandos recorrentes da Fase 13.
#[async_trait]
pub trait RecurringCommandsSource: Send + Sync {
    /// Indica se a fonte de memória contextual está disponível.
    fn is_available(&self) -> bool;

    /// Lista os comandos mais recorrentes do projeto (`1..=50`).
    async fn list_recurring(
        &self,
        project_id: &ProjectId,
        limit: u32,
    ) -> MtResult<Vec<RecurringCommandEntry>>;
}

/// Contexto sanitizado enviado ao `AiService::generate_structured` (Fase 12).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestPromptContext {
    /// Projeto alvo opcional.
    pub project_id: Option<ProjectId>,
    /// Painel de origem opcional.
    pub pane_id: Option<PaneId>,
    /// Escopo fixado pelo editor.
    pub scope: GroupScope,
    /// Nome do projeto (quando disponível).
    pub project_name: Option<String>,
    /// Stacks detectadas no projeto.
    pub project_stacks: Vec<String>,
    /// Dialeto do shell alvo.
    pub shell_dialect: ShellDialect,
    /// Objetivo descrito pelo usuário (sanitizado).
    pub goal: String,
    /// Rascunho atual do editor (sanitizado).
    pub existing_draft: Option<CommandGroupInput>,
    /// Comandos recorrentes sanitizados do projeto.
    pub recurring_commands: Vec<String>,
}

/// Porta para `AiService::generate_structured` (Fase 12), schema `command-group.suggestion.v1`.
#[async_trait]
pub trait GroupAiPort: Send + Sync {
    /// Solicita geração estruturada de um rascunho de `CommandGroupInput`.
    async fn generate_group_suggestion(
        &self,
        ai_request_id: &str,
        context: SuggestPromptContext,
    ) -> MtResult<CommandGroupInput>;
}
