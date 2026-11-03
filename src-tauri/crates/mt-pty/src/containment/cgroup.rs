//! Gerenciamento de subtree cgroup v2 delegado por instância e por PTY.

use std::collections::HashSet;
use std::fs;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mt_core::ids::PtyId;
use parking_lot::Mutex;
use rustix::fs::{Mode, OFlags, fstat, openat};
use rustix::io::{read, write};
use rustix::process::{PidfdFlags, getpid, pidfd_open};

use crate::containment::{ContainmentHandle, ContainmentProvider};
use crate::error::{PtyError, PtyResult};

/// Lê `/proc/self/cgroup` e retorna o caminho relativo do cgroup v2 (`0::/...`).
pub fn read_self_cgroup_relative_path() -> PtyResult<String> {
    let content = fs::read_to_string("/proc/self/cgroup").map_err(|e| {
        PtyError::ContainmentUnavailable(format!("Não foi possível ler /proc/self/cgroup: {e}"))
    })?;
    for line in content.lines() {
        if let Some(rel) = line.strip_prefix("0::") {
            let trimmed = rel.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
    }
    Err(PtyError::ContainmentUnavailable(
        "Hierarquia unificada cgroup v2 (0::) não encontrada em /proc/self/cgroup".to_string(),
    ))
}

fn verify_pidfd_support() -> PtyResult<()> {
    let pid = getpid();
    let _fd = pidfd_open(pid, PidfdFlags::empty()).map_err(|e| {
        PtyError::ContainmentUnavailable(format!(
            "Suporte a pidfd_open indisponível no kernel: {e}"
        ))
    })?;
    Ok(())
}

fn probe_cgroup_scope_dir(scope_dir: &Path) -> PtyResult<()> {
    fs::create_dir_all(scope_dir).map_err(|e| {
        PtyError::ContainmentUnavailable(format!(
            "Sem permissão para criar diretório de escopo cgroup em {}: {e}",
            scope_dir.display()
        ))
    })?;

    let probe_dir = scope_dir.join("pty-probe");
    let _ = fs::remove_dir(&probe_dir);
    fs::create_dir(&probe_dir).map_err(|e| {
        PtyError::ContainmentUnavailable(format!(
            "Falha ao criar subgrupo de sonda em {}: {e}",
            probe_dir.display()
        ))
    })?;

    let check_result = (|| -> PtyResult<()> {
        let dir_fd = openat(
            rustix::fs::CWD,
            &probe_dir,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| {
            PtyError::ContainmentUnavailable(format!("Falha ao abrir diretório do cgroup: {e}"))
        })?;

        let _procs_fd = openat(
            &dir_fd,
            "cgroup.procs",
            OFlags::RDWR | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| {
            PtyError::ContainmentUnavailable(format!(
                "Arquivo cgroup.procs ausente ou sem permissão de escrita em {}: {e}",
                probe_dir.display()
            ))
        })?;

        let _kill_fd = openat(
            &dir_fd,
            "cgroup.kill",
            OFlags::WRONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| {
            PtyError::ContainmentUnavailable(format!(
                "Arquivo cgroup.kill ausente ou sem permissão de escrita em {}: {e}",
                probe_dir.display()
            ))
        })?;

        let _events_fd = openat(
            &dir_fd,
            "cgroup.events",
            OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| {
            PtyError::ContainmentUnavailable(format!(
                "Arquivo cgroup.events ausente ou ilegível em {}: {e}",
                probe_dir.display()
            ))
        })?;

        Ok(())
    })();

    let _ = fs::remove_dir(&probe_dir);
    if check_result.is_err() {
        let _ = fs::remove_dir(scope_dir);
    }
    check_result
}

/// Handle real de cgroup v2 mantendo descritor aberto (`dir_fd`) e validação de inode.
pub struct SystemdCgroupHandle {
    dir_fd: OwnedFd,
    cgroup_path: PathBuf,
    cgroup_id: u64,
    released: AtomicBool,
}

impl SystemdCgroupHandle {
    fn open_or_create(cgroup_path: PathBuf) -> PtyResult<Self> {
        fs::create_dir(&cgroup_path).map_err(|e| {
            PtyError::ContainmentFailed(format!(
                "Falha ao criar diretório cgroup {}: {e}",
                cgroup_path.display()
            ))
        })?;

        let dir_fd = openat(
            rustix::fs::CWD,
            &cgroup_path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| {
            let _ = fs::remove_dir(&cgroup_path);
            PtyError::ContainmentFailed(format!(
                "Falha ao abrir dir_fd para {}: {e}",
                cgroup_path.display()
            ))
        })?;

        let stat = fstat(&dir_fd).map_err(|e| {
            let _ = fs::remove_dir(&cgroup_path);
            PtyError::ContainmentFailed(format!("Falha ao executar fstat no cgroup: {e}"))
        })?;

        #[allow(clippy::cast_sign_loss)]
        let cgroup_id = stat.st_ino;

        Ok(Self {
            dir_fd,
            cgroup_path,
            cgroup_id,
            released: AtomicBool::new(false),
        })
    }

    fn verify_inode(&self) -> PtyResult<()> {
        let stat = fstat(&self.dir_fd).map_err(|e| {
            PtyError::ContainmentFailed(format!("Falha ao revalidar fstat do cgroup: {e}"))
        })?;
        if stat.st_ino != self.cgroup_id {
            return Err(PtyError::ContainmentFailed(
                "Inode do diretório cgroup divergiu do registrado".to_string(),
            ));
        }
        Ok(())
    }

    fn read_file_at(&self, name: &str) -> PtyResult<String> {
        self.verify_inode()?;
        let fd = openat(
            &self.dir_fd,
            name,
            OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| {
            PtyError::ContainmentFailed(format!("Falha ao abrir {name} no cgroup: {e}"))
        })?;

        let mut buf = Vec::with_capacity(512);
        let mut chunk = [0u8; 512];
        loop {
            match read(&fd, &mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(rustix::io::Errno::INTR) => {}
                Err(e) => {
                    return Err(PtyError::ContainmentFailed(format!(
                        "Erro ao ler {name} no cgroup: {e}"
                    )));
                }
            }
        }
        String::from_utf8(buf).map_err(|e| {
            PtyError::ContainmentFailed(format!("Conteúdo UTF-8 inválido em {name}: {e}"))
        })
    }

    fn collect_subcgroup_pids(dir: &Path, out: &mut Vec<u32>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    if let Ok(procs) = fs::read_to_string(path.join("cgroup.procs")) {
                        for line in procs.lines() {
                            if let Ok(pid) = line.trim().parse::<u32>() {
                                out.push(pid);
                            }
                        }
                    }
                    Self::collect_subcgroup_pids(&path, out);
                }
            }
        }
    }

    fn kill_subcgroups_recursive(dir: &Path) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let _ = fs::write(path.join("cgroup.kill"), b"1");
                    Self::kill_subcgroups_recursive(&path);
                }
            }
        }
    }

    fn remove_subcgroups_recursive(dir: &Path) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    Self::remove_subcgroups_recursive(&path);
                    let _ = fs::remove_dir(&path);
                }
            }
        }
    }
}

impl ContainmentHandle for SystemdCgroupHandle {
    fn cgroup_path(&self) -> &Path {
        &self.cgroup_path
    }

    fn cgroup_id(&self) -> u64 {
        self.cgroup_id
    }

    fn enroll_pid(&self, pid: u32) -> PtyResult<()> {
        self.verify_inode()?;
        let procs_fd = openat(
            &self.dir_fd,
            "cgroup.procs",
            OFlags::WRONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| {
            PtyError::ContainmentFailed(format!(
                "Falha ao abrir cgroup.procs para inscrever PID {pid}: {e}"
            ))
        })?;

        let payload = format!("{pid}\n");
        write(&procs_fd, payload.as_bytes()).map_err(|e| {
            PtyError::ContainmentFailed(format!(
                "Falha ao inscrever PID {pid} em cgroup.procs: {e}"
            ))
        })?;
        Ok(())
    }

    fn members(&self) -> PtyResult<Vec<u32>> {
        if self.released.load(Ordering::Acquire) && !self.cgroup_path.exists() {
            return Ok(Vec::new());
        }
        let content = self.read_file_at("cgroup.procs")?;
        let mut pids = Vec::new();
        for line in content.lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                pids.push(pid);
            }
        }
        Self::collect_subcgroup_pids(&self.cgroup_path, &mut pids);
        pids.sort_unstable();
        pids.dedup();
        Ok(pids)
    }

    fn kill_all(&self) -> PtyResult<()> {
        self.verify_inode()?;
        let kill_fd = openat(
            &self.dir_fd,
            "cgroup.kill",
            OFlags::WRONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|e| PtyError::ContainmentFailed(format!("Falha ao abrir cgroup.kill: {e}")))?;

        write(&kill_fd, b"1").map_err(|e| {
            PtyError::ContainmentFailed(format!("Falha ao escrever '1' em cgroup.kill: {e}"))
        })?;
        Self::kill_subcgroups_recursive(&self.cgroup_path);
        Ok(())
    }

    fn is_empty(&self) -> PtyResult<bool> {
        if self.released.load(Ordering::Acquire) && !self.cgroup_path.exists() {
            return Ok(true);
        }
        let events = self.read_file_at("cgroup.events")?;
        let mut populated_zero = false;
        for line in events.lines() {
            let trimmed = line.trim();
            if trimmed == "populated 0" {
                populated_zero = true;
                break;
            }
        }
        if !populated_zero {
            return Ok(false);
        }
        Ok(self.members()?.is_empty())
    }

    fn wait_empty(&self, timeout: Duration) -> PtyResult<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.is_empty()? {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn release(&self) -> PtyResult<()> {
        self.released.store(true, Ordering::Release);
        Self::remove_subcgroups_recursive(&self.cgroup_path);
        match fs::remove_dir(&self.cgroup_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(PtyError::ContainmentFailed(format!(
                "Falha ao remover diretório cgroup {}: {e}",
                self.cgroup_path.display()
            ))),
        }
    }
}

/// Estado do driver de contenção cgroup v2.
#[derive(Debug, Clone)]
pub enum CgroupDriverState {
    /// Subtree cgroup v2 delegado ativo em `scope_dir`.
    Active {
        /// Diretório onde os subgrupos `pty-<id>` são criados.
        scope_dir: PathBuf,
    },
    /// Contenção indisponível (degradação segura: falha em `create` com `pty.containment_unavailable`).
    Disabled {
        /// Motivo técnico da indisponibilidade.
        reason: String,
    },
}

/// Provedor real de contenção via cgroup v2 delegado pelo systemd ao usuário.
///
/// Nunca move o processo do aplicativo nem o watchdog: cria apenas cgroups filhos para os PTYs.
#[derive(Debug)]
pub struct SystemdCgroupProvider {
    state: CgroupDriverState,
}

impl SystemdCgroupProvider {
    /// Descobre e valida o escopo cgroup v2 delegado para a instância atual.
    #[must_use]
    pub fn discover(cgroup_root_override: Option<&Path>, instance_id: &str) -> Self {
        if let Err(err) = verify_pidfd_support() {
            return Self {
                state: CgroupDriverState::Disabled {
                    reason: err.to_string(),
                },
            };
        }

        if let Some(override_root) = cgroup_root_override {
            let scope_dir = override_root.join(format!("maxterminal-ptys-{instance_id}.slice"));
            return match probe_cgroup_scope_dir(&scope_dir) {
                Ok(()) => Self {
                    state: CgroupDriverState::Active { scope_dir },
                },
                Err(err) => Self {
                    state: CgroupDriverState::Disabled {
                        reason: err.to_string(),
                    },
                },
            };
        }

        let rel_path = match read_self_cgroup_relative_path() {
            Ok(p) => p,
            Err(err) => {
                return Self {
                    state: CgroupDriverState::Disabled {
                        reason: err.to_string(),
                    },
                };
            }
        };

        let cgroup_mount = Path::new("/sys/fs/cgroup");
        let self_cgroup_dir = cgroup_mount.join(rel_path.trim_start_matches('/'));

        // Tenta o próprio diretório ou seus ancestrais imediatos (ex.: app.slice / user@1000.service)
        // sem nunca mover o processo principal do aplicativo.
        let mut current = Some(self_cgroup_dir.as_path());
        let mut last_error =
            String::from("Nenhum diretório cgroup v2 delegado gravável encontrado");

        while let Some(candidate) = current {
            if candidate == cgroup_mount {
                break;
            }
            let scope_dir = candidate.join(format!("maxterminal-ptys-{instance_id}.slice"));
            match probe_cgroup_scope_dir(&scope_dir) {
                Ok(()) => {
                    return Self {
                        state: CgroupDriverState::Active { scope_dir },
                    };
                }
                Err(err) => {
                    last_error = err.to_string();
                }
            }
            current = candidate.parent();
        }

        Self {
            state: CgroupDriverState::Disabled { reason: last_error },
        }
    }
}

impl Drop for SystemdCgroupProvider {
    fn drop(&mut self) {
        if let CgroupDriverState::Active { scope_dir } = &self.state {
            let _ = fs::remove_dir(scope_dir);
        }
    }
}

impl ContainmentProvider for SystemdCgroupProvider {
    fn create(&self, pty_id: PtyId) -> PtyResult<Arc<dyn ContainmentHandle>> {
        match &self.state {
            CgroupDriverState::Active { scope_dir } => {
                let pty_dir = scope_dir.join(format!("pty-{pty_id}"));
                let handle = SystemdCgroupHandle::open_or_create(pty_dir)?;
                Ok(Arc::new(handle))
            }
            CgroupDriverState::Disabled { reason } => {
                Err(PtyError::ContainmentUnavailable(reason.clone()))
            }
        }
    }

    fn is_available(&self) -> bool {
        matches!(self.state, CgroupDriverState::Active { .. })
    }

    fn subtree_root(&self) -> Option<&Path> {
        match &self.state {
            CgroupDriverState::Active { scope_dir } => Some(scope_dir.as_path()),
            CgroupDriverState::Disabled { .. } => None,
        }
    }
}

/// Handle de contenção em memória para testes unitários determinísticos.
pub struct FakeContainmentHandle {
    path: PathBuf,
    id: u64,
    pids: Mutex<HashSet<u32>>,
    never_empty: AtomicBool,
    synthetic_pids: AtomicBool,
}

impl FakeContainmentHandle {
    /// Cria um novo handle fake de contenção.
    #[must_use]
    pub fn new(path: PathBuf, id: u64) -> Self {
        Self {
            path,
            id,
            pids: Mutex::new(HashSet::new()),
            never_empty: AtomicBool::new(false),
            synthetic_pids: AtomicBool::new(false),
        }
    }

    /// Configura o handle para simular um processo preso em espera ininterruptível (`D`) que resiste ao kill.
    pub fn set_uninterruptible_survivor(&self, stuck: bool) {
        self.never_empty.store(stuck, Ordering::SeqCst);
    }

    /// Impede que `is_empty()` remova PIDs sintéticos que não existem no `/proc` real até `kill_all()`.
    pub fn set_synthetic_pids(&self, synthetic: bool) {
        self.synthetic_pids.store(synthetic, Ordering::SeqCst);
    }
}

impl ContainmentHandle for FakeContainmentHandle {
    fn cgroup_path(&self) -> &Path {
        &self.path
    }

    fn cgroup_id(&self) -> u64 {
        self.id
    }

    fn enroll_pid(&self, pid: u32) -> PtyResult<()> {
        self.pids.lock().insert(pid);
        Ok(())
    }

    fn members(&self) -> PtyResult<Vec<u32>> {
        let mut v: Vec<u32> = self.pids.lock().iter().copied().collect();
        v.sort_unstable();
        Ok(v)
    }

    fn kill_all(&self) -> PtyResult<()> {
        if !self.never_empty.load(Ordering::SeqCst) {
            let pids: Vec<u32> = self.pids.lock().drain().collect();
            if !self.synthetic_pids.load(Ordering::SeqCst) {
                for pid in pids {
                    if let Ok(raw) = i32::try_from(pid)
                        && let Some(rpid) = rustix::process::Pid::from_raw(raw)
                    {
                        let _ = rustix::process::kill_process(rpid, rustix::process::Signal::Kill);
                    }
                }
            }
        }
        Ok(())
    }

    fn is_empty(&self) -> PtyResult<bool> {
        if self.never_empty.load(Ordering::SeqCst) {
            return Ok(false);
        }
        if !self.synthetic_pids.load(Ordering::SeqCst) {
            self.pids
                .lock()
                .retain(|&pid| Path::new(&format!("/proc/{pid}")).exists());
        }
        Ok(self.pids.lock().is_empty())
    }

    fn wait_empty(&self, timeout: Duration) -> PtyResult<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.is_empty()? {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn release(&self) -> PtyResult<()> {
        Ok(())
    }
}

/// Provedor fake de contenção para testes unitários.
pub struct FakeContainmentProvider {
    root: PathBuf,
    available: bool,
    next_id: std::sync::atomic::AtomicU64,
}

impl FakeContainmentProvider {
    /// Cria um provedor fake disponível ou indisponível.
    #[must_use]
    pub fn new(root: PathBuf, available: bool) -> Self {
        Self {
            root,
            available,
            next_id: std::sync::atomic::AtomicU64::new(1000),
        }
    }
}

impl ContainmentProvider for FakeContainmentProvider {
    fn create(&self, pty_id: PtyId) -> PtyResult<Arc<dyn ContainmentHandle>> {
        if !self.available {
            return Err(PtyError::ContainmentUnavailable(
                "FakeContainmentProvider configurado como indisponível".to_string(),
            ));
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let path = self.root.join(format!("pty-{pty_id}"));
        Ok(Arc::new(FakeContainmentHandle::new(path, id)))
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn subtree_root(&self) -> Option<&Path> {
        self.available.then_some(self.root.as_path())
    }
}
