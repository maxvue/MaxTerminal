# Plano de Execução — Implementação 05: Integração Tauri IPC, Channel Binário, Eventos Globais e Capabilities

## 1. Checklist Sequencial de Implementação

### Passo 1: Dependências e Eventos Tauri + `BinaryPtyFrame` (`src-tauri/src/events/pty.rs`)
1. Atualizar `src-tauri/Cargo.toml` adicionando `mt-pty = { path = "crates/mt-pty" }` e `bytes` nas dependências do crate binário `maxterminal`.
2. Criar `src-tauri/src/events/mod.rs` e `src-tauri/src/events/pty.rs`:
   - Implementar `BinaryPtyFrame(pub Bytes)` com `IpcResponse` retornando `Ok(InvokeResponseBody::Raw(self.0.to_vec()))`. Antes de expandir a integração, executar a parte Rust/exportador do **gate T-06** (`spec.md` §3.1): inspecionar os fontes locais fixados, implementar o mecanismo Specta comprovado e compilar/exportar a prova mínima com `pty_attach` no builder único. O marcador opaco é proposta pendente, não código compilável fornecido pela spec. O TS recebe `ArrayBuffer` pelo tipo manual `PtyChannelMessage` — exceção única à regra de bindings.
   - Implementar `ChannelSink` adaptando `tauri::ipc::Channel<BinaryPtyFrame>` para `mt_pty::OutputSink`.
   - Implementar `TauriEventListener` emitindo `pty://spawned`, `pty://foreground-changed`, `pty://exit` e `pty://cleanup-failed`.

### Passo 2: Implementação dos 9 Comandos IPC (`src-tauri/src/commands/pty.rs`)
1. Criar `src-tauri/src/commands/pty.rs` com os nove comandos anotados com `#[tauri::command]` e `#[specta::specta]`:
   - `pty_spawn`, `pty_write`, `pty_resize`, `pty_kill`, `pty_list`, `pty_process_info`, `pty_ack`, `pty_attach` e `pty_detach`.
2. Garantir que `pty_spawn`, `pty_resize` e `pty_process_info` utilizem `tauri::async_runtime::spawn_blocking` e que `pty_attach` receba `on_output: Channel<BinaryPtyFrame>` como argumento direto (CA-35).
3. Expor `pub mod pty;` em `src-tauri/src/commands/mod.rs`.

### Passo 3: Registro no Builder, Ciclo de Vida de Saída e Capabilities (`lib.rs`, `build.rs`, `default.json`)
1. Atualizar `src-tauri/src/lib.rs`:
   - Incluir os 9 comandos `commands::pty::pty_*` no `collect_commands![]` do builder único `tauri_specta`.
   - Registrar os tipos de eventos (`PtySpawnedEvent`, `PtyForegroundChangedEvent`, `PtyExitEvent`, `PtyCleanupFailedEvent`).
   - No `setup` do Tauri, inicializar `Arc<PtyManager>`, instalar guards (`install_guards()`), registrar `TauriEventListener` e injetar o estado com `app.manage(manager)`.
   - Configurar o handler de execução do Tauri (`Builder::build(...).run(...)`) para tratar `WindowEvent::CloseRequested` (sem matar PTYs) e `RunEvent::ExitRequested` / `RunEvent::Exit` (coordenando `manager.shutdown(ExitReason::Shutdown)` em background antes de chamar `app.exit(0)` — CA-24).
2. Atualizar `src-tauri/build.rs` e `src-tauri/capabilities/default.json` para declarar e conceder as permissões dos nove comandos `pty_*` exclusivamente à janela `main`.

### Passo 4: Regeneração de `src/types/bindings.ts` e Testes de Integração Tauri
1. Executar `cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings` para atualizar `src/types/bindings.ts` e verificar a presença de `PtySummary`, `SpawnSpec`, `PtyExitEvent`, `ProcessInfo` e dos 9 comandos (CA-03).
2. Adicionar testes de integração Tauri em `src-tauri/src/` usando `tauri::test::mock_builder()`:
   - Exercitar os 9 comandos `pty_*` via IPC simulado (CA-29).
   - Validar que erros de domínio retornam `{ status: "error", error: { code: "pty.*", message: "..." } }` com mensagens em `pt-BR`.
   - Validar que `IpcResponse::body()` de `BinaryPtyFrame` retorna `InvokeResponseBody::Raw`; `ChannelSink::send` retorna `Result<(), SinkClosed>`, não o corpo. O envio bem-sucedido no `Channel` **não** libera crédito no `FlowControl` até que `pty_ack` seja invocado (CA-35).
   - Validar o comportamento de `CloseRequested` vs `ExitRequested` (CA-24).
3. Registrar a evidência Rust/exportador do gate T-06 antes de iniciar `02/06`. Completar o aceite conjunto com os testes TS e de WebView real descritos em `spec.md` §3.1; manter CA-35 pendente enquanto essa evidência não existir. Falha no gate bloqueia a integração, sem fallback JSON ou edição manual dos bindings.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Regenerar os bindings TypeScript e confirmar determinismo
cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings
git diff --exit-code src/types/bindings.ts

# 2. Verificar formatação e clippy em todo o workspace Cargo
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# 3. Executar os testes do crate binário maxterminal (incluindo mock_builder IPC)
cargo test --manifest-path src-tauri/Cargo.toml -p maxterminal
```

---

## 3. Critérios de Aceite da Implementação 05

| Critério da Fase 02 | Descrição da Validação nesta Implementação |
|---------------------|--------------------------------------------|
| **CA-01** | `cargo fmt --check` e `cargo clippy --workspace --all-targets -- -D warnings` passam sem avisos no crate binário e em `mt-pty`. |
| **CA-03 (bindings)** | `src/types/bindings.ts` é gerado sem edição manual contendo `PtySummary`, `SpawnSpec`, `PtyExitEvent`, `ProcessInfo` e os 9 comandos `pty_*`. |
| **CA-24** | `CloseRequested` preserva os PTYs vivos; `ExitRequested` bloqueia novos spawns, executa `shutdown` fora do event loop e só então encerra o app. |
| **CA-29** | Teste de integração Tauri com `tauri::test::mock_builder` cobre os 9 comandos e valida a serialização de `MtError` com mensagens em `pt-BR`. |
| **CA-35 (Rust)** | Gate T-06: `pty_attach` recebe `Channel<BinaryPtyFrame>` direto, com representação Specta comprovada no exportador (não se exige `ArrayBuffer` gerado); preserva `Result<AttachInfo, MtError>`, corpo Raw e `send(Ok)` sem `pty_ack` não reduz `unacked`. Aceite conjunto depende da prova de WebView/TS em `02/06`. |
