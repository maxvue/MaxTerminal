//! Watcher com debounce de 500 ms (`notify-debouncer-full` 0.5) sobre o repositório `MaxSkills` (`CA-30`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::skills::SyncResult;
use crate::skills::sync::SkillsService;

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
    _debouncer: Mutex<Debouncer<RecommendedWatcher, RecommendedCache>>,
}

impl SkillsWatcher {
    /// Inicia o monitoramento recursivo com debounce de 500 ms sobre o diretório fonte configurado.
    pub fn start(service: Arc<SkillsService>, sink: Arc<dyn SkillsEventSink>) -> Option<Self> {
        let source_dir = service.resolved_source_dir();
        if !source_dir.is_dir() {
            return None;
        }
        let canonical_source = std::fs::canonicalize(&source_dir).unwrap_or(source_dir);
        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        let (evt_tx, mut evt_rx) = mpsc::unbounded_channel::<Vec<PathBuf>>();

        let watch_root = canonical_source.clone();
        let mut debouncer = new_debouncer(
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
        )
        .ok()?;

        debouncer
            .watch(&watch_root, RecursiveMode::Recursive)
            .ok()?;

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop_rx.recv() => break,
                    Some(paths) = evt_rx.recv() => {
                        let changed_skills = extract_changed_skill_names(&canonical_source, &paths);
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
            _debouncer: Mutex::new(debouncer),
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
            if !name.starts_with('.') && name != "index.json" {
                names.insert(name);
            }
        }
    }
    names.into_iter().collect()
}
