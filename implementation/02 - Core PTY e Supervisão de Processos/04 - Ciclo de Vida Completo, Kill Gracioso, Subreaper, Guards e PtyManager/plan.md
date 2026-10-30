# Plano de Execução — Implementação 04: Ciclo de Vida Completo, Kill Gracioso, Subreaper, Guards e `PtyManager`

## 1. Checklist Sequencial de Implementação

### Passo 1: Plano de Kill e Escalonamento (`src/kill.rs`)
1. Implementar `src-tauri/crates/mt-pty/src/kill.rs` com `KillPlan`, `KillCoordinator` e `cleanup_session_after_shell_exit`.
2. Garantir a sequência exata:
   - `SIGHUP` + `SIGCONT` nos grupos do shell e de foreground validados por `pidfd` e cgroup.
   - Espera `hup_grace` monitorando `containment.is_empty()` e flag de promoção para `Force`.
   - `SIGTERM` + `SIGCONT` renovando a listagem de membros do cgroup durante `term_grace` para capturar forks concorrentes.
   - Acionamento de `cgroup.kill` (`containment.kill_all()`), confirmação de `populated == 0` e transição para `Exited` ou `CleanupFailed`.
3. Adicionar testes unitários em `kill.rs` usando `RecordingSignaler` e `FakeProcSource` para validar tempos e ordem de sinais (`SIGHUP → SIGTERM → SIGKILL`).

### Passo 2: Guards, Subreaper, Registro Efêmero e Recovery (`src/guard/`)
1. Implementar `src-tauri/crates/mt-pty/src/guard/registry_file.rs` com gravação atômica de `pty-registry.json` (`0600` em diretório `0700`), `flock` em `instance.lock` e `recover_dead_instances` no startup.
2. Implementar `src-tauri/crates/mt-pty/src/guard/panic.rs` e `src/guard/signals.rs` notificando o `emergency_fd` do watchdog sem alocações nem locks.
3. Implementar `src-tauri/crates/mt-pty/src/guard/mod.rs` com `set_child_subreaper(true)`, `DirectChildRegistry` e `reap_adopted_orphans` usando `waitpid(pid, WNOHANG)` exclusivamente em órfãos adotados confirmados.

### Passo 3: `PtyInstance` e `PtyManager` (`src/instance.rs`, `src/manager.rs`)
1. Implementar `src-tauri/crates/mt-pty/src/instance.rs` coordenando `MasterPty`, descritor duplicado de I/O, `ContainmentHandle`, threads `mt-pty-r-<id8>`, `mt-pty-w-<id8>` e `mt-pty-x-<id8>` (com stack size de `256 KiB`), deduplicação de `resize` com contador atômico de `ioctl` e `ExitSlot`.
2. Implementar `src-tauri/crates/mt-pty/src/manager.rs` com todos os métodos públicos de `PtyManager`:
   - Reserva atômica de capacidade (`max_ptys`) e verificação de `shutting_down`.
   - Ordenação garantida no `spawn`: criar cgroup → iniciar helper launcher bloqueado → registrar no watchdog e `pty-registry.json` → abrir `pidfd` → iniciar threads → emitir `PtyEvent::Spawned` → liberar `execve` no launcher.
   - `shutdown()` disparando escaladas em paralelo para encerrar múltiplos PTYs dentro do orçamento de tempo.
   - Implementação de `SupervisorHost` conectada ao `SupervisorHandle`.
3. Atualizar `src-tauri/crates/mt-pty/src/lib.rs` exportando toda a API pública.

### Passo 4: Exemplo CLI (`examples/pty_smoke.rs`)
1. Criar `src-tauri/crates/mt-pty/examples/pty_smoke.rs` aceitando `--cmd <comando>`, `--cwd <dir>` e `--shell <bin>`, instanciando `PtyManager`, anexando um `OutputSink` que decodifica os frames `Data` para `stdout` e aguardando o frame `End` / evento `Exited` para imprimir o código de saída (CA-31).

### Passo 5: Suíte Completa de Testes de Integração (`tests/*.rs`)
1. Criar um módulo utilitário de teste (ou helpers compartilhados) com `wait_until(cond, timeout)` e configuração automática de cgroup delegado de teste.
2. Implementar `src-tauri/crates/mt-pty/tests/spawn.rs` validando CA-04, CA-05, CA-06, CA-07 e CA-32.
3. Implementar `src-tauri/crates/mt-pty/tests/io.rs` validando CA-08 (1 MiB raw SHA-256 + bytes `0..=255`), CA-09 (UTF-8/ANSI cortados), CA-14 (eco p99 < 10 ms em 1 000 amostras) e CA-33 (`OutputTap`).
4. Implementar `src-tauri/crates/mt-pty/tests/backpressure.rs` validando CA-10, CA-11 (marca d'água 1 MiB, RSS < 16 MiB e bloqueio do produtor), CA-12, CA-13 (replay + attach) e CA-15.
5. Implementar `src-tauri/crates/mt-pty/tests/kill.rs` validando CA-16 e CA-17 (`trap '' HUP` e `trap '' HUP TERM`).
6. Implementar `src-tauri/crates/mt-pty/tests/orphans.rs` validando CA-18 (background, `nohup`, `setsid`, double-fork, isolamento entre 2 PTYs), CA-19 (saída espontânea do shell com `final_seq`), CA-20 (`shutdown` de 20 PTYs em ≤ 3,5 s e simulação de falha em `CleanupFailed`), CA-22 (`SIGKILL` + watchdog + recovery), CA-23 (reaping de adotados em ≤ 2 ciclos) e CA-34 (100 ciclos de stress sem vazamento de FDs/threads/cgroups).
7. Implementar `src-tauri/crates/mt-pty/tests/proc.rs` validando CA-25, CA-26, CA-27 e CA-28.
8. Implementar `src-tauri/crates/mt-pty/tests/panic_cleanup.rs` validando CA-21 (reexecução com `MT_PTY_TEST_PANIC=1` que cria PTY com descendentes e chama `panic!`, comprovando que o watchdog limpa todos os processos).

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Formatação e Clippy em todos os alvos (lib, bins, examples, tests, benches)
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# 2. Executar o exemplo de fumaça CLI (CA-31)
cargo run --manifest-path src-tauri/Cargo.toml -p mt-pty --example pty_smoke -- --cmd 'ls -la'

# 3. Executar toda a suíte de testes unitários e de integração do mt-pty (CA-02)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-pty -- --test-threads=4
```

---

## 3. Critérios de Aceite da Implementação 04

| Critério da Fase 02 | Descrição da Validação nesta Implementação |
|---------------------|--------------------------------------------|
| **CA-01 / CA-02** | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test -p mt-pty` passam integralmente. |
| **CA-04 a CA-09** | Testes `tests/spawn.rs` e `tests/io.rs` comprovam spawn, exit code, validações, env, resize com deduplicação de ioctl, escrita de 1 MiB e integridade UTF-8/ANSI. |
| **CA-10 a CA-15** | Testes `tests/backpressure.rs` e `tests/io.rs` comprovam vazão, limite `high_watermark`, RSS < 16 MiB, retomada pós-ACK, replay com `Gap` e eco p99 < 10 ms. |
| **CA-16 a CA-23** | Testes `tests/kill.rs`, `tests/orphans.rs` e `tests/panic_cleanup.rs` comprovam escalonamento de sinais, zero processos órfãos mesmo com `setsid`/double-fork/`panic!`/`SIGKILL` e reaping sem zumbis. |
| **CA-25 a CA-28** | Testes `tests/proc.rs` comprovam inspeção real de `/proc`, árvore de 3 níveis, bloqueio de sinal fora do cgroup/PID reciclado e evento `ForegroundChanged` ≤ 1,5 s. |
| **CA-31** | `cargo run -p mt-pty --example pty_smoke -- --cmd 'ls -la'` executa o comando no PTY real e imprime saída e código de retorno. |
| **CA-33 / CA-34** | Taps observam cada byte original uma única vez e 100 ciclos de `spawn/attach/resize/kill` não aumentam FDs, threads nem diretórios de cgroup. |
