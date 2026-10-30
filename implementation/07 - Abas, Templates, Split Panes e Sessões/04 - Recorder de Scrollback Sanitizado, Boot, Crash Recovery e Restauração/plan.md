# Plano de Execução — Implementação 04: Recorder de Scrollback Sanitizado, Boot, Crash Recovery e Restauração

## 1. Checklist Sequencial de Implementação

### Passo 1: Decoder UTF-8 Incremental, Parser ANSI e Sanitização no `ScrollbackRecorder`
1. Criar `src-tauri/src/workspace/recorder.rs` implementando a máquina de estados por execução (`ExecutionRecorderState`):
   - Buffer de continuação UTF-8 para bytes multibyte cortados entre frames.
   - Parser incremental de sequências ANSI (CSI, OSC, DCS) e detecção de entrada/saída de alternate screen (`?47`, `?1047`, `?1049`) emitindo `ALT_SCREEN_MARKER` uma única vez por ativação.
   - Tratamento de `\r` (carriage return sem `\n`) sobrescrevendo a linha pendente atual.
   - Acumulador de linha até `\n` com teto seguro de 16 KiB (`MAX_PENDING_LINE_BYTES`): linhas acima de 16 KiB são descartadas até o próximo `\n` com `TRUNCATED_LINE_MARKER`, sem partir contexto antes do `StreamSanitizer` da Fase 03.

### Passo 2: Orçamento por Sessão (`TabSessionId`), Fila Limitada e Barreira `finish_execution`
1. Implementar em `ScrollbackRecorder`:
   - `OutputTap::on_output` não bloqueante com fila global limitada a 8 MiB (`GLOBAL_CAPTURE_QUEUE_MAX_BYTES`) e tratamento de overflow descartando fragmentos nas bordas da lacuna e inserindo `OVERFLOW_GAP_MARKER` (`ScrollbackChunkKind::Gap`).
   - `GroupScrollbackBudget` compartilhado entre todos os painéis do mesmo `TabSessionId` (padrão 8 MiB lido de `workspace.scrollback.limit_mib`), emitindo `BUDGET_EXCEEDED_MARKER` ao atingir o teto.
   - Integração com `BatchWriter` da Fase 03 (200 ms / 512 itens / 1 MiB, chunks `<= 64 KiB` de `SanitizedText`).
   - Método `finish_execution(pty_id)` aguardando EOF da execução no worker e `BatchWriter::flush()` antes de retornar ao `TabActor`.

### Passo 3: Crash Recovery e Restauração Single-Flight (`workspace/restore.rs`)
1. Criar `src-tauri/src/workspace/restore.rs` implementando a rotina de inicialização e `WorkspaceManager::restore()`:
   - Pré-condição: `with_spawn_hooks` aplicado antes de compartilhar o manager/iniciar o boot. Cada painel restaurado passa por reserva e registro confirmado de `07/03` §3.4; testar com hook fake bloqueando o spawn até a reserva e com falha de registro exigindo compensação.
   - Etapa 1: Limpeza de processos órfãos via Fase 02 e registro do `OutputTap` e `WorkspaceSessionProvider`.
   - Etapa 2: `mark_stale_boots_interrupted` em `workspace_operations`, fechamento de blocos pendentes da Fase 05 como `unknown`, arquivamento de sessões base órfãs e fechamento de `tab_sessions` anteriores com `SessionEndReason::CrashRecovered` e `ended_at >= max(started_at, last_ts)`.
   - Etapa 3: Marcação de abas abertas de boots anteriores como fechadas (`closed_at = now_ms`) e verificação/reparo de `ensure_default_tx` para todos os projetos.
   - Etapa 4: Single-flight via `tokio::sync::OnceCell<WorkspaceRestoreReport>` restaurando templates com `add_to_workspace = 1` (até 4 abas em paralelo), reutilizando a aba `origin = 'workspace'`, seus `PaneId`s e `layout_json` quando válidos, tratando `cwd` ausente como painel `Failed` recuperável e habilitando o comando inicial apenas no `primary_pane_id`.

### Passo 4: Integração no `WorkspaceManager` e Testes Exaustivos de Recorder e Restore
1. Declarar `pub mod recorder;` em `src-tauri/src/workspace/mod.rs`, adicionar o campo `recorder: Arc<ScrollbackRecorder>` ao `WorkspaceManager` (injeção no construtor/builder; a Implementação 03 compila e é testada sem ele) e conectar `recorder.finish_execution(pty_id)` ao fluxo de `tab_close`, `pane_close`, `pane_restart` e `session_new` em `src-tauri/src/workspace/manager.rs`.
2. Escrever testes unitários e de integração cobrindo:
   - Cortes em todos os limites de bytes para: UTF-8 de 2/3/4 bytes, sequências CSI/OSC, segredo `Bearer` / `APP_KEY`, bloco PEM multi-linha (completo e truncado no EOF), linha `> 16 KiB`, alternate screen (`?1049h` ... `?1049l`).
   - Orçamento global de 8 MiB por `TabSessionId` dividido entre 4 painéis simultâneos (garantindo que a soma total dos 4 painéis nunca ultrapasse 8 MiB).
   - Rajada de 200 MB de output em um PTY verificando que a fila limitada aplica overflow determinístico sem bloquear o PTY e sem crescer memória indefinidamente.
   - Duas chamadas simultâneas a `restore()` no mesmo boot retornando os mesmos `TabId`/`TabSessionId`/`PtyId` e disparando o comando inicial no máximo uma vez.
   - Simulação de crash com sessão ativa, bloco running e operação em estágio `prepared`/`stopping`/`committed`, verificando o estado após o próximo boot (`crash_recovered`, `interrupted`, zero `exit_code = 0` fictício).

---

## 2. Comandos de Verificação

```bash
cd "$(git rev-parse --show-toplevel)"

# Formatação e análise estática
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings

# Testes do Recorder e de Boot/Restore
cargo test --manifest-path src-tauri/Cargo.toml --bin maxterminal workspace::recorder
cargo test --manifest-path src-tauri/Cargo.toml --bin maxterminal workspace::restore
```

---

## 3. Critérios de Aceite desta Implementação

- [ ] **CA-04.1 (Sanitização em Streaming e Limites do Recorder):** Segredos divididos entre frames, UTF-8 fragmentado, sequências ANSI/alt-screen, linhas `> 16 KiB`, blocos PEM incompletos, overflow da fila e término abrupto nunca vazam texto sensível nem escapes ativos para `session_scrollback` ou `session_scrollback_fts`; o limite de 8 MiB é respeitado por `TabSessionId` inteiro (não multiplicado por painel).
- [ ] **CA-04.2 (Crash Recovery Determinístico):** Após encerramento abrupto, o boot seguinte conclui blocos pendentes da Fase 05 como desconhecidos (sem inventar `exit 0`), arquiva sessões anteriores com `crash_recovered`, marca operações pendentes do ledger como `interrupted` e limpa processos remanescentes via Fase 02 antes de novos spawns.
- [ ] **CA-04.3 (Restauração Automática Idempotente):** Templates com `add_to_workspace = true` abrem exatamente uma aba `origin = 'workspace'` por template preservando `TabId`, `PaneId`s e layout válidos; abas manuais não são reabertas; dois pedidos de restore ou reload da WebView no mesmo boot retornam os mesmos IDs e não reexecutam comandos iniciais.
