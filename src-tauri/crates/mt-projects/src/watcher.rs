//! Monitoramento não recursivo de Pastas de Projetos (`RootWatcher`) usando `notify-debouncer-full`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mt_core::error::codes;
use mt_core::{MtError, MtResult};
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::model::ProjectRootId;

/// Sinal interno emitido pelo [`RootWatcher`] para o [`crate::service::ProjectService`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchSignal {
    /// Mudança relevante detectada no nível 1 da Pasta de Projetos.
    RootChanged(ProjectRootId),
    /// O diretório da Pasta de Projetos desapareceu ou foi movido.
    RootMissing(ProjectRootId),
    /// Erro reportado pelo subsistema `notify`/inotify.
    WatchError(ProjectRootId, String),
}

struct WatchedEntry {
    debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
    root_path: PathBuf,
    parent_path: Option<PathBuf>,
}

/// Gerenciador de observadores `notify` por `ProjectRootId` com modo estritamente `NonRecursive`.
pub struct RootWatcher {
    entries: HashMap<ProjectRootId, WatchedEntry>,
    tx: mpsc::Sender<WatchSignal>,
    dirty_roots: Arc<Mutex<HashSet<ProjectRootId>>>,
}

impl std::fmt::Debug for RootWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RootWatcher")
            .field("watched_roots", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl RootWatcher {
    /// Janela de debounce do `notify-debouncer-full` (`500 ms`).
    pub const DEBOUNCE: Duration = Duration::from_millis(500);
    /// Janela de estabilização pós-sinal no `ProjectService` (`1500 ms`).
    pub const SETTLE: Duration = Duration::from_millis(1500);

    /// Cria um novo [`RootWatcher`] acoplado ao canal de sinais e ao conjunto compartilhado `dirty_roots`.
    #[must_use]
    pub fn new(
        tx: mpsc::Sender<WatchSignal>,
        dirty_roots: Arc<Mutex<HashSet<ProjectRootId>>>,
    ) -> Self {
        Self {
            entries: HashMap::new(),
            tx,
            dirty_roots,
        }
    }

    /// Registra (ou substitui) o observador `NonRecursive` para a raiz `id` em `path`.
    pub fn watch(&mut self, id: ProjectRootId, path: &Path) -> MtResult<()> {
        self.unwatch(id);

        if !path.exists() {
            return Err(MtError::new(
                codes::PROJECT_ROOT_MISSING,
                "A Pasta de Projetos não existe no disco para iniciar o monitoramento.",
            ));
        }

        let root_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let parent_path = root_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty() && *p != root_path)
            .map(Path::to_path_buf);

        let tx = self.tx.clone();
        let dirty = Arc::clone(&self.dirty_roots);
        let watched_root = root_path.clone();

        let mut debouncer = new_debouncer(Self::DEBOUNCE, None, move |res: DebounceEventResult| {
            handle_debounce_result(id, &watched_root, &tx, &dirty, res);
        })
        .map_err(|err| {
            MtError::new(
                codes::PROJECT_ROOT_WATCH_FAILED,
                format!("Não foi possível inicializar o monitor de arquivos: {err}"),
            )
        })?;

        debouncer
            .watch(&root_path, RecursiveMode::NonRecursive)
            .map_err(|err| {
                MtError::new(
                    codes::PROJECT_ROOT_WATCH_FAILED,
                    format!("Não foi possível monitorar a Pasta de Projetos: {err}"),
                )
            })?;

        if let Some(ref parent) = parent_path {
            // O monitoramento do diretório pai é best-effort para detectar remoção/rename da própria raiz
            let _ = debouncer.watch(parent, RecursiveMode::NonRecursive);
        }

        self.entries.insert(
            id,
            WatchedEntry {
                debouncer,
                root_path,
                parent_path,
            },
        );

        Ok(())
    }

    /// Remove e encerra o observador associado à raiz `id`, se existir.
    pub fn unwatch(&mut self, id: ProjectRootId) {
        if let Some(mut entry) = self.entries.remove(&id) {
            let _ = entry.debouncer.unwatch(&entry.root_path);
            if let Some(ref parent) = entry.parent_path {
                let _ = entry.debouncer.unwatch(parent);
            }
            entry.debouncer.stop();
        }
    }

    /// Remove todos os observadores ativos e retorna os handles para descarte fora de locks.
    pub fn clear_all(&mut self) {
        let ids: Vec<ProjectRootId> = self.entries.keys().copied().collect();
        for id in ids {
            self.unwatch(id);
        }
    }

    /// Indica se a raiz `id` possui um watcher registrado no momento.
    #[must_use]
    pub fn is_watching(&self, id: ProjectRootId) -> bool {
        self.entries.contains_key(&id)
    }
}

fn handle_debounce_result(
    id: ProjectRootId,
    root_path: &Path,
    tx: &mpsc::Sender<WatchSignal>,
    dirty_roots: &Arc<Mutex<HashSet<ProjectRootId>>>,
    res: DebounceEventResult,
) {
    match res {
        Ok(events) => {
            let mut relevant = false;
            for ev in &events {
                if ev.need_rescan() {
                    relevant = true;
                    break;
                }
                for ev_path in &ev.paths {
                    if ev_path == root_path || ev_path.parent() == Some(root_path) {
                        relevant = true;
                        break;
                    }
                }
                if relevant {
                    break;
                }
            }

            if !relevant {
                return;
            }

            if root_path.exists() {
                // Marca em dirty_roots ANTES de try_send para que fila cheia nunca perca invalidação
                dirty_roots.lock().insert(id);
                let _ = tx.try_send(WatchSignal::RootChanged(id));
            } else {
                dirty_roots.lock().insert(id);
                let _ = tx.try_send(WatchSignal::RootMissing(id));
            }
        }
        Err(errors) => {
            if !root_path.exists() {
                dirty_roots.lock().insert(id);
                let _ = tx.try_send(WatchSignal::RootMissing(id));
                return;
            }
            dirty_roots.lock().insert(id);
            let msg = errors.first().map_or_else(
                || String::from("erro no monitor de arquivos"),
                ToString::to_string,
            );
            let _ = tx.try_send(WatchSignal::WatchError(id, msg));
        }
    }
}
