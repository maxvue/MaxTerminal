//! Modelos de domínio, DTOs IPC e enums do crate `mt-projects` (Fase 06).

use std::fmt;
use std::str::FromStr;

use mt_core::ids::ProjectId;
use mt_core::{MtError, MtResult};
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;
use uuid::Uuid;

/// Identificador único de uma "Pasta de Projetos" (`project_roots`), baseado em UUID v7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Type)]
#[serde(transparent)]
#[specta(transparent)]
pub struct ProjectRootId(pub Uuid);

impl ProjectRootId {
    /// Cria um novo identificador com timestamp atual (UUID v7).
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Reconstrói o identificador validando que o UUID é estritamente da versão 7.
    pub fn try_from_uuid(uuid: Uuid) -> MtResult<Self> {
        if uuid.get_version_num() != 7 {
            return Err(MtError::invalid_argument(
                "ProjectRootId",
                "identificador inválido: esperado UUID v7",
            ));
        }
        Ok(Self(uuid))
    }

    /// Retorna a referência ao `Uuid` subjacente.
    #[must_use]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for ProjectRootId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProjectRootId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ProjectRootId {
    type Err = MtError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(s).map_err(|_| {
            MtError::invalid_argument("ProjectRootId", "identificador inválido: esperado UUID v7")
        })?;
        Self::try_from_uuid(uuid)
    }
}

impl TryFrom<Uuid> for ProjectRootId {
    type Error = MtError;

    fn try_from(value: Uuid) -> Result<Self, Self::Error> {
        Self::try_from_uuid(value)
    }
}

impl<'de> Deserialize<'de> for ProjectRootId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::from_str(&raw).map_err(serde::de::Error::custom)
    }
}

/// Stack tecnológica primária ou detectada de um projeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StackKind {
    /// Aplicação Desktop Tauri (`src-tauri/tauri.conf.json`).
    Tauri,
    /// Extensão VS Code (`engines.vscode` em `package.json`).
    VscodeExtension,
    /// Projeto PHP Laravel (`artisan` + `composer.json`).
    Laravel,
    /// Aplicação ou biblioteca Vue (`vue` em dependências do `package.json`).
    Vue,
    /// Crate ou workspace Rust (`Cargo.toml`).
    Rust,
    /// Projeto Python (`pyproject.toml` ou `requirements.txt`).
    Python,
    /// Stack não identificada pelas regras canônicas.
    Unknown,
}

impl StackKind {
    /// Representação textual persistida no SQLite.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Tauri => "tauri",
            Self::VscodeExtension => "vscode_extension",
            Self::Laravel => "laravel",
            Self::Vue => "vue",
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Unknown => "unknown",
        }
    }

    /// Converte a string persistida no SQLite em [`StackKind`].
    #[must_use]
    pub fn from_db_str(raw: &str) -> Self {
        match raw {
            "tauri" => Self::Tauri,
            "vscode_extension" => Self::VscodeExtension,
            "laravel" => Self::Laravel,
            "vue" => Self::Vue,
            "rust" => Self::Rust,
            "python" => Self::Python,
            _ => Self::Unknown,
        }
    }
}

/// Origem da identidade visual (`icon` e `color`) de um projeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySource {
    /// Definida automaticamente pela detecção de stack ou fallback determinístico.
    Auto,
    /// Customizada explicitamente pelo usuário no cadastro ou edição.
    Manual,
}

impl IdentitySource {
    /// Representação textual persistida no SQLite.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }

    /// Converte a string persistida no SQLite em [`IdentitySource`].
    #[must_use]
    pub fn from_db_str(raw: &str) -> Self {
        match raw {
            "manual" => Self::Manual,
            _ => Self::Auto,
        }
    }
}

/// Modo de tratamento para novas subpastas descobertas em uma Pasta de Projetos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    /// Sugere a subpasta no painel "Novas pastas detectadas" aguardando aceite ou dispensa.
    Suggest,
    /// Importa automaticamente a subpasta como projeto cadastrado.
    AutoImport,
}

impl DiscoveryMode {
    /// Representação textual persistida no SQLite.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Suggest => "suggest",
            Self::AutoImport => "auto_import",
        }
    }

    /// Converte a string persistida no SQLite em [`DiscoveryMode`].
    #[must_use]
    pub fn from_db_str(raw: &str) -> Self {
        match raw {
            "auto_import" => Self::AutoImport,
            _ => Self::Suggest,
        }
    }
}

/// Estado em tempo de execução do observador de arquivos (`notify`) de uma Pasta de Projetos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WatchStatus {
    /// Monitoramento inotify ativo na pasta-mãe.
    Watching,
    /// Monitoramento desabilitado pelo usuário (`watch_enabled = false`).
    Disabled,
    /// Diretório da pasta-mãe ausente no disco (aguardando backoff).
    Missing,
    /// Falha ao registrar observador (ex.: limite de watches do kernel atingido).
    Error,
}

/// Identidade visual e tecnológica resolvida para um diretório de projeto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIdentity {
    /// Identificador Iconify (`prefixo:nome`, ex.: `"logos:laravel"`).
    pub icon: String,
    /// Cor hexadecimal em caixa alta (`#RRGGBB`).
    pub color: String,
    /// Stack primária (`detected[0]`).
    pub stack: StackKind,
    /// Todas as stacks encontradas na pasta, em ordem estrita de prioridade.
    pub detected: Vec<StackKind>,
}

/// Projeto cadastrado no `MaxTerminal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// Identificador único (UUID v7).
    pub id: ProjectId,
    /// Título visível na lista lateral (`1..=120` caracteres).
    pub title: String,
    /// Ícone Iconify.
    pub icon: String,
    /// Cor hexadecimal (`#RRGGBB`).
    pub color: String,
    /// Caminho absoluto canônico da pasta do projeto.
    pub path: String,
    /// Identificador opcional da Pasta de Projetos de origem primária.
    pub root_id: Option<ProjectRootId>,
    /// Stack tecnológica detectada.
    pub stack: StackKind,
    /// Fonte da identidade visual (`auto` ou `manual`).
    pub identity_source: IdentitySource,
    /// Posição de ordenação (`0..N-1`).
    pub sort_order: i64,
    /// Indica se o diretório não existe mais no disco na última checagem.
    pub missing: bool,
    /// Timestamp UNIX em ms UTC de criação.
    pub created_at: i64,
    /// Timestamp UNIX em ms UTC da última atualização.
    pub updated_at: i64,
}

/// Pasta-mãe ("Pasta de Projetos", ex.: `~/GitHub`) monitorada pelo `MaxTerminal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRoot {
    /// Identificador único (UUID v7).
    pub id: ProjectRootId,
    /// Caminho absoluto canônico da pasta-mãe.
    pub path: String,
    /// Modo de descoberta (`suggest` ou `auto_import`).
    pub discovery_mode: DiscoveryMode,
    /// Indica se o monitoramento automático via filesystem watcher está ativo.
    pub watch_enabled: bool,
    /// Estado operacional atual do watcher em memória.
    pub watch_status: WatchStatus,
    /// Timestamp UNIX em ms UTC de criação.
    pub created_at: i64,
    /// Timestamp UNIX em ms UTC da última atualização.
    pub updated_at: i64,
}

/// Payload de entrada para cadastrar um projeto individual (`project_add`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewProject {
    /// Caminho da pasta (aceita `~` / `~/...`).
    pub path: String,
    /// Título opcional (quando omitido, usa o nome da pasta).
    pub title: Option<String>,
    /// Ícone opcional (quando informado junto ou separado de `color`, define `identity_source = manual`).
    pub icon: Option<String>,
    /// Cor hexadecimal opcional (`#RRGGBB`).
    pub color: Option<String>,
}

/// Payload de atualização parcial de um projeto (`project_update`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPatch {
    /// Novo título opcional.
    pub title: Option<String>,
    /// Novo ícone opcional.
    pub icon: Option<String>,
    /// Nova cor hexadecimal opcional.
    pub color: Option<String>,
    /// Quando `Some(true)`, restaura `identity_source = auto` e aplica a identidade da stack detectada.
    pub reset_identity: Option<bool>,
}

/// Payload de entrada para cadastrar uma Pasta de Projetos (`project_root_add`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewProjectRoot {
    /// Caminho da pasta-mãe (aceita `~` / `~/...`).
    pub path: String,
    /// Modo de descoberta para novas subpastas.
    pub discovery_mode: DiscoveryMode,
    /// Quando `true`, importa imediatamente todas as subpastas elegíveis no cadastro inicial.
    pub import_existing: bool,
    /// Quando `true`, monitora automaticamente a criação/remoção de subpastas de nível 1.
    pub watch_enabled: bool,
}

/// Payload de atualização parcial de uma Pasta de Projetos (`project_root_update`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRootPatch {
    /// Novo modo de descoberta opcional.
    pub discovery_mode: Option<DiscoveryMode>,
    /// Nova flag de monitoramento opcional.
    pub watch_enabled: Option<bool>,
}

/// Subpasta descoberta em uma Pasta de Projetos e ainda pendente de aceite/dispensa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredFolder {
    /// Identificador da Pasta de Projetos representante.
    pub root_id: ProjectRootId,
    /// Caminho absoluto canônico da subpasta.
    pub path: String,
    /// Título sugerido higienizado (nome da pasta).
    pub suggested_title: String,
    /// Identidade visual e stack detectada.
    pub identity: ProjectIdentity,
    /// Timestamp UNIX em ms UTC da descoberta.
    pub discovered_at: i64,
}

/// Aviso não fatal ocorrido durante varredura, detecção de stack ou registro de watcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanWarning {
    /// Caminho relacionado ao aviso.
    pub path: String,
    /// Código estável do aviso (ex.: `project_root.watch_failed`, `core.permission_denied`).
    pub code: String,
    /// Mensagem descritiva em `pt-BR` sem conteúdo sensível.
    pub message: String,
}

/// Relatório retornado ao adicionar ou reescanear uma Pasta de Projetos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RootImportReport {
    /// Estado atualizado da Pasta de Projetos.
    pub root: ProjectRoot,
    /// Projetos importados automaticamente na operação.
    pub imported: Vec<Project>,
    /// Subpastas adicionadas/atualizadas na lista de sugestões.
    pub suggested: Vec<DiscoveredFolder>,
    /// Total de entradas ignoradas (ocultas, arquivos, já cadastradas ou dispensadas).
    pub skipped: u32,
    /// Avisos localizados coletados durante a varredura/watcher.
    pub warnings: Vec<ScanWarning>,
    /// Indica se a varredura de nível 1 foi concluída sem erros parciais de permissão/I/O.
    pub complete: bool,
}

/// Visão consistente e versionada de todo o domínio de projetos (`project_snapshot`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    /// UUID v4 gerado no boot do processo para identificar a época em memória.
    pub epoch: String,
    /// Contador monotônico de revisão em tempo de execução.
    pub revision: u32,
    /// Lista ordenada de projetos cadastrados.
    pub projects: Vec<Project>,
    /// Lista ordenada de Pastas de Projetos.
    pub roots: Vec<ProjectRoot>,
    /// Lista ordenada de sugestões pendentes.
    pub discovered: Vec<DiscoveredFolder>,
}
