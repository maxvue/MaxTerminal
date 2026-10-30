# Plano de Execução — Implementação 03: Adaptadores Binários, Comandos IPC, Eventos e Bindings Tauri

> **Fase:** 08 — Indicadores Contextuais: Git, Worktree e Portas Ativas
> **Unidade:** `03 - Adaptadores Binarios Comandos IPC Eventos e Bindings Tauri`
> **Especificação:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1 — Dependências do Crate Binário e Plugin Opener
1. Em `src-tauri/Cargo.toml`, adicionar `mt-git = { workspace = true }`, `mt-ports = { workspace = true }` e `tauri-plugin-opener = "2"` nas dependências do pacote `maxterminal`.
2. Atualizar `src-tauri/capabilities/default.json` (ou `main.json`) garantindo que os comandos customizados da Fase 08 sejam permitidos exclusivamente na janela `"main"`, sem adicionar `"opener:default"` para llamadas arbitrárias vindas do frontend.

### Passo 2 — Adaptadores de Processos, `pidfd` e Opener (`src-tauri/src/adapters/`)
1. Implementar `src-tauri/src/adapters/process_tree.rs` (`PtyProcessTreeAdapter` implementando `ProcessTreeSource` e `CwdFallback`), injetando `Arc<mt_pty::PtyManager>` e `Arc<WorkspaceManager>`. `verify_pane_ownership` é assíncrono, consulta `workspace.pane_pty(tab_id, pane_id).await` e valida o owner vivo antes de aceitar o PTY recebido.
2. Implementar `src-tauri/src/adapters/exact_process_signaler.rs` (`LinuxPidfdSignaler` implementando `ExactProcessSignaler` com `pidfd_open`, `pidfd_send_signal`, poll de saída no `pidfd` e revalidação de `starttime`, UID, namespace e inode em `/proc/<pid>/fd/*`).
3. Implementar `src-tauri/src/adapters/port_opener.rs` (`TauriPortUrlOpener` implementando `PortUrlOpener` via `tauri_plugin_opener::OpenerExt`).
4. Implementar `src-tauri/src/adapters/indicators.rs`: `GitWorkspaceSpawnHook: WorkspaceSpawnHook` e `GitWorkspaceSpawnGuard` adquirem `reserve_cwd(...).await?`, confirmam `track_pane` em `register` e desfazem somente a execução correspondente em `rollback` (`spec.md` §3.6). O guard RAII libera o token apenas após registro/compensação; futures são `Send`, sem empréstimo de `&self` no reserve nem reentrada em `WorkspaceManager`.
5. Implementar `IndicatorWorkspaceObserver: WorkspaceLifecycleObserver`, consumindo sinais internos da Fase 07 e eventos OSC 7 / OSC 133 `D` da Fase 05. Validar geração/ordem; `PaneSpawned` reconcilia idempotentemente o binding confirmado pelo hook, sem remover a proteção existente.

### Passo 3 — Emissores de Eventos e Comandos IPC (`events/` e `commands/`)
1. Implementar `src-tauri/src/events/git.rs` (`GitStatusChangedEvent` e `TauriGitEventSink`) e `src-tauri/src/events/ports.rs` (`PortOpenedEvent`, `PortUpdatedEvent`, `PortClosedEvent`, `PortSnapshotEvent` e `TauriPortEventSink`).
2. Implementar `src-tauri/src/commands/git.rs` com os 9 comandos (`git_track_pane`, `git_untrack_pane`, `git_set_visible_tab`, `git_refresh`, `git_status`, `git_worktree_list`, `git_worktree_create`, `git_worktree_remove`, `git_worktree_prune`) e sanitização de `stderr` via `mt_db::sanitizer::sanitize()`.
3. Implementar `src-tauri/src/commands/ports.rs` com os 7 comandos (`port_track_pane`, `port_untrack_pane`, `port_set_tab_status`, `port_list`, `port_refresh`, `port_open_browser`, `port_kill`).
4. Injetar `State<'_, Arc<WorkspaceManager>>` em `git_track_pane`: resolver `pty_id` com `pane_pty(...).await` e repassá-lo a `CwdFallback`/`GitService::track_pane`. `port_track_pane` deve aguardar `verify_pane_ownership(...).await?`. Testar PTY vivo, painel sem PTY, troca de geração e tentativa de registrar PTY de outra aba.

### Passo 4 — Registro no `lib.rs` e Regeneração de `src/types/bindings.ts`
1. Em `src-tauri/src/lib.rs`:
   - Registrar `.plugin(tauri_plugin_opener::init())`.
   - Instanciar `GitService`, `CliWorktreeManager` e `PortsService` no setup do app e registrá-los via `app.manage(...)`.
   - Antes de compartilhar o `WorkspaceManager`/disparar restore ou aceitar comandos, criar `GitWorkspaceSpawnHook` com o mesmo `Arc<GitService>` e instalar pelo builder `with_spawn_hooks`. A lista é imutável após composição; a Fase 07 isolada usa lista vazia.
   - Conectar `IndicatorWorkspaceObserver` via `WorkspaceManager::register_observer` (Fase 07) e `ShellRuntime` (Fase 05), compartilhando o mesmo `Arc<WorkspaceManager>` com comandos/adaptadores e registrando `Arc<dyn CwdFallback>` com `app.manage(...)`.
   - Registrar todos os 16 comandos e 5 eventos no `tauri_specta::Builder`.
2. Na raiz da worktree ativa, executar `cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings` (gerador da Fase 01) para atualizar `src/types/bindings.ts` e verificar que `GitContext`, `WorktreeEntry`, `ListeningPort`, `KillOutcome`, `PortSnapshot`, etc. foram exportados.

### Passo 5 — Testes do Hook e Abertura × Remoção
1. Com PTY/runner controlados e barreiras, suspender a abertura após reserva e antes do spawn: `remove(force=true)` retorna `git.worktree_in_use`. Suspender após registro e atrasar o observador: remover continua recusado depois do Drop do token, pelo binding ativo.
2. Fazer remoção vencer e suspender seu subprocesso: a reserva aguarda o gate; após reap, revalida caminho e falha, sem executar spawn. Repetir em subdiretório/alias e repositório aninhado.
3. Injetar falha de spawn, registro parcial, falha de hook posterior e cancelamento do caller: verificar rollback somente após parada, nenhum token órfão após compensação e nenhum binding removido de outra geração. Falha de parada mantém reserva em `Blocked`; retry bem-sucedido a libera.
4. Cobrir `tab_open`, split, reinício, nova sessão e restore com hook instalado antes do boot; testar futures em `tokio::spawn` (`Send`) e nenhum callback que reentre na mailbox. Incluir teste de composição provando que `GitService` do hook é o mesmo usado por `CliWorktreeManager`.

---

## 2. Comandos de Verificação e Testes

Execute os comandos abaixo na worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)"
cd src-tauri

# 1. Compilar todo o workspace Rust sem warnings
cargo build --workspace

# 2. Executar Clippy estrito nos crates da Fase 08 e no crate binário
cargo clippy -p mt-git -p mt-ports -p maxterminal --all-targets -- -D warnings

# 3. Executar testes do crate binário e gerar bindings tauri-specta
cargo test -p maxterminal
cargo run --bin export_bindings

# 4. Verificar que src/types/bindings.ts contém os novos contratos gerados
rg -n "GitContext|WorktreeEntry|ListeningPort|KillOutcome|gitTrackPane|portKill" ../src/types/bindings.ts
```

---

## 3. Critérios de Aceite da Implementação 03 (Mapeados para a Fase 08)

- [ ] **CA-08-13 (Workspace Build & Clippy):** `cargo build --workspace` e `cargo clippy -p mt-git -p mt-ports -p maxterminal -- -D warnings` passam sem warnings.
- [ ] **CA-08-14 (`pidfd` & Autoridade Rust):** `LinuxPidfdSignaler` usa `pidfd_open`/`pidfd_send_signal` sem fallback para `kill(pid)` cru nem `killpg`; `port_track_pane` valida a posse real do `(tabId, paneId, ptyId)` nos registros das Fases 02 e 07.
- [ ] **CA-08-15 (Plugin Opener Autônomo):** `tauri-plugin-opener` v2 está registrado e funcional na Fase 08 apenas com as Fases 01–08 (sem depender da Fase 09), restrito à janela `main`.
- [ ] **CA-08-16 (Sanitização IPC):** Qualquer `stderr` retornado por comandos Git passa por `mt_db::sanitizer::sanitize()` e é truncado a 2 KiB antes de compor `MtError.details`.
- [ ] **CA-08-17 (Bindings `tauri-specta`):** `src/types/bindings.ts` contém todos os DTOs, comandos e eventos da Fase 08 gerados automaticamente, sem duplicação manual em `src/`.
- [ ] **Contrato 07 ↔ 08:** `git_track_pane` preserva o `PtyId` real, `port_track_pane` rejeita ownership divergente e sinais antigos não alteram a geração atual. O hook pré-spawn de `spec.md` §3.6 está instalado antes do boot e os testes de ambos os vencedores da corrida abertura × remoção, registro antes de Drop e compensação/cancelamento/`Blocked` passam.
