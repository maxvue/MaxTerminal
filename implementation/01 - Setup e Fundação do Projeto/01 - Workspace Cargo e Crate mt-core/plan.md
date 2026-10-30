# Implementação 01 — Workspace Cargo e Crate `mt-core` (`plan.md`)

> Especificação técnica desta etapa: [`./spec.md`](./spec.md)
> Fase: [`../instructions.md`](../instructions.md) | [`../spec.md`](../spec.md)

---

## 1. Pré-requisitos e Regras Operacionais

- Executar todas as criações e edições de arquivos **estritamente dentro da worktree ativa**.
- Comandos Cargo devem ser executados com diretório de trabalho em `src-tauri/` para que `rust-toolchain.toml` e `rustfmt.toml` sejam aplicados automaticamente.
- Nenhum código pode conter blocos `unsafe` (`unsafe_code = "forbid"`) nem chamadas `.unwrap()` em código de produção (`unwrap_used = "deny"`).
- Todos os comentários de documentação (`///`, `//!`) e mensagens `MtError.message` devem estar em `pt-BR`.

---

## 2. Checklist Passo a Passo de Execução

### Passo 1 — Higiene da Raiz e Configuração de Toolchain
- [ ] **1.1.** Criar `.gitignore` na raiz da worktree cobrindo `.worktrees/`, `.max-code-worktrees/`, `node_modules/`, `dist/`, `coverage/`, `src-tauri/target/`, `src-tauri/gen/*` (com exceção `!src-tauri/gen/schemas/`) e `*.log`.
- [ ] **1.2.** Criar `.editorconfig` na raiz com `charset = utf-8`, `end_of_line = lf`, `indent_size = 4` por padrão e `indent_size = 2` para `*.{json,yml,yaml,toml,md}`.
- [ ] **1.3.** Criar `src-tauri/rust-toolchain.toml` e `src-tauri/rustfmt.toml` conforme especificado em `./spec.md` §2.3 e §2.4.

### Passo 2 — Manifesto do Workspace Cargo
- [ ] **2.1.** Criar `src-tauri/Cargo.toml` definindo `[workspace]` com `resolver = "3"` e `members = [".", "crates/mt-core"]`, `[workspace.package]`, `[workspace.dependencies]` (incluindo `notify = "8"` e `notify-debouncer-full = "0.5"`, consumidos por `workspace = true`) e `[workspace.lints]` (`unsafe_code = "forbid"`, `unwrap_used = "deny"`); declarar `default-run = "maxterminal"` no `[package]` raiz.
- [ ] **2.2.** Criar `src-tauri/crates/mt-core/Cargo.toml` herdando metadados, `edition.workspace = true` e lints do workspace (`publish = false`; todo crate futuro também usa `edition.workspace = true`) e dependendo exclusivamente de `serde`, `serde_json`, `thiserror`, `uuid` e `specta` (sem `tauri`).
- [ ] **2.3.** Caso `src-tauri/src/lib.rs` e `src-tauri/src/main.rs` ainda não existam (implementados definitivamente na Implementação 02), criar um entrypoint mínimo provisório em `src-tauri/src/lib.rs` (`//! Biblioteca principal do MaxTerminal.`) e `src-tauri/src/main.rs` (`fn main() {}`) mais `src-tauri/build.rs` mínimo apenas se necessário para validar a compilação isolada de `mt-core` antes da Implementação 02.

### Passo 3 — Implementação dos Módulos de `mt-core`
- [ ] **3.1.** Implementar `src-tauri/crates/mt-core/src/error.rs` com `ErrorCode`, `pub mod codes` (9 constantes `core.*`), `MtError`, `MtResult<T>`, construtores semânticos (`new`, `with_details`, `with_source`, `internal`, `not_found`, `invalid_argument`, `unsupported`) e conversões `From<std::io::Error>` e `From<serde_json::Error>`.
- [ ] **3.2.** Implementar `src-tauri/crates/mt-core/src/ids.rs` com a macro `define_id!` gerando `ProjectId`, `TabId`, `PaneId`, `PtyId`, `SessionId`, `TabSessionId`, `TemplateId`, `CommandBlockId` e `CommandGroupId` com validação de UUID v7 em `try_from_uuid`, `TryFrom<Uuid>`, `FromStr` e `Deserialize`.
- [ ] **3.3.** Implementar `src-tauri/crates/mt-core/src/paths.rs` com `EnvSource`, `SystemEnv`, `AppPaths::resolve`, `AppPaths::ensure_dirs` (criando diretórios com modo `0o700` sem criar `db_path`) e `AppPathsDto`.
- [ ] **3.4.** Implementar `src-tauri/crates/mt-core/src/app.rs` com `BuildProfile`, `AppInfo` e `FrontendLogEvent` (`as_str` e `static_message`).
- [ ] **3.5.** Implementar `src-tauri/crates/mt-core/src/lib.rs` exportando `app`, `error`, `ids`, `paths` e re-exportando `ErrorCode`, `MtError`, `MtResult`.

### Passo 4 — Testes Unitários de `mt-core`
- [ ] **4.1.** Adicionar módulo `#[cfg(test)] mod tests` em `ids.rs` cobrindo roundtrip dos 9 IDs, rejeição de UUID nil/v4/texto inválido/JSON não-string e ordenação temporal entre timestamps distintos.
- [ ] **4.2.** Adicionar módulo `#[cfg(test)] mod tests` em `error.rs` cobrindo contrato JSON de `MtError`, omissão de `details` quando `None`, omissão estrita de `source` no JSON e mapeamentos de `io::Error` e `serde_json::Error`.
- [ ] **4.3.** Adicionar módulo `#[cfg(test)] mod tests` em `paths.rs` usando `FakeEnv` (em memória, sem `std::env::set_var`) cobrindo `HOME`, `XDG_*_HOME` absolutos/relativos/vazios, ausência de `HOME`, override `MAXTERMINAL_HOME`, permissões `0o700` em diretório temporário da worktree e verificação de que `maxterminal.db` não é criado.
- [ ] **4.4.** Adicionar módulo `#[cfg(test)] mod tests` em `app.rs` validando serialização/desserialização de `FrontendLogEvent`, `BuildProfile` e `AppInfo`.

---

## 3. Comandos de Validação e Mapeamento para Critérios de Aceite

Execute os comandos abaixo para comprovar os critérios de aceite atendidos por esta implementação:

| Critério | Comando de Verificação | Resultado Esperado |
|----------|------------------------|--------------------|
| **CA-11** (Formatação Rust) | `cd src-tauri && cargo fmt --all --check` | Exit code `0` sem diffs de formatação. |
| **CA-12** (Clippy sem warnings) | `cd src-tauri && cargo clippy -p mt-core --all-targets -- -D warnings` | Exit code `0` com zero warnings. |
| **CA-13** (Testes de `mt-core`) | `cd src-tauri && cargo test -p mt-core` | Exit code `0`; todos os testes de `ids`, `error`, `paths` e `app` aprovados. |
| **CA-14** (Membros do workspace) | `cd src-tauri && cargo metadata --format-version 1 --no-deps` | JSON lista exatamente 2 pacotes no workspace: `maxterminal` e `mt-core`; `mt-core` não depende de `tauri`. |
| **CA-15** (Proibição de `unsafe`) | `rg -n "unsafe" src-tauri/crates/mt-core/ src-tauri/Cargo.toml` | Apenas `unsafe_code = "forbid"` em `src-tauri/Cargo.toml`. |
| **CA-21** (Criação XDG sem `.db`) | `cd src-tauri && cargo test -p mt-core paths::tests::ensure_dirs_creates_0700_dirs_and_never_creates_db_file` | Diretórios `config`, `data`, `state`, `cache` e `state/logs` criados com `0o700` e `maxterminal.db` inexistente. |
| **CA-26** (Regras de `.gitignore`) | `git check-ignore .worktrees/x .max-code-worktrees/x node_modules src-tauri/target` | Imprime as 4 entradas e retorna exit code `0`. |
| **CA-27** (Idioma `pt-BR` / `en-US`) | `cd src-tauri && cargo test -p mt-core error::tests` | Mensagens `MtError.message` e doc comments em `pt-BR`; identificadores em `en-US`. |
