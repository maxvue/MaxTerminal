# Implementação 01 — Crate `mt-commands` e Descoberta Artisan com Cache Compatível (`plan.md`)

> **Fase:** 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`)
> **Unidade:** `01 - Crate mt-commands e Descoberta Artisan com Cache Compativel`
> **Especificação associada:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Execução

### Passo 1 — Criação do Crate `mt-commands` e Fundação (`error.rs`, `ports.rs`, `fs_util.rs`)
- [ ] **1.1.** Criar `src-tauri/crates/mt-commands/Cargo.toml` com todas as dependências especificadas em `spec.md` §3.1 e registrar `crates/mt-commands` no `[workspace]` de `src-tauri/Cargo.toml`.
- [ ] **1.2.** Implementar `src-tauri/crates/mt-commands/src/error.rs` com o módulo `codes` contendo todas as constantes `ErrorCode` (`artisan.*` e `shortcut.*`) e funções construtoras com mensagens em `pt-BR`.
- [ ] **1.3.** Implementar `src-tauri/crates/mt-commands/src/ports.rs` com `RunLimits`, `ArtisanRunner`, `CommandsEventSink`, `ProjectLookup`, `MaxCodeConfigLocator`, `Clock` e `SystemClock`.
- [ ] **1.4.** Implementar `src-tauri/crates/mt-commands/src/fs_util.rs` com `ensure_safe_under_root`, `is_inside`, `ensure_dir_anchored` (verificando componente a componente que não há symlink), `atomic_write_under_root`, `create_new_file_under_root`, `read_capped`, `file_stamp` (SHA-256 + `mtime_ns` + `size`) e `posix_quote_token`.
- [ ] **1.5.** Criar `src-tauri/crates/mt-commands/src/lib.rs` exportando `pub mod artisan; pub mod error; pub mod fs_util; pub mod ports;`.

### Passo 2 — Modelos e Resolução de Raiz Laravel (`model.rs`, `root.rs`)
- [ ] **2.1.** Implementar `src-tauri/crates/mt-commands/src/artisan/model.rs` com `ArtisanCommand`, `ArtisanOption`, `ArtisanArgument`, `RunnerKind`, `LaravelLayout`, `CatalogSource`, `ArtisanRoot`, `ArtisanCatalog`, `CatalogErrorInfo`, `CatalogUpdatedPayload`, `DiscoveryFailedPayload` e `CommandCreatedPayload` (todos com `Serialize`, `Deserialize`, `specta::Type` e `#[serde(rename_all = "camelCase")]` ou `"snake_case"` conforme a spec).
- [ ] **2.2.** Implementar `src-tauri/crates/mt-commands/src/artisan/root.rs` com `RootResolver`, `is_laravel_composer`, `is_worktree`, `has_git_boundary`, `detect_layout`, `detect_laravel_version` e `detect_console_routes_loaded`.
- [ ] **2.3.** Escrever testes unitários em `src/artisan/root.rs` cobrindo:
  - Raiz Laravel padrão com `artisan` + `composer.json` contendo `laravel/framework`.
  - Subdiretório `app/Http/Controllers` gerando `invoker: "php ../../../artisan"` e `invoker_tokens: ["php", "../../../artisan"]`.
  - Worktree em `.max-code-worktrees/wt-feature` contendo `artisan` resolvendo para a própria worktree (CA-11).
  - Worktree sem `artisan` (ex.: frontend isolado) parando na fronteira `.git` sem vazar para a raiz principal (CA-10).
  - Caminho fora de projeto cadastrado retornando `Ok(None)` em `resolve` e `artisan.root_forbidden` em `ensure_authorized_laravel_root`.

### Passo 3 — Runners e Supervisor de Processos (`runner.rs`)
- [ ] **3.1.** Implementar `LocalPhpRunner`, `SailRunner`, `RunnerSelector` e `run_bounded` em `src-tauri/crates/mt-commands/src/artisan/runner.rs`.
- [ ] **3.2.** Garantir que `run_bounded` crie grupo de processos próprio (`process_group(0)`), drene `stdout` (teto 8 MiB) e `stderr` (ring buffer circular 64 KiB) concorrentemente, aplique `mt_db::sanitizer::sanitize` antes de truncar o diagnóstico a 4 KiB e envie `SIGKILL` ao grupo (`kill(-pgid, SIGKILL)`) + `child.wait().await` em caso de timeout, overflow ou cancelamento.
- [ ] **3.3.** Criar `src-tauri/crates/mt-commands/tests/artisan_runner.rs` testando:
  - Script fake `sleep 30` com timeout curto (ex.: 200 ms) retornando `artisan.timeout` e comprovando via `libc::kill(pgid, 0)` (`ESRCH`) que nenhum processo filho ou neto permaneceu vivo (CA-09, CA-38).
  - Binário PHP inexistente retornando `artisan.php_not_found`.
  - Saída com `exit 1` e `APP_KEY=base64:secret1234567890` + mais de 100 KiB de stderr não causando deadlock e retornando `artisan.exit_nonzero` com o segredo mascarado e `stderrTail.len() <= 4096` (CA-09, CA-38).
  - Saída > 8 MiB retornando `artisan.output_too_large`.

### Passo 4 — Parser do Symfony Console (`parser.rs`) e Fixtures
- [ ] **4.1.** Criar as fixtures `src-tauri/crates/mt-commands/tests/fixtures/artisan_list_laravel11.json`, `artisan_list_laravel10.json` e `maxcode_cache_legacy.json`.
- [ ] **4.2.** Implementar `parse_artisan_list` em `src-tauri/crates/mt-commands/src/artisan/parser.rs` tratando `arguments: []` (array vazio PHP), preservando ordem de argumentos com `IndexMap`, calculando opções globais por interseção, excluindo `hidden: true`, omitindo `shortcut` vazio e mantendo `--` no nome das opções.
- [ ] **4.3.** Adicionar testes unitários em `src/artisan/parser.rs` validando parsing das fixtures Laravel 10 e 11, lixo/warnings PHP antes do `{`, exclusão de `hidden: true`, preservação da ordem dos argumentos e retorno de `artisan.invalid_json` para entradas corrompidas.

### Passo 5 — Cache Compatível, Fingerprint e `ArtisanService` (`cache.rs`, `fingerprint.rs`, `service.rs`)
- [ ] **5.1.** Implementar `compute_fingerprint` em `src-tauri/crates/mt-commands/src/artisan/fingerprint.rs` inspecionando `artisan`, `composer.json`, `composer.lock`, `bootstrap/app.php`, `routes/console.php` e `app/Console/Commands/**/*.php` (máx. 5 000 arquivos, profundidade 6, sem symlinks).
- [ ] **5.2.** Implementar `read_disk_cache` e `write_disk_cache` em `src-tauri/crates/mt-commands/src/artisan/cache.rs`, gravando `.max-code/commands/artisan.json` (array JSON pretty 2 espaços), `.max-code/commands/artisan.meta.json` e `.max-code/.gitignore` (`*\n` via `create_new`).
- [ ] **5.3.** Implementar `ArtisanService` em `src-tauri/crates/mt-commands/src/artisan/service.rs` com single-flight por raiz, revalidação em segundo plano (*stale-while-revalidate*), verificação de hash do `artisan.json` mesmo com memória quente, backoff de 5 s em falhas sem cache e método `invalidate`.
- [ ] **5.4.** Criar `src-tauri/crates/mt-commands/tests/artisan_cache.rs` cobrindo:
  - 10 chamadas `service.get(root)` concorrentes com `FakeRunner` executando o runner **uma única vez** e criando `artisan.json`, `artisan.meta.json` e `.gitignore` sem sobrescrever `.gitignore` existente (CA-05).
  - Verificação do formato de `artisan.json` (array JSON indentado com 2 espaços, compatível com o schema do MaxCode — CA-06).
  - Leitura de `maxcode_cache_legacy.json` (sem sidecar e sem campos estendidos) retornando imediatamente `source: ForeignCache, stale: true` e emitindo `artisan://catalog-updated` após concluir a revalidação em background (CA-07).
  - Edição ou `touch` em `app/Console/Commands/Foo.php` (inclusive arquivo antigo que não tinha maior mtime), `composer.lock` ou `routes/console.php` disparando redescoberta, e segunda chamada sem alteração fazendo 0 execuções do runner (CA-08, CA-37).
  - Sobrescrita externa do `artisan.json` com memória quente sendo detectada como `ForeignCache` (CA-37).

---

## 2. Comandos de Verificação e Validação

Execute a partir de `/home/johnattas/GitHub/MaxTerminal/.max-code-worktrees/wt-6f18b3d0/src-tauri`:

```bash
cargo fmt --package mt-commands -- --check
cargo clippy -p mt-commands --all-targets -- -D warnings
cargo test -p mt-commands --lib
cargo test -p mt-commands --test artisan_runner
cargo test -p mt-commands --test artisan_cache
```

---

## 3. Mapeamento de Critérios de Aceite

| Critério | Cobertura nesta Implementação |
|----------|-------------------------------|
| **CA-01** | `cargo clippy -p mt-commands --all-targets -- -D warnings` e `cargo fmt --check` limpos. |
| **CA-02** | Testes unitários e de integração de descoberta, runner e cache passando em `mt-commands`. |
| **CA-05** | Single-flight (10 chamadas concorrentes → 1 execução) + criação de `artisan.json`, `artisan.meta.json` e `.max-code/.gitignore` (`*\n` exclusivo). |
| **CA-06** | `artisan.json` serializado como array JSON indentado com 2 espaços; `hidden: true` excluídos; `--` mantido; `shortcut` vazio omitido. |
| **CA-07** | Cache legado do MaxCode servido com `stale: true` (`ForeignCache`) e revalidado em background com emissão de `artisan://catalog-updated`. |
| **CA-08** | Invalidação por alteração em `app/Console/Commands/`, `composer.lock`, `routes/console.php`, `bootstrap/app.php` ou `composer.json`. |
| **CA-09** | Timeout mata o grupo de processos (`ESRCH`), binário ausente retorna `artisan.php_not_found`, JSON inválido retorna `artisan.invalid_json`, `exit != 0` retorna `artisan.exit_nonzero` com `stderr` sanitizado (≤ 4 KiB). |
| **CA-10 / CA-11** | `RootResolver` valida `artisan` + `laravel/framework`, resolve worktrees localmente e nunca atravessa fronteira `.git`. |
| **CA-33 / CA-34** | Mensagens de erro e comentários em `pt-BR`, identificadores em `en-US`, zero migrations SQLite. |
| **CA-37 / CA-38** | Fingerprint por conteúdo de cada arquivo, detecção de cache externo mesmo com memória quente, backoff de erro sem loop e drenagem concorrente stdout/stderr sem deadlock. |
