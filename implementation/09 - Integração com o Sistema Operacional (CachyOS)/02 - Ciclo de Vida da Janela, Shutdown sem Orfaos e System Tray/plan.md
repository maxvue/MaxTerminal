# Implementação 02 — Ciclo de Vida da Janela, Shutdown sem Órfãos e System Tray (`plan.md`)

> Plano de execução passo a passo para a **Implementação 02** da Fase 09.
> Especificação técnica correspondente: [`./spec.md`](./spec.md).

---

## 1. Pré-requisitos

1. Implementação 01 concluída (`error.rs`, `settings.rs`, `environment.rs`, `status.rs`, `attention.rs`).
2. Disponibilidade dos contratos da Fase 02 (`shutdown_all` / supervisão `/proc`), Fase 03 (flush/checkpoint WAL) e Fase 07 (`WorkspaceManager::shutdown`).

---

## 2. Etapas de Implementação

### Etapa 2.1 — Ícones da Bandeja (`crates/mt-os/icons/tray/`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/icons/tray/idle.png`
  - `src-tauri/crates/mt-os/icons/tray/active.png`
  - `src-tauri/crates/mt-os/icons/tray/attention.png`
- **Ações**:
  1. Gerar/incluir os 3 ícones PNG RGBA 32×32 px (compatíveis com `libayatana-appindicator` e StatusNotifierItem do KDE Plasma 6 / waybar) representando:
     - `idle.png`: símbolo do MaxTerminal em tom neutro/monocromático;
     - `active.png`: símbolo com indicador verde/teal (`--max-success-500` / `--max-info-500`);
     - `attention.png`: símbolo com ponto de alerta âmbar/vermelho (`--max-danger-500`).

### Etapa 2.2 — Decisão de Fechamento e `ShutdownCoordinator` (`lifecycle.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/lifecycle.rs`
- **Ações**:
  1. Implementar `CloseDecision`, `CloseContext`, `decide_close(ctx)` e `decide_close_action(ctx)`.
  2. Implementar `QuitSource`, `QuitOutcome`, `ShutdownPhase`, `ShutdownReport` e a trait `ShutdownHooks`.
  3. Implementar `ShutdownCoordinator<R>`:
     -Controle de fase via `tokio::sync::Mutex<ShutdownPhase>` e broadcast de conclusão via `tokio::sync::watch`.
     - Em `request_quit`: se já estiver em `Confirming`/`Stopping`/`Draining`, reutilizar a operação em curso (ou promover para `Stopping` se `source == QuitSource::Signal`).
     - Diálogo nativo via `tauri_plugin_dialog::DialogExt` com título `"Sair do MaxTerminal?"`, mensagem em pt-BR informando a quantidade de abas ativas e botões `"Sair e encerrar tudo"` / `"Cancelar"`.
     - Em `shutdown`: executar `freeze_spawns` → `shutdown_workspace` → `shutdown_ptys` → verificar `report.survivors` → `flush_db`.
     - Se `report.survivors` não estiver vazio, transicionar para `ShutdownPhase::Failed`, chamar `unfreeze_spawns`, exibir janela principal, emitir `os://shutdown-failed` e retornar erro `os.shutdown_timeout` (sem chamar `app.exit(0)`).
     - Se `report.survivors` estiver vazio, marcar `exit_authorized = true`, transicionar para `ReadyToExit` e chamar `app.exit(0)`.
  4. Implementar listener de sinais Unix (`SIGTERM`, `SIGINT`, `SIGHUP`) via `tokio::signal::unix::signal` acionando `ShutdownCoordinator::shutdown(QuitSource::Signal)`.

### Etapa 2.3 — Controlador de System Tray e Recovery D-Bus (`tray.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/tray.rs`
- **Ações**:
  1. Implementar `TrayVisualState::from_counters`.
  2. Implementar `TrayController::init`:
     - Construir `Menu` com os itens `tray.toggle`, separador, `tray.count.running`, `tray.count.continuous`, `tray.count.finished`, `tray.count.error`, `tray.count.awaiting` (todos os 5 contadores com `enabled = false` mas sempre presentes no menu), separador, `tray.preferences` e `tray.quit`.
     - Registrar handler `on_menu_event` para despachar `tray.toggle` (`QuakeController::toggle(ToggleSource::Tray)`), `tray.preferences` (mostrar janela + emitir `os://open-preferences`) e `tray.quit` (`ShutdownCoordinator::request_quit(QuitSource::Tray)`).
  3. Implementar loop de debounce de 250 ms (`request_refresh` → `apply`), atualizando textos via `MenuItem::set_text`, título via `tray.set_title` e trocando o ícone via `tray.set_icon` apenas quando `TrayVisualState` mudar.
  4. Implementar monitor D-Bus de `NameOwnerChanged` para `org.kde.StatusNotifierWatcher` / `org.freedesktop.StatusNotifierWatcher`:
     - Quando o watcher sumir com a janela oculta, executar `set_skip_taskbar(false)`, `unminimize()`, `show()`, atualizar `env.tray_available = false` e emitir `os://environment-changed`.

### Etapa 2.4 — Testes Automatizados e Script Anti-Órfãos
- **Arquivos**:
  - `src-tauri/crates/mt-os/tests/close_decision.rs`
  - `scripts/check-orphans.sh`
- **Ações**:
  1. Criar `tests/close_decision.rs` testando exaustivamente as 24 combinações de `decide_close` (`CloseBehavior::{Auto, AlwaysTray, AlwaysQuit}` × `has_active` × `tray_available` × `force_quit`).
  2. Adicionar em `tests/close_decision.rs` testes assíncronos da máquina de estados do `ShutdownCoordinator` com `MockShutdownHooks`:
     - Deduplicação de duas chamadas simultâneas a `request_quit`;
     - Promoção por `QuitSource::Signal` durante `Confirming`;
     - Bloqueio de saída (`exit_authorized == false` e fase `Failed`) quando `shutdown_ptys` retorna `survivors: vec![12345]`;
     - Autorização (`exit_authorized == true` e fase `ReadyToExit`) quando `survivors` é vazio.
  3. Criar `scripts/check-orphans.sh` com permissão de execução (`chmod +x`), registrando `(pid, starttime)` de `/proc/<pid>/stat` de toda a subárvore antes do encerramento e validando ausência total até 5 s após o término.

---

## 3. Checklist de Validação

- [ ] `decide_close()` é 100% pura e passa nos 24 casos da tabela-verdade.
- [ ] `WindowEvent::CloseRequested` chama `api.prevent_close()` e delega para `decide_close()`.
- [ ] `RunEvent::ExitRequested` chama `api.prevent_exit()` sempre que `exit_authorized == false`, mesmo quando `code: Some(_)` for informado.
- [ ] Ocultar a janela para a bandeja não invoca `ShutdownHooks` nem interrompe PTYs/servidores.
- [ ] `ShutdownCoordinator` impede `app.exit(0)` se houver qualquer PID sobrevivente no `ShutdownReport`.
- [ ] `TrayController` aplica debounce de 250 ms e só chama `set_icon` quando `TrayVisualState` muda.
- [ ] A perda do `StatusNotifierWatcher` com a janela oculta restaura imediatamente a janela acessível ao usuário.

---

## 4. Comandos de Verificação (Mapeados para `CA-*`)

```bash
# CA-01: Lint sem warnings
cd src-tauri && cargo clippy -p mt-os --all-targets -- -D warnings

# CA-02 (parcial): Tabela-verdade de decide_close e testes de ShutdownCoordinator + TrayVisualState
cd src-tauri && cargo test -p mt-os --test close_decision
cd src-tauri && cargo test -p mt-os tray::tests

# CA-09 / CA-11 / CA-14: Validação do script de checagem de órfãos
bash -n scripts/check-orphans.sh
```
