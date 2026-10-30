# Plano de Execução — Implementação 03: Orquestração de Runtime, Ciclo de Vida de PTYs, Split Panes e Sessões

## 1. Checklist Sequencial de Implementação

### Passo 1: Adaptadores com Fases 02, 05 e 06 (`workspace/adapters.rs`)
1. Criar `src-tauri/src/workspace/adapters.rs` implementando:
   - `ExecutionOwner` (`project_id`, `tab_id`, `pane_id`, `session_id`, `tab_session_id`, `generation`).
   - `PtyLifecycleAdapter` encapsulando `PtyManager::spawn` com `pre_read_hook`, `write`, `query_live_cwd` (via `/proc`) e `terminate_and_wait` (executando `kill_and_wait` em `spawn_blocking` com escalada `Graceful` → `Force`).
   - `WorkspaceSessionProvider` implementando `SessionProvider` da Fase 05 para associar `(PtyId, ExecutionOwner)` antes do primeiro byte lido do PTY.
   - `ShellWorkspaceAdapter` gerenciando prontidão de prompt OSC 133 A/B (incluindo retenção de prompt precoce em `early_prompts`), `ContinuousHint`, `drain_execution` e reset/remoção de status de aba.

### Passo 2: Resolvedor de Ambiente e Ponto de Extensão (`workspace/environment.rs`)
1. Criar `src-tauri/src/workspace/environment.rs` implementando:
   - `SpawnContext`, `ResolvedSpawn`, trait `EnvironmentResolver` e `EnvironmentRegistry`.
   - `ShellResolver` combinando `allowed_host_env()` da Fase 02, `Literal` e `Inherited` (resolvido apenas em memória) e variáveis internas `MAXTERMINAL_*`.
   - Retorno imediato de `MtError::workspace_environment_unavailable` ao tentar abrir template `EnvironmentKind::Agent` sem resolvedor registrado.

### Passo 3: Observadores e Ator de Aba (`workspace/observer.rs` e `workspace/runtime.rs`)
1. Criar `src-tauri/src/workspace/observer.rs` com `WorkspaceLifecycleSignal`, trait `WorkspaceLifecycleObserver` e fila de despacho assíncrono ordenada por `TabId` fora do lock do ator.
   - Criar e exportar `workspace/spawn_hooks.rs` com `SpawnReservationRequest`, `SpawnHookFuture`, `WorkspaceSpawnHook` e `WorkspaceSpawnGuard` (`spec.md` §3.4), sem dependência de `mt-git` nem mudanças em `SpawnContext`/`ResolvedSpawn`.
2. Criar `src-tauri/src/workspace/runtime.rs` implementando `TabActor` e `TabActorHandle`:
   - Mailbox limitada e serialização de comandos por aba (`Open`, `Split`, `ClosePane`, `RestartPane`, `UpdateLayout`, `NewSession`, `CloseTab`, `RunInitialCommand`, `HandlePtyExit`, `HandlePromptReady`, `HandleInitialCommandTimeout`).
   - Máquina de estados do comando inicial (`pending` → `claimed` → `sent` no primeiro prompt confiável, ou `pending` → `waiting` após timeout de 5 s sem injeção automática).

### Passo 4: `WorkspaceManager` e Integração com `ProjectRemovalHook` (`workspace/manager.rs`)
1. Criar `src-tauri/src/workspace/manager.rs` implementando todos os métodos de orquestração:
   - `template_list`, `template_get`, `template_create`, `template_update`, `template_delete`, `template_set_default`.
   - `tab_open` com guard de compensação (se falhar após o spawn antes da publicação, encerra o PTY e drena workers).
   - `pane_split` (herdando `cwd` confirmado pela Fase 05 ou `/proc` da Fase 02, sem comando inicial), `pane_close` (colapsando nó pai ou fechando a aba se for a última folha), `pane_restart` (apenas para `Exited`/`Failed`) e `pane_layout_update`.
   - `session_new` e `tab_close` com parada paralela de PTYs, retenção em `TabLifecycle::Blocked` caso um processo sobreviva, drenagem da Fase 05 (`drain_execution`; a barreira do recorder é acrescentada na Implementação 04) e transição transacional de agrupamento (`seq + 1`).
   - Campo `project_service: Arc<mt_projects::service::ProjectService>` (adquire `acquire_activity(project_id)` apenas durante a operação de abertura/reinício e libera o guard ao final), `pane_pty(tab_id, pane_id) -> Option<PtyId>`, `register_observer(...)` e implementação de `ProjectRemovalHook` da Fase 06 chamando `close_project_tabs_for_removal(project_id)` e `delete_project_workspace_data_tx` somente após confirmação de término de todos os processos.

### Passo 5: Testes de Integração de PTY, Concorrência e Supervisão
0. Integrar `with_spawn_hooks` antes de compartilhar o manager: lista imutável, vazia na Fase 07 isolada. Todos os fluxos que criam PTYs reservam antes do spawn, aguardam `register` antes de publicar e liberam após registro ou compensação; operações `Blocked` retêm os guards. Implementar a ordem de locks e cancelamento de `spec.md` §3.4–§3.5.
1. Escrever testes de integração em `src-tauri/src/workspace/` validando:
   - Abertura de aba com prompt chegando antes do retorno do spawn (prompt retido dispara o comando inicial uma única vez).
   - Prompt repetido na mesma sessão não reenvia o comando inicial; timeout de 5 s sem prompt deixa `initial_dispatch = waiting` sem injetar bytes no PTY.
   - `pane_split` cria novo `PaneId`/`PtyId`/`SessionId` no mesmo `TabSessionId`, herda `cwd` e não executa comando inicial.
   - `session_new` mantém `TabId`, `PaneId` e layout, troca `TabSessionId`, todos os `SessionId` e `PtyId`, incrementa `generation` e ignora eventos tardios da geração antiga.
   - Encerramento de processo filho desacoplado com `setsid`: `tab_close` e `session_new` aguardam o término real em `/proc`; simulação de processo que recusa terminar coloca a aba em `Blocked`, impede novo spawn e bloqueia a remoção do projeto.
   - Concorrência `pane_split` × `tab_close` × `session_new` × cancelamento de `invoke` sem deixar PTY órfão.
   - `ProjectActivityGuard` liberado após abertura/split/reinício/nova sessão (também em erro/compensação), remoção de projeto sem readquisição dentro do hook e sem lease permanente da aba.
   - `pane_pty(...).await` retorna o PTY atual somente para a aba/painel vivos corretos; retorna `None` após saída/fechamento ou para aba divergente. Observadores recebem sinais ordenados fora da mailbox.
   - Hook fake com barreiras comprova `reserve → spawn → register → publicação → Drop`, inclusive com observador atrasado; falha no segundo hook libera o primeiro sem spawn; registro parcial é desfeito após término do PTY.
   - Cancelamento antes do spawn libera reservas; cancelamento do caller após spawn mantém operação supervisionada; falha de compensação retém guards em `Blocked` até retry. Verificar futures `Send`, guard `Send + 'static` e uso por `tokio::spawn` sem empréstimos locais.

---

## 2. Comandos de Verificação

```bash
cd "$(git rev-parse --show-toplevel)"

# Formatação e análise estática
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# Testes unitários e de integração do orquestrador de workspace
cargo test --manifest-path src-tauri/Cargo.toml --bin maxterminal workspace::
```

---

## 3. Critérios de Aceite desta Implementação

- [ ] **CA-03.1 (Split e Fechamento de Painéis):** `pane_split` cria um novo `PaneId`/`PtyId` na sessão atual herdando o `cwd` do painel dividido sem repetir o comando inicial; fechar o último painel executa exatamente o fluxo de fechar aba (`PaneCloseOutcome::TabClosed`).
- [ ] **CA-03.2 (Identidade e Isolamento por Geração):** Em `session_new`, `TabId` e `PaneId` permanecem idênticos enquanto `TabSessionId`, todos os `SessionId` e `PtyId` mudam e `generation` incrementa; eventos tardios da geração anterior são descartados sem afetar status, layout ou contadores.
- [ ] **CA-03.3 (Supervisão de Processos e Estado `Blocked`):** Fechar aba/projeto, reiniciar painel ou iniciar nova sessão aguarda todos os descendentes rastreados (inclusive `setsid`) desaparecerem de `/proc`; se o encerramento falhar, a aba entra em `Blocked`, preserva ownership, bloqueia remoção do projeto e não cria PTYs substitutos.
- [ ] **CA-03.4 (Despacho Seguro do Comando Inicial):** O comando inicial é enviado no máximo uma vez por lançamento raiz após prompt confiável; timeout de 5 s sem prompt não injeta texto em processo desconhecido e habilita tentativa explícita via `workspace_initial_command_run`.
- [ ] **CA-03.5 (Falha Parcial Isolada):** Em `session_new` ou abertura multi-painel, falha de spawn em um painel marca apenas aquele `PaneView` como `Failed` com `MtError` recuperável (incluindo opção de fallback de pasta), sem impedir os demais painéis da aba.
- [ ] **CA-03.6 (Reserva Pré-Spawn):** Todos os caminhos de spawn passam pelo hook instalado antes do boot; guards sobrevivem até registro confirmado ou compensação, sem depender do observador e sem liberação prematura em cancelamento/`Blocked`. Testes com hooks fake passam sem `mt-git`.
