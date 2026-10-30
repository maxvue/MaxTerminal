# Implementação 03 — Single Instance, Deep Link, Atalho Global e Quake Mode (`plan.md`)

> Plano de execução passo a passo para a **Implementação 03** da Fase 09.
> Especificação técnica correspondente: [`./spec.md`](./spec.md).

---

## 1. Pré-requisitos

1. Implementações 01 e 02 concluídas (`settings.rs`, `environment.rs`, `lifecycle.rs`, `tray.rs`).
2. Dependências `ashpd`, `url`, `uuid` e `tauri-plugin-global-shortcut` declaradas em `crates/mt-os/Cargo.toml`.

---

## 2. Etapas de Implementação

### Etapa 2.1 — Parser de CLI, Single-Instance e Handshake de Startup (`cli.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/cli.rs`
- **Ações**:
  1. Implementar `CliAction` (`Show`, `Toggle`, `Hide`, `StartMinimized`, `None`) e `parse_cli_args` com whitelist estrita e prioridade `--show > --toggle > --hide > --minimized`.
  2. Implementar `EarlyCliInbox` limitada (`MAX_PENDING = 16`) para armazenar ações recebidas pelo callback de `single-instance` antes da conclusão do `setup` do Tauri.
  3. Implementar o controlador de prontidão da UI (`UiReadinessState`):
     - Armazena a intenção inicial (`initial_visible: bool`), decidida a partir de `parse_cli_args(std::env::args())`, `settings.autostart`/`settings.start_minimized` e `env.tray_available`.
     - Quando `os_ui_ready` é chamado pelo frontend, aplica a visibilidade inicial de forma idempotente.
     - Inicia watchdog de 3 s no startup para mostrar a janela caso a intenção inicial seja visível e o frontend atrase o handshake.
  4. Implementar `handle_second_instance`: ignorar `cwd`, ignorar invocações que já são tratadas pelo `deep-link` (`maxterminal://`) e despachar `Toggle`, `Show` ou `Hide` sem debounce de repetição de tecla.

### Etapa 2.2 — Roteador de Ativação e Deep Links (`activation.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/activation.rs`
- **Ações**:
  1. Implementar `ActivationSource`, `ActivationEnvelope`, a trait `ActivationHandler` e `ActivationRouter`.
  2. Implementar `ActivationRouter::parse_and_validate`:
     - Verificar tamanho `<= 8192` bytes, ausência de caracteres de controle/NUL e validade de escapes `%XX`.
     - Validar `Url::parse` com `scheme() == "maxterminal"`, sem `username`, `password`, `port` ou `fragment`.
  3. Implementar fila limitada (`MAX_QUEUE_LEN = 32`), expiração (`ENVELOPE_TTL = 30 s`) e janela de deduplicação de transporte (`1 s` para a mesma URI).
  4. Implementar `register_handler`, `enqueue` e `set_ready`, retornando `os.activation_busy`, `os.activation_invalid` ou `os.activation_unavailable` conforme o caso, sem nunca logar query strings.

### Etapa 2.3 — Quake Mode e Geometria Drop-Down (`quake.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/quake.rs`
- **Ações**:
  1. Implementar `ToggleDecision`, `ToggleDecisionDto`, `ToggleSource` e a função pura `decide_toggle(visible, focused, minimized)`.
  2. Implementar `DropDownGeometry`, `drop_down_geometry(monitor_pos, monitor_size, height_pct)` e o alias `compute_quake_bounds`.
  3. Implementar `QuakeController<R>`:
     - Debounce de 150 ms restrito a `ToggleSource::{Shortcut, Portal}`;
     - Salvar e restaurar `SavedNormalWindowState` ao alternar entre `QuakeMode::Normal` e `QuakeMode::DropDown` no X11;
     - Posicionar no monitor sob o cursor em X11 e degradar sem erro em Wayland;
     - Implementar `on_blur` com atraso de 120 ms e guarda `modal_interaction_guard` para não ocultar a janela ao abrir diálogos nativos, portal ou Preferências.

### Etapa 2.4 — Aceleradores e Backends de Atalho Global (`shortcut/`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/shortcut/mod.rs`
  - `src-tauri/crates/mt-os/src/shortcut/accelerator.rs`
  - `src-tauri/crates/mt-os/src/shortcut/plugin_backend.rs`
  - `src-tauri/crates/mt-os/src/shortcut/portal_backend.rs`
  - `src-tauri/crates/mt-os/src/shortcut/cli_backend.rs`
- **Ações**:
  1. Em `accelerator.rs`: implementar `Accelerator::parse`, `to_canonical_string`, `to_plugin_string` e `to_portal_trigger` (mapeando `Backquote` ↔ `grave`, `Ctrl` ↔ `Control`/`CTRL`, etc.).
  2. Em `plugin_backend.rs`: registrar atalho no X11 via `tauri-plugin-global-shortcut` acionando apenas em `ShortcutState::Pressed` e mapeando falhas de grab para `os.shortcut_conflict`.
  3. Em `portal_backend.rs`: integrar `ashpd::desktop::global_shortcuts::GlobalShortcuts` com sessão persistente cancelável, ID `"quake-toggle"` e captura do `effective_trigger` retornado pelo compositor.
  4. Em `cli_backend.rs`: gerar `ShortcutStatus` com `backend = CliOnly`, `cli_command = "maxterminal --toggle"` e `compositor_hint` para Hyprland e KDE Plasma 6.
  5. Em `shortcut/mod.rs`: implementar `ShortcutManager::apply` com rollback automático para o atalho anterior caso o novo registro falhe.

### Etapa 2.5 — Testes Unitários e de Integração (`tests/cli.rs`, `tests/activation.rs`, `tests/toggle.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/tests/cli.rs`
  - `src-tauri/crates/mt-os/tests/activation.rs`
  - `src-tauri/crates/mt-os/tests/toggle.rs`
- **Ações**:
  1. Em `tests/cli.rs`: validar whitelist de argumentos, descarte de flags desconhecidas e prioridade (`--minimized --toggle` ⇒ `Toggle`; `--toggle --show` ⇒ `Show`).
  2. Em `tests/activation.rs`: validar cold/warm start com deduplicação de 1 s, fila de até 32 itens antes de `set_ready()`, erro `os.activation_busy` no 33º item, expiração por TTL de 30 s, rejeição de scheme != `maxterminal`, credenciais na URL, escapes `%ZZ` inválidos e despacho para um `FakeActivationHandler`.
  3. Em `tests/toggle.rs`: validar as 8 combinações de `decide_toggle` e o cálculo de `drop_down_geometry` / `compute_quake_bounds` com offsets negativos, múltiplos monitores e alturas de 20% a 100%.
  4. Em `shortcut/accelerator.rs`: testar parse e formatação plugin/portal para aceleradores válidos e rejeição de entradas sem modificador (`"F12"`), vazias (`""`), incompletas (`"Ctrl+"`) ou inválidas (`"Foo+A"`).

---

## 3. Checklist de Validação

- [ ] `parse_cli_args` ignora `argv[0]` e qualquer flag fora da whitelist sem ecoar valores nos logs.
- [ ] `ActivationRouter` deduplica a mesma URI entregue em janela de 1 s e limita a fila a 32 itens com TTL de 30 s.
- [ ] `decide_toggle` cobre todas as 8 combinações de `(visible, focused, minimized)`.
- [ ] `drop_down_geometry` calcula largura e altura corretas sem aplicar fator de escala em duplicidade e suporta coordenadas negativas.
- [ ] `ShortcutManager` usa portal ou `CliOnly` em `Wayland` e `XWayland` (nunca confia em grab X11 isolado dentro de sessão Wayland).
- [ ] Falha ao registrar um novo atalho restaura o atalho anterior e retorna `os.shortcut_conflict` ou `os.shortcut_invalid`.

---

## 4. Comandos de Verificação (Mapeados para `CA-*`)

```bash
# CA-01: Lint estrito
cd src-tauri && cargo clippy -p mt-os --all-targets -- -D warnings

# CA-02 / CA-36: Testes de CLI, ActivationRouter, Toggle, Geometria e Accelerator
cd src-tauri && cargo test -p mt-os --test cli --test activation --test toggle
cd src-tauri && cargo test -p mt-os shortcut::accelerator::tests
```
