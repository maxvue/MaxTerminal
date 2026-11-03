//! Persistência atômica de `pty-registry.json` e recuperação de instâncias órfãs no startup.

use std::fs::{self, File, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use mt_core::ids::PtyId;
use parking_lot::Mutex;
use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};

use crate::error::{PtyError, PtyResult};

/// Versão atual do schema do arquivo `pty-registry.json`.
pub const REGISTRY_SCHEMA_VERSION: u32 = 1;

/// Cabeçalho de identificação da instância do `MaxTerminal` no arquivo `pty-registry.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryHeader {
    /// Versão do schema (`1`).
    pub version: u32,
    /// PID do processo principal do `MaxTerminal`.
    pub app_pid: u32,
    /// `starttime` em ticks de `/proc/<app_pid>/stat`.
    pub app_starttime_ticks: u64,
    /// PID do processo `mt-pty-helper --mode watchdog`, se ativo.
    pub watchdog_pid: Option<u32>,
    /// `starttime` em ticks do watchdog, se ativo.
    pub watchdog_starttime_ticks: Option<u64>,
    /// Diretório raiz delegado onde os cgroups desta instância residem.
    pub cgroup_subtree_root: PathBuf,
}

/// Registro persistido de um PTY ativo para recuperação pós-crash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryEntry {
    /// Identificador único do PTY.
    pub pty_id: PtyId,
    /// Caminho absoluto do cgroup v2 dedicado ao PTY.
    pub cgroup_path: PathBuf,
    /// Inode do diretório do cgroup v2.
    pub cgroup_id: u64,
    /// PGID do shell líder.
    pub pgid: i32,
    /// SID da sessão do PTY.
    pub sid: i32,
    /// PID do processo líder do PTY.
    pub shell_pid: u32,
    /// `starttime` em ticks do processo líder em `/proc/<pid>/stat`.
    pub shell_starttime_ticks: u64,
    /// Timestamp Unix em milissegundos de criação do PTY.
    pub created_at_epoch_ms: u64,
}

/// Conteúdo completo serializado em `pty-registry.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySnapshot {
    /// Metadados da instância dona do registro.
    #[serde(flatten)]
    pub header: RegistryHeader,
    /// Lista de PTYs atualmente vivos na instância.
    pub entries: Vec<RegistryEntry>,
}

/// Gerenciador thread-safe do arquivo `pty-registry.json` protegido por `flock` exclusivo.
#[derive(Debug)]
pub struct RegistryFile {
    instance_dir: PathBuf,
    registry_path: PathBuf,
    lock_path: PathBuf,
    _lock_file: File,
    state: Mutex<RegistrySnapshot>,
}

impl RegistryFile {
    /// Abre e bloqueia (`flock` exclusivo não-bloqueante) o diretório de runtime da instância.
    ///
    /// # Errors
    /// Retorna erro se o diretório não puder ser criado com `0700` ou se outra instância já detiver o lock.
    pub fn open(instance_dir: &Path, header: RegistryHeader) -> PtyResult<Self> {
        fs::create_dir_all(instance_dir).map_err(PtyError::Io)?;
        fs::set_permissions(instance_dir, Permissions::from_mode(0o700)).map_err(PtyError::Io)?;

        let lock_path = instance_dir.join("instance.lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(PtyError::Io)?;
        fs::set_permissions(&lock_path, Permissions::from_mode(0o600)).map_err(PtyError::Io)?;

        flock(&lock_file, FlockOperation::NonBlockingLockExclusive).map_err(|e| {
            PtyError::ContainmentFailed(format!(
                "Não foi possível adquirir lock exclusivo em '{}': {e}",
                lock_path.display()
            ))
        })?;

        let registry_path = instance_dir.join("pty-registry.json");
        let snapshot = RegistrySnapshot {
            header,
            entries: Vec::new(),
        };

        let this = Self {
            instance_dir: instance_dir.to_path_buf(),
            registry_path,
            lock_path,
            _lock_file: lock_file,
            state: Mutex::new(snapshot),
        };
        this.persist_locked(&this.state.lock())?;
        Ok(this)
    }

    /// Retorna o diretório da instância onde `pty-registry.json` e `instance.lock` estão armazenados.
    #[must_use]
    pub fn instance_dir(&self) -> &Path {
        &self.instance_dir
    }

    /// Retorna o caminho do arquivo `pty-registry.json`.
    #[must_use]
    pub fn registry_path(&self) -> &Path {
        &self.registry_path
    }

    /// Atualiza os metadados do watchdog no cabeçalho e regrava atomicamente.
    ///
    /// # Errors
    /// Retorna erro caso a escrita atômica falhe.
    pub fn set_watchdog(&self, pid: Option<u32>, starttime_ticks: Option<u64>) -> PtyResult<()> {
        let mut guard = self.state.lock();
        guard.header.watchdog_pid = pid;
        guard.header.watchdog_starttime_ticks = starttime_ticks;
        self.persist_locked(&guard)
    }

    /// Insere ou atualiza uma entrada de PTY e persiste atomicamente em disco.
    ///
    /// # Errors
    /// Retorna erro caso a escrita atômica falhe.
    pub fn upsert(&self, entry: RegistryEntry) -> PtyResult<()> {
        let mut guard = self.state.lock();
        if let Some(existing) = guard.entries.iter_mut().find(|e| e.pty_id == entry.pty_id) {
            *existing = entry;
        } else {
            guard.entries.push(entry);
        }
        self.persist_locked(&guard)
    }

    /// Remove um PTY do registro e persiste atomicamente em disco.
    ///
    /// # Errors
    /// Retorna erro caso a escrita atômica falhe.
    pub fn remove(&self, pty_id: PtyId) -> PtyResult<()> {
        let mut guard = self.state.lock();
        guard.entries.retain(|e| e.pty_id != pty_id);
        self.persist_locked(&guard)
    }

    /// Retorna uma cópia das entradas atualmente registradas.
    #[must_use]
    pub fn entries(&self) -> Vec<RegistryEntry> {
        self.state.lock().entries.clone()
    }

    /// Remove os arquivos `pty-registry.json` e `instance.lock` no encerramento limpo.
    pub fn cleanup_files(&self) {
        let _ = fs::remove_file(&self.registry_path);
        let _ = fs::remove_file(&self.lock_path);
    }

    fn persist_locked(&self, snapshot: &RegistrySnapshot) -> PtyResult<()> {
        let tmp_path = self.instance_dir.join("pty-registry.json.tmp");
        let payload = serde_json::to_vec_pretty(snapshot).map_err(|e| {
            PtyError::ContainmentFailed(format!("Falha ao serializar pty-registry.json: {e}"))
        })?;

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)
            .map_err(PtyError::Io)?;
        fs::set_permissions(&tmp_path, Permissions::from_mode(0o600)).map_err(PtyError::Io)?;
        file.write_all(&payload).map_err(PtyError::Io)?;
        file.sync_all().map_err(PtyError::Io)?;
        drop(file);

        fs::rename(&tmp_path, &self.registry_path).map_err(PtyError::Io)?;
        Ok(())
    }
}

/// Varre diretórios de runtime em `base_dir` procurando registros órfãos cujo `instance.lock`
/// não esteja retido por um processo vivo, mata os cgroups remanescentes e limpa os arquivos.
///
/// Retorna a quantidade de entradas de PTY órfãs que foram limpas.
///
/// # Errors
/// Retorna erro apenas se `base_dir` existir e não puder ser lido por falha crítica de I/O.
pub fn recover_stale_registries(base_dir: &Path) -> PtyResult<usize> {
    if !base_dir.exists() {
        return Ok(0);
    }

    let mut recovered = 0usize;

    // Caso o próprio base_dir contenha um pty-registry.json direto.
    if base_dir.join("pty-registry.json").is_file() {
        recovered += try_recover_instance_dir(base_dir);
    }

    let entries = match fs::read_dir(base_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(recovered),
        Err(e) => return Err(PtyError::Io(e)),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("pty-registry.json").is_file() {
            recovered += try_recover_instance_dir(&path);
        }
    }

    Ok(recovered)
}

fn try_recover_instance_dir(instance_dir: &Path) -> usize {
    let lock_path = instance_dir.join("instance.lock");
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path);

    let Ok(lock_file) = lock_file else {
        return 0;
    };

    // Se outra instância detém o lock exclusivo, ela está viva; não toque!
    if flock(&lock_file, FlockOperation::NonBlockingLockExclusive).is_err() {
        return 0;
    }

    let registry_path = instance_dir.join("pty-registry.json");
    let Ok(raw) = fs::read_to_string(&registry_path) else {
        let _ = fs::remove_file(&lock_path);
        return 0;
    };

    let mut count = 0usize;
    if let Ok(snapshot) = serde_json::from_str::<RegistrySnapshot>(&raw) {
        for item in &snapshot.entries {
            kill_stale_entry(item);
            count += 1;
        }
        if snapshot.header.cgroup_subtree_root.exists() {
            let _ = fs::remove_dir(&snapshot.header.cgroup_subtree_root);
        }
    }

    let _ = fs::remove_file(&registry_path);
    let _ = fs::remove_file(&lock_path);
    count
}

fn kill_stale_entry(entry: &RegistryEntry) {
    if entry.cgroup_path.exists()
        && let Ok(meta) = fs::metadata(&entry.cgroup_path)
        && (entry.cgroup_id == 0 || meta.ino() == entry.cgroup_id)
    {
        let kill_file = entry.cgroup_path.join("cgroup.kill");
        let _ = fs::write(&kill_file, b"1");

        let procs_file = entry.cgroup_path.join("cgroup.procs");
        if let Ok(content) = fs::read_to_string(&procs_file) {
            for line in content.lines() {
                if let Ok(pid) = line.trim().parse::<i32>()
                    && pid > 1
                {
                    let _ = nix::sys::signal::kill(
                        nix::unistd::Pid::from_raw(pid),
                        nix::sys::signal::Signal::SIGKILL,
                    );
                }
            }
        }

        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            if fs::remove_dir(&entry.cgroup_path).is_ok() {
                break;
            }
            let _ = fs::write(&kill_file, b"1");
            thread::sleep(Duration::from_millis(20));
        }
        let _ = fs::remove_dir(&entry.cgroup_path);
    }
}
