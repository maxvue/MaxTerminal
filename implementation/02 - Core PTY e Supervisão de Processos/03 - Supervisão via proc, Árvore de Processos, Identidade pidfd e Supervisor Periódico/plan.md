# Plano de Execução — Implementação 03: Supervisão via `/proc`, Árvore de Processos, Identidade `pidfd` e Supervisor Periódico

## 1. Checklist Sequencial de Implementação

### Passo 1: Traits `ProcSource`, `Signaler` e `PidfdSignaler` (`src/proc/mod.rs`)
1. Criar `src-tauri/crates/mt-pty/src/proc/mod.rs` definindo as traits `ProcSource` e `Signaler`.
2. Implementar `PidfdSignaler`:
   - Validação de `boot_id`.
   - Abertura de `pidfd` via `rustix::process::pidfd_open`.
   - Releitura de `proc_source.identity(pid)` enquanto o `pidfd` está aberto para confirmar igualdade exata de `start_time`.
   - Verificação de pertencimento ao cgroup (`signal_in_cgroup`) e envio do sinal via `pidfd`.
3. Implementar `RecordingSignaler` para testes unitários determinísticos.

### Passo 2: Parsing de `/proc/<pid>/stat`, `ProcfsSource` e `FakeProcSource` (`src/proc/info.rs`, `src/proc/source.rs`)
1. Criar `src-tauri/crates/mt-pty/src/proc/info.rs` com:
   - Mapeamento de caracteres de estado do Linux (`R`, `S`, `D`, `T`, `t`, `Z`, `X`, `I`) para `ProcState`.
   - Função `parse_proc_stat_line` que usa `find('(')` e `rfind(')')` para extrair `comm` com segurança mesmo quando contém espaços ou parênteses internos.
   - Leitura protegida de `ProcessInfo` verificando `starttime` antes e depois de ler `cmdline`, `exe` e `cwd`, convertendo `ENOENT`/`ESRCH` em `Ok(None)` e `EACCES` em `None` nos campos opcionais.
2. Criar `src-tauri/crates/mt-pty/src/proc/source.rs` implementando `ProcfsSource` (sobre o `/proc` real do Linux) e `FakeProcSource` (em memória para testes unitários).
3. Adicionar testes unitários em `info.rs` e `source.rs` cobrindo:
   - Parsing de `stat` com nomes complexos (`(a (b) c)`).
   - Omissão silenciosa quando o processo desaparece entre leituras.
   - Rejeição de sinalização no `PidfdSignaler` quando `start_time` muda (simulando reciclagem de PID — CA-27).

### Passo 3: Construção de `ProcessNode`, `PtyProcessReport` e Foreground (`src/proc/tree.rs`, `src/proc/foreground.rs`)
1. Criar `src-tauri/crates/mt-pty/src/proc/tree.rs` com `build_process_tree` e `build_process_report`:
   - Construir a árvore recursiva `sh → sh → sleep` a partir de `shell_identity` e do mapa `ppid → Vec<ProcessInfo>`.
   - Preencher `owned_processes` com todos os membros do cgroup (inclusive processos reparentados fora da árvore PPID).
2. Criar `src-tauri/crates/mt-pty/src/proc/foreground.rs` com `query_foreground_pgid` (usando `rustix::termios::tcgetpgrp`) e `resolve_foreground_process` (selecionando o líder `pid == fg_pgid` ou o membro vivo do grupo com maior `start_time`).
3. Escrever testes unitários em `tree.rs` e `foreground.rs` com `FakeProcSource` validando árvores de 3 níveis, processos que morrem no meio da amostragem e fallback de líder de foreground.

### Passo 4: Thread `mt-pty-supervisor` (`src/supervisor.rs`)
1. Criar `src-tauri/crates/mt-pty/src/supervisor.rs` com a trait `SupervisorHost` e `SupervisorHandle`.
2. Implementar a lógica de deduplicação de mudança de foreground por `(pgid, start_time)` para garantir que `PtyEvent::ForegroundChanged` seja emitido apenas uma vez por transição (CA-28).
3. Expor `pub mod proc;` e `pub mod supervisor;` em `src-tauri/crates/mt-pty/src/lib.rs` e adicionar testes unitários do ciclo do supervisor.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificar formatação e lint estrito
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# 2. Executar todos os testes unitários de /proc, árvore, pidfd e supervisor
cargo test --manifest-path src-tauri/Cargo.toml -p mt-pty proc::
cargo test --manifest-path src-tauri/Cargo.toml -p mt-pty supervisor::
```

---

## 3. Critérios de Aceite da Implementação 03

| Critério da Fase 02 | Descrição da Validação nesta Implementação |
|---------------------|--------------------------------------------|
| **CA-25** | `build_process_report` e `ProcfsSource` extraem `foreground.cmdline`, `foreground.state`, `shell.pid` e `cwd` real de processos Linux em execução. |
| **CA-26** | `build_process_tree` monta hierarquia de 3 níveis (`sh → sh → sleep`) com `ppid` consistentes e ignora sem erro processos que encerram durante a leitura. |
| **CA-27** | `PidfdSignaler::signal_in_cgroup` retorna `PtyError::PidNotOwned` (`"pty.pid_not_owned"`) quando o processo não pertence ao cgroup ou quando `start_time`/`boot_id` diverge da identidade esperada. |
| **CA-28** | O supervisor detecta alterações de `tcgetpgrp` e deduplica por `(pgid, start_time)`, emitindo no máximo um evento por mudança. |
