# Plano de Execução — Implementação 04: Integração no Binário Tauri, Comandos IPC, Eventos Revisionados e Wrappers TypeScript

> **Fase:** 05 — Shell Integration OSC 133 e Máquina de Estados das Abas
> **Implementação:** `04 - Comandos IPC Tauri, Eventos Revisionados e Wrappers TypeScript`
> **Especificação de referência:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1: Implementar o Adaptador de Eventos Tauri (`src-tauri/src/events/shell.rs`)**
  1. Criar `src-tauri/src/events/shell.rs` com as structs `TabStateChangedEvent`, `TabStateRemovedEvent`, `ShellEventEnvelope<T>`, `ShellCommandStartedEvent`, `ShellCommandFinishedEvent`, `ShellCwdChangedEvent` e `ShellIntegrationModeEvent`, anotadas com `#[derive(Serialize, Deserialize, specta::Type, tauri_specta::Event)]`.
  2. Implementar `TauriShellEventSink` para a trait `mt_shell::ShellEventSink`, incluindo o debounce por `PaneId` de `100 ms` para `shell://cwd-changed` e emissão imediata (sem aguardar SQLite) para `tab://state-changed`.
  3. Exportar `pub mod shell;` em `src-tauri/src/events/mod.rs`.

- [ ] **Passo 2: Implementar os Comandos Tauri (`commands/tab_state.rs`, `commands/shell.rs` e extensão de `commands/pty.rs`)**
  1. Criar `src-tauri/src/commands/tab_state.rs` com `tab_state_list`, `tab_state_get`, `tab_mark_seen`, `tab_set_focus` e `tab_set_continuous`, validando a janela chamadora via `WebviewWindow`.
  2. Criar `src-tauri/src/commands/shell.rs` com `shell_integration_info`, `command_block_list`, `command_block_get`, `command_block_error_context` e `command_block_search`, validando faixas de `limit` (`1..=500` em `list`, `1..=100` em `search`) e `max_lines` (`1..=200`, default `50` em `error_context`).
  3. Atualizar `src-tauri/src/commands/pty.rs` adicionando `integration: Option<ShellIntegrationRequest>` ao `pty_spawn`, preservando compatibilidade com chamadas sem `integration`.
  4. Exportar `pub mod shell;` e `pub mod tab_state;` em `src-tauri/src/commands/mod.rs`.

- [ ] **Passo 3: Compor `ShellService` em `src-tauri/src/lib.rs`, Atualizar Capabilities e Regenerar Bindings Specta**
  1. Em `src-tauri/src/lib.rs`:
     - Incluir `mt_db::repos::shell_fts_spec()` no `DbSetup.fts` antes de `Db::open`.
     - Executar `close_dangling` na abertura do banco antes de liberar spawns.
     - Instanciar `Arc<ShellService>`, registrar seu `OutputTap` no `PtyManager` (guardando o `TapHandle`) e seu listener de saída de PTY, e gerenciá-lo com `app.manage(shell_service)`.
     - Registrar todos os 10 novos comandos e 6 novos eventos no `tauri_specta::Builder`.
     - Integrar `shell_service.shutdown().await` no fluxo de `ExitRequested` antes de `db.shutdown().await`.
  2. Verificar e atualizar `src-tauri/capabilities/default.json` caso haja allowlist explícita de comandos/eventos.
  3. Executar o gerador/teste de exportação Specta para atualizar `src/types/bindings.ts` sem editar `bindings.ts` manualmente.

- [ ] **Passo 4: Criar os Wrappers TypeScript (`src/ipc/tabState.ts`, `src/ipc/shell.ts` e ajuste em `src/ipc/pty.ts`)**
  1. Atualizar `src/ipc/pty.ts` e `src/terminal/stream/ptyClient.ts` para aceitar `integration?: ShellIntegrationRequest | null` opcional em `ptySpawn` / `spawn`.
  2. Criar `src/ipc/tabState.ts` implementando `tabStateList`, `tabStateGet`, `tabMarkSeen`, `tabSetFocus`, `tabSetContinuous`, `onTabStateChanged` e `onTabStateRemoved` usando `unwrap` de `src/ipc/core.ts`.
  3. Criar `src/ipc/shell.ts` implementando `shellIntegrationInfo`, `commandBlockList`, `commandBlockGet`, `commandBlockErrorContext`, `commandBlockSearch`, `onCommandStarted`, `onCommandFinished`, `onCwdChanged` e `onIntegrationModeChanged`.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Compilar todo o workspace Rust (incluindo o binário maxterminal)
cargo check --workspace --all-targets

# 2. Executar os testes do workspace (incluindo exportação/verificação de bindings Specta)
cargo test --workspace

# 3. Validar clippy em todo o workspace sem warnings
cargo clippy --workspace --all-targets -- -D warnings

# 4. Validar tipagem estrita TypeScript dos novos wrappers IPC e bindings
npm run check-types
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 05)

- [x] **CA-IPC-01 (spec.md §§2.1 e 8.1):** `pty_spawn` aceita `integration: Option<ShellIntegrationRequest>` opcional sem quebrar chamadas legadas da Fase 02/04 e mantém `onOutput` exclusivamente em `pty_attach`.
- [x] **CA-IPC-02 (spec.md §8.1):** Todos os 10 comandos de `tab_state` e `shell` retornam `Result<T, MtError>` com códigos `shell.*`/`db.*` e mensagens em `pt-BR` sem vazar SQL, comandos crus ou nonces.
- [x] **CA-IPC-03 (spec.md §§8.2 e 9):** `src/types/bindings.ts` gerado via Specta com `u64` serializado como `string` decimal; nenhum DTO Rust duplicado manualmente em TypeScript.
- [x] **CA-PERF-02 (instructions.md §5):** `tab://state-changed` é emitido imediatamente após a transição runtime (p95 ≤ 50 ms após `D`), sem aguardar o commit no SQLite.
