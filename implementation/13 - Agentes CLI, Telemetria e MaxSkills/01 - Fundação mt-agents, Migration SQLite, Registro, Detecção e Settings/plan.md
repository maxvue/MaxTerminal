# Implementação 01 — Fundação `mt-agents`, Migration SQLite, Registro, Detecção e Settings (`plan.md`)

> Especificação técnica: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Execução

- [ ] **Passo 1 — Criar a Migration SQLite da Fase 13 (`mt-db`)**
  1. Confirmar em `src-tauri/crates/mt-db/migrations/` que existem `0001_03_base` … `0007_12_ai` (sequência contígua, decisão D-01); a migration desta fase é `0008`.
  2. Criar `src-tauri/crates/mt-db/migrations/0008_13_agents_skills_memory.sql` com as 10 tabelas `STRICT` especificadas em `spec.md` §3.2 (`agent_template_seeds`, `agent_template_options`, `agent_tab_links`, `agent_launch_operations`, `agent_telemetry_snapshots`, `skill_sync_targets`, `skill_sync_entries`, `command_memory`, `command_memory_processed`, `command_memory_preferences`) e seus índices parciais/únicos.
  3. Executar os testes do `mt-db` para confirmar que a migration aplica sem erros sobre um banco limpo e em sequência após as fases anteriores.

- [ ] **Passo 2 — Criar o crate `mt-agents` e registrá-lo no Workspace Cargo**
   1. Criar `src-tauri/crates/mt-agents/Cargo.toml` com as dependências de `spec.md` §3.1, `edition.workspace = true` e watchers herdados (`notify = "8"`, `notify-debouncer-full = "0.5"` centralizados no workspace).
  2. Adicionar `"crates/mt-agents"` em `[workspace.members]` e `mt-agents = { path = "crates/mt-agents" }` em `[workspace.dependencies]` no `src-tauri/Cargo.toml`.
  3. Criar `src-tauri/crates/mt-agents/src/lib.rs` com `#![forbid(unsafe_code)]` e declaração dos módulos `error`, `ports`, `registry`, `detect` e `settings`.

- [ ] **Passo 3 — Implementar `error.rs` e `ports.rs`**
  1. Implementar todos os códigos de erro em `crates/mt-agents/src/error.rs` (`agents.*`, `skills.*`, `memory.*`) com mensagens em `pt-BR` e payloads estruturados em `details`.
   2. Implementar `crates/mt-agents/src/ports.rs` com `RunBinding` incluindo `pty_id`/`generation`, `ForegroundProc`, `ProjectRecord`, `AttentionKind` e as traits `TabStatePort`, `NotifyPort`, `PtyPort`, `WorkspacePort` e `GitPort`; abertura e resolução de atenção recebem `&RunBinding` completo.

- [ ] **Passo 4 — Implementar Descritores TOML e `AgentRegistry` (`registry.rs`)**
  1. Criar os 4 arquivos embutidos `crates/mt-agents/src/descriptors/{claude,codex,agy,opencode}.toml`.
  2. Implementar `AgentKind`, `AgentCapabilities`, `AgentDescriptor`, `CapabilitySupport`, `CapabilityEvidence`, `CapabilityReport`, `AgentUserOverride` (`#[serde(deny_unknown_fields)]`) e `AgentRegistry` em `crates/mt-agents/src/registry.rs`.
  3. Garantir que `AgentRegistry::load` carregue os 4 descritores na ordem `AgentKind::ALL` e aplique `$XDG_CONFIG_HOME/maxterminal/agents.toml` se existir; caso o TOML seja inválido, retornar o erro `agents.settings_invalid` no segundo elemento da tupla mantendo os descritores embutidos ativos.

- [ ] **Passo 5 — Implementar `Detector` e `CapabilityReport` (`detect.rs`)**
  1. Implementar captura assíncrona do `PATH` do login shell (`$SHELL -lic 'printf %s "$PATH"'` com timeout de 2 s, `MAXTERMINAL_PATH_PROBE=1` e `kill_on_drop(true)`), combinada ao `PATH` do processo e aos 4 diretórios extras (`~/.local/bin`, `~/.bun/bin`, `~/.npm-global/bin`, `~/.opencode/bin`).
  2. Implementar `Detector::detect_all` executando a verificação dos 4 agentes em paralelo (`join_all`), com timeout individual de 3 s em `--version` e extração de versão pela regex do descritor.
  3. Implementar `Detector::build_capability_report` gerando as 9 features canônicas de acordo com a matriz normativa S-01..S-14.

- [ ] **Passo 6 — Implementar `AgentsSettings` e Validação de Atalhos (`settings.rs`)**
   1. Implementar `AgentsSettings`, `IsolationSettings`, `AttentionSettings`, `AttentionLevel`, `TelemetrySettings` e `WorktreeLocationDto`, com `as_db_str`/`from_db_str` (`max_code` no SQL, `maxCode` no IPC); testar round-trip e rejeição de valor inválido.
  2. Implementar `normalize_and_validate_shortcut` e `validate_settings`, verificando formato, exigência de modificador, conflito direto, conflito com a variante `+Shift` de isolamento e conflito com atalhos reservados das Fases 04, 07 e 09.
  3. Registrar os descritores `agents.settings` e `skills.settings` no `SettingsRegistry` da Fase 03.

- [ ] **Passo 7 — Expor Comandos Tauri Iniciais e Escrever Testes (`tests/registry.rs` e `tests/detect.rs`)**
  1. Criar `src-tauri/src/commands/agents.rs` com `agent_list`, `agent_detect_refresh`, `agent_settings_get`, `agent_settings_update` e `agent_descriptors_reload`, emitindo `agent://installations-changed` e `agent://settings-changed`.
  2. Criar `src-tauri/crates/mt-agents/tests/registry.rs` testando os 4 descritores, serialização de `AgentKind`, aplicação de overrides válidos em `agents.toml` e preservação dos embutidos diante de TOML inválido.
  3. Criar `src-tauri/crates/mt-agents/tests/detect.rs` com scripts falsos em diretório temporário: script `claude` que imprime `2.1.0 (Claude Code)`, binário ausente e script que faz `sleep 10`, provando que `detect_all` conclui em ≤ 3,5 s sem bloquear os demais agentes.

---

## 2. Comandos de Verificação e Testes

Executar na raiz da worktree ativa:

```bash
cd "$(git rev-parse --show-toplevel)"
# 1. Formatação e checagem de tipos/lint no workspace Rust
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# 2. Testes da migration SQLite no mt-db
cargo test --manifest-path src-tauri/Cargo.toml -p mt-db

# 3. Testes unitários e de integração de registry, detect e settings no mt-agents
cargo test --manifest-path src-tauri/Cargo.toml -p mt-agents --test registry --test detect
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

- **CA-01 (parcial)**: `cargo clippy --workspace --all-targets -- -D warnings` e `cargo fmt --all --check` passam sem alertas para `mt-db` e `mt-agents`.
- **CA-04**: `#![forbid(unsafe_code)]` ativo em `mt-agents`; nenhum `std::thread::sleep` ou IO bloqueante em tarefas `tokio`.
- **CA-05**: `agent_list` retorna exatamente os 4 descritores (`claude-code`, `codex`, `agy`, `opencode`) com `displayName`, ícone Iconify, token de cor semântica e `CapabilityReport` por versão/fonte; `claude` é o nome do executável e `claude-code` é o ID do agente/harness.
- **CA-06**: Teste com `PATH` temporário contendo script falso `claude` (`2.1.0 (Claude Code)`) retorna `installed = true` e `version = "2.1.0"`; binário ausente retorna `installed = false` com `reason` em pt-BR; binário que trava sofre timeout em ≤ 3,5 s em paralelo sem bloquear os demais.
- **CA-07**: Override em `agents.toml` (`[claude] binary = "/opt/claude/bin/claude"`, `extra_args = ["--verbose"]`) reflete em `agent_list`; TOML inválido retorna `MtError` com código `agents.settings_invalid` preservando os descritores embutidos.
- **CA-11 (backend)**: `agent_settings_update` rejeita atalhos conflitantes com `agents.shortcut_conflict` e atalhos malformados com `agents.shortcut_invalid`.
