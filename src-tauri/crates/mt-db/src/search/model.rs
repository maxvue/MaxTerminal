//! Modelos de domínio e DTOs tipados (`specta::Type`) do motor de Busca Global FTS5 (Fase 17).

use mt_core::ids::{
    CommandBlockId, CommandGroupId, PaneId, ProjectId, SessionId, TabId, TemplateId,
};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Requisição paginada de busca global disparada pela UI (`search_global`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    /// Identificador único correlacionado da requisição (`request_id`).
    pub request_id: String,
    /// Geração monotônica controlada pela `useSearchStore`.
    pub generation: u32,
    /// Consulta textual do usuário (até 256 caracteres / 1024 bytes).
    pub query: String,
    /// Filtros opcionais combinados (`AND`).
    pub filters: SearchFilters,
    /// Ordenação dentro de cada seção (`relevance` ou `recent`).
    pub sort: SearchSort,
    /// Tamanho de página por seção (padrão `25`, máximo `100`).
    pub page_size: u16,
    /// Cursor autenticado para paginação subsequente de uma seção específica.
    pub cursor: Option<String>,
}

/// Filtros estruturados aplicáveis à busca global.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchFilters {
    /// Restringe a um projeto específico.
    pub project_id: Option<ProjectId>,
    /// Restringe a uma aba específica.
    pub tab_id: Option<TabId>,
    /// Restringe a uma sessão específica.
    pub session_id: Option<SessionId>,
    /// Restringe a um painel específico.
    pub pane_id: Option<PaneId>,
    /// Limite inferior inclusivo de timestamp UNIX em milissegundos (`occurred_at >= from_ms`).
    pub from_ms: Option<i64>,
    /// Limite superior exclusivo de timestamp UNIX em milissegundos (`occurred_at < until_ms`).
    pub until_ms: Option<i64>,
    /// Filtro de código de saída (aplicável apenas a fontes com código de saída).
    pub exit: ExitFilter,
    /// Subconjunto de fontes a consultar (vazio = todas as fontes compatíveis na ordem canônica).
    pub sources: Vec<SearchSourceKind>,
}

/// Filtro semântico sobre `exit_code`.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExitFilter {
    /// Qualquer código de saída (incluindo fontes sem código de saída).
    #[default]
    Any,
    /// Sucesso (`exit_code == 0`).
    Zero,
    /// Falha (`exit_code IS NOT NULL AND exit_code != 0`).
    Nonzero,
    /// Indeterminado/em andamento (`exit_code IS NULL`).
    Unknown,
}

/// Critério de ordenação intra-seção.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchSort {
    /// Relevância textual (`bm25` ascendente + `occurred_at_ms` descendente + `key` ascendente).
    #[default]
    Relevance,
    /// Mais recente primeiro (`occurred_at_ms` descendente + `key` ascendente).
    Recent,
}

/// Categoria canônica de fonte de busca global.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Type,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchSourceKind {
    /// Blocos de comando (`command_blocks_fts`).
    Commands,
    /// Saída bruta / scrollback de sessão (`session_scrollback_fts`).
    Output,
    /// Sessões ativas, arquivadas ou legadas (`search_metadata_fts`).
    Sessions,
    /// Projetos cadastrados (`search_metadata_fts`).
    Projects,
    /// Templates de aba (`search_metadata_fts`).
    Templates,
    /// Grupos de comandos locais e globais (`search_metadata_fts`).
    Groups,
    /// Execuções históricas de grupos de comandos (`search_metadata_fts`).
    GroupRuns,
    /// Issues do GitHub cacheadas localmente (`search_metadata_fts`).
    Issues,
    /// Comentários de issues do GitHub (`search_metadata_fts`).
    Comments,
}

impl SearchSourceKind {
    /// Ordem canônica fixa das seções na UI e no motor de busca.
    pub const ALL_ORDERED: [Self; 9] = [
        Self::Commands,
        Self::Output,
        Self::Sessions,
        Self::Projects,
        Self::Templates,
        Self::Groups,
        Self::GroupRuns,
        Self::Issues,
        Self::Comments,
    ];

    /// Retorna o identificador textual em `snake_case` usado no SQL e logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Commands => "commands",
            Self::Output => "output",
            Self::Sessions => "sessions",
            Self::Projects => "projects",
            Self::Templates => "templates",
            Self::Groups => "groups",
            Self::GroupRuns => "group_runs",
            Self::Issues => "issues",
            Self::Comments => "comments",
        }
    }

    /// Converte uma string `snake_case` em `SearchSourceKind`.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "commands" => Some(Self::Commands),
            "output" => Some(Self::Output),
            "sessions" => Some(Self::Sessions),
            "projects" => Some(Self::Projects),
            "templates" => Some(Self::Templates),
            "groups" => Some(Self::Groups),
            "group_runs" => Some(Self::GroupRuns),
            "issues" => Some(Self::Issues),
            "comments" => Some(Self::Comments),
            _ => None,
        }
    }

    /// Verifica se esta fonte é compatível com os filtros fornecidos.
    #[must_use]
    pub const fn is_compatible_with(self, filters: &SearchFilters) -> bool {
        if !matches!(filters.exit, ExitFilter::Any)
            && !matches!(self, Self::Commands | Self::GroupRuns)
        {
            return false;
        }
        if filters.pane_id.is_some() && !matches!(self, Self::Commands | Self::Output) {
            return false;
        }
        if (filters.tab_id.is_some() || filters.session_id.is_some())
            && !matches!(self, Self::Commands | Self::Output | Self::Sessions)
        {
            return false;
        }
        true
    }
}

/// Segmento de texto com marcação de destaque segura (sem HTML cru).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct HighlightSegment {
    /// Texto literal sanitizado.
    pub text: String,
    /// Indica se este trecho casou com a consulta FTS5.
    pub matched: bool,
}

/// Resultado individual encontrado por uma fonte de busca.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// Chave única estável do item (`<source>:<id>`).
    pub key: String,
    /// Fonte dona deste resultado.
    pub source: SearchSourceKind,
    /// Alvo tipado para navegação contextual na UI.
    pub target: SearchTarget,
    /// Título segmentado com destaques.
    pub title: Vec<HighlightSegment>,
    /// Excerto (snippet) segmentado com destaques.
    pub excerpt: Vec<HighlightSegment>,
    /// Timestamp UNIX em milissegundos de ocorrência do evento/item.
    pub occurred_at_ms: Option<i64>,
    /// Indica se a fonte sofreu truncamento prévio no armazenamento.
    pub source_truncated: bool,
    /// Timestamp de sincronização (ex.: cache de issue do GitHub).
    pub fetched_at_ms: Option<i64>,
}

/// Destino navegável de um resultado da busca global.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SearchTarget {
    /// Bloco de comando individual.
    #[serde(rename_all = "camelCase")]
    Command {
        /// Projeto associado, se houver.
        project_id: Option<ProjectId>,
        /// Aba associada, se houver.
        tab_id: Option<TabId>,
        /// Sessão onde o comando foi executado.
        session_id: SessionId,
        /// Painel associado, se houver.
        pane_id: Option<PaneId>,
        /// Identificador único do bloco de comando.
        block_id: CommandBlockId,
    },
    /// Chunk de scrollback de uma sessão.
    #[serde(rename_all = "camelCase")]
    Output {
        /// Projeto associado, se houver.
        project_id: Option<ProjectId>,
        /// Aba associada, se houver.
        tab_id: Option<TabId>,
        /// Sessão proprietária do chunk.
        session_id: SessionId,
        /// Painel associado, se houver.
        pane_id: Option<PaneId>,
        /// Sequência monotônica do chunk na sessão.
        chunk_seq: i64,
    },
    /// Sessão de terminal (ativa, arquivada ou legada).
    #[serde(rename_all = "camelCase")]
    Session {
        /// Projeto associado, se houver.
        project_id: Option<ProjectId>,
        /// Aba associada, se houver.
        tab_id: Option<TabId>,
        /// Identificador único da sessão.
        session_id: SessionId,
    },
    /// Projeto cadastrado.
    #[serde(rename_all = "camelCase")]
    Project {
        /// Identificador único do projeto.
        project_id: ProjectId,
    },
    /// Template de aba de um projeto.
    #[serde(rename_all = "camelCase")]
    Template {
        /// Projeto proprietário do template.
        project_id: ProjectId,
        /// Identificador único do template.
        template_id: TemplateId,
    },
    /// Grupo de comandos local ou global.
    #[serde(rename_all = "camelCase")]
    Group {
        /// Identificador único do grupo de comandos.
        group_id: CommandGroupId,
        /// Projeto proprietário (`None` para grupos globais).
        project_id: Option<ProjectId>,
    },
    /// Execução histórica de um grupo de comandos.
    #[serde(rename_all = "camelCase")]
    GroupRun {
        /// Identificador da execução do grupo.
        run_key: String,
        /// Identificador opcional do item da execução.
        item_key: Option<String>,
        /// Projeto onde a execução foi disparada.
        project_id: Option<ProjectId>,
    },
    /// Issue ou comentário de issue do GitHub.
    #[serde(rename_all = "camelCase")]
    Issue {
        /// Chave canônica `owner/repo` do repositório.
        repo_key: String,
        /// Número da issue no GitHub.
        number: u32,
        /// Chave opcional do comentário dentro da issue.
        comment_key: Option<String>,
        /// Projeto local vinculado ao repositório.
        project_id: ProjectId,
    },
}

/// Seção de resultados agrupada por `SearchSourceKind`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchSection {
    /// Fonte desta seção.
    pub source: SearchSourceKind,
    /// Resultados da página atual desta seção.
    pub items: Vec<SearchHit>,
    /// Cursor autenticado para carregar a próxima página desta seção (`None` se esgotada).
    pub next_cursor: Option<String>,
    /// Indica se a coleta desta seção foi cortada pelo limite global do snapshot (`10.000` hits / `16 MiB`).
    pub truncated: bool,
}

/// Resposta completa de `search_global`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    /// Ecoado da requisição.
    pub request_id: String,
    /// Ecoado da requisição.
    pub generation: u32,
    /// Identificador opaco do snapshot materializado.
    pub snapshot_id: String,
    /// Timestamp UNIX em milissegundos de expiração do snapshot.
    pub expires_at_ms: i64,
    /// Seções na ordem canônica.
    pub sections: Vec<SearchSection>,
    /// Timestamp máximo indexado até o momento da consulta.
    pub indexed_through_ms: Option<i64>,
}

/// Confirmação de cancelamento de uma busca em voo (`search_cancel`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct CancelOutcome {
    /// `true` se a requisição estava ativa ou foi registrada como cancelada.
    pub cancelled: bool,
}

/// Estatísticas de saúde e volume de cada fonte FTS5 (`search_index_stats`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexStat {
    /// Fonte inspecionada.
    pub source: SearchSourceKind,
    /// Indica se a tabela FTS5 está íntegra e pronta.
    pub ready: bool,
    /// Total de linhas indexadas para esta fonte.
    pub row_count: u64,
    /// Maior timestamp `occurred_at` observado na fonte.
    pub updated_at_ms: Option<i64>,
    /// ID de job de reconstrução ativo, se houver.
    pub active_job_id: Option<String>,
}

/// Handle retornado ao disparar `search_index_rebuild`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct RebuildJobHandle {
    /// Identificador único do job de rebuild.
    pub job_id: String,
}

/// Payload emitido pelo evento `search://index-progress`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexProgressEvent {
    /// ID do job de rebuild.
    pub job_id: String,
    /// Fonte atualmente sendo reconstruída.
    pub source: SearchSourceKind,
    /// Quantidade de fontes já concluídas.
    pub completed_sources: u32,
    /// Total de fontes solicitadas no job.
    pub total_sources: u32,
    /// Indica se o job inteiro terminou.
    pub done: bool,
}

/// Payload emitido pelo evento `search://invalidated`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchInvalidatedEvent {
    /// Motivo da invalidação (ex.: `"project_removed"`, `"retention_purged"`, `"index_rebuilt"`).
    pub reason: String,
    /// IDs de snapshots invalidados.
    pub invalidated_snapshot_ids: Vec<String>,
}
