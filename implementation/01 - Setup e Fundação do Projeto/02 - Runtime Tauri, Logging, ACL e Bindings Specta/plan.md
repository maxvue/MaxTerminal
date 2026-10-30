# Implementação 02 — Runtime Tauri, Logging, ACL e Bindings Specta (`plan.md`)

> Especificação técnica desta etapa: [`./spec.md`](./spec.md)
> Fase: [`../instructions.md`](../instructions.md) | [`../spec.md`](../spec.md)

---

## 1. Pré-requisitos e Regras Operacionais

- A Implementação 01 (`mt-core` e `src-tauri/Cargo.toml`) deve estar concluída e passando em `cargo test -p mt-core`.
- Executar comandos Cargo sempre dentro de `src-tauri/`.
- Não utilizar `core:default` nas capabilities nem expor comandos extras de teste no handler de produção.
- Garantir que `src/types/bindings.ts` seja gerado deterministicamente sem abrir janela gráfica (`DISPLAY` ausente) e sem iniciar servidor Vite.

---

## 2. Checklist Passo a Passo de Execução

### Passo 1 — Build Script, ACL, Capabilities, Ícones e `tauri.conf.json`
- [ ] **1.1.** Criar `src-tauri/build.rs` declarando `tauri_build::AppManifest::new().commands(&["app_info", "app_log_frontend"])` em `tauri_build::Attributes::new().app_manifest(...)`.
- [ ] **1.2.** Criar `src-tauri/permissions/default.toml` e `src-tauri/capabilities/default.json` vinculando o identificador `"foundation"` exclusivamente à janela `"main"` e às permissões `"allow-app-info"` e `"allow-app-log-frontend"`.
- [ ] **1.3.** Criar `src-tauri/tauri.conf.json` com `productName: "MaxTerminal"`, `identifier: "com.johnattas.maxterminal"`, `devUrl: "http://127.0.0.1:5177"`, janela `main` (`1440x900`, min `960x600`), `csp` de release restritiva, `devCsp` restrita a `127.0.0.1:5177` e `bundle.active: false`.
- [ ] **1.4.** Garantir a presença dos ícones mínimos em `src-tauri/icons/` exigidos pelo `tauri::generate_context!()` em tempo de compilação.

### Passo 2 — Estado, Logging Estruturado e Comandos IPC
- [ ] **2.1.** Implementar `src-tauri/src/state.rs` com `AppState` e `FrontendLogState` (rate-limit monotônico de 200 eventos por segundo com contagem de descartes).
- [ ] **2.2.** Implementar `src-tauri/src/logging.rs` com `LogRedactor` (assinatura de 1 parâmetro `redact<'a>(&self, input: &'a str) -> Cow<'a, str>`), `FoundationRedactor`, `RedactedJsonFormatter`, abertura de arquivo diário `maxterminal.log.<AAAA-MM-DD>` com modo `0o600`, `NonBlockingBuilder` com `buffered_lines_limit(4096)`, filtro `MAXTERMINAL_LOG` restrito aos targets autorizados e panic hook estático.
- [ ] **2.3.** Implementar `src-tauri/src/commands/mod.rs` e `src-tauri/src/commands/app.rs` (`app_info` e `app_log_frontend` com `#[tauri::command]` e `#[specta::specta]`), além de `src-tauri/src/events/mod.rs`.

### Passo 3 — Builder Specta, Binário Exportador e Entrypoints
- [ ] **3.1.** Implementar `src-tauri/src/specta.rs` com `builder()`, registro dos 2 comandos e dos 15 tipos exportados (9 ids, `ErrorCode`, `MtError`, `AppPathsDto`, `BuildProfile`, `AppInfo`, `FrontendLogEvent`) (`ErrorHandlingMode::Result`), `export_bindings_to` e `export_bindings`.
- [ ] **3.2.** Implementar `src-tauri/src/bin/export_bindings.rs` e declarar/ajustar `default-run = "maxterminal"` em `src-tauri/Cargo.toml` para que `cargo run` execute o binário principal sem ambiguidade.
- [ ] **3.3.** Implementar `src-tauri/src/lib.rs` (`run()`) e `src-tauri/src/main.rs`.
- [ ] **3.4.** Executar a geração inicial de `src/types/bindings.ts` via `cargo test --test bindings export -- --ignored --exact` (ou `cargo run --bin export_bindings`).

### Passo 4 — Testes Unitários e de Integração do Backend
- [ ] **4.1.** Adicionar testes unitários em `src-tauri/src/logging.rs` e `src-tauri/src/commands/app.rs` verificando:
  - Formato JSON válido e permissão `0o600` no arquivo `maxterminal.log.<AAAA-MM-DD>`.
  - Rejeição/redação de strings arbitrárias e valores sentinela (ex.: `"SECRET_TOKEN_12345"`, `"Bearer eyJ..."`, stacks) tanto no arquivo quanto no formatador.
  - Rate-limiter com `Instant` injetado aceitando 200 eventos na janela de 1s, descartando o 201º e emitindo aviso agregado `frontend_log_rate_limited` na janela seguinte.
- [ ] **4.2.** Criar `src-tauri/tests/bindings.rs` testando determinismo em duas exportações seguidas, ausência de drift contra `../src/types/bindings.ts`, detecção de mutação artificial e presença de todos os tipos e comandos no arquivo gerado.
- [ ] **4.3.** Criar `src-tauri/tests/capabilities.rs` validando `tauri.conf.json`, `capabilities/default.json` e o bloqueio de janela/comando não autorizados pela ACL.

---

## 3. Comandos de Validação e Mapeamento para Critérios de Aceite

| Critério | Comando de Verificação | Resultado Esperado |
|----------|------------------------|--------------------|
| **CA-11** (Formatação Rust) | `cd src-tauri && cargo fmt --all --check` | Exit code `0`. |
| **CA-12** (Clippy workspace) | `cd src-tauri && cargo clippy --locked --workspace --all-targets -- -D warnings` | Exit code `0` com zero warnings. |
| **CA-13** (Testes do workspace) | `cd src-tauri && cargo test --workspace --locked` | Exit code `0` para `mt-core`, `maxterminal`, `bindings` e `capabilities`. |
| **CA-16** (Geração e drift de bindings) | `cd src-tauri && cargo test --locked --test bindings` e `git diff --exit-code -- src/types/bindings.ts` | Duas execuções idênticas, drift detectado em mutação e zero diff no arquivo versionado. |
| **CA-17** (Símbolos exportados em `bindings.ts`) | `cd src-tauri && cargo test --locked --test bindings exports_expected_symbols` | `AppInfo`, `AppPathsDto`, `BuildProfile`, `MtError`, `ErrorCode`, `FrontendLogEvent`, 9 IDs (incluindo `TabSessionId`) como `string` e wrappers dos 2 comandos presentes. |
| **CA-22** (Arquivo de log diário e `MAXTERMINAL_LOG`) | `cd src-tauri && cargo test --locked logging::tests` | Arquivo `maxterminal.log.<AAAA-MM-DD>` contém `MaxTerminal iniciado` com versão e respeita `MAXTERMINAL_LOG=debug`. |
| **CA-23** (Segurança de logs do frontend e rate-limit) | `cd src-tauri && cargo test --locked commands::app::tests` | `vue_error` gera log `target=frontend`, `level=ERROR`; sentinelas nunca aparecem; enum inválido falha na desserialização; taxa limitada a 200/s. |
| **CA-29** (Capability e ACL) | `cd src-tauri && cargo test --locked --test capabilities` | Apenas janela `main` e os 2 comandos na allowlist; janela ou comando fora da allowlist são negados. |
| **CA-30** (Configuração `devUrl` e CSP) | `cd src-tauri && cargo test --locked --test capabilities csp_and_dev_url_contract` | `devUrl` == `http://127.0.0.1:5177`, CSP de release sem origem de dev server, sem `unsafe-eval` e sem `core:default`. |
