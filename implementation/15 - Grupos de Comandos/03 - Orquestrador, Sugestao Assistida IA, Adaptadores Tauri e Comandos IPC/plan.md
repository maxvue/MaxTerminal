# Implementação 03 — Plano de Execução Passo a Passo (`plan.md`)

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1: Implementar `GroupSuggester` (`suggest.rs`)**
  1. Criar `src-tauri/crates/mt-commands/src/groups/suggest.rs` com `GroupSuggestRequest`, `GroupSuggestResponse`, `RecurringCommandsResponse` e `GroupSuggester`.
  2. Sanitizar `goal`, `draft` e comandos recorrentes com `mt_db::sanitizer::sanitize()` antes de construir o `SuggestPromptContext`.
  3. Invocar `GroupAiPort::generate_group_suggestion` com o `request_id` (registrado na Fase 12 para suportar cancelamento via `ai_cancel`), sobrescrever `scope` e `project_id` na resposta com os valores solicitados pelo editor e validar o rascunho retornado usando `validate_group_input`.
  4. Implementar `list_recurring` consultando `RecurringCommandsSource` (quando presente e `is_available() == true`) e retornando `{ available: false, items: vec![] }` quando ausente.

- [ ] **Passo 2: Implementar `CommandGroupService` (`service.rs`)**
  1. Criar `src-tauri/crates/mt-commands/src/groups/service.rs` integrando `CommandGroupRepo`, `SequentialRunner`, `ParallelRunner`, `MultiTabRunner`, `GroupSuggester` e `GroupEventSink`.
  2. Implementar os métodos de CRUD (`list`, `get`, `create`, `update`, `delete`, `reorder`), garantindo que `events.emit_group_changed(...)` seja chamado **somente após** o commit da transação SQLite.
  3. Implementar `run`:
     - Checagem de idempotência por `request_id` e `compute_request_hash(&request)` sob `accept_mutex` antes de checar a revisão do grupo.
     - Preflight completo por modo (`Sequential`, `Parallel`, `MultiTabs`), resolução de `cwd`/`env`/shell/templates, persistência do snapshot + itens `pending`, emissão de `command-group://run-started` e disparo assíncrono do runner sem aguardar o fim dos comandos.
  4. Implementar `cancel_run`, `list_runs` (limitado entre 1 e 100), `get_run`, `attach_item_output`, `detach_item_output`, `recover_interrupted`, `on_project_deleting` e `shutdown`.

- [ ] **Passo 3: Implementar Emissor de Eventos e Observador de Channel (`src-tauri/src/events/command_groups.rs`)**
  1. Criar `src-tauri/src/events/command_groups.rs` implementando `TauriGroupEventSink` sobre `tauri::AppHandle` (emitindo `command-group://changed`, `command-group://run-started`, `command-group://run-item-changed`, `command-group://run-finished`).
  2. Implementar `ChannelRunObserver` encapsulando `tauri::ipc::Channel<GroupOutputFrame>` e implementando `OutputFrameSubscriber`.

- [ ] **Passo 4: Implementar Adaptadores de Infraestrutura (`src-tauri/src/command_groups/adapters.rs` e `mod.rs`)**
  1. Criar `src-tauri/src/command_groups/adapters.rs` conectando:
     - `TauriGroupPtyAdapter` → `mt_pty::PtyManager` (usando `PtyOwner { tab_id: None, pane_id: None }` nos spawns headless).
     - `TauriGroupShellAdapter` → rastreador OSC 133 da Fase 05 e coordenador de leases de painel, além da detecção `env --version` / suporte a `env -C`.
     - `TauriGroupTabAdapter` → `WorkspaceManager` da Fase 07 e serviço de projetos da Fase 06.
     - `TauriGroupAiAdapter` → `AiService::generate_structured` da Fase 12 (`schema_id = "command-group.suggestion.v1"`); seguir §3.4 da spec para request, cancelamento e decode estrito.
     - `TauriRecurringCommandsAdapter` → repositório de memória contextual da Fase 13 (ou fallback inativo se não habilitado).
  2. Criar `src-tauri/src/command_groups/mod.rs` expondo `AppCommandGroupState { pub service: Arc<CommandGroupService> }`.

- [ ] **Passo 5: Registrar Comandos Tauri, Capabilities e Bindings (`src-tauri/src/commands/command_groups.rs`, `lib.rs`, `default.json`)**
  1. Criar `src-tauri/src/commands/command_groups.rs` com os 14 comandos anotados com `#[tauri::command]` e `#[specta::specta]`.
  2. Registrar os 14 comandos e os eventos no builder `tauri-specta` em `src-tauri/src/lib.rs`, inicializar `AppCommandGroupState`, invocar `recover_interrupted()` no boot e conectar `service.shutdown().await` ao hook de saída explícita do aplicativo e `service.on_project_deleting(&project_id).await` ao fluxo de remoção de projeto.
  3. Atualizar `src-tauri/capabilities/default.json` liberando os 14 comandos para a janela `main`.
  4. Regenerar `src/types/bindings.ts` e verificar que todos os tipos da Fase 15 foram exportados sem warnings.

- [ ] **Passo 6: Criar Testes de Integração do Serviço (`groups_service.rs`)**
  1. Criar `src-tauri/crates/mt-commands/tests/groups_service.rs` testando:
     - `CA-09`: inicialização com runs/itens `running`, `starting` e `pending` órfãos no SQLite → `recover_interrupted()` marca execução e itens não terminais como `interrupted` e preserva itens já `succeeded`/`failed`.
     - `CA-20`: `suggest()` sanitiza entrada, valida o rascunho retornado pela IA, rejeita sugestão contendo segredo ou comando com `\n` e nunca persiste nem executa o grupo sugerido.
     - `CA-23`: repetição de `request_id` com mesmo payload devolve o mesmo `GroupRun` (mesmo após edição/exclusão do grupo); repetição de `request_id` com payload diferente retorna `commands.group.request_conflict`.

---

## 2. Comandos Exatos de Verificação

Execute os comandos abaixo a partir de a raiz da worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)"
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands --test groups_service
cargo test --manifest-path src-tauri/Cargo.toml -p mt-commands
cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings
npm run typecheck
git diff --exit-code src/types/bindings.ts
```

---

## 3. Mapeamento dos Critérios de Aceite

| Critério da Fase 15 | Como é atendido e verificado nesta Implementação |
|---------------------|--------------------------------------------------|
| **CA-01** | `cargo fmt --check`, `cargo clippy --workspace -- -D warnings` e todos os testes de `mt-commands` verdes. |
| **CA-09** | Testado em `groups_service.rs`: `recover_interrupted()` converte linhas `running`/`starting`/`pending` órfãs em `interrupted`. |
| **CA-10 (bindings)** | Os 14 comandos e DTOs geram `src/types/bindings.ts` via `tauri-specta` sem drift. |
| **CA-20 (backend)** | `command_group_suggest` valida o rascunho sugerido com `validate_group_input`, respeita o `draft_revision` e nunca salva ou executa automaticamente. |
| **CA-21** | Hook de shutdown explícito chama `service.shutdown().await` antes de liberar DB/PTYs; minimizar para a bandeja não invoca shutdown. |
| **CA-22** | Mensagens de erro e prompts de sistema da sugestão IA em `pt-BR`. |
| **CA-23** | Testado em `groups_service.rs`: deduplicação por `(request_id, request_hash)` antes da checagem de revisão atual do grupo. |
