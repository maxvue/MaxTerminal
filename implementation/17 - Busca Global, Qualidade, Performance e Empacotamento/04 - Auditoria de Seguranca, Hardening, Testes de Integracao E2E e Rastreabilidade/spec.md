# Implementação 04 — Auditoria de Segurança, Hardening, Testes de Integração E2E e Rastreabilidade

> **Fase:** 17 — Busca Global, Qualidade, Performance e Empacotamento
> **Itens de Escopo Cobertos:** `I-10`, `I-11` (workflows `ci.yml`, `audit.yml`, `bench.yml`), `I-13`, `I-16`
> **Critérios de Aceite Cobertos:** `CA-22`, `CA-23`, `CA-24`, `CA-25`, `CA-26`, `CA-28`, `CA-30`, `CA-31`, `CA-32`, `CA-33`, `CA-34`, `CA-41`

---

## 1. Contexto, Objetivo e Escopo

Esta implementação consolida a **barreira de segurança (hardening), a qualidade fim a fim (testes de processos não-órfãos, sanitização ponta a ponta, E2E frontend simulado e E2E nativo com `tauri-driver`), a auditoria de dependências e a matriz executável de rastreabilidade** do MaxTerminal:
- **Hardening Tauri v2 (`CA-30`, `CA-31`, `CA-32`):** Configuração final de Content Security Policy (CSP) sem `unsafe-eval` nem CDNs remotas em `src-tauri/tauri.conf.json`; capabilities mínimas por janela (`src-tauri/capabilities/main.json`) com validação de janela chamadora e origem nos comandos próprios; políticas `src-tauri/deny.toml` (`cargo-deny`) e `src-tauri/.cargo/audit.toml` (`cargo-audit`).
- **Segurança do Bridge Loopback e Deep Links (`CA-33`, `CA-34`):** Testes de integração verificando rejeição de requisições autenticadas sem token (`401`), `Host` inválido (`403 bridge.host_forbidden`), `Origin` não autorizado (`403`), bind restrito a `127.0.0.1` e rejeição de URIs `maxterminal://` pelos códigos reais das Fases 09/11 (§3.4).
- **Garantia de Zero Processos Órfãos e Sanitização Ponta a Ponta (`CA-25`, `CA-26`):** Suítes de integração em Rust puro (`src-tauri/tests/no_orphans.rs` e `src-tauri/tests/sanitizer_e2e.rs`) testando árvores de processos reais (`fork`, `setsid`, double-fork, zumbis, `SIGTERM`, minimização para tray e reconciliação pós-`SIGKILL` via `(pid, start_time)`) e varredura binária do arquivo SQLite, `-wal`, páginas livres (`freelist`), vocabulário FTS5, backups, logs e payload do botão `"Inspecionar"`.
- **Suítes E2E Separadas (`CA-24`):** Configuração `e2e/frontend/` (Vite + IPC simulado declarado) e `e2e/wdio.conf.ts` + 6 specs nativas (`search`, `tabs`, `no-orphans`, `sanitizer`, `deeplink`, `tray`) sobre o binário real com `tauri-driver` + `WebKitWebDriver` + `xvfb-run`.
- **Rastreabilidade e Pipelines de CI (`CA-22`, `CA-23`, `CA-28`, `CA-41`):** Manifesto `traceability-manifest.json`, validador `scripts/check-traceability.mjs` e workflows `.github/workflows/{ci.yml, audit.yml, bench.yml}`.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

### 2.1. Criar ou Modificar em `src-tauri/`
- `src-tauri/tauri.conf.json` — Definir CSP final de produção e vincular `capabilities: ["main"]`.
- `src-tauri/capabilities/main.json` — Permissões mínimas para a janela `main` (incluindo ACL dos comandos próprios do aplicativo e bloqueio de origens remotas).
- `src-tauri/build.rs` — Registrar manifesto de comandos próprios (`tauri_build::Attributes::new().app_manifest(...)`) para enforcement real de capabilities.
- `src-tauri/deny.toml` — Política de licenças, banimentos, fontes confiáveis (`crates.io` apenas) e advisories para `cargo deny check`.
- `src-tauri/.cargo/audit.toml` — Configuração estrita do `cargo audit`.
- `src-tauri/tests/no_orphans.rs` — Suíte de integração de supervisão de processos e ausência de órfãos/zumbis.
- `src-tauri/tests/sanitizer_e2e.rs` — Suíte de integração ponta a ponta do sanitizador (PTY → OSC 133 / Scrollback → SQLite WAL / FTS5 / Backups / Bridge).
- `src-tauri/tests/bridge_hardening.rs` — Testes de segurança do servidor HTTP/WS loopback da Fase 11.
- `src-tauri/tests/deep_link_allowlist.rs` — Testes de validação e rejeição de URIs `maxterminal://` (Fases 09/11).
- `src-tauri/tests/capabilities_minimal.rs` — Inspeção das capabilities efetivas, labels de janela e teste negativo de chamada não autorizada.

### 2.2. Criar em `e2e/`
- `e2e/wdio.conf.ts` — Runner WebdriverIO nativo gerenciando `tauri-driver`, diretório temporário isolado (`XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_RUNTIME_DIR`) por spec e teardown garantido.
- `e2e/specs/search.e2e.ts` — Busca global FTS5 ponta a ponta no binário real, navegação para bloco vivo e sessão histórica read-only após reiniciar o app.
- `e2e/specs/tabs.e2e.ts` — Criação de abas, split panes, templates, nova sessão e verificação de CSP/capabilities no WebKitGTK real.
- `e2e/specs/no-orphans.e2e.ts` — Execução de árvore de processos em aba real, fechamento da aba e verificação de `/proc`.
- `e2e/specs/sanitizer.e2e.ts` — Digitação de segredo sintético no terminal real, busca na paleta global (0 resultados para o segredo cru) e verificação do banco em disco.
- `e2e/specs/deeplink.e2e.ts` — Despacho de URI `maxterminal://` válida e rejeição de URI maliciosa.
- `e2e/specs/tray.e2e.ts` — Minimização para a bandeja ao fechar janela com processo em execução e preservação do PTY vivo.
- `e2e/frontend/wdio.frontend.conf.ts` e `e2e/frontend/specs/search-ui.e2e.ts` — Suíte E2E de navegador com IPC simulado explicitamente identificado.

### 2.3. Criar na Raiz (`scripts/`, `.github/workflows/` e Manifesto)
- `traceability-manifest.json` — Mapeamento completo de todos os requisitos canônicos do `AGENTS.md` e `spec.md` §14 (`requirementId`, `canonicalRef`, `ownerPhases`, `testIds`, `evidenceArtifact`, `status`).
- `scripts/check-traceability.mjs` — Validador que cruza `traceability-manifest.json` com os arquivos de teste reais no repositório e com os relatórios JUnit/JSON de execução.
- `.github/workflows/ci.yml` — Pipeline obrigatório com os jobs `frontend`, `rust`, `e2e-frontend`, `e2e-native`, `audit`, `bundle-budget`, `traceability` e `package`.
- `.github/workflows/audit.yml` — Execução agendada e por PR de `cargo audit`, `cargo deny check` e `npm audit --omit=dev --audit-level=high`.
- `.github/workflows/bench.yml` — Workflow para runner dedicado comparando `bench/baseline.json`.

---

## 3. Especificação Técnica Exaustiva

### 3.1. CSP Final e Capabilities Mínimas (`tauri.conf.json` e `capabilities/main.json`)

Em `src-tauri/tauri.conf.json`:
```json
{
  "app": {
    "security": {
      "csp": {
        "default-src": "'self'",
        "script-src": "'self'",
        "style-src": "'self' 'unsafe-inline'",
        "img-src": "'self' data: asset: http://asset.localhost",
        "font-src": "'self' data:",
        "connect-src": "'self' ipc: http://ipc.localhost",
        "object-src": "'none'",
        "frame-src": "'none'",
        "base-uri": "'self'",
        "form-action": "'none'"
      },
      "freezePrototype": true,
      "capabilities": ["main"]
    }
  }
}
```
- **Proibições absolutas (`CA-30`):** Nenhuma diretiva contém `'unsafe-eval'`, `http://*` externo, `https://*` externo ou domínios de CDN. Toda comunicação com `MaxAiManager` e `GitHub` ocorre exclusivamente no backend Rust (`mt-ai`, `mt-github`).

Em `src-tauri/capabilities/main.json`:
```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "main",
  "description": "Permissões estritamente necessárias para a janela principal do MaxTerminal",
  "local": true,
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:window:allow-show",
    "core:window:allow-hide",
    "core:window:allow-set-focus",
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-close",
    "allow-os-integration",
    "maxterminal:allow-workspace-commands"
  ]
}
```
Além do ACL declarativo do Tauri v2, todo comando sensível (`search_*`, `db_backup_*`, `pty_*`, `command_group_*`) valida na fronteira IPC (`src-tauri/src/security.rs`):
```rust
pub fn ensure_authorized_window(webview_window: &tauri::WebviewWindow) -> Result<SearchOwner, MtError> {
    if webview_window.label() != "main" {
        return Err(MtError::new(
            ErrorCode::try_from("security.unauthorized_window").expect("código estático válido"),
            "Esta janela não possui permissão para executar esta operação.",
        ));
    }
    let url = webview_window.url().map_err(|_| {
        MtError::new(ErrorCode::try_from("security.invalid_origin").expect("código estático válido"), "Origem da janela inválida.")
    })?;
    let valid_origin = matches!(url.scheme(), "tauri" | "http" | "https")
        && matches!(url.host_str(), Some("localhost") | Some("tauri.localhost") | None);
    if !valid_origin {
        return Err(MtError::new(
            ErrorCode::try_from("security.unauthorized_origin").expect("código estático válido"),
            "Origem não autorizada para invocar comandos nativos.",
        ));
    }
    Ok(SearchOwner {
        window_label: webview_window.label().to_owned(),
    })
}
```

### 3.2. Testes de Não-Órfãos (`src-tauri/tests/no_orphans.rs`)

Cada caso de teste executa um helper real no PTY que spawna:
1. Processo filho direto (`sleep 300`).
2. Processo neto via subshell.
3. Processo em background com `nohup`.
4. Processo em nova sessão via `setsid()` (escapando do PGID original, mas rastreado pelo supervisor via cgroup/subreaper/`/proc/<pid>/task/*/children` + `(pid, start_time)`).
5. Processo double-fork (órfão reparentado ao subreaper do MaxTerminal).
6. Processo que instala `SIG_IGN` para `SIGHUP` e `SIGTERM`.

Cenários verificados (`CA-25`):
- **Fechar painel / fechar aba / "Iniciar nova Sessão" / cancelar grupo / saída explícita / `SIGTERM` no processo do app:** Após `kill_and_wait` dentro do orçamento de shutdown, inspeciona `/proc` verificando que **nenhum** `(pid, start_time)` da árvore permanece vivo e que **nenhum** processo permanece em estado `Z` (zumbi).
- **Reuso de PID:** Simula registro com `(pid_atual_de_outro_processo, start_time_falso)` e comprova que o supervisor jamais envia sinal quando `start_time` diverge.
- **Fechar janela com tarefa ativa:** Verifica que o app minimiza para o System Tray e mantém os processos vivos.
- **`SIGKILL` e recuperação no próximo boot:** Mata o processo supervisor de teste com `SIGKILL`, reinicia a instância com o mesmo diretório runtime e verifica que a rotina de reconciliação de boot encerra os sobreviventes cujo `(pid, start_time)` e `uid` coincidem com o journal de processos supervisionados.

### 3.3. Testes de Sanitização Ponta a Ponta (`src-tauri/tests/sanitizer_e2e.rs`)

Injeta segredos sintéticos cobrindo todas as categorias de `sanitizer_cases.toml`:
- `Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.synthetic_secret_token_99182`
- `APP_KEY=base64:dGhpc19pc19hX3N5bnRoZXRpY19sYXJhdmVsX2tleV8xMjM0NTY=`
- `Set-Cookie: laravel_session=synthetic_cookie_value_88273645; HttpOnly`
- `postgres://admin:Synthet1cP@ssw0rd!99@127.0.0.1:5432/prod`

Cenários de fragmentação e persistência (`CA-26`):
1. Envia cada segredo pelo PTY real partido byte a byte, partido no meio de codepoints UTF-8 multibyte, intercalado com sequências ANSI CSI/OSC 133 e no final de sessão sem `\n` antes de fechar o PTY.
2. Força flush/shutdown, cria um backup (`db_backup_create`), gera o payload do botão `"Inspecionar"` (Fase 11) e realiza busca global pelo prefixo.
3. Verifica:
   - `SELECT` em `command_blocks`, `session_scrollback`, `search_metadata` e `fts5vocab` de todas as tabelas FTS → 0 ocorrências dos segredos originais.
   - Leitura binária direta (`std::fs::read`) dos arquivos `maxterminal.db`, `maxterminal.db-wal`, do arquivo de backup `.db` (incluindo páginas livres após `DELETE`) e dos arquivos de log → 0 ocorrências da sequência de bytes dos segredos originais.

### 3.4. Hardening de Bridge Loopback e Deep Links (`bridge_hardening.rs` e `deep_link_allowlist.rs`)

- **`bridge_hardening.rs` (`CA-33`):**
  1. Confirma que `local_addr().ip()` do listener é estritamente `127.0.0.1` (nunca `0.0.0.0` ou `::`).
   2. Requisição `POST /v1/messages`/upgrade WS sem header `Authorization: Bearer <token>` ou com token inválido → `401 Unauthorized` (com Host/Origin válidos); `/v1/health` dispensa Bearer, mantendo Host/Origin.
   3. Requisição com header `Host: evil.example.com`, porta divergente, Host ausente ou duplicado → `403 Forbidden` + `bridge.host_forbidden`. Testar positivamente `127.0.0.1:<porta>` e `localhost:<porta>` (D-04).
  4. Requisição com header `Origin: https://evil.example.com` → `403 Forbidden`.
  5. Payload acima do limite (ex.: > 256 KiB) → `413 Payload Too Large`.
- **`deep_link_allowlist.rs` (`CA-34`):**
  1. Testa URIs fora das rotas permitidas (`maxterminal://exec?cmd=rm+-rf`, `maxterminal://open-project?path=/etc/../root`, caminhos fora de projetos cadastrados, bytes de controle/NUL).
   2. Conferir por camada: NUL/controle, escape percentual inválido e excesso de tamanho → `os.activation_invalid`; rota/parâmetro proibido ou desconhecido → `bridge.invalid_uri`; caminho inexistente/não absoluto → `bridge.invalid_path`; caminho existente sem projeto cadastrado → `bridge.project_not_found`; cwd fora do projeto/worktree verificada → `bridge.cwd_outside_project`. Não rejeitar `..` isoladamente: canonicalizar e testar o destino conforme `11/03`.
   3. Exercitar o único `ActivationRouter` (09) → `ActivationHandler` (11), cold/warm start, repasse de `worktree`/`correlationId`, sem segundo listener e sem efeitos nas rejeições. Erros `BridgeError` viram `MtError` preservando seu código.
   4. Em `capabilities_minimal.rs`, verificar a união efetiva de todas as capabilities: chamadas JS diretas de autostart/opener/deep-link/atalho continuam negadas; `os_get_autostart`/`os_set_autostart` funcionam somente na janela local autorizada. A consolidação de `main.json` preserva permissões próprias previamente registradas, inclusive `allow-os-integration`; o JSON acima é recorte, não substituto integral do ACL das fases anteriores.

---

## 4. Regras de Negócio e Casos de Borda

- **Proibição de filtros vazios (`spec.md` §12):** Nos scripts e no `ci.yml`, qualquer comando de teste cujo filtro selecione `0 tests` deve falhar imediatamente.
- **Cobertura de caminhos críticos (`CA-23`):** `sanitizer_e2e`, `no_orphans`, `search_cancel` e `upgrade_from_fixtures` são gates obrigatórios explícitos independentemente do percentual global de linhas cobertas.
