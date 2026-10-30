# Especificação Técnica — Implementação 03: Supervisão via `/proc`, Árvore de Processos, Identidade `pidfd` e Supervisor Periódico

## 1. Contexto, Objetivo e Escopo

Esta é a terceira unidade de implementação da **Fase 02 — Core PTY e Supervisão de Processos**. Seu objetivo é implementar o subsistema de observabilidade e supervisão de processos Linux (`src-tauri/crates/mt-pty/src/proc/` e `src-tauri/crates/mt-pty/src/supervisor.rs`), fornecendo os contratos consumidos pelo IPC desta fase e pelas Fases 05, 07, 08 e 13:
1. Leitura resiliente e sem TOCTOU de `/proc` (`ProcSource`, `ProcfsSource` e `FakeProcSource` para testes), extraindo `ProcessInfo`, `ProcessIdentity` (`boot_id`, `pid`, `start_time`), `cmdline`, `exe`, `cwd`, `state`, `rss_bytes` e `threads`, tratando graciosamente processos efêmeros (`ENOENT`/`ESRCH` → `Ok(None)`) e restrições de permissão (`EACCES`/`hidepid` → `None` nos campos opcionais).
2. Construção da árvore hierárquica de processos (`ProcessNode`) a partir do shell do PTY e listagem de todos os processos pertencentes ao cgroup (`owned_processes`, incluindo daemons reparentados para o subreaper/init), consolidados em `PtyProcessReport`.
3. Detecção precisa do grupo de processo em foreground do terminal via `tcgetpgrp` seguro sobre o master FD (`proc/foreground.rs`), identificando o líder do PGID de foreground ou, caso o líder já tenha saído, o membro vivo do grupo com maior `start_time`.
4. Sinalização segura de processos (`Signaler` e `PidfdSignaler`) baseada em `pidfd_open` + revalidação de `(boot_id, pid, start_time)` e verificação de associação ao cgroup do PTY antes de `pidfd_send_signal`, eliminando corridas de reutilização de PID.
5. Thread única `mt-pty-supervisor` (`supervisor.rs`) executando a cada `supervisor_interval` (`1 s`) para detectar mudanças de foreground (emitindo `PtyEvent::ForegroundChanged` deduplicado por `(pgid, start_time)`), expirar tombstones (`tombstone_ttl = 60 s`), verificar timeouts de ACK (`ack_timeout = 5 s`) e monitorar a saúde do watchdog.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Modificados
- `src-tauri/crates/mt-pty/src/lib.rs` — exposição dos módulos `pub mod proc;` e `pub mod supervisor;`.

### 2.2. Arquivos Criados
- `src-tauri/crates/mt-pty/src/proc/mod.rs` — trait `ProcSource`, `Signaler`, `PidfdSignaler`, `RecordingSignaler` (para testes) e leitura cacheada de `/proc/sys/kernel/random/boot_id`.
- `src-tauri/crates/mt-pty/src/proc/source.rs` — `ProcfsSource` (implementação real usando `procfs 0.17`) e `FakeProcSource` (para testes unitários determinísticos).
- `src-tauri/crates/mt-pty/src/proc/info.rs` — conversão de `procfs::process::Stat` e metadados de `/proc/<pid>/` para `ProcessInfo`, `ProcState` e `ProcessIdentity`, com dupla leitura de identidade (`start_time` antes e depois de `cmdline`/`cwd`/`exe`) para evitar mistura de dados em caso de reutilização de PID.
- `src-tauri/crates/mt-pty/src/proc/tree.rs` — montagem de `ProcessNode` (`build_process_tree`) e geração completa de `PtyProcessReport` (`build_process_report`) combinando membros do cgroup e relação `ppid → filhos`.
- `src-tauri/crates/mt-pty/src/proc/foreground.rs` — wrapper seguro `tcgetpgrp` (`rustix::termios::tcgetpgrp`) e resolução do `ProcessInfo` de foreground (`resolve_foreground_process`).
- `src-tauri/crates/mt-pty/src/supervisor.rs` — `SupervisorWorker` e `SupervisorTarget`, executando o loop periódico de 1 s na thread `mt-pty-supervisor`.

---

## 3. Especificação Técnica Detalhada e Assinaturas Rust

### 3.1. Traits `ProcSource` e `Signaler` (`src/proc/mod.rs`)

```rust
use std::collections::HashMap;
use std::path::Path;
use nix::sys::signal::Signal;
use parking_lot::Mutex;
use crate::error::{PtyError, PtyResult};
use crate::types::{ProcessIdentity, ProcessInfo};

/// Fonte de dados de processos do sistema operacional.
pub trait ProcSource: Send + Sync + 'static {
    /// Retorna informações detalhadas do processo ou `Ok(None)` se o PID não existir mais.
    fn process(&self, pid: u32) -> PtyResult<Option<ProcessInfo>>;

    /// Lista todos os PIDs visíveis em `/proc`.
    fn all_pids(&self) -> PtyResult<Vec<u32>>;

    /// Obtém a identidade imutável `(boot_id, pid, start_time)` de um PID vivo.
    fn identity(&self, pid: u32) -> PtyResult<Option<ProcessIdentity>>;

    /// Retorna o `boot_id` atual do kernel Linux.
    fn boot_id(&self) -> &str;
}

/// Abstração para envio de sinais validados por identidade de processo.
pub trait Signaler: Send + Sync + 'static {
    /// Envia `sig` ao processo se e somente se sua identidade `(boot_id, pid, start_time)`
    /// continuar válida no momento da abertura do pidfd.
    fn signal(&self, process: &ProcessIdentity, sig: Signal) -> PtyResult<()>;

    /// Envia `sig` validando simultaneamente a identidade do processo e sua presença no cgroup indicado.
    fn signal_in_cgroup(
        &self,
        process: &ProcessIdentity,
        cgroup_id: u64,
        sig: Signal,
    ) -> PtyResult<()>;
}

/// Implementação real de sinalização via `pidfd_open` + revalidação de `/proc/<pid>/stat` e `/proc/<pid>/cgroup`.
pub struct PidfdSignaler {
    proc_source: std::sync::Arc<dyn ProcSource>,
}

impl PidfdSignaler {
    pub fn new(proc_source: std::sync::Arc<dyn ProcSource>) -> Self {
        Self { proc_source }
    }
}

/// Implementação para testes unitários que grava a sequência exata de sinais enviados.
#[derive(Debug, Default)]
pub struct RecordingSignaler {
    pub sent: Mutex<Vec<(ProcessIdentity, Signal)>>,
    pub membership: Mutex<HashMap<u32, u64>>, // pid -> cgroup_id
}
```

### 3.2. Protocolo Anti-TOCTOU do `PidfdSignaler`

Para cumprir a seção §7.7 da especificação da Fase 02 e o **CA-27**:
1. Recebe `target: &ProcessIdentity` (`boot_id`, `pid`, `start_time`) e opcionalmente `expected_cgroup_id: u64`.
2. Verifica que `target.boot_id == proc_source.boot_id()`; se divergir, retorna `Err(PtyError::PidNotOwned(target.pid))`.
3. Abre um `OwnedFd` do processo via `rustix::process::pidfd_open(Pid::from_raw(target.pid as i32), PidfdFlags::empty())`:
   - Se retornar `ESRCH` (`Errno::SRCH`), o processo já terminou: retorna `Ok(())` (idempotente para processos que já saíram durante escalada) ou `PidNotOwned` quando chamado via `signal_process` externo sem membro vivo.
4. **Com o `pidfd` aberto** (o que congela a referência à `struct task_struct` no kernel), relê `proc_source.identity(target.pid)`:
   - Se retornar `None` ou se `current_identity.start_time != target.start_time`, significa que o PID original morreu antes de `pidfd_open` e o número do PID foi reutilizado por outro processo! Aborta imediatamente sem enviar sinal e retorna `Err(PtyError::PidNotOwned(target.pid))`.
5. Quando `expected_cgroup_id` é exigido (`signal_in_cgroup` / `PtyManager::signal_process`), verifica o inode do cgroup atual de `/proc/<pid>/cgroup` (ou se `target.pid` consta nos membros do `ContainmentHandle`). Se não pertencer ao cgroup do PTY, retorna `Err(PtyError::PidNotOwned(target.pid))`.
6. Envia o sinal diretamente ao descritor validado usando syscall segura sobre o `pidfd` (`rustix` / `pidfd_send_signal`).

### 3.3. Implementação `ProcfsSource` e Parsing de `/proc/<pid>/stat` (`src/proc/source.rs`, `src/proc/info.rs`)

- **Leitura de `boot_id`**: lido uma vez de `/proc/sys/kernel/random/boot_id` (trim de whitespace/hífens opcionais preservados de forma canônica).
- **Tamanho de página (`page_size`)**: obtido via `rustix::param::page_size() as u64` para calcular `rss_bytes = stat.rss * page_size`.
- **Mapeamento de `ProcState`**:
  - `'R' => ProcState::Running`
  - `'S' => ProcState::Sleeping`
  - `'D' => ProcState::DiskSleep`
  - `'T' => ProcState::Stopped`
  - `'t' => ProcState::TracingStop`
  - `'Z' => ProcState::Zombie`
  - `'X' | 'x' => ProcState::Dead`
  - `'I' => ProcState::Idle`
  - `_ => ProcState::Unknown`
- **Proteção contra mistura de campos durante reciclagem de PID (`src/proc/info.rs`)**:
  1. Lê `stat_before = proc.stat()` (extraindo `starttime` — campo 22). Se `ENOENT`/`ESRCH`, retorna `Ok(None)`.
  2. Lê `cmdline = proc.cmdline().ok().unwrap_or_default()`, `exe = proc.exe().ok().map(|p| p.to_string_lossy().into_owned())`, `cwd = proc.cwd().ok().map(|p| p.to_string_lossy().into_owned())`. Erros `EACCES`/`EPERM` nesses links simbólicos resultam em `None` no campo opcional, sem falhar a leitura do processo.
  3. Relê `stat_after = proc.stat()`. Se o processo morreu nesse meio-tempo ou `stat_after.starttime != stat_before.starttime`, retorna `Ok(None)` para jamais combinar `stat` de um processo com `cwd`/`cmdline` de outro.
  4. Também expõe `pub fn parse_proc_stat_line(line: &str, page_size: u64) -> PtyResult<ParsedStat>` para testes unitários diretos de strings brutas de `/proc/<pid>/stat` onde `comm` contém espaços, parênteses aninhados (ex.: `1234 (meu (proc) teste) S ...`) e caracteres especiais.

### 3.4. Árvore de Processos e Relatório (`src/proc/tree.rs`)

```rust
use mt_core::PtyId;
use crate::error::PtyResult;
use crate::proc::ProcSource;
use crate::types::{ProcessIdentity, ProcessInfo, ProcessNode, PtyPids, PtyProcessReport};

pub fn build_process_tree(
    shell_identity: &ProcessIdentity,
    candidates: &[ProcessInfo],
) -> Option<ProcessNode>;

pub fn build_process_report(
    pty_id: PtyId,
    shell_identity: &ProcessIdentity,
    pids: PtyPids,
    cgroup_pids: &[u32],
    proc_source: &dyn ProcSource,
    sampled_at_ms: u64,
) -> PtyResult<PtyProcessReport>;
```

Regras de `build_process_report`:
- Enumera os PIDs do cgroup (`cgroup_pids`, acrescidos do `shell_identity.pid`).
- Para cada PID, consulta `proc_source.process(pid)`. Se um processo sair entre a listagem do cgroup e a leitura de `/proc/<pid>`, ele retorna `Ok(None)` e é simplesmente omitido sem gerar erro (**CA-26**).
- Se uma falha de permissão/I/O impedir a leitura de parte dos membros, marca `partial = true`.
- Só retorna `Err(PtyError::ProcUnavailable(...))` se o `/proc` inteiro estiver inacessível e nem mesmo o próprio PTY vivo puder ser inspecionado.
- Monta `tree: Option<ProcessNode>` enraizada no `shell` (validando `shell.start_time == shell_identity.start_time`) seguindo os vínculos `ppid → pid` ordenados por `pid`.
- Calcula `descendant_count` como o total de nós na árvore abaixo do shell (ou total de processos adicionais no cgroup).
- Preenche `owned_processes` com todos os `ProcessInfo` vivos pertencentes ao cgroup (inclusive daemons que fizeram `setsid`/double-fork e cujo `ppid` mudou para o subreaper ou PID 1).
- Resolve `foreground: Option<ProcessInfo>` usando `resolve_foreground_process(pids.foreground_pgid, &owned_processes, proc_source)`.

### 3.5. Resolução de Foreground (`src/proc/foreground.rs`)

```rust
use std::os::fd::BorrowedFd;
use crate::proc::ProcSource;
use crate::types::ProcessInfo;

/// Obtém o PGID em foreground do terminal associado ao descritor master/slave via `tcgetpgrp`.
pub fn query_foreground_pgid(pty_fd: BorrowedFd<'_>) -> Option<u32>;

/// Determina o processo representativo do grupo de foreground:
/// 1. O processo vivo cujo `pid == fg_pgid` (líder do grupo de processos).
/// 2. Se o líder já tiver encerrado, o membro vivo com `pgid == fg_pgid` que possua o maior `start_time`.
pub fn resolve_foreground_process(
    fg_pgid: Option<u32>,
    owned_processes: &[ProcessInfo],
    proc_source: &dyn ProcSource,
) -> Option<ProcessInfo>;
```

### 3.6. Supervisor Periódico (`src/supervisor.rs`)

```rust
use std::sync::Arc;
use std::time::Duration;

pub trait SupervisorHost: Send + Sync + 'static {
    /// Inspeciona foreground dos PTYs `Running` e emite `PtyEvent::ForegroundChanged` se `(pgid, start_time)` mudou.
    fn poll_foreground_changes(&self);

    /// Executa `waitpid(pid, WNOHANG)` nos processos órfãos adotados pelo subreaper que não são filhos diretos com waiter.
    fn reap_adopted_orphans(&self);

    /// Expira tombstones cujo `Exited.at_ms` ultrapassou `tombstone_ttl` (nunca expira `CleanupFailed`).
    fn expire_tombstones(&self);

    /// Verifica timeouts de `ack_timeout` nas conexões pausadas/ativas e saúde do watchdog.
    fn check_leases_and_watchdog(&self);
}

pub struct SupervisorHandle {
    stop_tx: crossbeam_channel::Sender<()>,
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl SupervisorHandle {
    pub fn spawn(
        interval: Duration,
        stack_size: usize,
        host: Arc<dyn SupervisorHost>,
    ) -> std::io::Result<Self>;

    pub fn shutdown(&mut self);
}
```

Regras do `SupervisorHandle`:
- Thread nomeada `mt-pty-supervisor` com stack size configurado (`256 KiB`).
- A cada `supervisor_interval` (`1 s`), executa as 4 rotinas de manutenção sem nunca varrer `cmdline` de árvores inteiras de PTYs cujo `tcgetpgrp` não mudou (mantendo o custo proporcional apenas às mudanças de foreground).
- Deduplicação de `PtyEvent::ForegroundChanged`: cada PTY armazena `last_foreground: Option<(u32, u64)>` (`(pgid, leader_start_time)`). O evento só é emitido quando esse par muda (**CA-28**).

---

## 4. Regras de Negócio, Segurança e Casos de Borda

1. **Proibição de `kill(pid)` cru**: Nenhum código em `mt-pty` pode chamar `nix::sys::signal::kill(Pid::from_raw(pid), sig)` diretamente com base em um `u32` lido de `/proc` anteriormente. Toda sinalização passa por `Signaler` (`PidfdSignaler`).
2. **Corrida entre listagem e leitura de `/proc`**: Em sistemas Linux sob carga, processos curtos (ex.: subshells de prompt git) nascem e morrem em sub-milissegundos. `ENOENT` e `ESRCH` em qualquer etapa de `/proc/<pid>/*` são condições normais e retornam `Ok(None)`, nunca erro fatal.
3. **`comm` com parênteses e espaços**: Em `/proc/<pid>/stat`, o nome do executável (`comm`) fica entre `(` e o **último** `)` da linha (ex.: `(Web Content (1))`). O parser deve localizar o último `)` via `rfind(')')` antes de dividir os campos numéricos subsequentes.

---

## 5. Critérios de Aceite Mapeados

- **CA-25**: `process_info` (`PtyProcessReport`) de um PTY executando `sleep 30` retorna `foreground.cmdline == ["sleep", "30"]`, `foreground.state == ProcState::Sleeping`, `shell.pid == pids.shell_pid` e `cwd` atualizado após `cd /tmp`.
- **CA-26**: `process_tree` lista corretamente 3 níveis (`sh → sh → sleep`) com `ppid` consistentes; processos que saem entre a listagem e a leitura são omitidos sem gerar erro.
- **CA-27**: `signal_process` para processo fora do cgroup do PTY retorna `"pty.pid_not_owned"`; se o PID foi reutilizado (`start_time` divergente), o `PidfdSignaler` aborta antes de enviar o sinal e retorna `"pty.pid_not_owned"`.
- **CA-28**: `pty://foreground-changed` (`PtyEvent::ForegroundChanged`) é emitido no máximo uma vez por mudança de processo em foreground, com latência ≤ 1,5 s.
