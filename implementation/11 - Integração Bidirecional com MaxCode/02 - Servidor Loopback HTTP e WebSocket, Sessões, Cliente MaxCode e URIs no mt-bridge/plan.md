# Implementação 02 — Servidor Loopback HTTP e WebSocket, Sessões, Cliente MaxCode e URIs no `mt-bridge` (`plan.md`)

> Especificação técnica desta unidade: [`./spec.md`](./spec.md).
> Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md).

---

## 1. Checklist Sequencial de Execução

- [ ] **Passo 1: Implementar `uri.rs` e Testes `tests/uri_it.rs`**
  1. Criar `src-tauri/crates/mt-bridge/src/uri.rs` com `MaxTerminalUri::parse` e `MaxCodeUri::build`.
  2. Garantir validação de limite de 16 KiB para URI de entrada, 4096 bytes por parâmetro decodificado, rejeição de parâmetros duplicados/desconhecidos/proibidos (`template`, `command`, `env`, `prompt`, `token`), rejeição de userinfo, porta, fragmento e NUL, e decodificação UTF-8 única.
  3. Criar `src-tauri/crates/mt-bridge/tests/uri_it.rs` testando caminhos com acentos e espaços (`%20`), parâmetros proibidos, caminhos relativos em `path`/`project`, tamanhos > 4096 bytes / > 16 KiB e formato exato de `vscode://johnattas.maxcode/bridge/wake?requestId=<uuid>`.

- [ ] **Passo 2: Implementar `session.rs` (`PeerSession`, `PeerRegistry`, Rate Limit e Heartbeat)**
  1. Criar `src-tauri/crates/mt-bridge/src/session.rs` com `PeerSession`, `PeerRegistry`, `EphemeralHttpSessionStore` (TTL 5 min para `X-Bridge-Session`), token-bucket rate limiter (50 msg/s por sessão; excesso por 3 s fecha com `4008`) e semáforo global de 8 sessões WS e 128 pending ACKs.
  2. Implementar `PeerRegistry::pick_for`, `PeerRegistry::request`, `PeerRegistry::wait_for_peer` (via `tokio::sync::Notify`) e `PeerRegistry::close_all`.

- [ ] **Passo 3: Implementar `server.rs` (Bind `127.0.0.1` + Rotas HTTP/WS)**
  1. Criar `src-tauri/crates/mt-bridge/src/server.rs` com `bind(cfg)` usando exclusivamente `Ipv4Addr::LOCALHOST` e sequência `preferred_port` → `preferred_port + 1 ..= preferred_port + fallback_range` → `:0`.
  2. Implementar os handlers `axum`:
     - `GET /v1/health`: checa `HostPolicy` + `OriginPolicy` e retorna `200 {"ok":true,"protocol":1}`.
     - `POST /v1/messages`: checa `HostPolicy` + `OriginPolicy` + `verify_bearer` + `Content-Type: application/json`, aplica `RequestBodyLimitLayer(256 * 1024)` e `TimeoutLayer(10s)`, negocia `X-Bridge-Session` no `hello` e despacha mensagens autenticadas com `Deduplicator`.
     - `GET /v1/ws`: checa `HostPolicy` + `OriginPolicy` + `verify_bearer`, aplica timer de 5 s para `hello` (fecha `4001` se expirar), valida `protocol.contains(&1)` (fecha `4002` se incompatível), acumula fragmentos até 256 KiB (fecha `1009` se exceder), rejeita frames binários e inicia loop de ping (15 s) / verificação de pong (45 s).

- [ ] **Passo 4: Implementar `client.rs` (`MaxCodeClient`, `MaxCodeAdapter`, `XdgOpener` e Fila de Wake-up)**
  1. Criar `src-tauri/crates/mt-bridge/src/client.rs` com `DeliveryChannel`, `DeliveryReport`, `UriOpener`, `XdgOpener` (`tokio::process::Command::new("xdg-open")` sem shell e timeout de 5 s), `MaxCodeAdapter`, `DefaultMaxCodeAdapter` e `MaxCodeClient`.
  2. Implementar o fluxo de `open_plan` e `open_project`:
     - Sanitização via `host.sanitize()`, validação de tamanho (`<= 64 KiB`) e seleção de peer compatível.
     - Envio via WS se houver peer conectado; se não houver, tentativa HTTP local (`127.0.0.1`, `.no_proxy()`, `redirect::Policy::none()`) somente se `adapter.discover_http` retornar descoberta validada; se não houver conexão, enfileiramento na fila efêmera (máx. 32 itens / 2 MiB, TTL 30 s) + abertura de `vscode://johnattas.maxcode/bridge/wake?requestId=<uuid>` + espera por reconexão autenticada e ACK terminal.
     - Tratamento de `AckTimeout` pós-envio retornando `BridgeError::DeliveryUnknown` (ou retry com mesmo `id` se o peer suportar `IdempotentRequests`).

- [ ] **Passo 5: Implementar `BridgeHandle` em `lib.rs` e Testes `tests/server_it.rs`**
  1. Atualizar `src-tauri/crates/mt-bridge/src/lib.rs` com `BridgeHandle`, `start_bridge` e `start_bridge_with`, integrando `write_discovery`, `remove_discovery`, `restart`, `regenerate_token` (fechando sessões com `4003` e regravando `bridge.json`) e `shutdown` (fechando sessões com `1001` e removendo `bridge.json`).
  2. Criar `src-tauri/crates/mt-bridge/tests/server_it.rs` validando:
     - `/v1/health` retornando `200 {"ok":true,"protocol":1}`.
     - `/v1/messages` retornando `401` sem token/token inválido, `403` para `Origin: https://evil.example` e `Host: evil.example`, `413` para corpo > 256 KiB, `405` para `OPTIONS`.
     - Handshake WS completo (`hello` → `hello.ack`), timeout de 5 s sem `hello` (`4001`), desconexão por falta de `pong` em 45 s (com `tokio::time::pause()`), rotação de token (`4003`), fallback de porta quando a preferida está ocupada e fluxo completo de wake-up + entrega de `maxcode.open_conversation`.

---

## 2. Comandos Exatos de Verificação

```bash
# 1. Verificar lint estrito de todos os alvos do mt-bridge (CA-01)
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-bridge --all-targets -- -D warnings

# 2. Executar testes unitários, uri_it, discovery_it e server_it (CA-01)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-bridge
```

---

## 3. Critérios de Aceite Mapeados (`CA-*`)

| Critério | Descrição | Como é verificado nesta Implementação |
|----------|-----------|---------------------------------------|
| **CA-05** | Cobertura ≥ 85% em `uri.rs`. | Validado por `tests/uri_it.rs` e testes unitários de `uri.rs`. |
| **CA-06** | Bind exclusivo em `127.0.0.1:<porta>`. | Validado em `server.rs` e `tests/server_it.rs`. |
| **CA-07** | Fallback sequencial `47616..=47624` e porta efêmera quando ocupadas. | Validado em `tests/server_it.rs` ocupando portas com `TcpListener`. |
| **CA-09** | `GET /v1/health` retorna `200 {"ok":true,"protocol":1}`. | Validado em `tests/server_it.rs`. |
| **CA-10** | `POST /v1/messages` retorna `401`, `403` (Origin/Host) e `413` (> 256 KiB). | Validado em `tests/server_it.rs`. |
| **CA-11** | Handshake WS `hello` → `hello.ack` e fechamento `4001` sem `hello` em 5 s. | Validado em `tests/server_it.rs`. |
| **CA-13** | Sem `pong` por 45 s encerra sessão e emite `PeerDisconnected`. | Validado em `tests/server_it.rs`. |
| **CA-15** | `regenerate_token` encerra sessões com `4003` e grava novo token no `bridge.json`. | Validado em `tests/server_it.rs`. |
| **CA-21** | Wake-up abre apenas `/bridge/wake?requestId=<uuid>`, retém payload por até 30 s e exige ACK autenticado. | Validado nos testes de `client.rs` e `tests/server_it.rs`. |
| **CA-22** | `maxcode.open_conversation` com ACK em até 3 s, `bridge.delivery_unknown` em timeout pós-envio e dedup WS/HTTP. | Validado em `tests/server_it.rs`. |
| **CA-23** | Nenhuma URI/log contém prompt/token; prompt genérico > 64 KiB é rejeitado. | Validado em `uri_it.rs` e `client.rs`. |
| **CA-36** | Rate limits, limite de 256 KiB em frames WS fragmentados e destino HTTP restrito a `127.0.0.1` sem proxy/redirect. | Validado em `tests/server_it.rs`. |
