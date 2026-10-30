# Especificação Técnica — Implementação 04: Ciclo de Vida Completo, Kill Gracioso, Subreaper, Guards e `PtyManager`

## 1. Contexto, Objetivo e Escopo

Esta é a quarta unidade de implementação da **Fase 02 — Core PTY e Supervisão de Processos**. Ela integra todas as fundações das Implementações 01, 02 e 03 para entregar o crate `mt-pty` completo e 100% funcional:
1. O plano de encerramento escalonado `KillPlan` (`src/kill.rs`): `SIGHUP` + `SIGCONT` nos grupos shell/foreground → espera `hup_grace` (`1 s`) → `SIGTERM` + `SIGCONT` renovando enumeração dos membros atuais do cgroup durante `term_grace` (`2 s`) → escrita em `cgroup.kill` (`1`), confirmação obrigatória de `cgroup.events populated=0` e reaping seletivo, com promoção imediata para `Force` e tratamento de `CleanupFailed`.
2. O subsistema de guards e recuperação de falhas (`src/guard/`): configuração de `PR_SET_CHILD_SUBREAPER`, reaper seletivo de órfãos adotados que nunca usa `waitpid(-1)` (evitando roubar status do `waiter` do PTY ou de plugins Tauri), persistência atômica de `$XDG_RUNTIME_DIR/maxterminal/<instance_id>/pty-registry.json` (`0600` em diretório `0700`), varredura de recuperação de instâncias mortas no startup, hook de `panic!` lock-free e handler de sinais (`SIGINT`/`SIGTERM`/`SIGHUP`).
3. A coordenação de cada instância de terminal (`src/instance.rs` — `PtyInstance`, threads `reader`, `writer`, `waiter`, `resize` com deduplicação e contador de `ioctl`, e `cleanup_session` quando o shell sai sozinho) e a fachada pública `PtyManager` (`src/manager.rs`).
4. O exemplo executável `examples/pty_smoke.rs` e todas as 7 suítes de testes de integração de `mt-pty` (`tests/spawn.rs`, `tests/io.rs`, `tests/backpressure.rs`, `tests/kill.rs`, `tests/orphans.rs`, `tests/proc.rs`, `tests/panic_cleanup.rs`).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Modificados
- `src-tauri/crates/mt-pty/src/lib.rs` — exportação pública completa de `PtyManager`, `PtyConfig`, `PtyError`, `PtyResult`, tipos de domínio e traits.

### 2.2. Arquivos Criados
- `src-tauri/crates/mt-pty/src/kill.rs` — `KillPlan`, `KillCoordinator` e execução de escalada (`SIGHUP → SIGTERM → cgroup.kill`) e `cleanup_session_after_shell_exit`.
- `src-tauri/crates/mt-pty/src/guard/mod.rs` — `GuardHandle`, instalação de `PR_SET_CHILD_SUBREAPER`, registro de PIDs com dono de `waitpid` (`DirectChildRegistry`) e reaper seletivo de adotados.
- `src-tauri/crates/mt-pty/src/guard/registry_file.rs` — `RegistryFile`, escrita atômica (`temp + fsync + rename`) com permissões `0600` em diretório `0700` e lock de instância (`flock`), mais `recover_dead_instances`.
- `src-tauri/crates/mt-pty/src/guard/panic.rs` — instalação de panic hook encadeado que escreve 1 byte no `emergency_fd` do watchdog sem alocar memória nem adquirir mutexes.
- `src-tauri/crates/mt-pty/src/guard/signals.rs` — thread de sinais via `signal-hook` (`SIGTERM`, `SIGINT`, `SIGHUP`, `SIGCHLD`).
- `src-tauri/crates/mt-pty/src/instance.rs` — `PtyInstance`, gerenciamento das threads `mt-pty-r-<id8>`, `mt-pty-w-<id8>`, `mt-pty-x-<id8>`, `ExitSlot`, contador `ioctl_resize_count` e máquina de estados do ciclo de vida.
- `src-tauri/crates/mt-pty/src/manager.rs` — `PtyManager` e `ManagerInner` implementando toda a API pública da especificação §4 e `SupervisorHost`.
- `src-tauri/crates/mt-pty/examples/pty_smoke.rs` — utilitário CLI (`cargo run -p mt-pty --example pty_smoke -- --cmd 'ls -la'`).
- `src-tauri/crates/mt-pty/tests/spawn.rs` — testes de integração dos critérios CA-04, CA-05, CA-06, CA-07, CA-32.
- `src-tauri/crates/mt-pty/tests/io.rs` — testes de integração dos critérios CA-08, CA-09, CA-14, CA-33.
- `src-tauri/crates/mt-pty/tests/backpressure.rs` — testes de integração dos critérios CA-10, CA-11, CA-12, CA-13, CA-15.
- `src-tauri/crates/mt-pty/tests/kill.rs` — testes de integração dos critérios CA-16, CA-17.
- `src-tauri/crates/mt-pty/tests/orphans.rs` — testes de integração dos critérios CA-18, CA-19, CA-20, CA-22, CA-23, CA-34.
- `src-tauri/crates/mt-pty/tests/proc.rs` — testes de integração dos critérios CA-25, CA-26, CA-27, CA-28.
- `src-tauri/crates/mt-pty/tests/panic_cleanup.rs` — teste de integração em subprocesso para o critério CA-21 (`MT_PTY_TEST_PANIC=1`).

---

## 3. Especificação Técnica Detalhada e Assinaturas Rust

### 3.1. Plano de Kill e Limpeza de Sessão (`src/kill.rs`)

```rust
use std::time::Duration;
use crate::containment::ContainmentHandle;
use crate::error::PtyResult;
use crate::proc::{ProcSource, Signaler};
use crate::types::{KillMode, ProcessIdentity};

#[derive(Debug, Clone, Copy)]
pub struct KillPlan {
    pub hup_grace: Duration,
    pub term_grace: Duration,
    pub force: bool,
}

pub enum KillOutcome {
    Cleaned,
    CleanupFailed { survivors: Vec<ProcessIdentity> },
}

pub struct KillCoordinator {
    // Garante uma única execução de escalada por PTY; chamadas concorrentes aguardam o mesmo resultado
    // e chamadas subsequentes com KillMode::Force promovem imediatamente o estágio atual para cgroup.kill.
}
```

Algoritmo exato do `KillCoordinator`:
1. Transiciona `PtyInstance` de `Running` para `Exiting { since_ms }`, marca `kill_requested` (`1 = user`, `2 = shutdown`), fecha/cancela `WriteQueue` e acorda o `WriterWorker` e o `ReaderWorker` (para que consumidor pausado não bloqueie o encerramento).
2. Se `mode == KillMode::Graceful`:
   - Enumera os membros atuais do cgroup do PTY; identifica aqueles cujo `pgid == shell_pgid` ou `pgid == foreground_pgid`.
   - Envia `SIGHUP` e `SIGCONT` (para acordar jobs parados com `Ctrl+Z` e permitir execução de traps) a cada um via `Signaler::signal_in_cgroup`.
   - Aguarda até `hup_grace` (`1 s`, ou `300 ms` no shutdown) em intervalos curtos (ou notificação de `cgroup.events`), interrompendo imediatamente se `cgroup.is_empty()` se tornar verdadeiro ou se `Force` for solicitado.
3. Se o cgroup continuar populado e `Force` ainda não tiver sido ativado:
   - Durante `term_grace` (`2 s`, ou `700 ms` no shutdown), renova periodicamente (a cada `50 ms`) a enumeração dos membros atuais do cgroup e envia `SIGTERM` + `SIGCONT` por `pidfd` a quaisquer processos novos ou remanescentes (cobrindo forks concorrentes durante a escalada e daemons que fizeram `setsid`).
4. Se após `term_grace` o cgroup ainda estiver populado, ou imediatamente quando `mode == KillMode::Force`:
   - Aciona `containment.kill_all()` (escreve `"1"` em `cgroup.kill` do cgroup e eventuais subcgroups).
5. Aguarda confirmação de `containment.wait_empty(deadline)` (`cgroup.events` com `populated 0`), executa reaping dos filhos diretos e adotados daquele cgroup e aguarda o término das threads `waiter`, `reader` e `writer`.
6. Se dentro do deadline total (`3,5 s`) `populated == 0` for confirmado:
   - Libera o cgroup no watchdog e remove o diretório do cgroup.
   - Emite o frame `End(ExitInfo)` no Channel (se anexado) e em seguida emite `PtyEvent::Exited { pty_id, owner, exit, final_seq, revision }`.
7. Se processos permanecerem vivos no cgroup após o deadline (ex.: processo preso em estado ininterruptível `D` no kernel):
   - **Nunca** reporta sucesso falso: define status como `PtyStatus::CleanupFailed { error }`, emite `PtyEvent::CleanupFailed { pty_id, survivors, revision }` e mantém o cgroup registrado no supervisor e no watchdog (**CA-20**).

### 3.2. Saída Espontânea do Shell (`cleanup_session_after_shell_exit`)

Quando a thread `waiter` detecta que o processo principal do shell encerrou sozinho (ex.: comando `exit` ou `EOF`), mas deixou processos em background, `nohup`, `setsid` ou double-fork dentro do cgroup (**CA-19**):
1. O `waiter` converte `portable_pty::ExitStatus` em `ExitInfo`:
   - Se `kill_requested == 1`, `reason = ExitReason::KilledByUser`.
   - Se `kill_requested == 2`, `reason = ExitReason::Shutdown`.
   - Caso contrário, se `status.signal()` retornar `Some(sig_name)`, `code = None`, `signal_name = Some(sig_name)`, `signal = signal_number_from_name(sig_name)`, `reason = ExitReason::Signaled`.
   - Caso contrário, `code = Some(status.exit_code() as i32)`, `signal = None`, `signal_name = None`, `reason = ExitReason::Exited`.
2. Se `containment.is_empty()` for falso, executa limpeza dos remanescentes do cgroup (`SIGTERM` com grace curto → `cgroup.kill`) e aguarda `populated == 0`.
3. Drena quaisquer bytes finais pendentes no `ReaderWorker`, enfileira o frame `End(ExitInfo)` e só então publica `PtyEvent::Exited` carregando o `final_seq` exato.

### 3.3. Guards, Subreaper e Registro Efêmero (`src/guard/`)

- **Subreaper e Reaper Seletivo (`src/guard/mod.rs`)**:
  - `rustix::process::set_child_subreaper(true)` (equivalente seguro a `prctl(PR_SET_CHILD_SUBREAPER, 1)`).
  - `DirectChildRegistry`: conjunto protegido de PIDs que possuem dono de `waitpid` dedicado (cada shell pertence à sua thread `waiter`, e o processo `watchdog` pertence ao `WatchdogClient`).
  - `reap_adopted_orphans`: enumera os PIDs presentes nos cgroups dos PTYs gerenciados (ou processos com `ppid == getpid()` que pertençam aos nossos cgroups) **excluindo** todos os PIDs contidos em `DirectChildRegistry`. Para cada adotado, chama `nix::sys::wait::waitpid(Pid::from_raw(pid as i32), Some(WaitPidFlag::WNOHANG))`. **Proibido chamar `waitpid(Pid::from_raw(-1), ...)`**, garantindo zero corrida com `child.wait()` do `portable-pty` (**CA-23**).

- **Arquivo de Registro e Recovery (`src/guard/registry_file.rs`)**:
  - Caminho: `$XDG_RUNTIME_DIR/maxterminal/<instance_id>/pty-registry.json`.
  - Valida que o diretório base pertence ao UID atual e cria a árvore com modo `0700` e arquivo com modo `0600` (falha retorna `PtyError::RuntimeDirInvalid`).
  - Mantém um arquivo `instance.lock` com `flock(LOCK_EX | LOCK_NB)`.
  - Schema do `pty-registry.json`:
    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct InstanceRegistryData {
        pub version: u32, // 1
        pub instance_id: String,
        pub boot_id: String,
        pub app_identity: ProcessIdentity,
        pub watchdog_identity: Option<ProcessIdentity>,
        pub ptys: Vec<RegisteredPtyEntry>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct RegisteredPtyEntry {
        pub pty_id: PtyId,
        pub cgroup_path: PathBuf,
        pub cgroup_id: u64,
        pub shell_identity: ProcessIdentity,
    }
    ```
  - **Recovery no Startup (`recover_dead_instances`)**:
    - Varre os subdiretórios irmãos em `$XDG_RUNTIME_DIR/maxterminal/*`.
    - Tenta adquirir `flock(LOCK_EX | LOCK_NB)` em `instance.lock`. Se falhar (`EWOULDBLOCK`), a outra instância está viva: ignora imediatamente sem tocar em nada (**CA-22**).
    - Se adquirir o lock, lê `pty-registry.json`:
      - Se `boot_id != current_boot_id`: apenas remove o diretório obsoleto de boot anterior.
      - Se `boot_id == current_boot_id`: valida que `app_identity` e `watchdog_identity` realmente morreram (via `ProcSource::identity`), valida que cada `cgroup_path` está estritamente dentro do subtree delegado do usuário e que seu inode coincide com `cgroup_id`, escreve `"1"` em `cgroup.kill`, aguarda `populated == 0`, remove o cgroup e limpa o diretório da instância morta (**CA-22**).

### 3.4. API Completa do `PtyManager` (`src/manager.rs`)

Implementa todos os métodos especificados na seção §4 de `spec.md`:
- `new(config: PtyConfig) -> PtyResult<Self>`
- `with_deps(config: PtyConfig, proc: Arc<dyn ProcSource>, signaler: Arc<dyn Signaler>, env_policy: Arc<dyn EnvPolicy>) -> PtyResult<Self>`
- `spawn(&self, spec: SpawnSpec, sink: Option<Arc<dyn OutputSink>>) -> PtyResult<PtySummary>`
- `write(&self, id: PtyId, data: Bytes) -> PtyResult<()>`
- `resize(&self, id: PtyId, size: PtySize) -> PtyResult<()>` (com `pub fn resize_ioctl_count(&self, id: PtyId) -> PtyResult<u64>` para comprovar deduplicação no **CA-07**)
- `kill(&self, id: PtyId, mode: KillMode) -> PtyResult<()>`
- `kill_and_wait(&self, id: PtyId, mode: KillMode, timeout: Duration) -> PtyResult<ExitInfo>`
- `attach(&self, id: PtyId, sink: Arc<dyn OutputSink>, cursor: Option<OutputCursor>) -> PtyResult<AttachInfo>`
- `detach(&self, id: PtyId, attachment_id: AttachmentId) -> PtyResult<()>`
- `ack(&self, id: PtyId, attachment_id: AttachmentId, consumed_bytes: u64) -> PtyResult<()>`
- `list(&self) -> Vec<PtySummary>`
- `summary(&self, id: PtyId) -> PtyResult<PtySummary>`
- `shutdown(&self, reason: ExitReason) -> ShutdownReport` (executa escaladas em paralelo para todos os PTYs ativos, encerrando 20 PTYs em ≤ 3,5 s — **CA-20**)
- `pids(&self, id: PtyId) -> PtyResult<PtyPids>`
- `foreground_process(&self, id: PtyId) -> PtyResult<Option<ProcessInfo>>`
- `process_tree(&self, id: PtyId) -> PtyResult<ProcessNode>`
- `owned_processes(&self, id: PtyId) -> PtyResult<Vec<ProcessIdentity>>`
- `process_report(&self, id: PtyId) -> PtyResult<PtyProcessReport>`
- `signal_process(&self, id: PtyId, process: ProcessIdentity, sig: Signal) -> PtyResult<()>`
- `owner_of_process(&self, process: ProcessIdentity) -> Option<PtyId>`
- `add_listener(&self, l: Arc<dyn PtyEventListener>)`
- `add_output_tap(&self, tap: Arc<dyn OutputTap>) -> TapHandle`
- `subscribe(&self) -> crossbeam_channel::Receiver<PtyEvent>`
- `install_guards(&self) -> PtyResult<GuardHandle>`
- `emergency_kill_all(&self)`

---

## 4. Critérios de Aceite Mapeados

- **CA-01 / CA-02**: `cargo fmt --check`, `cargo clippy` e `cargo test -p mt-pty` passam 100%.
- **CA-04**: `spawn` de `/bin/sh -c 'printf hello; exit 3'` produz `hello` e `ExitInfo { code: Some(3), signal: None, reason: Exited }`.
- **CA-07**: `resize(40, 120)` reflete em `stty size` (`40 120`); dimensões inválidas retornam `pty.resize_invalid`; resize idêntico não incrementa `resize_ioctl_count`.
- **CA-16 / CA-17**: `pty_kill(Graceful)` encerra shell ocioso em < 500 ms com `KilledByUser`; processos que ignoram `SIGHUP` ou `SIGHUP + SIGTERM` são encerrados por `SIGTERM` (1 s) e `SIGKILL` (2 s) em ≤ 3,5 s.
- **CA-18 / CA-19 / CA-20**: Processos em background, `nohup`, `setsid`, double-fork e ambiente limpo são 100% encerrados (`populated=0`), isolados entre PTYs distintos, tanto no `kill` quanto na saída espontânea do shell e no `shutdown()` de 20 PTYs.
- **CA-21 / CA-22 / CA-23**: Subprocesso que sofre `panic!` ou `SIGKILL` tem todos os descendentes limpos pelo watchdog/recovery; órfãos adotados são reapados sem virar zumbis por mais de 2 ciclos do supervisor.
- **CA-31**: `cargo run -p mt-pty --example pty_smoke -- --cmd 'ls -la'` imprime o output e o código de saída.
- **CA-34**: 100 ciclos de `spawn/attach/resize/kill` não vazam FDs, threads ou cgroups, mesmo sob corridas de resize/kill/write bloqueado.
