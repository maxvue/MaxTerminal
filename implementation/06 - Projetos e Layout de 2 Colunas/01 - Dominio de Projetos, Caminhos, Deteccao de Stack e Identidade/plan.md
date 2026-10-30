# Implementação 01 — Plano de Execução Passo a Passo (`plan.md`)

> Parte 01 de 05 da **Fase 06: Projetos e Layout de 2 Colunas**.
> Especificação técnica correspondente: [`./spec.md`](./spec.md).

---

## 1. Checklist Sequencial de Implementação

### Passo 1 — Criar o crate `mt-projects` e registrá-lo no Workspace Cargo
1. Editar `src-tauri/Cargo.toml` para incluir `"crates/mt-projects"` na lista `members` de `[workspace]`.
2. Criar `src-tauri/crates/mt-projects/Cargo.toml` com as dependências definidas em `spec.md` §3.1 (`mt-core`, `mt-db`, `serde`, `serde_json`, `specta`, `tokio`, `tracing`, `notify = { workspace = true }`, `notify-debouncer-full = { workspace = true }` (versões 8 / 0.5 centralizadas na Fase 01), `dirs = "6"`, `async-trait = "0.1"`, `uuid`, e `tempfile = "3"` em `[dev-dependencies]`).
3. Se necessário para flags POSIX seguras em `detect.rs` (`O_NOFOLLOW`, `O_NONBLOCK`), utilizar `std::os::unix::fs::OpenOptionsExt` com constantes de `libc` já presentes na árvore de dependências do workspace.

### Passo 2 — Implementar Modelos e DTOs (`src-tauri/crates/mt-projects/src/model.rs`)
1. Implementar `ProjectRootId(pub uuid::Uuid)` com `#[serde(transparent)]`, `Type` do `specta`, `Display`, `FromStr` e `Default`.
2. Implementar os enums `StackKind`, `IdentitySource`, `DiscoveryMode` e `WatchStatus` com `#[serde(rename_all = "snake_case")]`, `Type` e métodos auxiliares `as_str()` / `from_db_str()`.
3. Implementar `ProjectIdentity`, `Project`, `ProjectRoot`, `NewProject`, `ProjectPatch`, `NewProjectRoot`, `ProjectRootPatch`, `DiscoveredFolder`, `ScanWarning`, `RootImportReport` e `ProjectSnapshot` com `#[serde(rename_all = "camelCase")]` e `specta::Type`.

### Passo 3 — Implementar Manipulação e Validação de Caminhos (`src-tauri/crates/mt-projects/src/paths.rs`)
1. Implementar `expand_home(input: &str) -> Result<PathBuf, MtError>`:
   - Rejeitar strings vazias ou contendo `'\0'` com código `"project.invalid_argument"`.
   - Expandir `"~"` e `"~/..."` usando `dirs::home_dir()`.
   - Rejeitar `"~usuario"` e variáveis `"$"` não expandidas ou caminhos relativos com `"project.invalid_argument"`.
2. Implementar `canonical_dir(input: &str) -> Result<PathBuf, MtError>`:
   - Chamar `expand_home`, verificar existência e se é diretório antes de `std::fs::canonicalize`, mapeando `NotFound` para `"project.path_not_found"` e não-diretório para `"project.path_not_directory"`.
   - Validar conversão UTF-8 (`to_str()`), retornando `"project.path_not_utf8"` caso contenha bytes inválidos.
3. Implementar `validate_project_dir` (bloqueia `/` e `$HOME` com `"project.path_forbidden"`) e `validate_root_dir` (bloqueia `/` com `"project.path_forbidden"`, permite `$HOME`).
4. Implementar `is_hidden`, `is_inside_excluded_discovery_dir` (filtra `.git`, `.worktrees`, `.max-code-worktrees`) e `default_title`.

### Passo 4 — Implementar Motor de Auto-detecção de Stack (`src-tauri/crates/mt-projects/src/detect.rs`)
1. Implementar `FolderProbe::read(dir: &Path)`:
   - Inspecionar marcadores com `symlink_metadata`: `artisan`, `composer.json`, `pyproject.toml`, `requirements.txt`, `Cargo.toml`.
   - Inspecionar `src-tauri/tauri.conf.json`, `src-tauri/tauri.conf.json5` e `src-tauri/Tauri.toml`.
   - Implementar leitura protegida de `package.json` (somente arquivo regular, `O_NOFOLLOW | O_NONBLOCK`, `take(MAX_PACKAGE_JSON_BYTES + 1)`), registrando `ScanWarning` quando exceder 1 MiB ou contiver JSON inválido.
2. Implementar as 6 structs que implementam `StackRule` na ordem estrita:
   - `TauriRule` → `VscodeExtensionRule` → `LaravelRule` → `VueRule` → `RustRule` → `PythonRule`.
3. Implementar `StackDetector::default_rules()`, `detect(&self, dir: &Path)` e `detect_from_probe(&self, probe: FolderProbe)` retornando `vec![StackKind::Unknown]` quando nenhuma regra casar.

### Passo 5 — Implementar Mapa de Identidade, Hash FNV-1a e Validadores (`src-tauri/crates/mt-projects/src/identity.rs`)
1. Definir constantes `FALLBACK_ICONS` (12 ícones `mdi:folder*`) e `FALLBACK_COLORS` (12 cores hexadecimais).
2. Implementar `fnv1a_64(bytes: &[u8]) -> u64` usando offset `14695981039346656037` e primo `1099511628211`.
3. Implementar `IdentityMap::for_stack`, `IdentityMap::fallback` e `IdentityMap::resolve`.
4. Implementar `validate_icon`, `validate_color` (normalizando para `#RRGGBB` maiúsculo) e `validate_title` (trim, `1..=120` chars, sem caracteres de controle).

### Passo 6 — Implementar Eventos de Domínio (`src-tauri/crates/mt-projects/src/events.rs`) e `lib.rs`
1. Implementar `ProjectDiscoveredPayload`, `RemovedKind`, `ProjectRemovedPayload`, `ProjectChangedPayload`, `ProjectRootChangedPayload`, `ProjectEvent<T>` e a trait `ProjectEventSink`.
2. Configurar `src-tauri/crates/mt-projects/src/lib.rs` exportando `pub mod detect; pub mod events; pub mod identity; pub mod model; pub mod paths;`.

### Passo 7 — Escrever Testes de Integração e Unitários (`src-tauri/crates/mt-projects/tests/detect_and_identity.rs`)
1. Criar testes cobrindo cada stack isolada em `tempfile::TempDir`:
   - `tauri` (nas 3 variantes: `tauri.conf.json`, `tauri.conf.json5`, `Tauri.toml`), `vscode_extension`, `laravel`, `vue` (em `dependencies`, `devDependencies` e `peerDependencies`), `rust`, `python` (`pyproject.toml` e `requirements.txt`), e pasta vazia (`unknown`).
2. Criar testes para combinações de prioridade:
   - Tauri + Vue + Rust → `detected = [Tauri, Vue, Rust]`, primária `Tauri` (`logos:tauri`, `#FFC131`).
   - Laravel + Vue → `detected = [Laravel, Vue]`, primária `Laravel` (`logos:laravel`, `#FF2D20`).
   - VS Code Extension + Vue → `detected = [VscodeExtension, Vue]`, primária `VscodeExtension` (`logos:visual-studio-code`, `#007ACC`).
   - Rust + Python → `detected = [Rust, Python]`, primária `Rust` (`logos:rust`, `#CE422B`).
   - `artisan` sem `composer.json` → não detecta `Laravel`.
3. Criar testes para casos de borda em `package.json`:
   - JSON malformado, arquivo > 1 MiB (1 MiB + 10 bytes), diretório chamado `package.json` e symlink apontando para fora.
4. Criar testes para `identity.rs` e `paths.rs`:
   - Vetores determinísticos fixos de `fnv1a_64` e `IdentityMap::fallback`.
   - Validação de ícones válidos/inválidos, normalização de cores (`#ff2d20` → `#FF2D20`), títulos com caracteres de controle ou > 120 chars.
   - Rejeição de `/`, `$HOME` (para projetos), caminhos relativos e expansão correta de `~/...`.
5. Criar teste de serialização JSON dos DTOs e `ProjectEvent<T>` verificando chaves `camelCase` e variantes `snake_case`.

---

## 2. Comandos de Verificação

```bash
cd "$(git rev-parse --show-toplevel)"

# Compilar o crate mt-projects
cargo check --manifest-path src-tauri/Cargo.toml -p mt-projects

# Executar todos os testes unitários e de integração do crate mt-projects
cargo test --manifest-path src-tauri/Cargo.toml -p mt-projects

# Validar ausência de warnings no Clippy
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-projects --all-targets -- -D warnings

# Verificar garantia de segurança: nenhuma invocação de processo externo
! rg -n "process::Command" src-tauri/crates/mt-projects/src
```

---

## 3. Critérios de Aceite desta Implementação (Mapeamento Fase 06)

- **CA-1 (Completo):** `cargo test -p mt-projects` passa cobrindo cada stack isolada, combinações de prioridade (`Tauri+Vue+Rust -> tauri`, `Laravel+Vue -> laravel`, `VS Code+Vue -> vscode_extension`, `Rust+Python -> rust`), pasta vazia (`unknown`) e `package.json` inválido ou > 1 MiB tratado sem pânico.
- **CA-2 (Parcial — núcleo de cálculo e validação):** Vetores fixos de FNV-1a 64 bits verificados e validadores de ícone/cor/título testados.
- **CA-5 (Parcial):** `cargo clippy -p mt-projects -- -D warnings` passa sem avisos.
- **CA-6 (Parcial — serialização):** Fixtures de serialização confirmam `camelCase` nos structs e `snake_case` nos enums.
