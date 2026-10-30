# Implementação 02 — Plano de Execução Passo a Passo (`plan.md`)

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1: Definir Portas e Contratos de Eventos (`ports.rs` e `events.rs`)**
  1. Criar `src-tauri/crates/mt-commands/src/groups/ports.rs` com as traits `GroupPtyPort`, `GroupShellPort`, `PaneExecutionLease`, `GroupTabPort`, `GroupAiPort`, `RecurringCommandsSource` e structs associadas (`HeadlessSpawnSpec`, `SupervisedExitInfo`, `PaneShellState`, `ShellLifecycleEvent`, `ProjectShellTemplate`, `GroupTabOpenSpec`, `OpenedGroupTab`).
  2. Criar `src-tauri/crates/mt-commands/src/groups/events.rs` com as constantes dos 4 eventos (`command-group://changed`, `command-group://run-started`, `command-group://run-item-changed`, `command-group://run-finished`), structs de payload (`GroupChangedEvent`, `GroupRunEvent`, `GroupRunItemEventDelta`, `GroupOutputFrame`, `OutputAttachReceipt`) e traits `GroupEventSink` e `OutputFrameSubscriber`.

- [ ] **Passo 2: Implementar `SequentialComposer` (`compose.rs`)**
  1. Criar `src-tauri/crates/mt-commands/src/groups/compose.rs` implementando `quote_arg` para `ShellDialect::Bash`, `ShellDialect::Zsh` e `ShellDialect::Fish`.
  2. Implementar `compose_submission`:
     - Caminho direto sem overrides: retorna `format!("{command}\n").into_bytes()`.
     - Caminho isolado com overrides de `cwd` e/ou `env`: valida `env_chdir_supported` quando houver `cwd`, monta o comando `env -C <cwd> <K=V...> <shell> -c <command>` com cada argumento individualmente escapado para o dialeto externo do painel.

- [ ] **Passo 3: Implementar `GroupRunSink` e `ParallelRunner` (`runner.rs`)**
  1. Em `src-tauri/crates/mt-commands/src/groups/runner.rs`, implementar `GroupRunSink` integrando `mt_db::sanitizer::StreamSanitizer`, ring buffer de `256 KiB` com corte em fronteira UTF-8 (`str::is_char_boundary`), fila não bloqueante por assinante com geração de `GroupOutputFrameKind::Gap` quando houver backpressure e `attach`/`detach` atômico na fronteira replay/live.
  2. Implementar `ParallelRunner`:
     - Semáforo global `Arc::new(Semaphore::new(16))`.
     - Fila por item adquirindo permissão do semáforo, revalidando se o item/run não foi cancelado enquanto aguardava vaga, persistindo `Starting`, chamando `pty_port.spawn_headless` com `args = ["-c", &command]`, `login = false`, persistindo `Running` + `pty_id` ANTES de emitir `run-item-changed`.
     - Aguardar `pty_port.wait_exit(&pty_id)`, realizar flush final sanitizado do `GroupRunSink`, persistir `Succeeded` (`exit_code == Some(0)` e `tree_cleaned`) ou `Failed`/`Cancelled` e emitir `run-item-changed` e, quando último item terminar, `run-finished`.
     - Implementar `cancel` idempotente e `shutdown_all`.

- [ ] **Passo 4: Implementar `SequentialRunner` (`sequential.rs`)**
  1. Criar `src-tauri/crates/mt-commands/src/groups/sequential.rs`.
  2. Para cada item da fila sob o `PaneExecutionLease`:
     - Se cancelado antes de enviar, marcar `Cancelled`.
     - Persistir `Starting`, anexar tap de saída ao `pty_id` do painel para capturar a saída apenas durante a janela do item, compor submissão via `SequentialComposer` e escrever no PTY.
     - Aguardar no stream do lease: `BlockStarted` (gravar `command_block_id` e `Running`), depois `BlockFinished` (capturar `exit_code` e `signal`) e finalmente `PromptReady` da mesma `generation`.
     - Se ocorrer `GenerationInvalidated` (ex.: `exit` do shell, substituição de PTY, perda de tracking), marcar o item atual e os pendentes como `Interrupted` e liberar o lease sem escrever na nova geração.
     - Aplicar política `SequentialSeparator::And` (marcar itens pendentes restantes como `Skipped` se `exit_code != 0`) ou `SequentialSeparator::Semicolon` (continuar para o próximo item).
  3. Implementar `cancel(run_id, item_id)` enviando `\x03` (Ctrl+C) apenas se o item alvo estiver `Running` na geração atual do lease, ou removendo da fila se `Pending`.

- [ ] **Passo 5: Implementar `MultiTabPlanner` e `MultiTabRunner` (`plan.rs` e `multi_tab.rs`)**
  1. Criar `src-tauri/crates/mt-commands/src/groups/plan.rs` implementando `resolve_templates_for_items` para validar compatibilidade de `continuous` e ambiente Shell no preflight.
  2. Criar `src-tauri/crates/mt-commands/src/groups/multi_tab.rs` implementando `MultiTabRunner`:
     - Iterar pelos itens em ordem (`position ASC`), persistindo `Starting`, assinando eventos antes da abertura e invocando `tab_port.open_tab` com `focus: index == 0` e `cwd_fallback_to_project: false`.
     - Se `open_tab` falhar, marcar o item como `Failed` (`commands.group.tab_open_failed`) e continuar abrindo os próximos itens sem fechar as abas anteriores já abertas.
     - Para cada aba aberta com sucesso, persistir `(tab_id, pane_id, pty_id, session_id)` e acompanhar o comando inicial em task dedicada: se `continuous == true`, permanece `Running` enquanto o processo viver; quando terminar, registra `exit_code` e status final sem fechar a aba.
     - Em `cancel`, interromper novas aberturas e invocar `close_tab_if_running_initial` somente para itens cujo comando inicial ainda esteja em execução.

- [ ] **Passo 6: Criar Testes de Shells Reais e Runners (`groups_compose.rs` e `groups_runner.rs`)**
  1. Criar `src-tauri/crates/mt-commands/tests/groups_compose.rs` executando `bash`, `zsh` e `fish` reais para validar `CA-05`:
     - Quoting com aspas simples, aspas duplas, espaços, `$VAR`, pipes `|`, comentários `#`, backslashes `\`.
     - Fila `[false, printf ok]` com `&&` vs `;`.
     - Preservação de `cd subpasta` entre Item 1 e Item 2 quando não há override, e isolamento total com `env -C` quando Item 1 tem override de `cwd`/`env`.
     - Tratamento de `exit` no meio da fila e rejeição de shell desconhecido (`commands.group.unsupported_shell`).
  2. Criar `src-tauri/crates/mt-commands/tests/groups_runner.rs` validando `CA-06`, `CA-07`, `CA-08` e `CA-24`:
     - Execução paralela de 3 itens (`sleep 0.2; exit 0`, `exit 3`, `sleep 30` com processo filho), cancelamento do 3º em < 3,5 s e verificação em `/proc` de que nenhum PID descendente restou vivo.
     - Execução de 20 itens concorrentes verificando que o pico simultâneo nunca ultrapassa 16 PTYs.
     - Item gerando 2 MiB de saída contendo `ghp_1234567890abcdefghijklmnopqrstuvwx` verificando `output_tail.len() <= 262_144`, `output_truncated == true` e token substituído por `[REDACTED]`.
     - Falha parcial no `MultiTabRunner` (falha no item 2 preserva item 1 e abre item 3) e não fechamento de aba cujo comando inicial já concluiu.

---

## 2. Comandos Exatos de Verificação

Execute os comandos abaixo a partir de `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0/src-tauri`:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p mt-commands --test groups_compose
cargo test -p mt-commands --test groups_runner
```

---

## 3. Mapeamento dos Critérios de Aceite

| Critério da Fase 15 | Como é atendido e verificado nesta Implementação |
|---------------------|--------------------------------------------------|
| **CA-01** | `cargo fmt --check`, `cargo clippy --workspace -- -D warnings` e `cargo test -p mt-commands` verdes. |
| **CA-05** | Testado em `groups_compose.rs` contra binários reais `bash`, `zsh` e `fish`: fila `[false, printf ok]` com `&&` e `;`, aspas, pipes, comentários, `cd`, `exit`, isolamento `env -C` sem vazar cwd/env ao pai e erro explícito para shell desconhecido. |
| **CA-06** | Testado em `groups_runner.rs`: 3 itens com `sleep 0.2; exit 0`, `exit 3` e `sleep 30`, cancelamento em < 3,5 s sem processos órfãos em `/proc` e `finished_at` gravado. |
| **CA-07** | Testado em `groups_runner.rs`: 20 itens concorrentes respeitam o limite global de 16 PTYs simultâneos com 4 aguardando em `pending`. |
| **CA-08** | Testado em `groups_runner.rs`: saída de 2 MiB truncada em ≤ 256 KiB em fronteira UTF-8 e sanitizada (`[REDACTED]`) sem bloquear o leitor do PTY. |
| **CA-17 (backend)** | `SequentialRunner` exige lease exclusivo, prompt pronto, sem alt-screen e aguarda fim de bloco + novo prompt antes do próximo item. |
| **CA-18 (backend)** | `ParallelRunner` + `GroupRunSink` suportam replay + live via `attach_output`/`detach_output` e cancelamento por item ou execução inteira. |
| **CA-19 (backend)** | `MultiTabPlanner` valida template compatível com `continuous` no preflight; `MultiTabRunner` abre abas sequencialmente e preserva abas anteriores em falha parcial. |
| **CA-21 (parcial)** | `shutdown_all()` implementado nos três runners para encerrar filas e PTYs supervisionados no encerramento explícito do aplicativo. |
| **CA-24** | Testado em `groups_runner.rs`: falha de spawn, falha após abrir parte das abas, mistura sucesso/erro (`partial`) e cancelamento concorrente a exit sem matar abas reutilizadas. |
