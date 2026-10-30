# Implementação 03 — Plano de Execução Passo a Passo (`plan.md`)

> Parte 03 de 05 da **Fase 06: Projetos e Layout de 2 Colunas**.
> Especificação técnica correspondente: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Implementação

### Passo 1 — Implementar `CatalogCoordinator`, `ProjectActivityGuard` e `ProjectRemovalHook` (`src-tauri/crates/mt-projects/src/service.rs`)
1. Implementar a trait `ProjectRemovalHook` e o guard RAII `ProjectActivityGuard`.
2. Implementar `CatalogCoordinator` gerenciando:
   - `epoch: String` (UUID v7 do processo) e `revision: u32` (com rotação automática de `epoch` caso `revision` atinja `u32::MAX`).
   - Cache em memória de `projects`, `roots`, `pending` (`HashMap<String, DiscoveredFolder>`), `watch_status` (`HashMap<ProjectRootId, WatchStatus>`) e `root_generations` (`HashMap<ProjectRootId, u64>`).
   - Cálculo determinístico de sugestões pendentes a partir de `entries`, `projects` e `dismissed`, escolhendo sempre a raiz elegível mais antiga `(created_at, id)` como `DiscoveredFolder.root_id`.

### Passo 2 — Implementar Operações de Domínio em `ProjectService`
1. Implementar `list`, `snapshot`, `preview_identity` (sem alterar banco nem transformar identidade em manual).
2. Implementar `add` e `update`:
   - Validar `title`, `icon` e `color`; se `icon` ou `color` forem informados pelo usuário, definir `identity_source = IdentitySource::Manual`.
   - Rejeitar `reset_identity = Some(true)` combinado com `icon.is_some()` ou `color.is_some()` retornando `MtError` com código `"project.invalid_patch"`.
   - Editar apenas `title` preserva `identity_source` inalterado.
   - Em `add`, se o caminho canônico pertencer a uma raiz já cadastrada (pai direto ou presente em `project_root_entries`), preencher `root_id` com a raiz elegível mais antiga e limpar dispensas/sugestões daquele caminho em todas as raízes.
3. Implementar `redetect(id)` e `refresh_missing()`:
   - `redetect`: executa `StackDetector` no caminho do projeto; se `identity_source == Manual`, atualiza apenas `stack` mantendo `icon`/`color`; se `Auto`, atualiza `stack`, `icon` e `color`.
   - `refresh_missing`: verifica `std::fs::metadata(&project.path)` para todos os projetos; apenas `NotFound`/`NotADirectory` marca `missing = true`.
4. Implementar `remove(id)` com reserva `removing_ids`, aquisição exclusiva no gate de `acquire_activity`, execução de `removal_hooks` e gravação atômica de `Remove { id, dismiss }` para todas as origens conhecidas.
5. Implementar `add_root`, `update_root`, `remove_root`, `rescan_root`, `list_discovered`, `accept_discovered` e `dismiss_discovered`:
   - Validar limite de 1000 paths em `accept_discovered` e `dismiss_discovered`.
   - Recanonicalizar cada caminho em `accept_discovered` e `dismiss_discovered`, abortando o lote inteiro com `project.discovery_stale` se o destino desapareceu ou mudou.

### Passo 3 — Implementar Loops de Background, Coalescência `SETTLE` e Backoff em `ProjectService`
1. Implementar `start(self: &Arc<Self>)`:
   - Carregar catálogo inicial do `store`.
   - Registrar watchers para todas as raízes com `watch_enabled == true`.
   - Iniciar loop de recepção de `WatchSignal` + drenagem de `dirty_roots` a cada 1 s:
     - Agendar reconciliação por raiz após `SETTLE` (1,5 s), reiniciando o timer em novos sinais até o teto máximo de 5 s.
     - Em `RootMissing`, atualizar `watch_status = Missing` e agendar retry com backoff exponencial de `5s` a `60s`.
   - Iniciar timer de reconciliação periódica a cada 30 s para raízes habilitadas (cobrindo re-detecção automática de projetos `Auto` + `Unknown` e mudanças em alvos de symlinks).
   - Executar reconciliação inicial não bloqueante usando `scan_semaphore` (`Semaphore::new(8)`).
2. Implementar `shutdown(&self)` cancelando tasks via canal `watch`, fechando watchers e garantindo que nenhum evento seja emitido após o término.

### Passo 4 — Integrar `TauriProjectEventSink`, Comandos IPC e Capabilities no Crate Binário
1. Criar `src-tauri/src/events/projects.rs` com `TauriProjectEventSink` e registrá-lo em `src-tauri/src/events/mod.rs`.
2. Criar `src-tauri/src/commands/projects.rs` com os 17 comandos anotados com `#[tauri::command]` e `#[specta::specta]`, e registrá-lo em `src-tauri/src/commands/mod.rs`.
3. Editar `src-tauri/src/lib.rs`:
   - Invocar `mt_projects::register_db(&mut db_setup)` antes de `Db::open`.
   - Adicionar os 17 comandos em `tauri_specta::collect_commands![...]`.
   - Inicializar `tauri_plugin_dialog::init()`.
   - Instanciar `Arc<ProjectService>` com `SqliteProjectStore::new(db.clone())` e `TauriProjectEventSink::new(app.handle().clone())`, registrá-lo via `app.manage(project_service.clone())` e disparar `tauri::async_runtime::spawn(async move { let _ = project_service.start().await; })` dentro de `.setup(...)`.
4. Atualizar `src-tauri/capabilities/default.json` adicionando permissões para os comandos `project_*`, `project_root_*`, `project_discovery_*` e `dialog:allow-open` na janela `main`.
5. Executar `cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings` na raiz da worktree ativa para atualizar `src/types/bindings.ts` e confirmar que todos os DTOs e comandos da Fase 06 estão presentes.

### Passo 5 — Escrever Testes de Integração de Serviço, Concorrência e Scheduler (`service_and_concurrency.rs`)
1. Criar `src-tauri/crates/mt-projects/tests/service_and_concurrency.rs` com `MemorySink` e `FailingHook`:
   - Testar preservação de identidade manual após `redetect` e reconciliação periódica; testar transição automática de `unknown` → `rust` quando `Cargo.toml` surge na pasta.
   - Testar bloqueio de remoção por `FailingHook` (`project.removal_blocked`), garantindo que o projeto permanece no banco e o gate de `acquire_activity` é liberado; testar recusa de `acquire_activity` enquanto `remove` está em andamento.
   - Testar casos de integridade de aliases e symlinks:
     - Dois aliases e duas raízes apontando para o mesmo destino → geram 1 único projeto;
     - Remover um alias não marca o destino real ainda existente como `missing`;
     - Redirecionar symlink não altera `Project.id` nem `Project.path`, mantendo o projeto antigo e sugerindo o novo destino;
     - Renomear pasta real marca o projeto antigo como `missing = true` e sugere o novo caminho;
     - Garantir em todos os testes de `remove` e `remove_root` que os diretórios e arquivos no disco permanecem 100% intactos.
   - Testar concorrência: `accept_discovered` × `auto_import` × `add` manual concorrentes para o mesmo caminho não geram duplicata nem falha fatal; sugestão stale aborta o lote inteiro sem commit parcial; rescan atrasado de geração antiga após `remove_root` ou `watch_enabled = false` é descartado.
   - Testar scheduler com `tokio::time::pause()`: teto de 5 s sob sinais contínuos, reconciliação de 30 s, backoff `5s..60s` em raiz ausente e `shutdown()` limpo.

---

## 2. Comandos de Verificação

```bash
cd "$(git rev-parse --show-toplevel)"

# Compilar todo o workspace Rust (incluindo o binário Tauri)
cargo check --manifest-path src-tauri/Cargo.toml --workspace

# Executar toda a suíte de testes de mt-projects e geração de bindings
cargo test --manifest-path src-tauri/Cargo.toml -p mt-projects
cargo test --manifest-path src-tauri/Cargo.toml -p maxterminal
cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings

# Validar Clippy sem avisos em mt-projects e maxterminal
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-projects -p maxterminal --all-targets -- -D warnings

# Confirmar ausência de process::Command em mt-projects
! rg -n "process::Command" src-tauri/crates/mt-projects/src
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 06)

- **CA-2 (Completo):** Fallback preserva ícone/cor após persistência e restart; edição manual não é sobrescrita; `preview_identity` não altera estado nem transforma identidade automática em manual.
- **CA-3 (Completo):** Watcher e scheduler respeitam `SETTLE` de 1,5 s, teto de 5 s sob rajadas, reconciliação de 30 s, distinção entre `missing` real e erro de permissão, e backoff `5s..60s`.
- **CA-5 (Completo):** `cargo clippy -p mt-projects -p maxterminal -- -D warnings` passa sem avisos.
- **CA-6 (Completo):** `src/types/bindings.ts` regenerado com todos os DTOs, `ProjectSnapshot`, comandos e envelopes de eventos.
- **CA-9 (Completo):** Todos os casos de aliases, raízes sobrepostas, symlink redirecionado e rename de diretório validados.
- **CA-10 (Completo):** Concorrência entre `accept_discovered`, `auto_import` e `add` manual validada sem duplicação nem sucesso parcial em lotes.
- **CA-19 (Completo):** `project_remove` e `project_root_remove` jamais apagam arquivos do disco.
