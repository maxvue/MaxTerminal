//! Serviço de catálogo Artisan com single-flight por raiz, stale-while-revalidate,
//! verificação de fingerprint e backoff de falhas sem cache.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dashmap::{DashMap, DashSet};
use mt_core::{MtError, MtResult};
use uuid::Uuid;

use crate::artisan::cache::{invalidate_sidecar, read_cache_files, write_cache_files};
use crate::artisan::fingerprint::{self, Fingerprint};
use crate::artisan::model::{
    ArtisanCatalog, CatalogSource, CatalogUpdatedPayload, DiscoveryFailedPayload,
};
use crate::artisan::parser::parse_artisan_list;
use crate::artisan::root::laravel_version;
use crate::artisan::runner::RunnerSelector;
use crate::ports::{Clock, CommandsEventSink, RunLimits};

const DEFAULT_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
const FAILURE_BACKOFF_MS: i64 = 5_000;

#[derive(Clone, Debug)]
struct MemEntry {
    catalog: ArtisanCatalog,
    fingerprint: Fingerprint,
    artisan_json_sha256: String,
    generated_at_ms: i64,
}

#[derive(Clone, Debug)]
struct FailureEntry {
    error: MtError,
    failed_at_ms: i64,
}

enum DiskState {
    Fresh {
        catalog: ArtisanCatalog,
        fingerprint: Fingerprint,
        sha256: String,
        generated_at_ms: i64,
    },
    Stale(ArtisanCatalog),
    Outdated(ArtisanCatalog),
    Missing,
}

/// Serviço coordenador de descoberta e cache de comandos Artisan.
pub struct ArtisanService {
    runners: RunnerSelector,
    sink: Arc<dyn CommandsEventSink>,
    clock: Arc<dyn Clock>,
    locks: DashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>,
    memory: DashMap<PathBuf, Arc<MemEntry>>,
    failures: DashMap<PathBuf, FailureEntry>,
    revalidating: DashSet<PathBuf>,
    revisions: DashMap<PathBuf, Arc<AtomicU64>>,
    limits: RunLimits,
    ttl_ms: i64,
}

impl ArtisanService {
    /// Cria uma nova instância de [`ArtisanService`].
    #[must_use]
    pub fn new(
        runners: RunnerSelector,
        sink: Arc<dyn CommandsEventSink>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self> {
        Arc::new(Self {
            runners,
            sink,
            clock,
            locks: DashMap::new(),
            memory: DashMap::new(),
            failures: DashMap::new(),
            revalidating: DashSet::new(),
            revisions: DashMap::new(),
            limits: RunLimits::default(),
            ttl_ms: DEFAULT_TTL_MS,
        })
    }

    /// Configura limites de execução personalizados (útil para testes de timeout/overflow).
    #[must_use]
    pub fn with_limits(mut self: Arc<Self>, limits: RunLimits) -> Arc<Self> {
        if let Some(inner) = Arc::get_mut(&mut self) {
            inner.limits = limits;
        }
        self
    }

    /// Configura o TTL do cache em memória/disco.
    #[must_use]
    pub fn with_ttl(mut self: Arc<Self>, ttl: Duration) -> Arc<Self> {
        if let Some(inner) = Arc::get_mut(&mut self) {
            inner.ttl_ms = i64::try_from(ttl.as_millis()).unwrap_or(DEFAULT_TTL_MS);
        }
        self
    }

    /// Incrementa e retorna a revisão monotônica de eventos para `root`.
    #[must_use]
    pub fn next_revision(&self, root: &Path) -> u64 {
        let counter = self
            .revisions
            .entry(root.to_path_buf())
            .or_insert_with(|| Arc::new(AtomicU64::new(0)))
            .clone();
        counter.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Retorna o catálogo em memória (se disponível) sem disparar subprocessos.
    #[must_use]
    pub fn peek_memory(&self, root: &Path) -> Option<ArtisanCatalog> {
        let canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        self.memory.get(&canon).map(|e| e.catalog.clone())
    }

    /// Obtém o catálogo Artisan para `root`, aplicando single-flight, validação de fingerprint e stale-while-revalidate.
    pub async fn get(self: &Arc<Self>, root: &Path) -> MtResult<ArtisanCatalog> {
        let canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

        if let Some(hit) = self.fresh_from_memory(&canon) {
            return Ok(hit);
        }

        let lock = self.locks.entry(canon.clone()).or_default().clone();
        let _guard = lock.lock().await;

        if let Some(hit) = self.fresh_from_memory(&canon) {
            return Ok(hit);
        }

        match self.load_disk(&canon)? {
            DiskState::Fresh {
                catalog,
                fingerprint,
                sha256,
                generated_at_ms,
            } => {
                self.failures.remove(&canon);
                self.memory.insert(
                    canon,
                    Arc::new(MemEntry {
                        catalog: catalog.clone(),
                        fingerprint,
                        artisan_json_sha256: sha256,
                        generated_at_ms,
                    }),
                );
                Ok(catalog)
            }
            DiskState::Stale(catalog) => {
                self.failures.remove(&canon);
                self.spawn_revalidate(canon);
                Ok(catalog)
            }
            DiskState::Outdated(old_catalog) => match self.discover_locked(&canon).await {
                Ok(fresh) => Ok(fresh),
                Err(err) => {
                    self.emit_discovery_failed(&canon, &err);
                    Ok(old_catalog.stale_with_error(&err))
                }
            },
            DiskState::Missing => {
                let now = self.clock.now_ms();
                if let Some(fail) = self.failures.get(&canon) {
                    if now.saturating_sub(fail.failed_at_ms) < FAILURE_BACKOFF_MS {
                        return Err(fail.error.clone());
                    }
                }
                match self.discover_locked(&canon).await {
                    Ok(fresh) => Ok(fresh),
                    Err(err) => {
                        self.failures.insert(
                            canon.clone(),
                            FailureEntry {
                                error: err.clone(),
                                failed_at_ms: self.clock.now_ms(),
                            },
                        );
                        self.emit_discovery_failed(&canon, &err);
                        Err(err)
                    }
                }
            }
        }
    }

    /// Força uma nova descoberta imediata via runner (botão Atualizar `↻`), ignorando backoff.
    pub async fn refresh(self: &Arc<Self>, root: &Path) -> MtResult<ArtisanCatalog> {
        let canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        self.failures.remove(&canon);
        let lock = self.locks.entry(canon.clone()).or_default().clone();
        let _guard = lock.lock().await;
        match self.discover_locked(&canon).await {
            Ok(fresh) => Ok(fresh),
            Err(err) => {
                self.emit_discovery_failed(&canon, &err);
                Err(err)
            }
        }
    }

    /// Invalida a memória e o sidecar de `root` (chamado após criar um novo comando Artisan).
    pub fn invalidate(&self, root: &Path) {
        let canon = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        self.memory.remove(&canon);
        self.failures.remove(&canon);
        invalidate_sidecar(&canon);
    }

    fn fresh_from_memory(&self, root: &Path) -> Option<ArtisanCatalog> {
        let mem = self.memory.get(root)?.clone();
        let now = self.clock.now_ms();
        if now.saturating_sub(mem.generated_at_ms) > self.ttl_ms {
            return None;
        }
        let current_fp = fingerprint::compute(root).ok()?;
        if current_fp != mem.fingerprint {
            return None;
        }
        let disk = read_cache_files(root).ok()??;
        if disk.artisan_json_sha256 != mem.artisan_json_sha256 {
            return None;
        }
        let meta = disk.meta?;
        if meta.artisan_json_sha256 != disk.artisan_json_sha256 {
            return None;
        }
        let mut cat = mem.catalog.clone();
        cat.source = CatalogSource::Cache;
        cat.stale = false;
        Some(cat)
    }

    fn load_disk(&self, root: &Path) -> MtResult<DiskState> {
        let Some(loaded) = read_cache_files(root)? else {
            return Ok(DiskState::Missing);
        };

        let root_str = root.display().to_string();
        let Some(meta) = loaded.meta else {
            return Ok(DiskState::Stale(ArtisanCatalog {
                root: root_str,
                commands: loaded.commands,
                source: CatalogSource::ForeignCache,
                stale: true,
                generated_at_ms: None,
                last_error: None,
            }));
        };

        if meta.artisan_json_sha256 != loaded.artisan_json_sha256 {
            return Ok(DiskState::Stale(ArtisanCatalog {
                root: root_str,
                commands: loaded.commands,
                source: CatalogSource::ForeignCache,
                stale: true,
                generated_at_ms: Some(meta.generated_at_ms),
                last_error: None,
            }));
        }

        let current_fp = fingerprint::compute(root)?;
        if current_fp != meta.fingerprint {
            return Ok(DiskState::Outdated(ArtisanCatalog {
                root: root_str,
                commands: loaded.commands,
                source: CatalogSource::Cache,
                stale: true,
                generated_at_ms: Some(meta.generated_at_ms),
                last_error: None,
            }));
        }

        let now = self.clock.now_ms();
        if now.saturating_sub(meta.generated_at_ms) > self.ttl_ms {
            return Ok(DiskState::Stale(ArtisanCatalog {
                root: root_str,
                commands: loaded.commands,
                source: CatalogSource::Cache,
                stale: true,
                generated_at_ms: Some(meta.generated_at_ms),
                last_error: None,
            }));
        }

        let catalog = ArtisanCatalog {
            root: root_str,
            commands: loaded.commands,
            source: CatalogSource::Cache,
            stale: false,
            generated_at_ms: Some(meta.generated_at_ms),
            last_error: None,
        };
        Ok(DiskState::Fresh {
            catalog,
            fingerprint: current_fp,
            sha256: loaded.artisan_json_sha256,
            generated_at_ms: meta.generated_at_ms,
        })
    }

    async fn discover_locked(&self, root: &Path) -> MtResult<ArtisanCatalog> {
        let runner = self.runners.pick(root);
        let raw_bytes = runner.list_json(root, self.limits).await?;
        let commands = parse_artisan_list(&raw_bytes)?;
        let fp = fingerprint::compute(root)?;
        let now = self.clock.now_ms();
        let lv = laravel_version(root);

        let sha256 = match write_cache_files(root, &commands, fp.clone(), runner.kind(), lv, now) {
            Ok(hash) => hash,
            Err(err) => {
                tracing::warn!(
                    target: "mt_commands",
                    root = %root.display(),
                    error = %err,
                    "Não foi possível persistir cache Artisan em disco; servindo em memória"
                );
                String::new()
            }
        };

        let catalog = ArtisanCatalog {
            root: root.display().to_string(),
            commands,
            source: CatalogSource::Fresh,
            stale: false,
            generated_at_ms: Some(now),
            last_error: None,
        };

        self.failures.remove(root);
        self.memory.insert(
            root.to_path_buf(),
            Arc::new(MemEntry {
                catalog: catalog.clone(),
                fingerprint: fp,
                artisan_json_sha256: sha256,
                generated_at_ms: now,
            }),
        );

        let rev = self.next_revision(root);
        self.sink.catalog_updated(CatalogUpdatedPayload {
            root: root.display().to_string(),
            project_id: None,
            command_count: u32::try_from(catalog.commands.len()).unwrap_or(u32::MAX),
            source: CatalogSource::Fresh,
            generated_at_ms: now,
            revision: rev,
            request_id: Uuid::now_v7().to_string(),
        });

        Ok(catalog)
    }

    fn emit_discovery_failed(&self, root: &Path, err: &MtError) {
        let rev = self.next_revision(root);
        self.sink.discovery_failed(DiscoveryFailedPayload {
            root: root.display().to_string(),
            project_id: None,
            error: err.clone(),
            revision: rev,
            request_id: Uuid::now_v7().to_string(),
        });
    }

    /// Dispara revalidação assíncrona deduplicada em segundo plano para `root`.
    pub fn spawn_revalidate(self: &Arc<Self>, root: PathBuf) {
        if !self.revalidating.insert(root.clone()) {
            return;
        }
        let this = Arc::clone(self);
        tokio::spawn(async move {
            struct RevalidatingGuard {
                svc: Arc<ArtisanService>,
                root: PathBuf,
            }
            impl Drop for RevalidatingGuard {
                fn drop(&mut self) {
                    self.svc.revalidating.remove(&self.root);
                }
            }
            let _guard = RevalidatingGuard {
                svc: Arc::clone(&this),
                root: root.clone(),
            };
            let lock = this.locks.entry(root.clone()).or_default().clone();
            let _lock_guard = lock.lock().await;
            if this.fresh_from_memory(&root).is_some() {
                return;
            }
            if let Err(err) = this.discover_locked(&root).await {
                this.emit_discovery_failed(&root, &err);
            }
        });
    }
}
