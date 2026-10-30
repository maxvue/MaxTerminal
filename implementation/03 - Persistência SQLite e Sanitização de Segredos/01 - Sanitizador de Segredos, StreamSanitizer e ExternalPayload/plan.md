# Implementação 01 — Sanitizador de Segredos, `StreamSanitizer` e `ExternalPayload` (`plan.md`)

> Guia de execução sequencial para a Implementação 01 da Fase 03.
> Referência técnica: [`./spec.md`](./spec.md).

---

## Passo 1 — Configurar o Crate `mt-db` no Workspace Cargo

1. Editar `src-tauri/Cargo.toml`:
   - Adicionar `"crates/mt-db"` à lista `members` de `[workspace]`.
   - Registrar em `[workspace.dependencies]` as dependências: `mt-db`, `rusqlite` (`features = ["bundled", "backup", "functions", "serde_json"]`), `r2d2`, `r2d2_sqlite`, `crossbeam-channel`, `tokio` (`features = ["sync", "rt", "time", "macros"]`), `regex`, `include_dir`, `sha2`, `tempfile`, `proptest`, `rstest`, `criterion`.
2. Criar `src-tauri/crates/mt-db/Cargo.toml` herdando `version`, `edition`, `rust-version` e `[lints]` do workspace, declarando os targets `[[bench]]` para `sanitizer` (e reservando `batch`).
3. Criar `src-tauri/crates/mt-db/src/time.rs` com `pub fn now_ms() -> i64`.
4. Criar `src-tauri/crates/mt-db/src/error.rs` inicial e `src-tauri/crates/mt-db/src/lib.rs` expondo `pub mod error; pub mod sanitizer; pub mod time;`.
5. Verificar no `Cargo.lock` que existe apenas uma versão de `rusqlite` e `libsqlite3-sys`.

---

## Passo 2 — Implementar `SecretKind`, `SanitizedText` e Catálogo de Regras (`sanitizer/rules.rs`)

1. Criar `src-tauri/crates/mt-db/src/sanitizer/rules.rs`:
   - Definir `SecretKind` com as 13 variantes (`UrlCredentials`, `AppKey`, `Bearer`, `Cookie`, `GithubToken`, `AwsAccessKeyId`, `AwsSecretKey`, `OpenAiKey`, `AnthropicKey`, `Jwt`, `EnvAssignment`, `CliFlag`, `PemPrivateKey`), `SecretKind::ALL`, `as_str()` e `redaction_marker()`.
   - Compilar `RegexSet` (`PREFILTER`) e os `Regex` individuais das 13 famílias dentro de `std::sync::LazyLock`.
   - Implementar as funções de exclusão de falsos positivos:
     - Preservação de valores já redigidos (`[REDACTED:...]`).
     - Preservação de valores vazios e referências simples inteiras `$VAR` / `${VAR}` em `EnvAssignment`, `CliFlag`, `AppKey`, `AwsSecretKey` e `UrlCredentials`.
     - Isenção de nome de chave para `PWD` e `OLDPWD` em `EnvAssignment`.
   - Implementar o detector contextual de sobreposições e de caudas truncadas em EOF (PEM sem `END`, aspas abertas em atribuição/flag sensível, `Bearer ` truncado, `Cookie:` truncado, credencial de URL truncada).
   - Garantir que `Bearer <JWT>` ou `GITHUB_TOKEN=ghp_...` emitam uma única máscara com o `SecretKind` do contexto externo (`bearer` / `env_assignment`), sem deixar sufixos ou máscaras duplicadas.

---

## Passo 3 — Implementar `Utf8Decoder`, `TerminalNormalizer` e `StreamSanitizer` (`sanitizer/stream.rs`)

1. Criar `src-tauri/crates/mt-db/src/sanitizer/stream.rs`:
   - Implementar `Utf8Decoder` retendo de 1 a 3 bytes UTF-8 incompletos na fronteira do chunk sem usar `from_utf8_lossy` por chunk isolado.
   - Implementar `TerminalNormalizer`:
     - Máquina de estados para sequências ANSI: texto normal, `ESC` (`\x1b`), `CSI` (`\x1b[...` ou `\x9b...` até byte final `0x40..=0x7E`), `OSC` (`\x1b]...` até BEL `\x07` ou ST `\x1b\\`), `DCS`/`PM`/`APC` (`\x1bP`, `\x1b^`, `\x1b_` até ST `\x1b\\`).
     - Remover sequências CSI/SGR e descartar inteiramente payloads OSC/DCS para que escapes inseridos no meio de um segredo não impeçam o match.
     - Tratar `\r` (sem `\n` imediato) e `\x08` (backspace) detectando sobrescrita intra-linha ambígua.
   - Implementar `StreamSanitizer`:
     - Estados `Text`, `QuotedValue`, `Pem`, `Suppressed` e `Finished`.
     - Em `push(&[u8])`: decodificar UTF-8 incrementalmente, normalizar ANSI, processar blocos PEM (emitindo `[REDACTED:pem_private_key]` no `BEGIN` e descartando até `END` com janela deslizante), emitir unidades apenas em `\n` fora de PEM/aspas/continuação `\\`, e acionar supressão com `[REDACTED:oversize]` ao ultrapassar `max_carry` (16 KiB).
     - Em `on_gap()`: limpar buffers parciais e entrar em `Suppressed` até a próxima fronteira segura `\n`.
     - Em `finish()`: fechar estados pendentes com redação conservadora de EOF e transitar para `Finished` (chamadas subsequentes a `finish()` retornam `""`; chamadas a `push()` após `finish()` retornam `Err(SanitizeError::StreamFinished)`).

---

## Passo 4 — Implementar `sanitize`, `SanitizedText`, `ExternalPayload` e Inspeção JSON (`sanitizer/mod.rs`)

1. Criar `src-tauri/crates/mt-db/src/sanitizer/mod.rs`:
   - Implementar `sanitize(input: &str) -> Cow<'_, str>` retornando `Cow::Borrowed(input)` quando o texto não possuir controles ANSI/CR/BS, não casar no `PREFILTER` e não terminar em estado contextual aberto.
   - Implementar `sanitize_terminal(input: &str) -> SanitizedText` e `sanitize_with(input: &str, opts: &SanitizeOptions) -> SanitizeOutcome<'_>`.
   - Implementar `SanitizedText` com `Debug` opaco e `rusqlite::ToSql`.
   - Implementar `inspect_settings_json(value: &serde_json::Value) -> Result<(), SanitizeError>` (máximo 16 níveis e 1024 nós; rejeita chaves sensíveis ou valores string que sofram alteração por `sanitize()`).
   - Implementar `ExternalPayload` e `prepare_external_payload(value: serde_json::Value) -> Result<ExternalPayload, SanitizeError>` (máximo 16 níveis, 1024 nós e 256 KiB pós-sanitização).

---

## Passo 5 — Criar Fixtures, Testes Unitários/Propriedade e Benchmarks

1. Criar `src-tauri/crates/mt-db/tests/fixtures/sanitizer_cases.toml` com ≥ 3 casos positivos e ≥ 2 negativos para cada uma das 13 famílias de `SecretKind`.
2. Criar `src-tauri/crates/mt-db/tests/sanitizer_table.rs` validando a cobertura por família, o resultado exato e `Cow::Borrowed` nos casos negativos.
3. Criar `src-tauri/crates/mt-db/tests/sanitizer_props.rs` com `proptest` (10 000 casos para idempotência e ausência de panic), corte exaustivo em todos os pontos entre 2 chunks para cada `SecretKind`, PEM em 50 chunks e ANSI entremeado no meio de tokens.
4. Criar `src-tauri/crates/mt-db/tests/sanitizer_stream.rs` cobrindo cortes UTF-8/ANSI por byte, linha > 16 KiB (`[REDACTED:oversize]`), PEM sem `END`, aspas abertas, `finish()` repetido, `on_gap()`, `inspect_settings_json` e `prepare_external_payload`.
5. Criar `src-tauri/crates/mt-db/benches/sanitizer.rs` (e um stub compilável em `benches/batch.rs` até a Implementação 03) medindo throughput em texto limpo e texto com 1 segredo a cada 4 KiB.

---

## Passo 6 — Comandos de Verificação e Validação

Executar a partir de `src-tauri/`:

```bash
cargo check -p mt-db --all-targets
cargo clippy -p mt-db --all-targets -- -D warnings
cargo fmt --all --check
cargo test -p mt-db --test sanitizer_table --test sanitizer_props --test sanitizer_stream
cargo bench -p mt-db --bench sanitizer -- --quick
```

---

## Critérios de Aceite Cobertos nesta Implementação

- [x] **CA-27**: `tests/fixtures/sanitizer_cases.toml` possui ≥ 3 casos positivos e ≥ 2 negativos para cada `SecretKind` (13 famílias); `sanitizer_table` passa 100%.
- [x] **CA-28**: Texto sem segredos retorna `Cow::Borrowed` (`matches!(out, Cow::Borrowed(_))`).
- [x] **CA-29**: Idempotência `sanitize(sanitize(x)) == sanitize(x)` em proptest com 10 000 casos e zero panics.
- [x] **CA-30**: `StreamSanitizer` redige segredos divididos em qualquer ponto entre dois chunks e blocos PEM divididos em 50 chunks.
- [x] **CA-31**: Benchmark `benches/sanitizer.rs` atinge ≥ 100 MiB/s para texto limpo e ≥ 30 MiB/s para texto com 1 segredo a cada 4 KiB em modo release.
- [x] **CA-46**: Fragmentação em todos os bytes (UTF-8/ANSI), 3+ chunks, linha > 16 KiB (`[REDACTED:oversize]`), PEM sem `END`, aspas abertas, `finish()` repetido, `on_gap()` e EOF sem newline testados sem vazamento de prefixo suspeito.
