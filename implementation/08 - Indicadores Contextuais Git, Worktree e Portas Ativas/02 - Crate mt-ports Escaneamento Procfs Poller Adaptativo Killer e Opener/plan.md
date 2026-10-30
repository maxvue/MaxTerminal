# Plano de Execução — Implementação 02: Crate `mt-ports`

> **Fase:** 08 — Indicadores Contextuais: Git, Worktree e Portas Ativas
> **Unidade:** `02 - Crate mt-ports Escaneamento Procfs Poller Adaptativo Killer e Opener`
> **Especificação:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1 — Estrutura do Crate `mt-ports`, Tipos e Erros
1. Registrar `"crates/mt-ports"` em `src-tauri/Cargo.toml` (`[workspace].members` e `[workspace.dependencies]`).
2. Criar `src-tauri/crates/mt-ports/Cargo.toml` conforme `spec.md` §3.1.
3. Implementar `src-tauri/crates/mt-ports/src/types.rs` com `IpFamily`, `PortProtocol`, `PortId`, `PortAddress`, `ListeningPort`, `KillOutcome`, `PortKillResult`, `PollMode`, `PortSnapshot`, `PortOpened`, `PortUpdated`, `PortClosed` e `SocketIdentity`.
4. Implementar `src-tauri/crates/mt-ports/src/error.rs` com `PortsError`, mapeamento dos 10 códigos `ports.*` e conversão `From<PortsError> for mt_core::MtError`.

### Passo 2 — Parser de `/proc/net/{tcp,tcp6,udp,udp6}` (`procnet.rs`) e Fixtures
1. Implementar `parse_proc_net`, `read_listen_sockets` e `read_netns_inode_for_pid` em `src-tauri/crates/mt-ports/src/procnet.rs`.
2. Garantir decodificação exata de IPv4 little-endian (`swap_bytes`), IPv6 em 4 palavras de 32 bits little-endian, endereços IPv4-mapeados (`::ffff:a.b.c.d`), filtro de estado TCP `0A` e UDP `07`/`01` (`port > 0`, `inode > 0`), filtro `uid == geteuid()` e descarte seguro de linhas truncadas/malformadas sem *panic*.
3. Criar as fixtures `tests/fixtures/proc_net_{tcp,tcp6,udp,udp6}.txt` e a suíte `tests/procnet.rs` incluindo o teste determinístico de parsing e o benchmark de 10 000 linhas (< 5 ms).

### Passo 3 — Árvore de Processos (`tree.rs`) e Mapeamento de FDs (`fdmap.rs`)
1. Implementar `ProcId`, a trait `ProcessTreeSource` e `ProcfsTreeSource` (lendo `/proc/<pid>/task/*/children`) em `src-tauri/crates/mt-ports/src/tree.rs`.
2. Implementar `read_proc_stat`, `read_starttime` e `map_inodes` em `src-tauri/crates/mt-ports/src/fdmap.rs`:
   - Parser seguro de `/proc/<pid>/stat` localizando o último `')'` do campo `(comm)`.
   - Leitura de `starttime` (campo 22) antes e depois de varrer `/proc/<pid>/fd/*`, descartando amostra caso `starttime` mude.
   - Comparação de `/proc/<pid>/ns/net` com o `expected_netns`.
   - Suporte a múltiplos `FdOwner` para o mesmo `inode` (sockets herdados por fork).

### Passo 4 — Resolução de URL Local (`opener.rs`) e Killer Anti-TOCTOU (`killer.rs`)
1. Implementar `PortUrlOpener`, `resolve_browser_url` e `open_in_browser` em `src-tauri/crates/mt-ports/src/opener.rs` cobrindo dual-stack (`localhost`), IPv4 (`127.0.0.1`), IPv6 (`[::1]`), wildcard (`0.0.0.0`/`::`) e recusa de UDP ou LAN exclusivo (`PortsError::NotBrowserCompatible`).
2. Implementar `ExactProcessSignaler`, `ProcessHandle` e `kill_port_owner` em `src-tauri/crates/mt-ports/src/killer.rs`:
   - Recusa do shell raiz (`PortsError::KillRootRefused`) e de processos fora da árvore (`PortsError::NotTracked`).
   - Revalidação antes do `SIGTERM`, espera de até 3 s observando saída via handle, detecção de `KillOutcome::SocketReleased` se o socket fechar sem o processo sair, escalada para `SIGKILL` e nova varredura (`remaining`).

### Passo 5 — Poller Adaptativo e `PortsService` (`poller.rs`, `lib.rs`)
1. Implementar `PortsService` e o loop assíncrono do poller com `tokio::sync::Notify` e `tokio::time::sleep_until`:
   - Leitura única de `read_sockets()` por tick compartilhada entre todas as abas vencidas (`next_due <= now`).
   - Agendamento adaptativo: `Active` (1 s), `Lingering` (5 s), `IdleProbe` (30 s), `Off` (sem PTY vivo).
   - Varredura imediata em transições de status (`running → idle/finished/error`), fim de comando e `scan_now`.
   - Diff por aba mantendo `PortId` estável enquanto `(dono, protocol, port)` e conjunto de `SocketIdentity` permanecerem iguais, emitindo `PortOpened`, `PortUpdated`, `PortClosed` e `PortSnapshot` com `epoch` e `revision` monotônica.
   - Preservação de estado `stale: true` sem emitir `PortClosed` falsos caso ocorra erro parcial de leitura em `/proc`.

### Passo 6 — Testes Unitários e de Integração (`tests/poller.rs`, `tests/live_listen.rs`)
1. Criar `tests/poller.rs` usando `tokio::time::pause()`, `FakeTreeSource` e `FakeSocketTableSource` para validar cadências de 1 s / 5 s / 30 s / Off, bind atrasado após retorno ao prompt, diff sem duplicatas e preservação em falha parcial (`stale`).
2. Criar `tests/live_listen.rs` testando:
   - `TcpListener::bind("127.0.0.1:0")` e `UdpSocket::bind("127.0.0.1:0")` no processo atual com `ProcfsTreeSource` enraizado em `std::process::id()`.
   - Simulações determinísticas de TOCTOU no killer (mudança de `starttime`, liberação de socket antes do `SIGKILL` ⇒ `SocketReleased`, recusa do PID raiz ⇒ `KillRootRefused`, múltiplos donos no mesmo socket).
   - Testes de integração reais com `python3` (marcados com `#[ignore]` para execução via `cargo test -p mt-ports -- --ignored`) validando encerramento por `SIGTERM` (`Terminated`) e processo que ignora `SIGTERM` sendo morto por `SIGKILL` após ~3 s (`Killed`).

---

## 2. Comandos de Verificação e Testes

Execute os comandos abaixo na worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)"
cd src-tauri

# 1. Verificar compilação e tipos do crate mt-ports
cargo check -p mt-ports --all-targets

# 2. Verificar lint estrito sem warnings
cargo clippy -p mt-ports --all-targets -- -D warnings

# 3. Executar todos os testes unitários e de integração padrão
cargo test -p mt-ports

# 4. Executar testes de integração com processos python3 reais (SIGTERM / SIGKILL)
cargo test -p mt-ports -- --ignored
```

---

## 3. Critérios de Aceite da Implementação 02 (Mapeados para a Fase 08)

- [ ] **CA-08-07 (`procnet`):** Decodifica `0100007F:1F90` ⇒ `127.0.0.1:8080`, IPv6 `::1`, `::` e IPv4-mapeado; filtra TCP `0A` e UDP vinculado; ignora linhas malformadas sem *panic*; benchmark de 10 000 linhas executa em < 5 ms em `--release`.
- [ ] **CA-08-08 (`poller`):** Com `tokio::time::pause()`, aba `running` varre a cada 1 s; `idle` com descendente ou porta conhecida varre a cada 5 s; `idle` sem descendentes varre a cada 30 s; sem PTY vivo fica `Off`; transição `running → idle` dispara varredura imediata; diff nunca emite `opened` repetido para o mesmo token.
- [ ] **CA-08-09 (`udp` & identidade):** Porta UDP/UDP6 aparece com `PortProtocol::Udp` e `can_open_browser: false`; mesma porta em TCP e UDP ou em dois painéis/PIDs distintos produz `PortId` distintos; socket herdado por dois processos lista ambos os donos com `shared_socket: true`.
- [ ] **CA-08-10 (`killer` & TOCTOU):** Recusa PID fora da árvore (`ports.not_tracked`), recusa PID raiz do shell (`ports.kill_root_refused`), detecta reuso de PID por `starttime` divergente (`ports.owner_gone`), retorna `SocketReleased` sem enviar `SIGKILL` se o socket fechar durante a espera de 3 s, e mata com `SIGKILL` processos que ignoram `SIGTERM`.
- [ ] **CA-08-11 (`opener`):** Gera `http://localhost:PORT` (dual-stack), `http://127.0.0.1:PORT` (IPv4) e `http://[::1]:PORT` (IPv6); recusa UDP e bind exclusivo em IP de LAN com `ports.not_browser_compatible`.
- [ ] **CA-08-12 (`clippy`):** `cargo clippy -p mt-ports --all-targets -- -D warnings` termina com código 0.
