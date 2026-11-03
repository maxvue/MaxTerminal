//! Sincronizador `MaxSkills` (`catalog`, `targets`, `conflict`, `sync`, `watch`).

pub mod catalog;
pub mod conflict;
pub mod sync;
pub mod targets;
pub mod watch;

use serde::{Deserialize, Serialize};

use mt_core::ProjectId;

use crate::registry::AgentKind;

pub use catalog::{
    MAX_SKILL_BYTES, MAX_SKILL_DEPTH, MAX_SKILL_FILES, SKILL_MANIFEST_FILENAME,
    compute_skill_tree_hash, scan_catalog,
};
pub use conflict::{ManagedEntryRecord, classify_target_actions, compute_plan_fingerprint};
pub use sync::{PLAN_TTL_MS, SkillsService};
pub use targets::{agent_skill_dir, canonical_or_lexical_path, resolve_target_dest_dir};
pub use watch::{SkillsChangedPayload, SkillsWatcher};

/// Relevância de uma skill em relação à stack do projeto ativo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SkillRelevance {
    /// A skill declara suporte explícito à stack do projeto.
    Recommended,
    /// A skill é genérica ou de outra stack.
    Neutral,
}

/// Entrada individual de skill descoberta no catálogo `MaxSkills`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillEntry {
    /// Nome canônico da skill (`^[a-z0-9][a-z0-9._-]{0,63}$`).
    pub name: String,
    /// Descrição curta da skill.
    pub description: String,
    /// Caminho absoluto do diretório da skill na fonte (`contém SKILL.md`).
    pub source_dir: String,
    /// Tags associadas à skill.
    pub tags: Vec<String>,
    /// Stacks associadas (`"laravel"`, `"vue"`, `"rust"`, `"tauri"`, etc.).
    pub stacks: Vec<String>,
    /// Hash determinístico SHA-256 da árvore da skill.
    pub hash: String,
    /// Relevância calculada para o projeto consultado.
    pub relevance: SkillRelevance,
}

/// Catálogo completo resultante do scan do diretório fonte de `MaxSkills`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalog {
    /// Caminho resolvido do diretório fonte escaneado.
    pub source_dir: String,
    /// Indica se os metadados vieram de `index.json` validado (`true`) ou fallback `SKILL.md` (`false`).
    pub from_index: bool,
    /// Lista ordenada de skills encontradas.
    pub skills: Vec<SkillEntry>,
    /// Timestamp UNIX em ms do scan.
    pub scanned_at_ms: i64,
}

/// Escopo de sincronização de um alvo de skills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SyncScope {
    /// Sincroniza na pasta do projeto/worktree.
    Project,
    /// Sincroniza no diretório global do usuário para o agente.
    Global,
}

impl SyncScope {
    /// Representação textual no SQLite (`'project' | 'global'`).
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }

    /// Converte do valor persistido no SQLite.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "project" => Some(Self::Project),
            "global" => Some(Self::Global),
            _ => None,
        }
    }
}

/// Modo de instalação das skills no destino.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SyncMode {
    /// Cria link simbólico apontando para o diretório da skill na fonte.
    Symlink,
    /// Copia os arquivos da skill + `.maxterminal-skill.json`.
    Copy,
}

impl SyncMode {
    /// Representação textual no SQLite (`'symlink' | 'copy'`).
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Symlink => "symlink",
            Self::Copy => "copy",
        }
    }

    /// Converte do valor persistido no SQLite.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "symlink" => Some(Self::Symlink),
            "copy" => Some(Self::Copy),
            _ => None,
        }
    }
}

/// Filtro de seleção de skills para um alvo de sincronização.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SkillSelection {
    /// Sincroniza apenas skills recomendadas para a stack do projeto.
    Recommended,
    /// Sincroniza todas as skills do catálogo.
    All,
    /// Sincroniza apenas a lista explícita de nomes de skills.
    Explicit {
        /// Nomes das skills selecionadas.
        names: Vec<String>,
    },
}

/// Configuração de um alvo de sincronização de skills (`skill_sync_targets`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillSyncTarget {
    /// ID do alvo no SQLite (`None` ao criar novo).
    pub id: Option<i64>,
    /// Agente CLI destinatário.
    pub agent: AgentKind,
    /// Escopo (`project` ou `global`).
    pub scope: SyncScope,
    /// Projeto associado quando `scope == Project`.
    pub project_id: Option<ProjectId>,
    /// Modo de sincronização (`symlink` ou `copy`).
    pub mode: SyncMode,
    /// Se verdadeiro, sincroniza automaticamente mudanças sem conflito detectadas pelo watcher.
    pub auto_sync: bool,
    /// Se o alvo está habilitado.
    pub enabled: bool,
    /// Critério de seleção de skills.
    pub selection: SkillSelection,
    /// Raiz efetiva do projeto ou worktree (quando `scope == Project`).
    pub project_root: Option<String>,
    /// Caminho de destino explícito (obrigatório para `agy`).
    pub dest_override: Option<String>,
    /// Caminho resolvido do diretório de destino (calculado pelo backend).
    pub dest_dir: Option<String>,
}

/// Natureza do conflito detectado no destino durante `skills_plan`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConflictKind {
    /// Existe um symlink apontando para outro destino externo.
    ForeignSymlink {
        /// Destino atual do symlink existente.
        target: String,
    },
    /// Existe um diretório regular não gerenciado pelo MaxTerminal.
    Unmanaged,
    /// Existe uma cópia gerenciada cujo conteúdo foi modificado localmente após o último sync.
    LocallyModified {
        /// Hash esperado que foi sincronizado pelo MaxTerminal.
        expected: String,
        /// Hash atual encontrado no disco.
        actual: String,
    },
}

/// Ação individual planejada para uma skill em `SyncPlan`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum SyncAction {
    /// Criar nova entrada (symlink ou cópia) no destino.
    Create {
        /// Nome da skill.
        skill: String,
        /// Caminho completo no destino.
        dest: String,
    },
    /// Atualizar entrada já gerenciada e não modificada localmente.
    Update {
        /// Nome da skill.
        skill: String,
        /// Caminho completo no destino.
        dest: String,
    },
    /// Entrada já está sincronizada e idêntica à fonte.
    Unchanged {
        /// Nome da skill.
        skill: String,
        /// Caminho completo no destino.
        dest: String,
    },
    /// Conflito detectado que exige resolução explícita.
    Conflict {
        /// Nome da skill.
        skill: String,
        /// Caminho completo no destino.
        dest: String,
        /// Detalhes do conflito.
        conflict: ConflictKind,
    },
    /// Remover entrada gerenciada que saiu da seleção do alvo (e não foi modificada localmente).
    Prune {
        /// Nome da skill.
        skill: String,
        /// Caminho completo no destino.
        dest: String,
    },
}

/// Plano dry-run retornado por `skills_plan` (somente leitura, válido por 5 min).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlan {
    /// Identificador único do plano em memória.
    pub plan_id: String,
    /// ID do alvo de sincronização.
    pub target_id: i64,
    /// Diretório de destino resolvido.
    pub dest_dir: String,
    /// Ações planejadas por skill.
    pub actions: Vec<SyncAction>,
    /// Avisos informativos em pt-BR.
    pub warnings: Vec<String>,
    /// Hash SHA-256 do estado observado da fonte, destino e seleção.
    pub fingerprint: String,
}

/// Estratégia escolhida pelo usuário para resolver um item em conflito.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ConflictResolution {
    /// Mantém o arquivo/pasta existente e pula a sincronização deste item.
    Keep,
    /// Faz backup completo do item existente em `skills-backup/` e sobrescreve com a fonte.
    Overwrite,
    /// Adota o item existente como gerenciado (permitido somente se já apontar para a fonte ou tiver hash idêntico).
    Adopt,
}

/// Resultado da execução de `skills_apply`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    /// ID do alvo sincronizado.
    pub target_id: i64,
    /// Quantidade de ações aplicadas com sucesso (`Create`, `Update`, `Prune`, `Overwrite`, `Adopt`).
    pub applied: u32,
    /// Quantidade de ações mantidas/puladas (`Unchanged`, `Keep`).
    pub skipped: u32,
    /// Caminhos absolutos dos backups criados antes de `Overwrite` ou `Prune`.
    pub backups: Vec<String>,
    /// Mensagens de erro em pt-BR para itens que falharam individualmente.
    pub errors: Vec<String>,
}

/// Manifesto gravado dentro de cópias gerenciadas (`.maxterminal-skill.json`).
/// Nunca é gravado em modo `Symlink` para não sujar o repositório fonte (`CA-28`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCopyManifest {
    /// Versão do schema do manifesto (`1`).
    pub version: u8,
    /// Nome da skill.
    pub skill_name: String,
    /// Caminho da fonte no momento da cópia.
    pub source_path: String,
    /// Hash SHA-256 da árvore da fonte.
    pub source_hash: String,
    /// Hash SHA-256 sincronizado.
    pub synced_hash: String,
    /// Identificador da operação de sync.
    pub operation_id: String,
    /// Timestamp UNIX em ms da sincronização.
    pub synced_at_ms: i64,
}
