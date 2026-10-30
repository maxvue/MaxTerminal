# Plano de Execução — Implementação 01: Domínio, Remotes, Autenticação e Transporte GitHub (`mt-github`)

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `01 - Dominio-Remotes-Auth-e-Transporte-GitHub`
> **Especificação Técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Criar a estrutura do crate `mt-github` e registrar no Workspace Cargo
1. Editar `src-tauri/Cargo.toml` para incluir `"crates/mt-github"` na lista `members` do `[workspace]` e declarar `mt-github = { path = "crates/mt-github" }` em `[workspace.dependencies]`.
2. Criar `src-tauri/crates/mt-github/Cargo.toml` com as dependências exatas especificadas em `spec.md` §3.1 (`reqwest` com `rustls-tls`, `keyring` v3 com `linux-native-sync-persistent`, `tokio`, `specta`, `serde`, `serde_json`, `url`, `httpdate`, `pulldown-cmark`, `hmac`, `sha2`, `uuid` e dev-dependencies `wiremock`, `tempfile`, `insta`).
3. Criar `src-tauri/crates/mt-github/src/lib.rs` expondo os módulos `error`, `model`, `ports`, `events`, `remote`, `auth`, `rate_limit` e `client`.

### Passo 2: Implementar Erros (`error.rs`), Modelo de Domínio (`model.rs`), Portas (`ports.rs`) e Eventos (`events.rs`)
1. Implementar `src-tauri/crates/mt-github/src/error.rs` com `GithubErrorKind` (todos os 19 códigos prefixados com `github.`), `GithubError` aplicando `mt_db::sanitizer::sanitize()` na mensagem e conversão `From<GithubError> for mt_core::MtError`.
2. Implementar `src-tauri/crates/mt-github/src/model.rs` com `RepoId`, `AuthScopeId`, `RequestId`, `GithubRepoRef`, `RepoContext`, `IssueState`, `LabelKind`, `LabelDefinition`, `LabelInfo`, `IssueSummary`, `IssueComment`, `IssuePlan`, `IssueDetail`, `CacheMeta`, `Cached<T>`, `OpenIssueCount`, `IssueListQuery`, `IssuePage`, `IssueCreateRequest`, `LabelPatchRequest`, `MutationOutcome`, `MutationReceipt<T>`, `LabelPatchResult`, `AuthSource` e `AuthStatus`. Todos os DTOs devem derivar `specta::Type` com `#[serde(rename_all = "camelCase")]` (structs) ou `#[serde(rename_all = "snake_case")]` (enums).
3. Implementar `src-tauri/crates/mt-github/src/ports.rs` com as traits assíncronas desacopladas (`ProjectPort`, `GitPort`, `WorkspacePort`, `AgentPort`, `MaxCodePort`, `NotificationPort`).
4. Implementar `src-tauri/crates/mt-github/src/events.rs` com `GithubEventEnvelope<T>`, os payloads dos 8 eventos `github://*` (`repo-changed`, `issues-updated`, `issue-updated`, `labels-updated`, `sync-state`, `rate-limit`, `bug-cycle-progress`, `auth-changed`) e a trait `GithubEventSink`.

### Passo 3: Implementar o Parser e Resolvedor de Remotes GitHub (`remote.rs`)
1. Implementar `parse_github_remote_url(raw_url: &str) -> Option<ParsedGithubRemote>` em `src-tauri/crates/mt-github/src/remote.rs`:
   - Tratar sintaxe SCP-like (`[user@]host:owner/repo[.git][/]`) e URLs padrão (`ssh://`, `https://`).
   - Normalizar `host` em ASCII lowercase e aceitar apenas `"github.com"`.
   - Validar portas permitidas (`None` ou `Some(22)` para `ssh://`; `None` ou `Some(443)` para `https://`).
   - Rejeitar URLs com query (`?`), fragmento (`#`), segmentos extras, caracteres de controle ou `.` / `..`.
   - Remover barra final e sufixo `.git` do segmento `repo` sem truncar nomes legítimos terminados nas letras `g`, `i`, `t`.
2. Implementar `GithubRemoteResolver<G: GitPort>` que consulta o diretório `.git` (suportando worktrees via `GitPort::common_dir` e `GitPort::remote_origin_url`) e constrói `GithubRepoRef` com `html_url = format!("https://github.com/{}/{}", owner, name)`.

### Passo 4: Implementar Autenticação e Isolamento por Escopo (`auth.rs`)
1. Implementar `SecretToken` com `Debug` redigido (`SecretToken([REDACTED])`) para impedir vazamento em logs de `tracing`.
2. Implementar `KeyringSecretStore` encapsulando `keyring::Entry::new("maxterminal", "github.com:pat")` via `tokio::task::spawn_blocking` e mapeando falhas de D-Bus/Secret Service para `GithubErrorKind::KeyringUnavailable`.
3. Implementar `DefaultTokenProvider`:
   - Consultar `gh auth token --hostname github.com` em processo filho com `GH_PROMPT_DISABLED=1` e timeout de 5s.
   - Se o `gh` retornar token válido, usar `AuthSource::GhCli`; caso contrário, consultar `KeyringSecretStore`.
   - Gerenciar `AuthScopeId` em memória e persistir apenas a metadado opaco de escopo/fonte (nunca o token!) no `SettingsRegistry` da Fase 03.
   - Ao executar `set_keyring_token` ou `clear_keyring_token`, rotacionar `AuthScopeId` e emitir `github://auth-changed`.

### Passo 5: Implementar Controle de Rate Limit (`rate_limit.rs`) e Clientes (`client/*`)
1. Implementar `src-tauri/crates/mt-github/src/rate_limit.rs`:
   - Extrair `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset`, `X-RateLimit-Resource` e `Retry-After` (segundos ou HTTP-date via `httpdate`).
   - Distinguir `403` de permissão/issues desabilitadas de `403`/`429` de rate limit primário (`remaining == 0`) ou secundário (`Retry-After` presente ou mensagem `"secondary rate limit"` / `"abuse detection"`).
   - Implementar backoff exponencial com jitter (base 60s, teto 15 min) compartilhado por `(auth_scope, resource)` para que `GhCliClient` e `RestClient` respeitem a mesma janela de pausa.
2. Implementar `src-tauri/crates/mt-github/src/client/rest.rs` (`RestClient`):
   - Configurar `reqwest::Client` com `rustls-tls`, gzip, timeout de 30s e headers fixos da API `2026-03-10`.
   - Suportar envio de `If-None-Match` e retorno de `FetchResult::NotModified` em status `304`.
   - Fazer parse do header `Link` (`rel="next"`), validar domínio `https://api.github.com` e filtrar objetos com campo `"pull_request"` em listagens e detalhes de issues.
3. Implementar `src-tauri/crates/mt-github/src/client/gh_cli.rs` (`GhCliClient`):
   - Executar `gh api --hostname github.com --method <M> --include` passando argumentos como array (`Command::args`, sem `sh -c`).
   - Separar cabeçalhos HTTP (com suporte a `\r\n\r\n` e `\n\n` e múltiplos blocos em caso de `100 Continue`) do corpo JSON, limitando leitura a 8 MiB por página.
   - Em escritas (`POST`), enviar payload via `stdin` (`--input -`).
   - Garantir `kill_on_drop(true)` + `kill().await` e `wait().await` em timeout de 30s.
4. Implementar `src-tauri/crates/mt-github/src/client/fallback.rs` (`FallbackClient`):
   - Orquestrar `GhCliClient` com fallback para `RestClient` em leituras quando `gh` estiver ausente ou indisponível.
   - Bloquear replay automático em escritas quando o processo `gh` já tiver sido iniciado e falhar por timeout/EOF (`GithubErrorKind::MutationUnknown`).

### Passo 6: Escrever Testes Automatizados (`tests/remote.rs`, `tests/rest_client.rs`, `tests/gh_cli.rs`)
1. Criar `src-tauri/crates/mt-github/tests/remote.rs` validando ≥ 12 variações de URLs GitHub válidas e ≥ 8 URLs rejeitadas (GitLab, Bitbucket, host falso, path traversal, query string, caminho local).
2. Criar `src-tauri/crates/mt-github/tests/rest_client.rs` usando `wiremock`:
   - `304 Not Modified` com `If-None-Match`.
   - Paginação de 3 páginas via header `Link` e detecção de loop/host externo no `Link`.
   - Descarte automático de Pull Requests (`pull_request` presente no JSON).
   - `403` com `X-RateLimit-Remaining: 0` → `github.rate_limited` com `resetAtMs`; `429` com `Retry-After` respeitado.
3. Criar `src-tauri/crates/mt-github/tests/gh_cli.rs` usando diretório temporário (`tempfile`) com script `gh` simulado no `PATH`:
   - Parse de status, headers e JSON com `--include`.
   - Fallback para `RestClient` quando `gh` não existe no `PATH`.
   - Timeout de processo `gh` matando e recolhendo o PID filho sem deixar zumbi.
   - `POST` com timeout retornando `github.mutation_unknown` sem repetir a chamada no `RestClient`.
   - Erro `422 Unprocessable Entity` retornado diretamente sem acionar fallback.
   - Teste de segurança garantindo que o token salvo nunca aparece em `tracing` capturado nem em tabelas do SQLite.

---

## 2. Comandos de Verificação e Validação

```bash
# 1. Verificar compilação e tipagem do crate mt-github
cargo check -p mt-github --all-targets

# 2. Executar suíte de testes de remote, rest_client, gh_cli e segurança de token
cargo test -p mt-github --test remote --test rest_client --test gh_cli

# 3. Verificar lint estrito sem warnings
cargo clippy -p mt-github --all-targets -- -D warnings
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

| Critério (Fase 16) | Como é atendido e verificado |
|--------------------|------------------------------|
| **CA-1 (`remote`)** | `tests/remote.rs` valida ≥ 12 formatos de URL GitHub (SSH scp-like, `ssh://`, HTTPS com/sem `.git`, usuário, porta, barra final, maiúsculas no host) e retorna `None` para GitLab/Bitbucket/local |
| **CA-1 (`rest_client`)** | `tests/rest_client.rs` com `wiremock` valida `304 Not Modified`, paginação de 3 páginas por `Link`, `403` rate limit com `reset_at`, `429`/`Retry-After` e descarte de PRs |
| **CA-1 (`gh_cli`)** | `tests/gh_cli.rs` valida `gh` falso com `--include`, fallback para REST quando ausente, kill/wait em timeout, ausência de replay em `POST` com timeout e não-fallback em `422` |
| **CA-2 (Segurança de Token)** | Teste automatizado garante que o token nunca é gravado em tabelas SQLite, `settings` ou logs `tracing` |
| **CA-3 (Clippy)** | `cargo clippy -p mt-github --all-targets -- -D warnings` executa com zero avisos |
| **CA-9 (Fallback REST + Keyring)** | Sem `gh` no `PATH`, `FallbackClient` opera via `RestClient` com token do keyring sem vazar credenciais |
