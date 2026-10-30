# Implementação 01 — Fundamentos do Crate `mt-bridge` (Erros, Config, Descoberta, Autenticação e Protocolo) (`plan.md`)

> Especificação técnica desta unidade: [`./spec.md`](./spec.md).
> Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md).

---

## 1. Checklist Sequencial de Execução

- [ ] **Passo 1: Configurar Workspace e Manifesto do `mt-bridge`**
  1. Editar `src-tauri/Cargo.toml` para incluir `"crates/mt-bridge"` em `[workspace].members` e declarar no `[workspace.dependencies]` (se ainda não declarados): `axum`, `tower-http`, `subtle`, `base64`, `url`, `percent-encoding`, `strip-ansi-escapes`, `toml`, `futures-util`, `async-trait`, `reqwest`.
  2. Criar `src-tauri/crates/mt-bridge/Cargo.toml` exatamente conforme a seção 3.1 do `spec.md`, garantindo zero dependência de `tauri`.

- [ ] **Passo 2: Implementar `error.rs` e `handler.rs`**
  1. Criar `src-tauri/crates/mt-bridge/src/error.rs` com o enum `BridgeError`, todos os 20 variantes, mensagens em `pt-BR`, método `BridgeError::code(&self) -> &'static str` e `impl From<BridgeError> for mt_core::MtError`.
  2. Criar `src-tauri/crates/mt-bridge/src/handler.rs` com `BridgeHost`, `ProjectRef`, `SyncContext`, `FocusRequest`, `OpenTabRequest`, `BridgeState`, `PeerInfo`, `ProjectFocusRequested`, `BridgeStatus`, `PeerDisconnectedEvent`, `TabOpenedEvent`, `DeliveryFailedEvent` e `BridgeEvent`.

- [ ] **Passo 3: Implementar `config.rs` com Testes Unitários**
  1. Criar `src-tauri/crates/mt-bridge/src/config.rs` implementando `BridgeConfig`, `Default`, `config_path`, `load`, `save` (escrita atômica `0600`) e `validate`.
  2. Adicionar testes unitários no próprio `src/config.rs` cobrindo:
     - Geração do arquivo padrão com permissão `0600` quando ausente.
     - Rejeição de porta `< 1024`, `fallback_range > 9`, `ack_timeout_ms` fora de `500..=10000`.
     - Rejeição de `extra_allowed_origins` com `http://localhost:5177`, `https://evil.example`, `"null"`, curinga `*`, `> 256` bytes ou mais de 8 entradas.

- [ ] **Passo 4: Implementar `auth.rs` com Testes Unitários**
  1. Criar `src-tauri/crates/mt-bridge/src/auth.rs` com `Token`, `OriginPolicy`, `HostPolicy` e `verify_bearer`.
  2. Implementar `fmt::Debug` de `Token` exibindo sempre `"Token(***)"`.
  3. Adicionar testes unitários em `src/auth.rs` cobrindo:
     - Comprimento de 43 chars base64url no `Token::generate()`.
     - `verify_bearer` com header ausente, esquema diferente de `Bearer`, múltiplos headers `Authorization`, token incorreto e token válido.
     - `OriginPolicy::check` com `Origin` ausente (`Ok`), `vscode-file://vscode-app` (`Ok`), `http://localhost:5177` (`Err(OriginForbidden)`), `https://evil.example` (`Err(OriginForbidden)`), `Origin: null` (`Err(OriginForbidden)`).
     - `HostPolicy::check` com `127.0.0.1:47615` (`Ok`), `localhost:47615` (`Ok`), `evil.example:47615` (`Err(HostForbidden)`), porta errada (`Err(HostForbidden)`) e header ausente (`Err(HostForbidden)`).

- [ ] **Passo 5: Implementar `protocol.rs` e `dedup.rs` com Testes Unitários**
  1. Criar `src-tauri/crates/mt-bridge/src/protocol.rs` com `Envelope`, `MessageType`, `Message`, `Capability`, `ConversationMode`, `ConversationSource`, payloads e métodos `Message::decode` e `Message::encode`.
  2. Adicionar testes unitários em `src/protocol.rs` validando roundtrip de todos os tipos de mensagem, os exemplos JSON da spec §5.4 como fixtures, `v: 2` retornando `BridgeError::UnsupportedVersion(2)`, `type` desconhecido retornando `BridgeError::UnknownType`, `prompt > 64 KiB` rejeitado e campos desconhecidos ignorados.
  3. Criar `src-tauri/crates/mt-bridge/src/dedup.rs` com `Deduplicator`, reserva atômica `InFlight`, broadcast para chamadas concorrentes com mesma `(sender_id, message_id)`, cache de `Completed`/`Rejected` por 5 minutos (máx. 1024 itens) e detecção de `IdempotencyConflict` quando o fingerprint SHA-256 divergir.
  4. Adicionar testes unitários em `src/dedup.rs` cobrindo: reserva simultânea em duas tasks recebendo o mesmo ACK, conflito de payload para o mesmo ID, preservação de entradas `InFlight` e retorno `BridgeError::Busy` quando saturado.

- [ ] **Passo 6: Implementar `discovery.rs` e Testes de Integração `tests/discovery_it.rs`**
  1. Criar `src-tauri/crates/mt-bridge/src/discovery.rs` com `DiscoveryFile`, `MaxCodeDiscovery`, leitura de `/proc/sys/kernel/random/boot_id` e `/proc/<pid>/stat`, `write_discovery`, `remove_discovery`, `foreign_live_instance` e `read_maxcode_discovery`.
  2. Criar `src-tauri/crates/mt-bridge/src/lib.rs` exportando os módulos `auth`, `config`, `dedup`, `discovery`, `error`, `handler`, `protocol`.
  3. Criar `src-tauri/crates/mt-bridge/tests/discovery_it.rs` testando:
     - Modo `0700` no diretório e `0600` no `bridge.json`.
     - Sobrescrita de arquivo órfão com PID inexistente.
     - Detecção de `foreign_live_instance` quando `pid` pertence a processo vivo e `instance_id` é distinto.
     - Não remoção do `bridge.json` se `instance_id` no disco pertencer a outra instância.
     - Rejeição imediata de symlink no caminho do `bridge.json` ou do `maxcode/bridge.json`.

---

## 2. Comandos Exatos de Verificação

Execute a partir da raiz da worktree (`/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0`):

```bash
# 1. Verificar que mt-bridge não possui dependência de tauri (CA-04)
cargo tree --manifest-path src-tauri/Cargo.toml -p mt-bridge -e normal | grep -c '^.*tauri ' || true

# 2. Verificar compilação e lint estrito do crate mt-bridge (CA-01)
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-bridge --all-targets -- -D warnings

# 3. Executar todos os testes unitários e de integração de descoberta (CA-01)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-bridge
```

---

## 3. Critérios de Aceite Mapeados (`CA-*`)

| Critério | Descrição | Como é verificado nesta Implementação |
|----------|-----------|---------------------------------------|
| **CA-01** | `cargo clippy -p mt-bridge --all-targets -- -D warnings` e `cargo test -p mt-bridge` passam. | Compilação limpa e suíte unitária + `discovery_it.rs` verdes. |
| **CA-04** | `mt-bridge` não depende de `tauri`. | `cargo tree -p mt-bridge -e normal \| grep -c '^.*tauri '` retorna `0`. |
| **CA-05** | Cobertura de testes Rust ≥ 85% de linhas em `protocol.rs` e `auth.rs`. | Testes unitários exaustivos de `protocol.rs` e `auth.rs`. |
| **CA-08** | `bridge.json` com modo `0600`, diretório `0700` e campos obrigatórios. | Validado em `tests/discovery_it.rs`. |
| **CA-12** | `v: 2` → `bridge.unsupported_version`; `type` desconhecido → `bridge.unknown_type`. | Validado nos testes unitários de `Message::decode` em `protocol.rs`. |
| **CA-14** | Remoção de `bridge.json` no encerramento e sobrescrita de arquivo órfão após `kill -9`. | Validado em `tests/discovery_it.rs`. |
| **CA-36** | Descoberta rejeita symlinks, dono/modo incorretos e instância obsoleta; cache de dedup limitado. | Validado em `tests/discovery_it.rs` e `src/dedup.rs`. |
