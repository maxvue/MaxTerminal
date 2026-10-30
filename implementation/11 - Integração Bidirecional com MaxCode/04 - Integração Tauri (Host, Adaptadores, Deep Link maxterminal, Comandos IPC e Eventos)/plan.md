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
  2. Criar `src-tauri/src/bridge/deep_link.rs` com `DeepLinkRouter`, barreira `projects_ready` (timeout de 10 s), fila de 32 itens, deduplicação por `requestId` (TTL 5 min) e coalescência de 2 s em `project/open`, tratando erros de URI malformada/projeto não encontrado com notificação/toast em `pt-BR` e log `WARN` sem vazar a URI bruta.
  3. Criar `src-tauri/src/bridge/mod.rs` inicializando `BridgeConfig::load`, `start_bridge`, `deep_link::init` e registrando `bridge.shutdown().await` no `ShutdownHooks` da Fase 09.

- [ ] **Passo 4: Implementar os 9 Comandos IPC em `commands/bridge.rs` e Regenerar Bindings**
  1. Criar `src-tauri/src/commands/bridge.rs` implementando `bridge_status`, `bridge_get_config`, `bridge_set_config`, `bridge_restart`, `bridge_regenerate_token`, `maxcode_open_plan`, `maxcode_open_project`, `maxcode_inspect_block` e `bridge_notify_active_project`.
  2. Registrar todos os 9 comandos no `tauri_specta::Builder` em `src-tauri/src/lib.rs`.
  3. Executar `npm run bindings` para atualizar `src/types/bindings.ts` e verificar que todos os 9 comandos e os tipos `BridgeStatus`, `BridgeConfig`, `BridgeState`, `PeerInfo`, `DeliveryChannel`, `DeliveryReport`, `ProjectFocusRequested` e `SyncContext` foram exportados.

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
| **CA-17** | Com o app fechado, `xdg-open 'maxterminal://project/open?path=...'` inicia o app e foca o projeto. | Validado por `deep_link::init` (`get_current()` + barreira `projects_ready`). |
| **CA-18** | Com o app aberto/minimizado na bandeja, `xdg-open` não cria 2ª instância, exibe janela e foca projeto. | Validado pela integração `single-instance` (Fase 09) → `DeepLinkRouter::enqueue` → `show_and_focus`. |
| **CA-19** | `maxterminal://tab/open` abre aba Shell neutra sem comando inicial após validar projeto/`cwd`. | Validado em `adapters::open_workspace_tab` e `deep_link::route_single_uri`. |
| **CA-20** | URIs malformadas geram erro `pt-BR` e log `WARN` sem crash. | Validado no tratador de erros de `DeepLinkRouter`. |
| **CA-35** | URI recebida por startup, plugin e single-instance é roteada apenas uma vez após stores prontas. | Validado pelo dedup `requestId` (5 min) / coalescência (2 s) e barreira `mark_ready`. |
| **CA-37** | `maxcode_open_plan` permanece genérico sem dependências das Fases 12–17. | Validado na assinatura e implementação de `maxcode_open_plan`. |
