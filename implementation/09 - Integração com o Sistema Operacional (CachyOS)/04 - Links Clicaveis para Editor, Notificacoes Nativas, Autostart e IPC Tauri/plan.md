# Implementação 04 — Links Clicáveis para Editor, Notificações Nativas, Autostart e IPC Tauri (`plan.md`)

> Plano de execução passo a passo para a **Implementação 04** da Fase 09.
> Especificação técnica correspondente: [`./spec.md`](./spec.md).

---

## 1. Pré-requisitos

1. Implementações 01, 02 e 03 concluídas no crate `mt-os`.
2. Contratos das Fases 02 (`mt-pty`), 03 (`mt-db`), 05 (`mt-shell`) e 07 (`WorkspaceManager`) disponíveis no workspace Cargo.

---

## 2. Etapas de Implementação

### Etapa 2.1 — Resolvedor de Links (`crates/mt-os/src/links.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/links.rs`
- **Ações**:
  1. Implementar `LinkCandidate`, `ResolvedLink`, `LinkContext` e a trait `TabCwdProvider`.
  2. Implementar `LinkResolver::join(cwd, home, raw)` puro para caminhos absolutos, `~`, `~/...`, `./`, `../` e relativos, rejeitando `~outro_usuario`, NUL/controles e URLs.
  3. Implementar `LinkResolver::resolve(context, candidates)`:
     - Validar `candidates.len() <= 64` (`MAX_BATCH`), retornando `os.link_rejected` se exceder.
     - Validar `line`/`column` (`1..=10_000_000`, `column` exige `line`).
     - Consultar `cwd_provider.cwd(context)` antes e depois do pipeline `buffer_unordered(16)` de `canonicalize` + `metadata`, retornando `os.link_context_stale` se o contexto mudou.
     - Filtrar apenas arquivos regulares (`metadata.is_file()`).

### Etapa 2.2 — Construtor e Abridor de Editores (`crates/mt-os/src/editor.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/editor.rs`
- **Ações**:
  1. Implementar `EditorInvocation` e `build_editor_invocation(settings, link)` cobrindo `VsCode`, `VsCodeInsiders`, `MaxCode` (`vscode://file/` com percent-encoding que preserva `/`) e `Custom` (substituição de `{file}`, `{line}`, `{column}` por elemento, sem shell).
  2. Implementar a trait `EditorOpener` e `EditorLauncher` (`register_opener`, `set_bridge_opener`, `open`, `detect` via `which::which("code")`, `which::which("code-insiders")` e `which::which("xdg-open")`). O setter substitui slot prioritário idempotentemente; testar registro repetido, fallback e ausência de locks durante `await`.
  3. Garantir que `Exec` use `tokio::process::Command` com `stdin/stdout/stderr` nulos e task de reap assíncrona.

### Etapa 2.3 — Notificações Nativas, Rate Limiter e Comando Longo (`notify.rs`, `long_command.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/notify.rs` (e re-export `pub use notify as notifications;` em `lib.rs`)
  - `src-tauri/crates/mt-os/src/long_command.rs`
- **Ações**:
  1. Implementar `RateLimiter` com dedupe de 10 s (`LimitDecision::Replace`) e teto global de 5 notificações por 30 s (`LimitDecision::Suppress`) com relógio `Instant` injetável para testes determinísticos.
  2. Implementar `Notifier::notify_user`:
     - Sanitizar título e corpo com `mt_db::sanitizer::sanitize()`, escapar `<`/`&` e truncar em 80/200 caracteres;
     - Enviar via D-Bus `org.freedesktop.Notifications` (`notify-rust` / `zbus`) com `appname("MaxTerminal")`, `icon("maxterminal")` e ação `"default"` = `"Abrir aba"` quando `env.notification_actions == true`;
     - Manter stream compartilhado cancelável de `ActionInvoked` e `NotificationClosed` com mapa limitado a 32 entradas e TTL de 5 minutos, emitindo `os://focus-tab` ao clicar.
  3. Implementar `CommandFinishedInfo` e `long_command::should_notify` pura em `long_command.rs`.

### Etapa 2.4 — Autostart e Fachada `OsIntegration` (`autostart.rs`, `lib.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/src/autostart.rs`
  - `src-tauri/crates/mt-os/src/lib.rs`
- **Ações**:
  1. Implementar `AutostartSync` (`apply`, `status`, reconciliação no startup e gravação atômica de `~/.config/autostart/maxterminal.desktop` respeitando `start_minimized` com `--start-hidden` apenas quando ativo).
  2. Implementar `OsDeps`, `OsRuntimeSnapshot` e `OsIntegration<R>` em `crates/mt-os/src/lib.rs`, incluindo `update_settings` serializado com compensação automática (rollback de atalho/autostart/geometria se a gravação no SQLite falhar).

### Etapa 2.5 — Adaptadores, Comandos IPC, Eventos e Capabilities no Binário (`src-tauri/src/`)
- **Arquivos**:
  - `src-tauri/src/adapters/os.rs`
  - `src-tauri/src/events/os.rs`
  - `src-tauri/src/commands/os.rs`
  - `src-tauri/src/lib.rs`
  - `src-tauri/build.rs`
  - `src-tauri/permissions/os.toml`
  - `src-tauri/capabilities/os.json`
  - `src-tauri/capabilities/default.json`
- **Ações**:
  1. Em `adapters/os.rs`: implementar `TabStatusProvider`, `TabCwdProvider` (mantendo `cwd_epoch` por painel e emitindo `os://link-context-changed`) e `ShutdownHooks` integrando Fases 02, 03, 05 e 07.
  2. Em `events/os.rs`: definir `OsEvent<T>` e helpers de emissão com revisão monotônica.
  3. Em `commands/os.rs`: implementar os 17 comandos `os_*` com validação de janela `"main"` e exportação `tauri-specta`.
  4. Em `src-tauri/src/lib.rs`: registrar `tauri_plugin_single_instance` como **primeiro** plugin (com feature `deep-link`), seguido de `tauri_plugin_deep_link`, `tauri_plugin_dialog`, `tauri_plugin_opener`, `tauri_plugin_global_shortcut`, `tauri_plugin_notification` e `tauri_plugin_autostart`; inicializar `OsIntegration` no `setup` e conectar `on_window_event` e `RunEvent::ExitRequested`.
  5. Configurar `build.rs`, `permissions/os.toml`, `capabilities/os.json` e `capabilities/default.json`.

### Etapa 2.6 — Suítes de Testes (`tests/links.rs`, `tests/editor_argv.rs`, `tests/rate_limit.rs`, `tests/capabilities.rs`)
- **Arquivos**:
  - `src-tauri/crates/mt-os/tests/links.rs`
  - `src-tauri/crates/mt-os/tests/editor_argv.rs`
  - `src-tauri/crates/mt-os/tests/rate_limit.rs`
  - `src-tauri/tests/capabilities.rs`
- **Ações**:
  1. Em `tests/links.rs`: criar ≥ 25 casos cobrindo caminho relativo, absoluto, `~`, `./`, `../`, symlink válido, diretório (rejeitado), arquivo inexistente (rejeitado), linha/coluna 0, overflow `> 10_000_000`, coluna sem linha, lote de 65 itens (`os.link_rejected`) e mudança de `cwd_epoch` durante resolução (`os.link_context_stale`).
  2. Em `tests/editor_argv.rs`: testar os 4 editores com caminhos contendo espaços, `#`, `%`, `:` e acentos (`ç`, `ã`), além da rejeição de `custom_argv` sem `{file}`.
  3. Em `tests/rate_limit.rs`: testar dedupe de 20 chamadas idênticas em 2 s (1 envio + 19 `Replace`) e 20 chamadas distintas em 2 s (5 `Allow` + 15 `Suppress`).
  4. Em `tests/capabilities.rs`: validar que `os.json` restringe acesso à janela `"main"` local e não concede permissões diretas de plugins ao frontend.

---

## 3. Checklist de Validação

- [ ] `LinkResolver::resolve` limita o lote a 64 candidatos, usa `buffer_unordered(16)` e valida `cwd_epoch` antes e depois do I/O.
- [ ] `os_link_open` re-resolve o candidato no backend antes de invocar o editor.
- [ ] Nenhum comando de editor utiliza `sh -c`; `vscode://file/` codifica `#`, `%`, `?`, `:` e espaços preservando `/`.
- [ ] `Notifier::notify_user` sanitiza segredos com `mt_db::sanitizer::sanitize()` antes de enviar ao D-Bus.
- [ ] Os 17 comandos `os_*` estão exportados no `tauri-specta` e protegidos por `allow-os-integration` em `permissions/os.toml`.

---

## 4. Comandos de Verificação (Mapeados para `CA-*`)

```bash
# CA-01: Lint estrito de mt-os e do binário
cd src-tauri && cargo clippy -p mt-os --all-targets -- -D warnings

# CA-02: Testes completos de links (≥ 25 casos), editor_argv, rate_limit e long_command
cd src-tauri && cargo test -p mt-os --test links --test editor_argv --test rate_limit
cd src-tauri && cargo test -p mt-os long_command::tests

# CA-06: Geração e verificação de bindings TypeScript
npm run bindings && git diff --exit-code src/types/bindings.ts

# CA-37: Verificação de capabilities e ACL
cd src-tauri && cargo test --test capabilities
```
