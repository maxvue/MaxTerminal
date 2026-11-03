//! Watcher com debounce de 500 ms (`notify-debouncer-full` 0.5) sobre o repositório `MaxSkills` (`CA-30`).

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer_opt};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::skills::SyncResult;
use crate::skills::sync::SkillsService;

fn is_ignored_watch_dir_name(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules" | "target" | "vendor" | "projects" | "docs"
        )
}

fn is_watchable_top_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if is_ignored_watch_dir_name(name) {
        return false;
    }
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false)
}

/// Payload do evento `skills://changed` emitido quando a fonte `MaxSkills` sofre alterações.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillsChangedPayload {
    /// Caminho da fonte observada.
    pub source_dir: String,
    /// Lista ordenada de nomes de skills afetadas.
    pub changed_skills: Vec<String>,
}

/// Sink de eventos de skills para emissão ao frontend (`skills://changed` e `skills://sync-completed`).
pub trait SkillsEventSink: Send + Sync {
    /// Emite `skills://changed`.
    fn emit_changed(&self, payload: SkillsChangedPayload);
    /// Emite `skills://sync-completed`.
    fn emit_sync_completed(&self, result: SyncResult);
}

/// Monitor em tempo real do diretório fonte de `MaxSkills`.
pub struct SkillsWatcher {
    _stop_tx: mpsc::Sender<()>,
    _debouncer: Arc<Mutex<Debouncer<RecommendedWatcher, RecommendedCache>>>,
}

impl SkillsWatcher {
    /// Inicia o monitoramento com debounce de 500 ms sobre o diretório fonte configurado,
    /// sem seguir symlinks e ignorando diretórios ocultos, worktrees e pastas de projetos vinculados.
    pub fn start(service: Arc<SkillsService>, sink: Arc<dyn SkillsEventSink>) -> Option<Self> {
        let source_dir = service.resolved_source_dir();
        if !source_dir.is_dir() {
            return None;
        }
        let canonical_source = std::fs::canonicalize(&source_dir).unwrap_or(source_dir);
        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        let (evt_tx, mut evt_rx) = mpsc::unbounded_channel::<Vec<PathBuf>>();

        let watch_root = canonical_source.clone();
        let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, RecommendedCache>(
            Duration::from_millis(500),
            None,
            move |result: DebounceEventResult| {
                if let Ok(events) = result {
                    let mut paths = Vec::new();
                    for ev in events {
                        for p in &ev.paths {
                            paths.push(p.clone());
                        }
                    }
                    if !paths.is_empty() {
                        let _ = evt_tx.send(paths);
                    }
                }
            },
            RecommendedCache::new(),
            NotifyConfig::default().with_follow_symlinks(false),
        )
        .ok()?;

        debouncer
            .watch(&watch_root, RecursiveMode::NonRecursive)
            .ok()?;

        let mut watched_top_dirs = HashSet::new();
        if let Ok(rd) = std::fs::read_dir(&watch_root) {
            for entry in rd.flatten() {
                let path = entry.path();
                if is_watchable_top_dir(&path)
                    && debouncer.watch(&path, RecursiveMode::Recursive).is_ok()
                {
                    watched_top_dirs.insert(path);
                }
            }
        }

        let debouncer = Arc::new(Mutex::new(debouncer));
        let debouncer_for_loop = Arc::clone(&debouncer);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
                    Some(paths) = evt_rx.recv() => {
                        for p in &paths {
                            if let Ok(rel) = p.strip_prefix(&canonical_source)
                                && let Some(first) = rel.components().next()
                            {
                                let top_dir = canonical_source.join(first.as_os_str());
                                if !watched_top_dirs.contains(&top_dir)
                                    && is_watchable_top_dir(&top_dir)
                                    && debouncer_for_loop
                                        .lock()
                                        .watch(&top_dir, RecursiveMode::Recursive)
                                        .is_ok()
                                {
                                    watched_top_dirs.insert(top_dir);
                                }
                            }
                        }

                        let changed_skills = extract_changed_skill_names(&canonical_source, &paths);
                        if changed_skills.is_empty()
                            && !paths.iter().any(|p| p.file_name().and_then(|n| n.to_str()) == Some("index.json"))
                        {
                            continue;
                        }
                        service.invalidate_plans();
                        sink.emit_changed(SkillsChangedPayload {
                            source_dir: canonical_source.to_string_lossy().to_string(),
                            changed_skills,
                        });
                        let auto_results = service.run_auto_sync_all().await;
                        for res in auto_results {
                            sink.emit_sync_completed(res);
                        }
                    }
                }
            }
        });

        Some(Self {
            _stop_tx: stop_tx,
            _debouncer: debouncer,
        })
    }
}

fn extract_changed_skill_names(source_root: &Path, paths: &[PathBuf]) -> Vec<String> {
    let mut names = BTreeSet::new();
    for p in paths {
        if let Ok(rel) = p.strip_prefix(source_root)
            && let Some(first) = rel.components().next()
        {
            let name = first.as_os_str().to_string_lossy().to_string();
            if !is_ignored_watch_dir_name(&name) && name != "index.json" {
                names.insert(name);
            }
        }
    }
    names.into_iter().collect()
}
