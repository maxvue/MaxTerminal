//! Tipos de domínio e DTOs públicos do subsistema Git e Worktrees (`mt-git` — Fase 08).

use std::path::{Path, PathBuf};

use mt_core::ids::{PaneId, TabId};
use serde::{Deserialize, Serialize};

/// Chave canônica de um repositório/worktree (`worktree_root` após `fs::canonicalize`).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
#[specta(transparent)]
pub struct RepoKey(pub PathBuf);

impl RepoKey {
    /// Cria uma [`RepoKey`] a partir de um [`PathBuf`].
    #[must_use]
    pub const fn new(path: PathBuf) -> Self {
        Self(path)
    }

    /// Retorna referência ao [`Path`] subjacente.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for RepoKey {
    fn from(value: PathBuf) -> Self {
        Self(value)
    }
}

/// Estado completo de inspeção Git associado a um caminho ou repositório.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum GitContext {
    /// Caminho válido que não pertence a nenhum repositório Git.
    #[serde(rename_all = "camelCase")]
    NonRepository {
        /// Caminho inspecionado.
        path: PathBuf,
        /// Revisão monotônica por chave.
        revision: u64,
        /// Timestamp UNIX em milissegundos da última atualização.
        refreshed_at_ms: i64,
    },
    /// Inspeção em modo degradado (ex.: `git` ausente, timeout ou falha de permissão).
    #[serde(rename_all = "camelCase")]
    Degraded {
        /// Caminho inspecionado.
        path: PathBuf,
        /// Motivo semântico da degradação.
        reason: DegradedReason,
        /// Mensagem descritiva sanitizada em `pt-BR`.
        message: String,
        /// Revisão monotônica por chave.
        revision: u64,
        /// Timestamp UNIX em milissegundos da última tentativa.
        refreshed_at_ms: i64,
    },
    /// Repositório ou worktree Git inspecionado com sucesso.
    #[serde(rename_all = "camelCase")]
    Repository {
        /// Chave canônica da worktree.
        repo_key: RepoKey,
        /// Diretório raiz canônico da worktree.
        worktree_root: PathBuf,
        /// Diretório `.git` efetivo desta worktree.
        git_dir: PathBuf,
        /// Diretório `.git` comum compartilhado (`common_dir`).
        common_dir: PathBuf,
        /// Estado do `HEAD` / branch.
        branch: GitBranch,
        /// Contadores de alterações da árvore de trabalho.
        changes: GitChanges,
        /// Divergência em relação ao upstream, se configurado.
        upstream: Option<GitUpstream>,
        /// Metadados da worktree atual.
        worktree: GitWorktree,
        /// Revisão monotônica estritamente crescente por `repo_key`.
        revision: u64,
        /// Timestamp UNIX em milissegundos da coleta.
        refreshed_at_ms: i64,
    },
}

impl GitContext {
    /// Retorna a revisão monotônica do contexto.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        match self {
            Self::NonRepository { revision, .. }
            | Self::Degraded { revision, .. }
            | Self::Repository { revision, .. } => *revision,
        }
    }

    /// Retorna o caminho principal representado pelo contexto.
    #[must_use]
    pub fn primary_path(&self) -> &Path {
        match self {
            Self::NonRepository { path, .. } | Self::Degraded { path, .. } => path.as_path(),
            Self::Repository { worktree_root, .. } => worktree_root.as_path(),
        }
    }

    /// Retorna a [`RepoKey`] quando o contexto é [`GitContext::Repository`].
    #[must_use]
    pub const fn repo_key(&self) -> Option<&RepoKey> {
        match self {
            Self::Repository { repo_key, .. } => Some(repo_key),
            Self::NonRepository { .. } | Self::Degraded { .. } => None,
        }
    }
}

/// Estado do `HEAD` no repositório.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum GitBranch {
    /// Branch nomeada com commit inicial existente (ou opcional).
    #[serde(rename_all = "camelCase")]
    Named {
        /// Nome curto da branch (ex.: `main`, `feat/x`).
        name: String,
        /// OID curto (8 chars) do commit atual, quando disponível.
        oid: Option<String>,
    },
    /// `HEAD` destacado (`detached HEAD`) apontando diretamente para um commit.
    #[serde(rename_all = "camelCase")]
    Detached {
        /// OID curto (8 chars) do commit.
        oid: String,
    },
    /// Branch recém-inicializada ainda sem commits (`oid == (initial)`).
    #[serde(rename_all = "camelCase")]
    Unborn {
        /// Nome da branch órfã/inicial.
        name: String,
    },
}

/// Contadores de alterações da árvore de trabalho e índice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitChanges {
    /// Quantidade de entradas com alteração no index (`staged`).
    pub staged: u32,
    /// Quantidade de entradas com alteração na working tree (`unstaged`).
    pub unstaged: u32,
    /// Quantidade de arquivos não rastreados (`untracked`).
    pub untracked: u32,
    /// Quantidade de entradas em conflito de merge (`unmerged`).
    pub conflicted: u32,
    /// Indica se há qualquer alteração local (`staged > 0 || unstaged > 0 || untracked > 0 || conflicted > 0`).
    pub dirty: bool,
    /// Indica se a saída excedeu o limite de segurança de entradas processadas.
    pub summary_truncated: bool,
}

/// Estado de sincronização em relação à branch de upstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitUpstream {
    /// Nome da referência upstream (ex.: `origin/main`).
    pub name: String,
    /// Quantidade de commits à frente do upstream (`+ahead`).
    pub ahead: u32,
    /// Quantidade de commits atrás do upstream (`-behind`).
    pub behind: u32,
}

/// Classificação canônica da origem/localização de uma worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeKind {
    /// Worktree principal do repositório (`git_dir == common_dir`).
    Main,
    /// Worktree isolada em `<main>/.worktrees/<nome>`.
    MaxTerminal,
    /// Worktree isolada em `<main>/.max-code-worktrees/<nome>`.
    MaxCode,
    /// Worktree vinculada localizada em outro caminho externo.
    External,
}

/// Metadados resumidos da worktree ativa de uma aba/painel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktree {
    /// Classificação da worktree.
    pub kind: WorktreeKind,
    /// Nome curto da worktree (último componente do caminho).
    pub name: String,
    /// Caminho canônico da worktree atual.
    pub path: PathBuf,
    /// Caminho canônico da worktree principal do repositório.
    pub main_worktree_path: PathBuf,
    /// `true` quando é uma linked worktree (`kind != Main`).
    pub is_linked: bool,
    /// `true` quando a worktree possui lock ativo no Git.
    pub locked: bool,
    /// `true` quando o Git reporta a worktree como `prunable`.
    pub prunable: bool,
}

/// Motivo semântico para um [`GitContext::Degraded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DegradedReason {
    /// Binário `git` não encontrado no `$PATH`.
    GitBinaryMissing,
    /// Execução de subprocesso `git` excedeu o timeout.
    CommandTimeout,
    /// Subprocesso `git` retornou erro não recuperável.
    CommandFailed,
    /// Estrutura `.git` inválida ou corrompida.
    InvalidRepository,
    /// Sem permissão de leitura no repositório.
    PermissionDenied,
}

/// Item detalhado retornado por `git_worktree_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeEntry {
    /// Caminho da worktree.
    pub path: PathBuf,
    /// OID completo ou curto do `HEAD` da worktree.
    pub head_oid: String,
    /// Nome curto da branch (`refs/heads/` removido), se não estiver em detached HEAD.
    pub branch: Option<String>,
    /// `true` se `HEAD` estiver destacado.
    pub detached: bool,
    /// `true` se for repositório bare.
    pub bare: bool,
    /// Motivo do lock caso a worktree esteja bloqueada (`Some("")` se sem motivo).
    pub locked: Option<String>,
    /// Motivo de poda caso a worktree seja podável (`Some("")` se sem motivo).
    pub prunable: Option<String>,
    /// Classificação da worktree.
    pub kind: WorktreeKind,
    /// Nome curto da worktree.
    pub name: String,
    /// `true` se for a worktree principal (`kind == WorktreeKind::Main`).
    pub is_main: bool,
}

/// Requisição para criação de worktree gerenciada (`git_worktree_create`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreateRequest {
    /// Caminho raiz do projeto ou de qualquer worktree pertencente ao repositório.
    pub project_root: PathBuf,
    /// Nome da pasta da worktree (`^[A-Za-z0-9._-]{1,64}$`).
    pub name: String,
    /// Nome opcional da branch a criar/usar (se `None`, usa `wt/<name>`).
    pub branch: Option<String>,
    /// Referência base opcional (se `None`, usa `HEAD`).
    pub base_ref: Option<String>,
    /// Pasta gerenciada onde a worktree será criada.
    pub location: WorktreeLocation,
}

/// Localização gerenciada onde novas worktrees podem ser criadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeLocation {
    /// Subpasta `<main_worktree>/.worktrees/<name>`.
    MaxTerminal,
    /// Subpasta `<main_worktree>/.max-code-worktrees/<name>`.
    MaxCode,
}

/// Razão pela qual uma atualização de status Git foi disparada.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RefreshReason {
    /// Primeira consulta para o caminho/painel.
    Initial,
    /// Evento de filesystem (`inotify`) em metadados do `.git`.
    FsEvent,
    /// Mudança de diretório (`shell://cwd-changed`) no painel.
    CwdChanged,
    /// Conclusão de bloco de comando (`shell://command-finished`).
    CommandFinished,
    /// Solicitação explícita via `git_status_refresh`.
    Manual,
    /// Criação ou remoção de worktree via `git_worktree_create` / `git_worktree_remove`.
    WorktreeMutated,
}

/// Payload do evento `git://status-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusChanged {
    /// Novo contexto Git consolidado.
    pub context: GitContext,
    /// Painéis atualmente vinculados ao `repo_key` (ou caminho).
    pub affected_pane_ids: Vec<PaneId>,
    /// Abas proprietárias dos painéis afetados.
    pub affected_tab_ids: Vec<TabId>,
    /// Causa do refresh.
    pub reason: RefreshReason,
}
