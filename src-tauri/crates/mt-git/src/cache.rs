//! Cache central de contexto Git por [`RepoKey`], deduplicação de refresh (`single-flight`)
//! e coordenação de eventos (`GitService`).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use mt_core::ids::{PaneId, TabId};
use parking_lot::{Mutex, RwLock};
use tokio::sync::Notify;

use crate::discover::{DiscoveredRepo, discover_repository};
use crate::error::{GitError, GitResult};
use crate::runner::GitRunner;
use crate::types::{
    DegradedReason, GitBranch, GitChanges, GitContext, GitStatusChanged, GitUpstream, GitWorktree,
    RefreshReason, RepoKey,
};
use crate::watcher::RepoMetadataWatcher;

/// Sink de eventos do subsistema Git para desacoplar o `mt-git` do runtime Tauri.
pub trait GitEventSink: Send + Sync + 'static {
    /// Invocado quando o contexto Git de um repositório ou caminho sofre alteração semântica.
    fn emit_status_changed(&self, payload: GitStatusChanged);
}

/// Sink nulo para testes ou execução sem UI.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopGitEventSink;

impl GitEventSink for NoopGitEventSink {
    fn emit_status_changed(&self, _payload: GitStatusChanged) {}
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticSnapshot {
    branch: GitBranch,
    changes: GitChanges,
    upstream: Option<GitUpstream>,
    worktree: GitWorktree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SemanticState {
    NonRepo(PathBuf),
    Degraded(PathBuf, DegradedReason, String),
    Repo(SemanticSnapshot),
}

struct RepoSlot {
    revision: u64,
    last_semantic: Option<SemanticState>,
    last_context: Option<GitContext>,
    watcher: Option<RepoMetadataWatcher>,
    in_flight: bool,
    pending_refresh: bool,
    pending_reason: Option<RefreshReason>,
    notify: Arc<Notify>,
}

impl RepoSlot {
    fn new() -> Self {
        Self {
            revision: 0,
            last_semantic: None,
            last_context: None,
            watcher: None,
            in_flight: false,
            pending_refresh: false,
            pending_reason: None,
            notify: Arc::new(Notify::new()),
        }
    }
}

#[derive(Debug, Clone)]
struct PaneBinding {
    tab_id: TabId,
    cwd: PathBuf,
    slot_key: PathBuf,
}

/// Serviço coordenador de status Git, cache por `RepoKey` e watchers de metadados `.git`.
#[derive(Clone)]
pub struct GitService {
    inner: Arc<GitServiceInner>,
}

struct GitServiceInner {
    runner: Arc<dyn GitRunner>,
    event_sink: Arc<dyn GitEventSink>,
    enable_fs_watcher: bool,
    shutting_down: AtomicBool,
    slots: Mutex<HashMap<PathBuf, RepoSlot>>,
    panes: RwLock<HashMap<PaneId, PaneBinding>>,
}

impl std::fmt::Debug for GitService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitService")
            .field("tracked_panes", &self.inner.panes.read().len())
            .field("cached_slots", &self.inner.slots.lock().len())
            .finish_non_exhaustive()
    }
}

fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl GitService {
    /// Cria uma nova instância do [`GitService`].
    #[must_use]
    pub fn new(
        runner: Arc<dyn GitRunner>,
        event_sink: Arc<dyn GitEventSink>,
        enable_fs_watcher: bool,
    ) -> Self {
        Self {
            inner: Arc::new(GitServiceInner {
                runner,
                event_sink,
                enable_fs_watcher,
                shutting_down: AtomicBool::new(false),
                slots: Mutex::new(HashMap::new()),
                panes: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Verifica se algum painel registrado está posicionado dentro de `worktree_path`.
    #[must_use]
    pub fn is_worktree_in_use(&self, worktree_path: &Path) -> bool {
        let canonical =
            std::fs::canonicalize(worktree_path).unwrap_or_else(|_| worktree_path.to_path_buf());
        let panes = self.inner.panes.read();
        panes
            .values()
            .any(|binding| binding.slot_key == canonical || binding.cwd.starts_with(&canonical))
    }

    /// Registra ou atualiza o diretório de trabalho (`cwd`) de um painel e agenda atualização se necessário.
    pub async fn bind_pane_cwd(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        cwd: PathBuf,
        reason: RefreshReason,
    ) -> GitResult<GitContext> {
        let resolved_slot = Self::resolve_slot_key(&cwd);
        let old_slot = {
            let mut guard = self.inner.panes.write();
            let prev = guard.insert(
                pane_id,
                PaneBinding {
                    tab_id,
                    cwd: cwd.clone(),
                    slot_key: resolved_slot.clone(),
                },
            );
            prev.map(|p| p.slot_key)
        };

        if let Some(prev_key) = old_slot
            && prev_key != resolved_slot
        {
            self.cleanup_orphan_slot(&prev_key);
        }

        self.refresh_slot(&resolved_slot, &cwd, reason, false).await
    }

    /// Remove o vínculo de um painel encerrado e libera watchers órfãos.
    pub fn unbind_pane(&self, pane_id: PaneId) {
        let prev = self.inner.panes.write().remove(&pane_id);
        if let Some(binding) = prev {
            self.cleanup_orphan_slot(&binding.slot_key);
        }
    }

    /// Remove todos os painéis associados a uma aba encerrada.
    pub fn unbind_tab(&self, tab_id: TabId) {
        let removed_keys: Vec<PathBuf> = {
            let mut guard = self.inner.panes.write();
            let to_remove: Vec<PaneId> = guard
                .iter()
                .filter_map(|(pid, b)| (b.tab_id == tab_id).then_some(*pid))
                .collect();
            to_remove
                .into_iter()
                .filter_map(|pid| guard.remove(&pid).map(|b| b.slot_key))
                .collect()
        };
        for key in removed_keys {
            self.cleanup_orphan_slot(&key);
        }
    }

    /// Notifica que um comando terminou em um painel, disparando refresh para seu repositório.
    pub async fn notify_command_finished(&self, pane_id: PaneId) -> Option<GitContext> {
        let binding = self.inner.panes.read().get(&pane_id).cloned()?;
        self.refresh_slot(
            &binding.slot_key,
            &binding.cwd,
            RefreshReason::CommandFinished,
            true,
        )
        .await
        .ok()
    }

    /// Consulta o contexto Git para `path` (usando cache se já existir, ou coletando na hora).
    pub async fn status_for_path(&self, path: &Path) -> GitResult<GitContext> {
        if path.as_os_str().is_empty() {
            return Err(GitError::InvalidPath {
                path: path.to_path_buf(),
                reason: "Caminho vazio".to_string(),
            });
        }
        let slot_key = Self::resolve_slot_key(path);
        if let Some(cached) = self
            .inner
            .slots
            .lock()
            .get(&slot_key)
            .and_then(|s| s.last_context.clone())
        {
            return Ok(cached);
        }
        self.refresh_slot(&slot_key, path, RefreshReason::Initial, false)
            .await
    }

    /// Força a reavaliação imediata do status Git para `path`.
    pub async fn refresh_path(&self, path: &Path, reason: RefreshReason) -> GitResult<GitContext> {
        if path.as_os_str().is_empty() {
            return Err(GitError::InvalidPath {
                path: path.to_path_buf(),
                reason: "Caminho vazio".to_string(),
            });
        }
        let slot_key = Self::resolve_slot_key(path);
        self.refresh_slot(&slot_key, path, reason, true).await
    }

    /// Encerra todos os watchers de filesystem ativos.
    pub fn shutdown(&self) {
        self.inner.shutting_down.store(true, Ordering::SeqCst);
        let mut slots = self.inner.slots.lock();
        for slot in slots.values_mut() {
            slot.watcher = None;
        }
        slots.clear();
    }

    fn resolve_slot_key(path: &Path) -> PathBuf {
        if let Ok(Some(discovered)) = discover_repository(path, None) {
            return discovered.worktree_root;
        }
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }

    fn cleanup_orphan_slot(&self, slot_key: &Path) {
        let still_used = self
            .inner
            .panes
            .read()
            .values()
            .any(|b| b.slot_key == slot_key);
        if !still_used {
            let mut slots = self.inner.slots.lock();
            if let Some(slot) = slots.get_mut(slot_key) {
                slot.watcher = None;
            }
        }
    }

    async fn refresh_slot(
        &self,
        slot_key: &Path,
        inspect_path: &Path,
        reason: RefreshReason,
        force: bool,
    ) -> GitResult<GitContext> {
        if self.inner.shutting_down.load(Ordering::Acquire) {
            return Err(GitError::InvalidPath {
                path: inspect_path.to_path_buf(),
                reason: "Serviço Git em desligamento".to_string(),
            });
        }

        let wait_notify = {
            let mut slots = self.inner.slots.lock();
            let slot = slots
                .entry(slot_key.to_path_buf())
                .or_insert_with(RepoSlot::new);
            if !force
                && !slot.in_flight
                && let Some(ctx) = &slot.last_context
            {
                return Ok(ctx.clone());
            }
            if slot.in_flight {
                slot.pending_refresh = true;
                slot.pending_reason = Some(reason);
                Some(Arc::clone(&slot.notify))
            } else {
                slot.in_flight = true;
                None
            }
        };

        if let Some(notify) = wait_notify {
            notify.notified().await;
            let slots = self.inner.slots.lock();
            if let Some(ctx) = slots.get(slot_key).and_then(|s| s.last_context.clone()) {
                return Ok(ctx);
            }
        }

        let mut current_reason = reason;
        loop {
            let collected = self.collect_once(inspect_path).await;
            let (ctx, emit_payload) = self.commit_collection(slot_key, collected, current_reason);

            if let Some(payload) = emit_payload {
                self.inner.event_sink.emit_status_changed(payload);
            }

            let next_pending = {
                let mut slots = self.inner.slots.lock();
                let Some(slot) = slots.get_mut(slot_key) else {
                    return Ok(ctx);
                };
                if slot.pending_refresh {
                    slot.pending_refresh = false;
                    Some(slot.pending_reason.take().unwrap_or(RefreshReason::FsEvent))
                } else {
                    slot.in_flight = false;
                    slot.notify.notify_waiters();
                    None
                }
            };

            if let Some(next_reason) = next_pending {
                current_reason = next_reason;
            } else {
                return Ok(ctx);
            }
        }
    }

    async fn collect_once(&self, inspect_path: &Path) -> (SemanticState, Option<DiscoveredRepo>) {
        match discover_repository(inspect_path, None) {
            Ok(None) => {
                let canonical = std::fs::canonicalize(inspect_path)
                    .unwrap_or_else(|_| inspect_path.to_path_buf());
                (SemanticState::NonRepo(canonical), None)
            }
            Err(err) => (
                SemanticState::Degraded(
                    inspect_path.to_path_buf(),
                    err.to_degraded_reason(),
                    err.to_string(),
                ),
                None,
            ),
            Ok(Some(discovered)) => match self.inner.runner.status(&discovered.worktree_root).await
            {
                Ok(parsed) => {
                    let snapshot = SemanticSnapshot {
                        branch: parsed.branch,
                        changes: parsed.changes,
                        upstream: parsed.upstream,
                        worktree: discovered.to_git_worktree(),
                    };
                    (SemanticState::Repo(snapshot), Some(discovered))
                }
                Err(err) => (
                    SemanticState::Degraded(
                        discovered.worktree_root.clone(),
                        err.to_degraded_reason(),
                        err.to_string(),
                    ),
                    Some(discovered),
                ),
            },
        }
    }

    fn commit_collection(
        &self,
        slot_key: &Path,
        collected: (SemanticState, Option<DiscoveredRepo>),
        reason: RefreshReason,
    ) -> (GitContext, Option<GitStatusChanged>) {
        let (semantic, discovered_opt) = collected;
        let now_ms = now_epoch_ms();

        let (affected_pane_ids, affected_tab_ids) = self.affected_bindings_for(slot_key);

        let mut slots = self.inner.slots.lock();
        let slot = slots
            .entry(slot_key.to_path_buf())
            .or_insert_with(RepoSlot::new);

        if self.inner.enable_fs_watcher
            && slot.watcher.is_none()
            && !affected_pane_ids.is_empty()
            && let Some(discovered) = &discovered_opt
        {
            let svc = self.clone();
            let watch_path = discovered.worktree_root.clone();
            if let Ok(watcher) = RepoMetadataWatcher::start(discovered, move |_repo_key| {
                let svc_bg = svc.clone();
                let p = watch_path.clone();
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.spawn(async move {
                        let _ = svc_bg.refresh_path(&p, RefreshReason::FsEvent).await;
                    });
                }
            }) {
                slot.watcher = Some(watcher);
            }
        }

        let changed = slot.last_semantic.as_ref() != Some(&semantic);
        if !changed && let Some(existing_ctx) = &slot.last_context {
            return (existing_ctx.clone(), None);
        }

        slot.revision = slot.revision.saturating_add(1);
        let revision = slot.revision;
        slot.last_semantic = Some(semantic.clone());

        let context = match semantic {
            SemanticState::NonRepo(path) => GitContext::NonRepository {
                path,
                revision,
                refreshed_at_ms: now_ms,
            },
            SemanticState::Degraded(path, degraded_reason, message) => GitContext::Degraded {
                path,
                reason: degraded_reason,
                message,
                revision,
                refreshed_at_ms: now_ms,
            },
            SemanticState::Repo(snap) => {
                let Some(disc) = discovered_opt else {
                    return (
                        GitContext::Degraded {
                            path: slot_key.to_path_buf(),
                            reason: DegradedReason::InvalidRepository,
                            message: "Repositório não encontrado.".to_string(),
                            revision,
                            refreshed_at_ms: now_ms,
                        },
                        None,
                    );
                };
                GitContext::Repository {
                    repo_key: RepoKey(disc.worktree_root.clone()),
                    worktree_root: disc.worktree_root,
                    git_dir: disc.git_dir,
                    common_dir: disc.common_dir,
                    branch: snap.branch,
                    changes: snap.changes,
                    upstream: snap.upstream,
                    worktree: snap.worktree,
                    revision,
                    refreshed_at_ms: now_ms,
                }
            }
        };

        slot.last_context = Some(context.clone());

        let payload = GitStatusChanged {
            context: context.clone(),
            affected_pane_ids,
            affected_tab_ids,
            reason,
        };

        (context, Some(payload))
    }

    fn affected_bindings_for(&self, slot_key: &Path) -> (Vec<PaneId>, Vec<TabId>) {
        let panes = self.inner.panes.read();
        let mut pane_ids = Vec::new();
        let mut tab_set = BTreeSet::new();
        for (pane_id, binding) in panes.iter() {
            if binding.slot_key == slot_key || binding.cwd.starts_with(slot_key) {
                pane_ids.push(*pane_id);
                tab_set.insert(binding.tab_id);
            }
        }
        pane_ids.sort_by_key(ToString::to_string);
        let tab_ids: Vec<TabId> = tab_set.into_iter().collect();
        (pane_ids, tab_ids)
    }
}
