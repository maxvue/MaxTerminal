# Plano de Execução — Implementação 04: Auditoria de Segurança, Hardening, Testes de Integração E2E e Rastreabilidade

> **Fase:** 17 — Busca Global, Qualidade, Performance e Empacotamento
> **Especificação de Referência:** [`spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: CSP Final, Capabilities Mínimas e Auditoria de Dependências
1. Configurar a CSP de produção sem `'unsafe-eval'` nem origens remotas em `src-tauri/tauri.conf.json`.
2. Criar `src-tauri/capabilities/main.json` e registrar o manifesto de comandos próprios em `src-tauri/build.rs` e a validação `ensure_authorized_window` em `src-tauri/src/security.rs`.
3. Criar `src-tauri/deny.toml` e `src-tauri/.cargo/audit.toml`.
4. Implementar `src-tauri/tests/capabilities_minimal.rs` testando as permissões efetivas da janela `main` e a recusa para janelas/origens não autorizadas.

### Passo 2: Testes de Hardening do Bridge Loopback e Deep Links
1. Criar `src-tauri/tests/bridge_hardening.rs` validando bind exclusivo em `127.0.0.1`, falta de token nas rotas autenticadas (`401`), `Host` inválido (`403 bridge.host_forbidden`) e `Origin` proibido (`403`). Aceitar Host `127.0.0.1:<porta>` e `localhost:<porta>`; health permanece sem Bearer.
2. Criar `src-tauri/tests/deep_link_allowlist.rs` validando códigos por camada: `os.activation_invalid`, `bridge.invalid_uri`, `bridge.invalid_path`, `bridge.project_not_found` e `bridge.cwd_outside_project`. Canonicalizar `..` e testar o destino; testar o fluxo único `ActivationRouter` → `ActivationHandler` sem duplicação de efeitos. Em `capabilities_minimal.rs`, negar autostart JS direto e preservar comandos Rust `os_*` autorizados.

### Passo 3: Suítes de Integração `no_orphans.rs` e `sanitizer_e2e.rs`
1. Criar `src-tauri/tests/no_orphans.rs` cobrindo filhos diretos, netos, `nohup`, `setsid`, double-fork, processos que ignoram `SIGHUP`/`SIGTERM`, detecção de zumbis, proteção contra reuso de PID via `(pid, start_time)`, preservação no tray e reconciliação no boot após `SIGKILL`.
2. Criar `src-tauri/tests/sanitizer_e2e.rs` injetando segredos sintéticos (`Bearer`, `APP_KEY=base64:`, `laravel_session`, senha em URL) fragmentados em UTF-8/ANSI pelo PTY e varrendo inclusive bytes brutos e páginas livres de `maxterminal.db`, `maxterminal.db-wal`, backups, FTS5, logs e payload do `"Inspecionar"`.

### Passo 4: Suítes E2E (Frontend Simulado e Tauri Real com `tauri-driver`)
1. Criar `e2e/frontend/wdio.frontend.conf.ts` e `e2e/frontend/specs/search-ui.e2e.ts` para testes rápidos de fluxos visuais com IPC simulado declarado.
2. Criar `e2e/wdio.conf.ts` e as 6 specs nativas obrigatórias em `e2e/specs/{search,tabs,no-orphans,sanitizer,deeplink,tray}.e2e.ts` executando sobre o binário real com `tauri-driver` + `WebKitWebDriver` e diretórios XDG isolados por execução.

### Passo 5: Matriz de Rastreabilidade e Workflows de CI
1. Criar `traceability-manifest.json` e `scripts/check-traceability.mjs` validando que todos os requisitos canônicos possuem fase responsável, teste executável existente e artefato de evidência.
2. Criar `.github/workflows/ci.yml` (jobs `frontend`, `rust`, `e2e-frontend`, `e2e-native`, `audit`, `bundle-budget`, `traceability`, `package`), `.github/workflows/audit.yml` e `.github/workflows/bench.yml`.
3. Atualizar o script `"check"` no `package.json` para encadear `lint`, `lint:style`, `typecheck`, `typecheck:vue`, `vitest run`, `cargo fmt --check`, `cargo clippy -- -D warnings` e `cargo nextest run`.

---

## 2. Comandos Exatos de Verificação e Testes

Executar a partir da raiz da worktree ativa (`cd "$(git rev-parse --show-toplevel)"`):

```bash
# 1. Executar gate unificado de qualidade (CA-22)
npm run check

# 2. Executar testes críticos de segurança e processos (CA-25, CA-26, CA-31, CA-33, CA-34)
cargo nextest run --manifest-path src-tauri/Cargo.toml -p maxterminal --test no_orphans
cargo nextest run --manifest-path src-tauri/Cargo.toml -p maxterminal --test sanitizer_e2e
cargo nextest run --manifest-path src-tauri/Cargo.toml -p maxterminal --test bridge_hardening
cargo nextest run --manifest-path src-tauri/Cargo.toml -p maxterminal --test deep_link_allowlist
cargo nextest run --manifest-path src-tauri/Cargo.toml -p maxterminal --test capabilities_minimal

# 3. Executar auditoria de dependências Rust e npm (CA-32)
cargo audit --file src-tauri/Cargo.lock
cargo deny --manifest-path src-tauri/Cargo.toml check
npm audit --omit=dev --audit-level=high

# 4. Executar suíte E2E frontend e suíte E2E nativa sob xvfb-run (CA-24, CA-30)
npm run e2e:frontend
xvfb-run -a -s "-screen 0 1440x900x24" npm run e2e

# 5. Validar matriz de rastreabilidade (CA-41)
node scripts/check-traceability.mjs
```

---

## 3. Mapeamento para os Critérios de Aceite da Fase 17

| Critério | Teste / Verificação Responsável | Resultado Esperado |
|----------|---------------------------------|--------------------|
| `CA-22` | `npm run check` | Passa integralmente (`lint`, `lint:style`, `tsgo`, `vue-tsc`, `vitest`, `cargo fmt`, `clippy -D warnings`, `nextest`). |
| `CA-23` | Relatórios de cobertura Vitest/Rust + suítes obrigatórias | Cobertura publicada por módulo sem regressão sobre o baseline; caminhos de segredo, órfãos, perda, cancelamento e migrations 100% exercitados. |
| `CA-24` | `npm run e2e` (`wdio.conf.ts` + `tauri-driver`) | As 6 specs (`search`, `tabs`, `no-orphans`, `sanitizer`, `deeplink`, `tray`) passam com exit code 0. |
| `CA-25` | `cargo nextest run -p maxterminal --test no_orphans` | Nenhum descendente supervisionado ou zumbi permanece após fechar aba, nova sessão, saída ou `SIGTERM`; tray preserva tarefas vivas; `SIGKILL` reconcilia no próximo boot. |
| `CA-26` | `cargo nextest run -p maxterminal --test sanitizer_e2e` | Zero ocorrências dos segredos sintéticos em tabelas, projeções, vocabulário FTS, bytes brutos/páginas livres de DB/WAL/backups, logs e payload `"Inspecionar"`. |
| `CA-28` | `.github/workflows/ci.yml` | Workflow configurado para PR e push na branch de integração com todos os jobs obrigatórios. |
| `CA-30` | `tauri.conf.json` + `e2e/specs/tabs.e2e.ts` | CSP sem `unsafe-eval` ou CDNs; bloqueio de script remoto confirmado no WebView real. |
| `CA-31` | `tests/capabilities_minimal.rs` | Valida permissões mínimas por janela e rejeição de janela/origem não autorizada nos comandos próprios. |
| `CA-32` | `cargo audit`, `cargo deny check`, `npm audit` | Zero vulnerabilidades ou licenças proibidas não justificadas. |
| `CA-33` | `tests/bridge_hardening.rs` | Sem token nas rotas autenticadas → `401`; `Host` inválido → `403 bridge.host_forbidden`; Hosts loopback canônicos aceitos; `Origin` proibido → `403`; bind exclusivo em `127.0.0.1`. |
| `CA-34` | `tests/deep_link_allowlist.rs` | Rejeições usam códigos reais por camada conforme spec §3.4, após canonicalização dos caminhos; nenhum efeito colateral. |
| `CA-41` | `node scripts/check-traceability.mjs` | Todos os requisitos canônicos possuem fase, teste existente e artefato de evidência validados. |
