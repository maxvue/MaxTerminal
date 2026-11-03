//! Modelos de domínio e DTOs serializáveis para os Atalhos Universais (`/`).

use serde::{Deserialize, Serialize};

/// Identificadores canônicos dos 7 atalhos universais do projeto (`AGENTS.md` §8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum ShortcutId {
    /// `/install`
    Install,
    /// `/install-force`
    InstallForce,
    /// `/update`
    Update,
    /// `/update-force`
    UpdateForce,
    /// `/build`
    Build,
    /// `/deploy`
    Deploy,
    /// `/test`
    Test,
}

impl ShortcutId {
    /// Lista estática ordenada dos 7 atalhos universais.
    pub const ALL: [Self; 7] = [
        Self::Install,
        Self::InstallForce,
        Self::Update,
        Self::UpdateForce,
        Self::Build,
        Self::Deploy,
        Self::Test,
    ];

    /// Rótulo visível no popup de autocompletar (`"/install"`, etc.).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Install => "/install",
            Self::InstallForce => "/install-force",
            Self::Update => "/update",
            Self::UpdateForce => "/update-force",
            Self::Build => "/build",
            Self::Deploy => "/deploy",
            Self::Test => "/test",
        }
    }

    /// Descrição em `pt-BR` do atalho.
    #[must_use]
    pub const fn description_pt_br(self) -> &'static str {
        match self {
            Self::Install => "Instalar dependências do projeto",
            Self::InstallForce => "Reinstalar dependências ignorando cache (--force / --no-cache)",
            Self::Update => "Atualizar dependências dentro dos intervalos declarados",
            Self::UpdateForce => "Forçar atualização de dependências sem cache",
            Self::Build => "Compilar o projeto para produção",
            Self::Deploy => "Executar script de publicação/deploy do projeto",
            Self::Test => "Executar suítes de testes do projeto",
        }
    }

    /// Indica se a seleção do atalho exige confirmação explícita do usuário antes de inserir no input.
    #[must_use]
    pub const fn requires_confirmation(self) -> bool {
        matches!(self, Self::InstallForce | Self::UpdateForce | Self::Deploy)
    }
}

/// Gerenciadores de pacotes suportados pelo resolvedor de atalhos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PackageManager {
    /// PHP Composer (`composer.json`).
    Composer,
    /// Node Package Manager (`package.json` / `package-lock.json` / `npm-shrinkwrap.json`).
    Npm,
    /// Performant NPM (`pnpm-lock.yaml`).
    Pnpm,
    /// Yarn Classic ou Modern (`yarn.lock`).
    Yarn,
    /// Bun (`bun.lock` / `bun.lockb`).
    Bun,
    /// Rust Cargo (`Cargo.toml` ou `src-tauri/Cargo.toml`).
    Cargo,
}

impl PackageManager {
    /// Nome curto do executável CLI correspondente.
    #[must_use]
    pub const fn binary_name(self) -> &'static str {
        match self {
            Self::Composer => "composer",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
            Self::Cargo => "cargo",
        }
    }
}

/// Evidência de arquivo ou campo que fundamentou a detecção de um gerenciador de pacotes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ManagerEvidence {
    /// Caminho relativo do arquivo de manifesto ou lockfile.
    pub path: String,
    /// Campo inspecionado no manifesto (ex.: `"packageManager"`), quando aplicável.
    pub field: Option<String>,
    /// Valor observado ou resumo da evidência.
    pub value: String,
}

/// Gerenciador de pacotes detectado na raiz do projeto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DetectedManager {
    /// Tipo do gerenciador.
    pub manager: PackageManager,
    /// Caminho relativo do manifesto principal (`"composer.json"`, `"package.json"`, `"Cargo.toml"`, `"src-tauri/Cargo.toml"`).
    pub manifest: String,
    /// Lockfile principal associado, quando existente.
    pub lockfile: Option<String>,
    /// Versão ou variante detectada (ex.: `"9.12.0"`, `"classic"`, `"modern"`).
    pub version: Option<String>,
    /// Evidências coletadas na detecção.
    pub evidence: Vec<ManagerEvidence>,
    /// Explicação em `pt-BR` do motivo da escolha.
    pub reason: String,
}

/// Origem de uma etapa individual dentro de um atalho resolvido.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PartSource {
    /// Tabela canônica de comandos por gerenciador.
    Table,
    /// Script de `package.json`.
    PackageScript,
    /// Script de `composer.json`.
    ComposerScript,
    /// Comando Artisan (`php artisan test`).
    Artisan,
    /// Binário em `vendor/bin/` (`pest` / `phpunit`).
    VendorBin,
    /// Override configurado em `config-trabalho.json` do `MaxCode`.
    MaxCodeConfig,
}

/// Etapa individual de execução que compõe um [`ResolvedShortcut`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutPart {
    /// Gerenciador de pacotes associado (`None` quando vem de comando shell livre do `MaxCode`).
    pub manager: Option<PackageManager>,
    /// Diretório de trabalho onde a etapa será executada.
    pub cwd: String,
    /// Programa invocável (`None` apenas para `configured_shell` do `MaxCode`).
    pub program: Option<String>,
    /// Argumentos passados ao programa.
    pub args: Vec<String>,
    /// Linha de shell explícita vinda da configuração do `MaxCode`.
    pub configured_shell: Option<String>,
    /// Linha formatada desta etapa.
    pub line: String,
    /// Fonte da resolução desta etapa.
    pub source: PartSource,
}

/// Atalho universal resolvido para um diretório de projeto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedShortcut {
    /// Identificador do atalho.
    pub id: ShortcutId,
    /// Rótulo com barra (`"/install"`).
    pub label: String,
    /// Descrição em `pt-BR`.
    pub description: String,
    /// Linha completa pronta para inserção no input (`None` quando desabilitado).
    pub line: Option<String>,
    /// Etapas individuais que compõem `line`.
    pub parts: Vec<ShortcutPart>,
    /// Motivo em `pt-BR` pelo qual o atalho está desabilitado (`line == None`).
    pub disabled_reason: Option<String>,
    /// Indica se requer confirmação antes de inserir no input.
    pub requires_confirmation: bool,
    /// Hash SHA-256 das evidências de manifesto/lockfile/config usadas na resolução.
    pub evidence_revision: String,
}

/// Lista completa de atalhos e gerenciadores detectados para um `cwd`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutList {
    /// Raiz canônica onde os manifestos foram detectados.
    pub root: String,
    /// Gerenciadores detectados em ordem de composição (`Composer` → `JS` → `Cargo`).
    pub managers: Vec<DetectedManager>,
    /// Avisos de conflito ou inconsistência de lockfiles em `pt-BR`.
    pub warnings: Vec<String>,
    /// Os 7 atalhos universais resolvidos.
    pub items: Vec<ResolvedShortcut>,
}
