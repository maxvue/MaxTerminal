# Plano de Execução — Implementação 03: Tracker de Execução, Captura Sanitizada, Fallback `/proc` e Máquina de Estados das Abas

> **Fase:** 05 — Shell Integration OSC 133 e Máquina de Estados das Abas
> **Implementação:** `03 - Tracker de Execução, Captura Sanitizada, Fallback proc e Máquina de Estados`
> **Especificação de referência:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

- [ ] **Passo 1: Implementar o Classificador Contínuo (`state/daemon.rs`) e a Máquina de Estados Pura (`state/machine.rs`)**
  1. Criar `src-tauri/crates/mt-shell/src/state/daemon.rs` com `ContinuousDetector::classify`, implementando tokenização conservadora sem regex com look-around, regras de exclusão (`build`, `--help`, `--version`, `docker compose up -d`/`--detach`) e regras positivas (`npm/pnpm/yarn/bun run dev`, `vite dev`, `php artisan serve|horizon|queue:work`, `cargo watch`, `tail -f`, servidores Python).
  2. Criar `src-tauri/crates/mt-shell/src/state/machine.rs` com `PaneState`, `StateInput`, `StateChange` e a função pura `transition(state, input, next_ordinal) -> Option<StateChange>`.
  3. Implementar o cálculo de `is_seen()` por igualdade exata de `ExecutionRef` (`seen_execution == Some(completion.execution)`) e `badge_bucket()` seguindo a precedência estrita da §6.4.

- [ ] **Passo 2: Implementar o `TabStateRegistry` (`state/registry.rs`) e Testes da Tabela de Estados (`tests/state_table.rs`)**
  1. Criar `src-tauri/crates/mt-shell/src/state/registry.rs` gerenciando o mapa de abas, painéis, tombstones e revisões monotônicas, mais o rastreamento de foco por janela (`set_window_focus`) e validação completa de `mark_seen`.
  2. Implementar `counts_for(tab_ids)` deduplicando `TabId`s e ignorando abas removidas, e `has_active_processes()` fazendo OR de todos os painéis ativos em `Running | Continuous` mesmo quando outro painel da aba possui erro não visto.
  3. Criar `src-tauri/crates/mt-shell/tests/state_table.rs` cobrindo todas as linhas da tabela de estados da §6.1:
     - `sleep 2` → `running`; conclusão com exit 0 → `finished` (`seen=false`, bucket `finished`); ACK da geração atual → `seen=true`, bucket `idle` preservando `status=finished` e `lastExitCode=0`.
     - Falha (`exit != 0`, inclusive `130` de `Ctrl+C`) → `error` (`seen=false`, bucket `error`).
     - Conclusão com `exitCode = None` → `idle`, `outcome = unknown`, sem badge de sucesso ou erro.
     - ACK de geração antiga ou enviado durante `running` é ignorado e não confirma a conclusão futura.
     - Foco mantido durante toda a execução não marca `seen=true` sozinho até a chegada do `SeenRequest` com a revisão da conclusão.
     - Agregação determinística de múltiplos painéis (ex.: Pane 1 com erro não visto + Pane 2 rodando `npm run dev` → `badgeBucket = error` e `hasActiveProcesses = true`).

- [ ] **Passo 3: Implementar o `CommandTracker` (`tracker.rs`) e a Captura Sanitizada (`capture.rs`)**
  1. Criar `src-tauri/crates/mt-shell/src/tracker.rs` com `CommandTracker`, mapeamento separado `hook_generation -> ExecutionRef.generation`, segmentação intra-chunk (`texto anterior → C → saída do bloco → D → prompt`), tratamento de `C` duplicado, `C` de nova geração sem `D` (`MissingEnd`), `A` após execução sem `D` (`MissingEnd`) e preservação do `cwd` capturado antes de `C`.
  2. Criar `src-tauri/crates/mt-shell/src/capture.rs` integrando `vte::Parser` (remoção de CSI/OSC/DCS e tratamento de `\r` isolado) com `mt_db::sanitizer::StreamSanitizer` **antes** do recorte em `head` (16 KiB), `tail` (48 KiB) e `error_lines` (200 × 4 KiB).
  3. Implementar `BlockCapture::record_gap` chamando `sanitizer.on_gap()` e descartando fragmentos parciais até fronteira segura.
  4. Criar `src-tauri/crates/mt-shell/tests/tracker.rs` validando:
     - Segmentação de múltiplos comandos e saídas dentro de um único frame de bytes.
     - **Fallback `/proc` (T-07):** (a) `IntegrationMode::Proc` + bloco ativo + `on_pty_exit(Some(1))` → `BlockFinished { status: Failed, exit_code: Some(1), reason: PtyExited }`, `TabStatus::Error` e `error_context` aceito (sem `shell.block_not_failed`); (b) `Proc` + bloco ativo + `on_pty_exit(None)` e `on_pty_exit(Some(0))` → `Unknown` com `exit_code: None`, aba `Idle`; (c) `IntegrationMode::Hooks` + bloco ativo sem `D` + `on_pty_exit(Some(1))` → continua `Unknown` (código do shell não é atribuído ao filho); (d) `on_pty_exit` após bloco já concluído não altera o resultado.
     - Segredo (`Authorization: Bearer abcdef123456` e chave PEM) partido exatamente na fronteira de chunk, na fronteira de 16 KiB do `head`, na fronteira de 48 KiB do `tail` e ao redor de um `Gap`: zero vazamento em `output_text` e `error_tail`.

- [ ] **Passo 4: Implementar o Monitor `/proc` (`fallback.rs`), Mailbox Limitada (`runtime.rs`), Persistência (`persist.rs`) e `ShellService` (`service.rs`)**
  1. Criar `src-tauri/crates/mt-shell/src/fallback.rs` com timer de 5 s para ausência de handshake (`Pending → Proc`) e loop de 250 ms consultando `PtyManager::process_report`/`foreground_process`, emitindo execuções inferidas (`source = Proc`, `confidence = Inferred`, `exit_code = None` enquanto o PTY vive) e encerrando com `ModeSwitched` caso o handshake chegue tardiamente. Em `IntegrationMode::Proc`, `CommandTracker::on_pty_exit(exit_code, ..)` finaliza o bloco ativo como `BlockStatus::Failed` com o `exit_code` do PTY quando `!= 0` (`BlockEndReason::PtyExited`, aba em `error`, "Inspecionar" habilitado); sem informação de código o bloco continua `Unknown`.
  2. Criar `src-tauri/crates/mt-shell/src/runtime.rs` com `SessionProvider`, `PtyScopedSessionProvider` e `PtySequencerMailbox` (limite de 8 MiB e 4.096 mensagens + 64 controles reservados + 1 slot terminal para `Exit`/`Overflow`).
  3. Criar `src-tauri/crates/mt-shell/src/persist.rs` com worker assíncrono limitado, cache LRU sanitizado (32 blocos / 4 MiB globais) e gravação via `SessionsRepo` e `CommandBlocksRepo`.
  4. Criar `src-tauri/crates/mt-shell/src/service.rs` integrando `ShellService::prepare_spawn`, `SpawnLifecycle`, `OutputTap` (mantendo o `TapHandle` vivo no serviço), `error_context(block_id, max_lines)` e `shutdown()`.
  5. Preservar `ExitInfo.code` no slot terminal e repassá-lo a `CommandTracker::on_pty_exit`; aplicar `CommandFinished` e finalizar captura antes de `PtyExited`, mantendo a conclusão falha após `alive=false`. Testar o fluxo completo lifecycle → mailbox → tracker → registry → contexto sanitizado, não apenas a função isolada.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificar compilação de todo o crate mt-shell
cargo check -p mt-shell --all-targets

# 2. Executar testes da máquina de estados, detector contínuo e agregação
cargo test -p mt-shell --test state_table -- --nocapture

# 3. Executar testes do tracker, segmentação intra-chunk, captura sanitizada e overflow
cargo test -p mt-shell --test tracker -- --nocapture

# 4. Executar toda a suíte do mt-shell
cargo test -p mt-shell

# 5. Validar clippy sem warnings
cargo clippy -p mt-shell --all-targets -- -D warnings
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 05)

- [x] **CA-PAR-03 (instructions.md §5):** Marcas sem nonce, nonce errado, `g` antigo, metadado duplicado ou forjado não mudam estado autoritativo.
- [x] **CA-SHL-05 (instructions.md §5):** Opt-out/shell incompatível → `proc` imediatamente; sem handshake → após 5 s; chegada tardia de handshake reconcilia com `ModeSwitched` sem dupla conclusão.
- [x] **CA-SHL-06 (T-07):** Em modo `Proc`, PTY encerrado com `exit_code != 0` durante bloco ativo → bloco `Failed` com esse código, aba `error` e `command_block_error_context` habilitado; sem informação de código permanece `Unknown`/`idle`.
- [x] **CA-EST-01 a CA-EST-06 (instructions.md §5):** Tabela da spec §6 coberta integralmente; `sleep 2` → verde, sucesso não visto → âmbar, falha não vista → vermelho, visto → neutro preservando resultado; foco contínuo exige ACK pós-D; `exitCode=null` → `unknown`/`idle`; `npm run dev` → contínuo e `vite build`/`docker compose up -d` não promovem; agregação determinística e `hasActiveProcesses` por OR de todos os panes.
- [x] **CA-CAP-01 a CA-CAP-04 (instructions.md §5):** Segredos partidos entre chunks/head/tail/gaps nunca vazam; `echo 'Authorization: Bearer abcdef123456'; false` produz `ErrorContext` mascarado sem ANSI (padrão 50 linhas, máx. 200); mailbox limitada a 8 MiB e 4.096 msgs não congela o PTY; falha de DB reporta `persistence=failed` e serve contexto do cache sanitizado.
