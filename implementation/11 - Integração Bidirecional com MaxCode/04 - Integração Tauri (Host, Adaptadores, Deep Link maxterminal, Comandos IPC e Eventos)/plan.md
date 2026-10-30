# Implementação 04 — Integração Tauri (Host, Adaptadores, Deep Link `maxterminal://`, Comandos IPC e Eventos) (`plan.md`)

> Especificação técnica desta unidade: [`./spec.md`](./spec.md).
> Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md).

---

## 1. Checklist Sequencial de Execução

- [ ] **Passo 1: Configurar Dependências do Binário Tauri e Manifesto Desktop**
  1. Editar `src-tauri/Cargo.toml` adicionando `mt-bridge = { path = "crates/mt-bridge" }`, `tauri-plugin-deep-link = "2"` e habilitando a feature `"deep-link"` em `tauri-plugin-single-instance`.
  2. Editar `src-tauri/tauri.conf.json` adicionando `"deep-link": { "desktop": { "schemes": ["maxterminal"] } }` em `"plugins"`.
  3. Verificar `src-tauri/capabilities/default.json` para assegurar que o webview não possua permissão de registro de scheme arbitrário no SO.
  4. Criar `packaging/linux/maxterminal.desktop` com `MimeType=x-scheme-handler/maxterminal;` e `Exec=maxterminal %u`.

- [ ] **Passo 2: Implementar `events/bridge.rs` e `bridge/adapters.rs`**
  1. Criar `src-tauri/src/events/bridge.rs` com as 6 constantes `EVENT_BRIDGE_*` e helpers de emissão restritos à janela principal (`main`).
  2. Criar `src-tauri/src/bridge/adapters.rs` conectando:
     - `mt_db::sanitizer::sanitize` (Fase 03).
     - `ShellService::error_context(block_id, 80)` e resolução de `project_path` (Fase 05).
     - Repositório de projetos (Fase 06).
     - `WorkspaceManager::open_tab` e `TemplateRepo::list` (Fase 07), garantindo que para `template == None` (URI) somente um template `Shell` neutro sem `env_vars` seja aberto com `initial_command: Some("")`.
     - `QuakeController::show_and_focus` e `ShutdownHooks` (Fase 09).

- [ ] **Passo 3: Implementar `bridge/host.rs`, `bridge/deep_link.rs` e `bridge/mod.rs`**
  1. Criar `src-tauri/src/bridge/host.rs` implementando `BridgeHost` para `TauriBridgeHost`.
   2. Criar `src-tauri/src/bridge/deep_link.rs` com `BridgeActivationHandler`, registrado via `ActivationRouter::register_handler(...).await` da 09 antes de `set_ready`. Consumir a fila existente de 32 envelopes/TTL 30 s; coordenar projetos/workspace/UI prontos (timeout UI 10 s). Manter dedup por `requestId` (5 min), coalescência de foco (2 s), `parse_parts` sem segundo decoding e códigos reais de erro. Nenhum segundo listener `on_open_url`/`get_current` ou reenfileiramento manual de argv.
   3. Criar `src-tauri/src/bridge/mod.rs` inicializando `BridgeConfig::load`, `start_bridge`, `deep_link::init(...).await` e registrando `bridge.shutdown().await` no `ShutdownHooks` da Fase 09. Registrar estado por `app.manage(...)`, sem agregar serviços ao `AppState` da 01.
   4. Implementar `bridge/editor.rs` com `BridgeEditorOpener`, registrar via `EditorLauncher::set_bridge_opener`. Estender `protocol.rs`/`client.rs` conforme spec §3.3.1 (`editor_open_v1`, `maxcode.open_file`, `open_file`). Exigir validação da extensão e capability; fallback explícito da 09 quando indisponível.
   5. Criar `src-tauri/tests/bridge_os_adapters.rs`: cold/warm start entrega única, readiness, repasse `worktree`/`correlationId`, sem segundo percent-decoding, editor autenticado com linha/coluna, capability ausente, timeout/fallback e registro repetido.

- [ ] **Passo 4: Implementar os 9 Comandos IPC em `commands/bridge.rs` e Regenerar Bindings**
  1. Criar `src-tauri/src/commands/bridge.rs` implementando `bridge_status`, `bridge_get_config`, `bridge_set_config`, `bridge_restart`, `bridge_regenerate_token`, `maxcode_open_plan`, `maxcode_open_project`, `maxcode_inspect_block` e `bridge_notify_active_project`.
  2. Registrar todos os 9 comandos no `tauri_specta::Builder` em `src-tauri/src/lib.rs`.
   3. Executar `npm run bindings` para atualizar `src/types/bindings.ts` e verificar que todos os 9 comandos e os tipos `BridgeStatus`, `BridgeConfig`, `BridgeState`, `PeerInfo`, `DeliveryChannel`, `DeliveryReport`, `ProjectFocusRequested` e `SyncContext` foram exportados.
   4. Conferir serialização camelCase de DTOs de estado/entrega/eventos e fixtures TS; configuração `BridgeConfig` conserva seu contrato explícito snake_case. O comando genérico chama `open_plan(..., false)`; Inspecionar propaga `pre_truncated`.

---

## 2. Comandos Exatos de Verificação

```bash
# 1. Validar o arquivo .desktop (CA-16)
desktop-file-validate packaging/linux/maxterminal.desktop

# 2. Compilar e testar todo o workspace Rust incluindo o binário Tauri (CA-02)
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --workspace

# 3. Regenerar e validar os bindings TypeScript via tauri-specta (CA-03)
npm run bindings && git diff --exit-code src/types/bindings.ts
```

---

## 3. Critérios de Aceite Mapeados (`CA-*`)

| Critério | Descrição | Como é verificado nesta Implementação |
|----------|-----------|---------------------------------------|
| **CA-02** | `cargo test --manifest-path src-tauri/Cargo.toml --workspace` passa sem erros. | Compilação e testes de todo o workspace `src-tauri`. |
| **CA-03** | `npm run bindings && git diff --exit-code src/types/bindings.ts` retorna 0 com os 9 comandos e 6 tipos exigidos. | Geração automática via `tauri-specta`. |
| **CA-16** | `desktop-file-validate packaging/linux/maxterminal.desktop` passa e contém `MimeType=x-scheme-handler/maxterminal;`. | Validado pelo comando `desktop-file-validate`. |
| **CA-17** | Com o app fechado, `xdg-open 'maxterminal://project/open?path=...'` inicia o app e foca o projeto. | `ActivationRouter` (09) → `BridgeActivationHandler` após prontidão conjunta. |
| **CA-18** | Com o app aberto/minimizado na bandeja, `xdg-open` não cria 2ª instância, exibe janela e foca projeto. | `single-instance` → listener único/`ActivationRouter` (09) → handler (11) → `show_and_focus`. |
| **CA-19** | `maxterminal://tab/open` abre aba Shell neutra sem comando inicial após validar projeto/`cwd`. | Validado em `adapters::open_workspace_tab` e `deep_link::route_single_uri`. |
| **CA-20** | URIs malformadas geram erro `pt-BR` e log `WARN` sem crash. | Códigos da 09 para estrutura e códigos da 11 para rota/caminho, preservados na conversão. |
| **CA-35** | URI recebida por startup, plugin e single-instance é roteada apenas uma vez após stores prontas. | Validado pelo dedup `requestId` (5 min) / coalescência (2 s) e barreira `mark_ready`. |
| **CA-37** | `maxcode_open_plan` permanece genérico sem dependências das Fases 12–17. | Validado na assinatura e implementação de `maxcode_open_plan`. |
| **CA-40** | Links de arquivo usam `EditorOpener` e setter da 09. | `bridge_os_adapters.rs`: envio autenticado, capability, fallback e registro idempotente. |
