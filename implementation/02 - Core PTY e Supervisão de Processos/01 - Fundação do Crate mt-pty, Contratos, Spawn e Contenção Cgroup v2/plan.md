# Plano de Execução — Implementação 01: Fundação do Crate `mt-pty`, Contratos, Spawn e Contenção Cgroup v2

## 1. Checklist Sequencial de Implementação

### Passo 1: Configuração do Workspace Cargo e Manifesto do `mt-pty`
1. Editar `src-tauri/Cargo.toml` para incluir `"crates/mt-pty"` em `[workspace.members]` e registrar as dependências da Fase 02 (`portable-pty`, `rustix`, `nix`, `procfs`, `crossbeam-channel`, `parking_lot`, `arc-swap`, `bytes`, `signal-hook`, `tempfile`, `criterion`, `proptest`, `assert_matches`).
2. Criar `src-tauri/crates/mt-pty/Cargo.toml` herdando `[lints] workspace = true` (com `unsafe_code = "forbid"`) e declarando o binário `[[bin]] name = "mt-pty-helper"`.

### Passo 2: Configuração, Erros e Tipos de Domínio (`config.rs`, `error.rs`, `types.rs`)
1. Implementar `src-tauri/crates/mt-pty/src/config.rs` com `PtyConfig` e `Default` exato conforme especificação (marcas d'água de 1 MiB / 256 KiB, janela de 4 ms, replay de 1 MiB, graces de 1 s / 2 s, stack de 256 KiB e `registry_dir` em `$XDG_RUNTIME_DIR/maxterminal/<instance_id>`).
2. Implementar `src-tauri/crates/mt-pty/src/error.rs` com `PtyError`, método `code(&self) -> &'static str` cobrindo todos os códigos `pty.*` e `From<PtyError> for mt_core::MtError` com mensagens em `pt-BR`.
3. Implementar `src-tauri/crates/mt-pty/src/types.rs` com `U64Decimal`, `AttachmentId`, `PtySize` (incluindo `validate()`), `PtyOwner`, `SpawnSpec`, `ExitReason`, `ExitInfo`, `PtyStatus`, `KillMode`, `ProcessIdentity`, `PtyPids`, `PtySummary`, `ProcState`, `ProcessInfo`, `ProcessNode`, `PtyProcessReport`, `OutputCursor`, `AttachInfo`, `ShutdownReport`, `TrackedProcess` e `PtyInput`.
4. Escrever testes unitários em `types.rs` e `error.rs` verificando a serialização JSON de `U64Decimal` como string decimal, validação de `PtySize` (`0` ou `> 1000` falham com `pty.resize_invalid`) e mapeamento de `PtyError` para `MtError`.

### Passo 3: Traits e Pontos de Extensão (`sink.rs`, `events.rs`, `tap.rs`)
1. Implementar `src-tauri/crates/mt-pty/src/sink.rs` com `OutputSink`, `SinkClosed`, `NullSink` e `VecSink`.
2. Implementar `src-tauri/crates/mt-pty/src/events.rs` com `PtyEvent`, structs exportadas (`PtySpawnedEvent`, `PtyForegroundChangedEvent`, `PtyExitEvent`, `PtyCleanupFailedEvent`), trait `PtyEventListener` e `SubscriberHub` (canais `crossbeam_channel::bounded` por assinante para fan-out sem bloqueio).
3. Implementar `src-tauri/crates/mt-pty/src/tap.rs` com `OutputTap`, `TapRegistry` e `TapHandle`, garantindo snapshot lock-free durante `dispatch_output` e espera sincronizada de callbacks em andamento no `drop` do `TapHandle` (com proteção contra auto-remoção dentro do próprio callback).
4. Adicionar testes unitários em `tap.rs` validando registro, despacho de bytes, remoção concorrente e ausência de chamadas após `drop(handle)`.

### Passo 4: Resolução de Shell, Política de Ambiente e Validação de `cwd` (`spawn/`)
1. Implementar `src-tauri/crates/mt-pty/src/spawn/shell.rs` com `resolve_shell`, respeitando a precedência `spec.shell` → `$SHELL` → `getpwuid` → `/bin/bash` → `/bin/sh`, validação de executável regular e injeção de `-l` / `--login` apenas para shells compatíveis.
2. Implementar `src-tauri/crates/mt-pty/src/spawn/env.rs` com `EnvPolicy`, `DefaultEnvPolicy`, validação estrita de chaves (`^[A-Za-z_][A-Za-z0-9_]*$`, sem `\0`) e valores (sem `\0`, `<= 128 KiB`), precedência de variáveis e variáveis forçadas (`TERM`, `COLORTERM`, `TERM_PROGRAM`, `TERM_PROGRAM_VERSION`, `MAXTERMINAL`, `MAXTERMINAL_PTY_ID`, fallback `LANG=C.UTF-8`).
3. Implementar `src-tauri/crates/mt-pty/src/spawn/mod.rs` com `resolve_cwd` (expansão de `~`, `canonicalize`, verificação de diretório e acesso) e struct `PreparedSpawn`.
4. Adicionar testes unitários exaustivos em `spawn/shell.rs`, `spawn/env.rs` e `spawn/mod.rs` cobrindo todos os casos de erro (`pty.cwd_invalid`, `pty.shell_not_found`, `pty.env_invalid`).

### Passo 5: Contenção Cgroup v2, Launcher, Watchdog e Binário `mt-pty-helper` (`containment/` e `bin/`)
1. Implementar `src-tauri/crates/mt-pty/src/containment/mod.rs` definindo `ContainmentProvider`, `ContainmentHandle` e as mensagens de protocolo entre o processo principal, o launcher e o watchdog.
2. Implementar `src-tauri/crates/mt-pty/src/containment/cgroup.rs` com `SystemdCgroupProvider` (contenção real via `/sys/fs/cgroup` com verificação de `cgroup.procs`, `cgroup.kill`, `cgroup.events` e `pidfd_open`) e `FakeContainmentProvider` para testes unitários isolados. Nunca mover o app nem o watchdog para cgroup (alinhado a `02/spec.md` §7.6): somente os filhos `<subtree>/ptys/pty-<id>` em escopo delegado dedicado às cargas; sem escopo utilizável o driver degrada para `CgroupDriver::Disabled` (spawn falha com `pty.containment_unavailable`, sem fallback silencioso).
3. Implementar `src-tauri/crates/mt-pty/src/containment/launcher.rs` para criação do socket Unix temporário com permissão `0600` em diretório `0700`, aceitação da conexão do helper, verificação de `SO_PEERCRED` + `start_time`, inscrição do PID no cgroup antes da autorização e envio de `LauncherReleaseMsg`.
4. Implementar `src-tauri/crates/mt-pty/src/containment/watchdog.rs` para inicialização do processo `mt-pty-helper --mode watchdog`, registro/remoção de cgroups e sinalização de emergência via FD pré-aberto.
5. Implementar `src-tauri/crates/mt-pty/src/bin/mt-pty-helper.rs` suportando `--mode launcher` e `--mode watchdog`.
6. Expor os módulos em `src-tauri/crates/mt-pty/src/lib.rs` e escrever testes de integração do launcher/watchdog verificando:
   - Timeout/cancelamento do launcher antes da autorização encerra o helper sem executar o shell (CA-32).
   - Execução autorizada substitui o processo via `execve` mantendo o mesmo PID já contido no cgroup e com as variáveis de ambiente e `cwd` esperados (CA-06).
   - Fechamento abrupto (EOF) do canal de controle faz o watchdog acionar `cgroup.kill` imediatamente nos cgroups registrados.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificar formatação e lint estrito no workspace Cargo
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# 2. Compilar o crate mt-pty e o binário auxiliar mt-pty-helper
cargo build --manifest-path src-tauri/Cargo.toml -p mt-pty --bins

# 3. Executar todos os testes unitários e de contenção da Implementação 01
cargo test --manifest-path src-tauri/Cargo.toml -p mt-pty
```

---

## 3. Critérios de Aceite da Implementação 01

| Critério da Fase 02 | Descrição da Validação nesta Implementação |
|---------------------|--------------------------------------------|
| **CA-01** | `cargo fmt --check` e `cargo clippy --workspace --all-targets -- -D warnings` passam sem avisos. |
| **CA-05** | Testes comprovam que `cwd` inexistente retorna `pty.cwd_invalid`, shell inexistente retorna `pty.shell_not_found`, e chave de env inválida (`"A=B"`, vazia ou com `\0`) retorna `pty.env_invalid`. |
| **CA-06** | O processo lançado via `mt-pty-helper --mode launcher` enxerga `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=MaxTerminal`, `MAXTERMINAL_PTY_ID=<id>` e o `cwd` canonicalizado. |
| **CA-32** | Sem delegação de cgroup v2 / `cgroup.kill` / pidfd, a inicialização de contenção falha com `pty.containment_unavailable` antes de criar ou executar o shell; se o handshake do launcher sofrer timeout ou cancelamento, o shell jamais é executado. |
| **CA-33 (base)** | `TapRegistry` registra e remove `TapHandle` sem permitir chamadas a `on_output` após a conclusão do `drop`. |
