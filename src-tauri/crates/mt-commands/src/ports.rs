//! Portas hexagonais de entrada e saída para `mt-commands`.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mt_core::{MtResult, ProjectId};

use crate::artisan::model::{
    CatalogUpdatedPayload, CommandCreatedPayload, DiscoveryFailedPayload, RunnerKind,
};

/// Limites de tempo e memória para execução supervisionada de subprocessos de descoberta.
#[derive(Clone, Copy, Debug)]
pub struct RunLimits {
    /// Tempo máximo de execução antes de `SIGKILL` no grupo de processos.
    pub timeout: Duration,
    /// Limite máximo de bytes retidos de `stdout` (padrão 8 MiB).
    pub max_stdout: usize,
    /// Limite máximo de bytes retidos no buffer circular de `stderr` (padrão 64 KiB).
    pub max_stderr: usize,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(15),
            max_stdout: 8 * 1024 * 1024,
            max_stderr: 64 * 1024,
        }
    }
}

/// Porta de execução de `php artisan list --format=json` sem shell.
#[async_trait::async_trait]
pub trait ArtisanRunner: Send + Sync + 'static {
    /// Retorna a variante do runner (`Local` ou `Sail`).
    fn kind(&self) -> RunnerKind;
    /// Prefixo de comando inserido no input relativo ao `cwd` da aba (`"php artisan"`, `"php ../artisan"`, etc.).
    fn invoker(&self, root: &Path, cwd: &Path) -> String;
    /// Tokens estruturados do invocador para o parser de linha no frontend.
    fn invoker_tokens(&self, root: &Path, cwd: &Path) -> Vec<String>;
    /// Executa a listagem JSON de comandos Artisan dentro de `root` respeitando `limits`.
    async fn list_json(&self, root: &Path, limits: RunLimits) -> MtResult<Vec<u8>>;
}

/// Sink de eventos emitidos pelo domínio de Comandos Rápidos.
pub trait CommandsEventSink: Send + Sync + 'static {
    /// Notifica que o catálogo Artisan de `root` foi atualizado.
    fn catalog_updated(&self, payload: CatalogUpdatedPayload);
    /// Notifica que a descoberta Artisan de `root` falhou.
    fn discovery_failed(&self, payload: DiscoveryFailedPayload);
    /// Notifica que um novo comando Artisan foi criado em `root`.
    fn command_created(&self, payload: CommandCreatedPayload);
}

/// Implementação nula de [`CommandsEventSink`] para testes sem UI.
#[derive(Debug, Default)]
pub struct NoopCommandsEventSink;

impl CommandsEventSink for NoopCommandsEventSink {
    fn catalog_updated(&self, _payload: CatalogUpdatedPayload) {}
    fn discovery_failed(&self, _payload: DiscoveryFailedPayload) {}
    fn command_created(&self, _payload: CommandCreatedPayload) {}
}

/// Porta de consulta aos projetos cadastrados no `MaxTerminal` (Fase 06).
pub trait ProjectLookup: Send + Sync + 'static {
    /// Verifica se `path` (canônico) pertence a algum projeto cadastrado (incluindo worktrees internas).
    fn owning_project(&self, path: &Path) -> Option<(ProjectId, PathBuf)>;
    /// Busca o caminho canônico raiz de um projeto pelo seu `ProjectId`.
    fn project_by_id(&self, id: ProjectId) -> Option<PathBuf>;
}

/// Localizador do arquivo `config-trabalho.json` do `MaxCode`.
pub trait MaxCodeConfigLocator: Send + Sync + 'static {
    /// Retorna a lista de caminhos candidatos para `config-trabalho.json`.
    fn candidate_files(&self) -> Vec<PathBuf>;
}

/// Relógio injetável para timestamps determinísticos em testes.
pub trait Clock: Send + Sync + 'static {
    /// Retorna o instante atual em milissegundos desde a Unix Epoch.
    fn now_ms(&self) -> i64;
}

/// Implementação padrão de [`Clock`] baseada no relógio do sistema operacional.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or(0)
    }
}
