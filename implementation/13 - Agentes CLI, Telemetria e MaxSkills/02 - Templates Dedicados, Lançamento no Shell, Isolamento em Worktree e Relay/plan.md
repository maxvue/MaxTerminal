# Implementação 02 — Templates Dedicados, Lançamento no Shell, Isolamento em Worktree e Relay (`plan.md`)

> Especificação técnica: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Execução

- [ ] **Passo 1 — Implementar o Protocolo e Servidor Relay (`crates/mt-agents/src/relay/`)**
  1. Criar `crates/mt-agents/src/relay/protocol.rs` com `RelayEnvelope`, `RelayMessage` (`Statusline`, `Hook`, `CodexNotify`) e `RelayAck`.
  2. Criar `crates/mt-agents/src/relay/server.rs` com `RelayToken` (geração com `rand::rngs::OsRng`, `base64url` e hash SHA-256) e `RelayServer::bind` sobre `tokio::net::UnixListener` com permissões `0700`/`0600`, verificação `SO_PEERCRED` (`geteuid`), leitura limitada a 64 KiB com timeout de 1 s e comparação de token via `subtle::ConstantTimeEq`.
   3. Implementar reserva de token com `PendingRunBinding`, ativação com `RunBinding` completo em `PaneSpawned`, fila limitada pré-ativação, `revoke_run` e `revoke_tab`; falha de spawn descarta reserva/fila.

- [ ] **Passo 2 — Implementar o Binário Sidecar `mt-agent-relay` (`crates/mt-agents/src/bin/mt-agent-relay.rs`)**
  1. Declarar `[[bin]] name = "mt-agent-relay"` em `crates/mt-agents/Cargo.toml` e registrar `"binaries/mt-agent-relay"` em `src-tauri/tauri.conf.json`.
  2. Implementar os 3 subcomandos usando apenas `std`, `serde_json` e `base64`:
     - `statusline [--chain <b64>]`: lê stdin, envia ao socket com timeout de 150 ms e executa o comando encadeado via `sh -c` repassando o mesmo stdin e preservando o stdout (ou imprime o fallback padrão).
     - `hook <Evento>`: envia o evento ao socket e encerra sempre com exit code `0` e stdout vazio.
     - `codex-notify [--chain <b64>] <json>`: envia ao socket, executa o `notify` encadeado com o mesmo argumento JSON e encerra com exit code `0`.

- [ ] **Passo 3 — Implementar `LaunchPlanner`, Templates Dedicados e Quoting por Shell (`crates/mt-agents/src/launch.rs`)**
  1. Implementar os repositórios SQLite para `agent_template_seeds` e `agent_template_options` (`ensure_agent_templates` idempotente respeitando `template_id = NULL` quando excluído pelo usuário).
  2. Implementar `LaunchPlanner::resolve_cli_model` e `LaunchPlanner::build_plan`, gerando `<runtime_dir>/claude-settings.json` (`0600`) com encadeamento `--chain <b64>` do `statusLine` efetivo do usuário e argumentos `-c notify=[...]` para o Codex.
  3. Implementar `command_line(plan, initial_command, shell)` com quoting específico e testado para `Bash`, `Zsh` e `Fish`.

- [ ] **Passo 4 — Implementar `IsolationService` (`crates/mt-agents/src/isolation.rs`)**
  1. Implementar `propose` (descoberta do repositório via `mt-git`, resolução de `HEAD` para OID completo, geração de slug/branch/path sem colisão e verificação `git check-ignore -q`).
  2. Implementar `prepare` e `rollback` com persistência de estágios em `agent_launch_operations` e vínculos em `agent_tab_links` (`WorktreeIsolation::{None, Create, Existing}`).
  3. Implementar `inspect`, `mark_tab_closed`, `list_pending`, `reconcile_startup` e `resolve` com validação de `inspection_token` sob lock por repositório, bloqueando remoção se houver arquivos modificados, untracked, ignorados (`!!`), submódulos sujos, locks ou processos ativos, e usando exclusivamente `force = false` e `git branch -d`.

- [ ] **Passo 5 — Implementar Adaptadores do Binário e `AgentResolver` (`src-tauri/src/agents/`)**
   1. Criar `src-tauri/src/agents/adapters.rs` implementando `WorkspacePort`, `GitPort`, `PtyPort` e `AgentWorkspaceObserver::on_signal` (`WorkspaceLifecycleSignal::{PaneSpawned, PaneExited, SessionArchived, TabClosed}`).
   2. Criar `src-tauri/src/agents/resolver.rs` implementando `EnvironmentResolver::resolve(&SpawnContext)` síncrono: usar `ctx.owner`/`ctx.snapshot`/`ctx.is_primary`/`ctx.emit_initial_command`, compor `ShellResolver`, consumir `PendingLaunch`, retirar a chave de `spawn_spec.env`, preservar `shell_integration_enabled` e preencher `initial_command`. Registrar via `register_agent_resolver`.
   3. Implementar `AppAgentState::open_agent_tab` e `AgentTabOpened` conforme `spec.md` §3.3, compartilhando `PendingLaunch` com o resolvedor e registrando `app.manage(AppAgentState { ... })`; handler `agent_tab_open` somente delega. Exercitar o método diretamente, como fará `AgentPort` da Fase 16.
   4. Adicionar os comandos `agent_templates_ensure`, `agent_template_options_get`, `agent_template_options_set`, `agent_worktree_propose`, `agent_tab_open`, `agent_tab_link_get`, `agent_tab_close_check`, `agent_worktree_resolve` e `agent_worktree_pending_list` em `src-tauri/src/commands/agents.rs`.

- [ ] **Passo 6 — Escrever Testes de Integração (`tests/relay.rs`, `tests/launch.rs`, `tests/isolation.rs`)**
  1. Em `tests/relay.rs`: testar autenticação com token válido, rejeição de token inválido/revogado, rejeição de payload > 64 KiB e execução real do binário `mt-agent-relay` preservando stdout do comando `--chain` e exit 0 em `hook`.
   2. Em `tests/launch.rs`: testar `agent_templates_ensure` (4 na 1ª chamada, 0 na 2ª), quoting com espaços/aspas/`$` em `bash`, `zsh` e `fish`, encadeamento de `statusLine` do Claude e remoção de `MAXTERMINAL_AGENT_LAUNCH`; no binário, testar o adaptador com os tipos reais da Fase 07 (split/restauração não relançam, inicial único, ativação de token e geração corretas) e paridade entre serviço direto/IPC.
  3. Em `tests/isolation.rs`: sobre um repositório Git temporário real, testar `propose` (com e sem `.gitignore`), `Create`, `Existing` (do mesmo repo e de repo externo → `agents.worktree_foreign`), inspeção e resolução (`Keep`, `Remove` em worktree limpa, bloqueio com `agents.worktree_dirty` quando há arquivo modificado/untracked/ignorado e bloqueio de exclusão de branch com commit não mesclado).

---

## 2. Comandos de Verificação e Testes

Executar na raiz da worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)"
# 1. Compilar o crate e o binário sidecar mt-agent-relay
cargo build --manifest-path src-tauri/Cargo.toml -p mt-agents --bins

# 2. Executar clippy em todos os alvos
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# 3. Executar as suítes de relay, launch e isolation
cargo test --manifest-path src-tauri/Cargo.toml -p mt-agents --test relay --test launch --test isolation
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

- **CA-08**: `agent_templates_ensure(project_id)` cria 4 templates `environment_kind = 'agent'` na primeira chamada e 0 na segunda (idempotente).
- **CA-09 (backend)**: `agent_tab_open` abre uma aba cujo shell executa o binário do agente no `cwd` da pasta do template/projeto ou worktree.
- **CA-12**: Abrir aba isolada cria `<repo>/.worktrees/wt-<slug>` na branch `agent/<agente>/<slug>` (ou `.max-code-worktrees/`), define o `cwd` do PTY para a worktree e grava o vínculo em `agent_tab_links`.
- **CA-13 (backend)**: `agent_worktree_propose` verifica `git check-ignore` na raiz de worktrees e retorna `root_ignored = false` + `warnings` em pt-BR quando não ignorada.
- **CA-14**: Após fechar a aba vinculada, worktree própria limpa permite `Keep`, `Remove` e (se branch mesclada) `RemoveAndDeleteBranch`; alterações rastreadas, não rastreadas, ignoradas, submódulos, lock ou uso compartilhado bloqueiam remoção; commits exclusivos bloqueiam apagar a branch; pendências sobrevivem a reinício.
- **CA-15**: Worktree `Existing` é validada contra o `common_dir` do repositório e vinculada com `created_by_app = false`, oferecendo somente `Keep`.
- **CA-16**: A inspeção e remoção ocorrem via `WorkspaceLifecycleSignal::TabClosed` após o encerramento da árvore de processos pela Fase 02, sem deixar processos órfãos.
- **CA-21**: O `statusLine` original do usuário no Claude Code é encadeado via `--chain <b64>` no `mt-agent-relay` e seu stdout continua sendo exibido no TUI.
