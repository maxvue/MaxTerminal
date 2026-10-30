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
1. Implementar `src-tauri/src/adapters/process_tree.rs` (`PtyProcessTreeAdapter` implementando `ProcessTreeSource` e `CwdFallback` sobre `mt_pty::PtyManager` e o registro da Fase 07).
2. Implementar `src-tauri/src/adapters/exact_process_signaler.rs` (`LinuxPidfdSignaler` implementando `ExactProcessSignaler` com `pidfd_open`, `pidfd_send_signal`, poll de saída no `pidfd` e revalidação de `starttime`, UID, namespace e inode em `/proc/<pid>/fd/*`).
3. Implementar `src-tauri/src/adapters/port_opener.rs` (`TauriPortUrlOpener` implementando `PortUrlOpener` via `tauri_plugin_opener::OpenerExt`).
4. Implementar `src-tauri/src/adapters/indicators.rs` (`IndicatorWorkspaceObserver` integrando os ciclos de vida de abas/painéis da Fase 07, eventos OSC 7 / OSC 133 `D` da Fase 05 e a reserva de `cwd` para mutações de worktree).

### Passo 3 — Emissores de Eventos e Comandos IPC (`events/` e `commands/`)
1. Implementar `src-tauri/src/events/git.rs` (`GitStatusChangedEvent` e `TauriGitEventSink`) e `src-tauri/src/events/ports.rs` (`PortOpenedEvent`, `PortUpdatedEvent`, `PortClosedEvent`, `PortSnapshotEvent` e `TauriPortEventSink`).
2. Implementar `src-tauri/src/commands/git.rs` com os 9 comandos (`git_track_pane`, `git_untrack_pane`, `git_set_visible_tab`, `git_refresh`, `git_status`, `git_worktree_list`, `git_worktree_create`, `git_worktree_remove`, `git_worktree_prune`) e sanitização de `stderr` via `mt_db::sanitizer::sanitize()`.
3. Implementar `src-tauri/src/commands/ports.rs` com os 7 comandos (`port_track_pane`, `port_untrack_pane`, `port_set_tab_status`, `port_list`, `port_refresh`, `port_open_browser`, `port_kill`).

### Passo 4 — Registro no `lib.rs` e Regeneração de `src/types/bindings.ts`
1. Em `src-tauri/src/lib.rs`:
   - Registrar `.plugin(tauri_plugin_opener::init())`.
   - Instanciar `GitService`, `CliWorktreeManager` e `PortsService` no setup do app e registrá-los via `app.manage(...)`.
   - Conectar `IndicatorWorkspaceObserver` ao `WorkspaceManager` (Fase 07) e `ShellRuntime` (Fase 05).
   - Registrar todos os 16 comandos e 5 eventos no `tauri_specta::Builder`.
2. Executar o gerador de bindings (`cargo test` de exportação specta ou binário de bindings configurado na Fase 01) para atualizar `src/types/bindings.ts` e verificar que `GitContext`, `WorktreeEntry`, `ListeningPort`, `KillOutcome`, `PortSnapshot`, etc. foram exportados.

---

## 2. Comandos de Verificação e Testes

Execute os comandos abaixo a partir de `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0/src-tauri`:

```bash
# 1. Compilar todo o workspace Rust sem warnings
cargo build --workspace

# 2. Executar Clippy estrito nos crates da Fase 08 e no crate binário
cargo clippy -p mt-git -p mt-ports -p maxterminal --all-targets -- -D warnings

# 3. Executar testes do crate binário (incluindo exportação de bindings tauri-specta)
cargo test -p maxterminal

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
