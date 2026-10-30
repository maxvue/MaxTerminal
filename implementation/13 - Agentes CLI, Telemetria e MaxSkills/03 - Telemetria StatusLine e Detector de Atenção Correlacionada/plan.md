# Implementação 03 — Telemetria `StatusLine` e Detector de Atenção Correlacionada (`plan.md`)

> Especificação técnica: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Execução

- [ ] **Passo 1 — Implementar `JsonlTail` e Tipos de Telemetria (`crates/mt-agents/src/telemetry/{mod.rs,tail.rs}`)**
  1. Criar `crates/mt-agents/src/telemetry/mod.rs` com `TelemetrySourceKind`, `MeasurementQuality`, `MetricKey`, `MetricAvailability`, `MetricState`, `Field<T>`, `ModelInfo`, `ContextUsage`, `QuotaWindow`, `AgentTelemetry`, `FieldUpdate<T>`, `TelemetryUpdate`, `SessionHint`, `TelemetryContext` e a trait `TelemetrySource`.
  2. Criar `crates/mt-agents/src/telemetry/tail.rs` com `JsonlTail::open_from_end` (janela inicial de 256 KiB, descarte da 1ª linha parcial, validação de UID e arquivo regular) e `JsonlTail::read_new` (detecção de truncamento e troca de `inode`, limite de 64 KiB por linha e descarte seguro de JSON inválido).

- [ ] **Passo 2 — Implementar as Fontes de Telemetria (`claude_statusline.rs`, `claude_transcript.rs`, `codex_session.rs`, `output_heuristic.rs`)**
  1. Implementar `ClaudeStatusLineSource` parseando o JSON recebido do `mt-agent-relay statusline` (`model`, `context_window.used_percentage`, `rate_limits.five_hour`, `rate_limits.seven_day`, `cost.total_cost_usd`, `session_id`, `transcript_path`) e propagando `SessionHint` via `watch::Sender`.
  2. Implementar `ClaudeTranscriptSource` observando via `notify` o `transcript_path` correlacionado, extraindo `usage` com denominador explícito e emitindo `invalidate_previous_context = true` em eventos de compactação ou `current_usage == null`.
  3. Implementar `CodexSessionSource` correlacionando o arquivo rollout pelo `thread-id` recebido do `codex-notify` e extraindo apenas janelas com `window_minutes == 300` (5h) e `window_minutes == 10080` (semanal).
  4. Implementar `OutputHeuristicSource` (opt-in, qualidade `Estimated`) e suporte inicial à fonte `Configured` quando o agente é aberto com modelo explícito.

- [ ] **Passo 3 — Implementar `TelemetryAggregator` e Persistência de Snapshots (`aggregator.rs`)**
  1. Implementar o merge por campo respeitando a prioridade das fontes (`ClaudeStatusLine` > `ClaudeTranscript`/`CodexSession` > `OutputHeuristic` > `Configured`) e a semântica `Unchanged` / `Clear` / `Set`.
  2. Implementar validação de percentuais (`0.0..=100.0`, não-NaN), conversão segura de `resets_at` (segundos → ms), expiração de cota vencida, marcação `Stale` após 60 s sem observação e throttle trailing de ≤ 1 Hz por aba.
  3. Implementar persistência de snapshots sanitizados em `agent_telemetry_snapshots` (a cada 30 s e na remoção da aba) e hidratação com `agent_active = false`.

- [ ] **Passo 4 — Implementar `PatternSet`, `AttentionTap` e `AttentionDetector` (`crates/mt-agents/src/attention/`)**
  1. Criar `crates/mt-agents/src/patterns/default.toml` e `crates/mt-agents/src/attention/patterns.rs` com compilação de regex de duas âncoras (`regex` + `confirm`), filtro de linhas citadas/diffs e suporte a override em `$XDG_CONFIG_HOME/maxterminal/agent-patterns.toml`.
  2. Implementar `AttentionTap` (`OutputTap` da Fase 02) mantendo `TailBuffer` circular de 8 KiB sem sequências ANSI (`strip-ansi-escapes`) e extraindo sinais OSC 9, OSC 777 e BEL incrementalmente.
  3. Implementar `AttentionDetector` em `crates/mt-agents/src/attention/detector.rs` com máquina de estados por execução e episódio (`Inactive`, `Working`, `Suspected`, `Awaiting`), deduplicação de notificações por episódio, supressão quando a janela está focada e a aba é a ativa, distinção entre `UserInput`/`ack` e resolução nativa correlacionada, e integração com `TabStatePort` e `NotifyPort`.

- [ ] **Passo 5 — Conectar Portas no Binário e Expor Comandos IPC (`src-tauri/src/commands/agents.rs`)**
  1. Implementar `TabStatePort` e `NotifyPort` em `src-tauri/src/agents/adapters.rs` conectando ao `TabStateRegistry` (Fase 05) e ao `AttentionRegistry` + `Notifier` (Fase 09).
  2. Expor os comandos `agent_telemetry_get`, `agent_telemetry_list`, `agent_attention_get` e `agent_attention_ack` em `src-tauri/src/commands/agents.rs` e registrá-los em `src-tauri/src/specta.rs`.

- [ ] **Passo 6 — Criar Fixtures Versionados e Testes (`tests/telemetry.rs` e `tests/attention.rs`)**
  1. Criar os fixtures em `crates/mt-agents/tests/fixtures/` (`claude/statusline.json`, `claude/transcript_sample.jsonl`, `codex/rollout_sample.jsonl`, `attention/positive_prompts.txt`, `attention/negative_outputs.txt`).
  2. Em `tests/telemetry.rs`: testar ingestão via `mt-agent-relay statusline` (emissão de `agent://telemetry` em ≤ 1,5 s), `Clear` vs `Unchanged`, invalidação por compactação/rotação de JSONL, isolamento entre duas abas no mesmo `cwd`, marcação `Stale` após 60 s e throttle (50 atualizações/s resultando em ≤ 2 emissões em 1 s).
  3. Em `tests/attention.rs`: testar `Notification(permission_prompt)` → `AwaitingApproval` + `TabStatus = 'error'` + 1 única notificação, supressão quando aba ativa/focada, `auth_success`/`Stop`/`codex-notify` não gerando `error`, fixtures ANSI positivos gerando apenas `Suspected`, textos citados/BEL não confirmando espera, `UserInput`/`ack` mantendo espera até resolução correlacionada em ≤ 500 ms e precedência de `PtyExit`.

---

## 2. Comandos de Verificação e Testes

Executar na raiz da worktree (`/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0`):

```bash
# 1. Checagem de lint e formatação
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# 2. Executar as suítes de telemetria e atenção
cargo test --manifest-path src-tauri/Cargo.toml -p mt-agents --test telemetry --test attention
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

- **CA-17**: Enviar via `mt-agent-relay statusline` o fixture `fixtures/claude/statusline.json` com o token da aba emite em ≤ 1,5 s o evento `agent://telemetry` com `model.display_name`, `context.percent` e `quota_5h`/`quota_weekly`.
- **CA-18**: Parsers de arquivos internos só produzem valores para sessão inequivocamente correlacionada e com denominador explícito; linhas corrompidas não causam erro fatal; duas abas no mesmo `cwd` não compartilham medições.
- **CA-20**: Emissão de `agent://telemetry` é limitada a ≤ 1 evento/s por aba sob carga de 50 atualizações/s.
- **CA-22**: Fixtures ANSI versionados produzem `Suspected` com motivo/fonte; texto citado, BEL, silêncio e saída normal não produzem espera confirmada; padrões sem fixture ficam desabilitados.
- **CA-23**: `Notification` do Claude com `notification_type = 'permission_prompt'` da execução correta marca a aba como `error` (`seen = false`) e dispara notificação `"Claude Code aguarda aprovação"` conforme visibilidade; `auth_success`, `Stop` e Codex `agent-turn-complete` não geram esse alerta.
- **CA-24**: Digitar tecla, focar ou dispensar apenas reconhece visualização (`acknowledged = true`); resolução nativa correlacionada limpa a atenção em ≤ 500 ms; `PreToolUse` sozinho não resolve aprovação; saída do PTY tem precedência; nenhuma duplicação de notificação por episódio.
- **CA-25 (backend)**: A notificação enviada via `Notifier` da Fase 09 carrega `Some(tab_id)` para acionar `os://focus-tab` ao clicar em "Abrir aba".
- **CA-34 (backend)**: Rotação/truncamento de JSONL, compactação, mudança de modelo e nova sessão invalidam candidatos antigos; `null` explícito limpa o valor daquela fonte.
