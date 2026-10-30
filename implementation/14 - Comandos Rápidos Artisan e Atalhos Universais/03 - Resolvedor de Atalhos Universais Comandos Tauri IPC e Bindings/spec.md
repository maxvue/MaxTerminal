# Implementação 03 — Resolvedor de Atalhos Universais (`/`), Comandos Tauri IPC e Bindings (`spec.md`)

> **Fase:** 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`)
> **Unidade:** `03 - Resolvedor de Atalhos Universais Comandos Tauri IPC e Bindings`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §8.3, [`../instructions.md`](../instructions.md) (I-14, I-16, CA-01, CA-02, CA-04, CA-26 a CA-30, CA-35, CA-40) e [`../spec.md`](../spec.md) (§5, §6, §8, §10.1, §15.1).

---

## 1. Contexto, Objetivo e Escopo

Esta terceira implementação entrega o módulo `mt-commands::shortcuts` para detecção baseada em evidências dos gerenciadores de pacotes do projeto (`Composer`, `Npm`, `Pnpm`, `Yarn`, `Bun`, `Cargo`) e resolução dos 7 Atalhos Universais (`/install`, `/install-force`, `/update`, `/update-force`, `/build`, `/deploy`, `/test`), além de conectar todo o crate `mt-commands` (`artisan`, `generator` e `shortcuts`) ao binário Tauri (`src-tauri/src/`), exportar os bindings TypeScript via `tauri-specta` (`src/types/bindings.ts`) e implementar o cliente IPC tipado (`src/ipc/quickCommands.ts`).

### 1.1. Objetivos Específicos
1. Implementar a detecção de ecossistemas e gerenciadores (`shortcuts/detect.rs`, `shortcuts/manifests.rs`) respeitando fronteiras de projeto e worktree (`.git`), lendo `composer.json`, `package.json` (`packageManager` e `scripts`) e `Cargo.toml` (na raiz ou em `src-tauri/Cargo.toml`) com `read_capped(2 MiB)`.
2. Identificar variantes e capacidades reais de cada gerenciador (`Yarn Classic 1.x` vs `Yarn Modern >= 2`, `pnpm`, `bun` com `bun.lock`/`bun.lockb`, `npm` com `package-lock.json`/`npm-shrinkwrap.json`) e reportar conflitos de múltiplos gerenciadores JS sem evidência conclusiva (`shortcut.manager_conflict`) sem escolha arbitrária (CA-29, CA-35).
3. Implementar o leitor da configuração do MaxCode (`shortcuts/maxcode_config.rs`) lendo `comandoBuild` e `comandoDeploy` de `config-trabalho.json` (`versao: 1`, herança `geral` → `projetos[raiz]`, limite 1 MiB, memorizado por `(path, mtime)`).
4. Implementar a resolução e composição multi-gerenciador (`Composer → JS → Cargo` unidos por ` && ` em `shortcuts/resolve.rs` e `shortcuts/service.rs`), gerando quoting POSIX seguro em duas camadas quando `cwd != root` (`sh -c '...'`) e calculando `evidence_revision` para revalidação no momento da seleção (CA-26 a CA-30, CA-35, CA-40).
5. Integrar `QuickCommandsState`, `TauriCommandsEventSink`, `ProjectLookupImpl`, `MaxCodeConfigLocatorImpl` e os 10 comandos IPC (`artisan_*` e `shortcut_*`) no binário `src-tauri`, regenerar `src/types/bindings.ts` e criar `src/ipc/quickCommands.ts`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Novos Arquivos em `src-tauri/crates/mt-commands/`
- `src-tauri/crates/mt-commands/src/shortcuts/mod.rs`
- `src-tauri/crates/mt-commands/src/shortcuts/model.rs`
- `src-tauri/crates/mt-commands/src/shortcuts/manifests.rs`
- `src-tauri/crates/mt-commands/src/shortcuts/detect.rs`
- `src-tauri/crates/mt-commands/src/shortcuts/maxcode_config.rs`
- `src-tauri/crates/mt-commands/src/shortcuts/resolve.rs`
- `src-tauri/crates/mt-commands/src/shortcuts/service.rs`

### 2.2. Modificações em `src-tauri/crates/mt-commands/`
- `src-tauri/crates/mt-commands/src/lib.rs` — exposição de `pub mod shortcuts;`.

### 2.3. Arquivos a Criar ou Modificar no Binário Tauri (`src-tauri/src/`)
- `src-tauri/src/Cargo.toml` — dependência `mt-commands = { path = "crates/mt-commands" }`.
- `src-tauri/src/commands/quick_commands.rs` (novo) — comandos IPC `artisan_*` e `shortcut_*` + `QuickCommandsState`.
- `src-tauri/src/commands/mod.rs` — exposição de `pub mod quick_commands;`.
- `src-tauri/src/events/quick_commands.rs` (ou em `src-tauri/src/events/mod.rs`) — implementação de `TauriCommandsEventSink`.
- `src-tauri/src/lib.rs` (`setup`) — `app.manage(QuickCommandsState { … })` (estado próprio do domínio; o `AppState` da Fase 01 não agrega serviços e **não** ganha campo `quick_commands`/`commands` — D-09). Os comandos recebem `tauri::State<'_, QuickCommandsState>`.
- `src-tauri/src/specta.rs` e `src-tauri/src/lib.rs` — registro dos 10 comandos IPC e dos 3 eventos (`CatalogUpdatedPayload`, `DiscoveryFailedPayload`, `CommandCreatedPayload`).

### 2.4. Arquivos Frontend a Criar ou Atualizar (`src/`)
- `src/types/bindings.ts` — regenerado na raiz da worktree via `cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings`.
- `src/ipc/quickCommands.ts` (novo) — wrappers tipados sobre `commands.artisan*`, `commands.shortcut*` e listeners de eventos.

### 2.5. Testes e Fixtures a Criar
- `src-tauri/crates/mt-commands/tests/shortcuts.rs`
- `src-tauri/crates/mt-commands/tests/fixtures/shortcuts/laravel-vue/`
- `src-tauri/crates/mt-commands/tests/fixtures/shortcuts/tauri-pnpm/`
- `src-tauri/crates/mt-commands/tests/fixtures/shortcuts/npm-default-test/`
- `src-tauri/crates/mt-commands/tests/fixtures/shortcuts/multi-lock/`
- `src-tauri/crates/mt-commands/tests/fixtures/shortcuts/maxcode-config/config-trabalho.json`

---

## 3. Especificação Técnica e Assinaturas Exatas

### 3.1. Modelos de Atalhos (`src/shortcuts/model.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum ShortcutId {
    Install,
    InstallForce,
    Update,
    UpdateForce,
    Build,
    Deploy,
    Test,
}

impl ShortcutId {
    pub const ALL: [ShortcutId; 7] = [
        Self::Install,
        Self::InstallForce,
        Self::Update,
        Self::UpdateForce,
        Self::Build,
        Self::Deploy,
        Self::Test,
    ];

    pub fn label(self) -> &'static str;
    pub fn description_pt_br(self) -> &'static str;
    pub fn requires_confirmation(self) -> bool; // true para InstallForce, UpdateForce e Deploy
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PackageManager {
    Composer,
    Npm,
    Pnpm,
    Yarn,
    Bun,
    Cargo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum YarnVariant {
    Classic,
    Modern,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ManagerEvidence {
    pub path: String,
    pub field: Option<String>,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DetectedManager {
    pub manager: PackageManager,
    pub manifest: String,
    pub lockfile: Option<String>,
    pub version: Option<String>,
    pub evidence: Vec<ManagerEvidence>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PartSource {
    Table,
    PackageScript,
    ComposerScript,
    Artisan,
    VendorBin,
    MaxCodeConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutPart {
    pub manager: Option<PackageManager>,
    pub cwd: String,
    pub program: Option<String>,
    pub args: Vec<String>,
    pub configured_shell: Option<String>,
    pub line: String,
    pub source: PartSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedShortcut {
    pub id: ShortcutId,
    pub label: String,
    pub description: String,
    pub line: Option<String>,
    pub parts: Vec<ShortcutPart>,
    pub disabled_reason: Option<String>,
    pub requires_confirmation: bool,
    pub evidence_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutList {
    pub root: String,
    pub managers: Vec<DetectedManager>,
    pub warnings: Vec<String>,
    pub items: Vec<ResolvedShortcut>,
}
```

### 3.2. Detecção de Manifestos, Evidências e Capacidades (`manifests.rs`, `detect.rs`)

```rust
#[derive(Debug, Clone)]
pub struct JsResolution {
    pub selected: Option<DetectedManager>,
    pub yarn_variant: Option<YarnVariant>,
    pub conflict_reason: Option<String>,
    pub manifest_error: Option<String>,
    pub warnings: Vec<String>,
    pub scripts: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct EcosystemSnapshot {
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub composer: Option<ComposerSnapshot>,
    pub js: Option<JsResolution>,
    pub cargo: Option<CargoSnapshot>,
    pub evidence_revision: String,
}

pub fn detect_ecosystem(
    cwd: &Path,
    lookup: &dyn ProjectLookup,
) -> MtResult<EcosystemSnapshot>;
```

**Regras de Negócio da Detecção (`detect.rs`, `manifests.rs` — CA-26 a CA-29, CA-35, CA-40):**
1. **Fronteira da Raiz:** Sobe a partir de `canonicalize(cwd)` até a fronteira do projeto cadastrado em `lookup` ou até encontrar `.git` (arquivo ou diretório). Escolhe o primeiro ancestral contendo `composer.json`, `package.json`, `Cargo.toml` ou `src-tauri/Cargo.toml`. Nunca atravessa `.git` para mesclar manifestos de fora da worktree.
2. **Detecção JS (`package.json`):**
   - Lê `package.json` com `read_capped(2 MiB)`. Se o JSON for inválido, registra `manifest_error` ("package.json contém JSON inválido") e desabilita atalhos que dependem de JS sem cair para outro gerenciador.
   - Avalia `packageManager` (ex.: `"pnpm@9.12.0+sha256..."`, `"yarn@1.22.22"`, `"yarn@4.5.0"`, `"npm@10.8.0"`, `"bun@1.1.30"`):
     - Nome deve ser `npm`, `pnpm`, `yarn` ou `bun` seguido de `@` e semver válido.
     - Se `packageManager` contiver valor malformado ou gerenciador desconhecido, registra `conflict_reason` ("Campo packageManager inválido em package.json") e desabilita a parte JS (não faz fallback para npm — CA-29).
     - Se `packageManager` for válido e houver lockfiles de outro gerenciador JS no diretório, mantém o gerenciador declarado em `packageManager` e adiciona aviso em `warnings` listando os lockfiles divergentes (CA-29).
   - Quando `packageManager` não está declarado:
     - Inspeciona lockfiles por família de gerenciador:
       - `npm`: `npm-shrinkwrap.json` (prevalece sobre `package-lock.json`) ou `package-lock.json`.
       - `pnpm`: `pnpm-lock.yaml`.
       - `yarn`: `yarn.lock`.
       - `bun`: `bun.lock` ou `bun.lockb` (ambos representam apenas `Bun`, nunca conflito entre si).
     - Se **exatamente 1** família tiver lockfile, seleciona esse gerenciador.
     - Se **2 ou mais** famílias diferentes tiverem lockfile (ex.: `package-lock.json` e `pnpm-lock.yaml`), define `conflict_reason = Some("Conflito de lockfiles JS detectado (package-lock.json, pnpm-lock.yaml) sem packageManager declarado.")` (CA-29).
     - Se **nenhum** lockfile existir e `packageManager` estiver ausente, seleciona `Npm` com `reason: "Apenas package.json presente (fallback padrão npm)"` (CA-29).
   - **Variante do Yarn (`YarnVariant`):**
     - `Classic` se versão major em `packageManager` for `1`, ou se `yarn.lock` contiver `# yarn lockfile v1` e não houver `.yarnrc.yml`.
     - `Modern` se versão major em `packageManager` for `>= 2`, ou se `yarn.lock` contiver `__metadata:` / `.yarnrc.yml` existir sem conflito.
     - `Unknown` se contraditório (ex.: `# yarn lockfile v1` com `.yarnrc.yml` apontando para Berry sem `packageManager`).
   - **Filtro do script `test` padrão do `npm init`:** Se `scripts["test"]` (após `trim`) for exatamente `echo "Error: no test specified" && exit 1`, é ignorado como se não existisse (CA-28).

### 3.3. Configuração do MaxCode (`src/shortcuts/maxcode_config.rs`)

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaxCodeProjectOverrides {
    pub build_command: Option<String>,
    pub deploy_command: Option<String>,
    pub source_path: Option<PathBuf>,
    pub ambiguous_warning: Option<String>,
}

pub struct MaxCodeConfigReader {
    locator: Arc<dyn MaxCodeConfigLocator>,
    cache: DashMap<PathBuf, (i64, u64, ParsedMaxCodeFile)>,
}

impl MaxCodeConfigReader {
    pub fn new(locator: Arc<dyn MaxCodeConfigLocator>) -> Self;
    pub fn resolve_for_root(&self, root: &Path) -> MaxCodeProjectOverrides;
}
```

- Lê candidatos de `locator.config_files()` com `read_capped(1 MiB)`.
- Exige `"versao": 1`. Calcula o valor efetivo de `comandoBuild` e `comandoDeploy` aplicando `projetos[root_str]` sobre `geral`:
  - Campo ausente em `projetos[root_str]` herda de `geral`.
  - Campo presente com string vazia `""` em `projetos[root_str]` limpa o override herdado de `geral` (retorna `None`), permitindo fallback para o script do manifesto (`package.json`/`composer.json`).
- Arquivos ausentes, inválidos ou `> 1 MiB` são ignorados silenciosamente sem erro (CA-30).

### 3.4. Resolução dos 7 Atalhos (`src/shortcuts/resolve.rs`, `src/shortcuts/service.rs`)

```rust
pub struct ShortcutService {
    lookup: Arc<dyn ProjectLookup>,
    artisan: Arc<ArtisanService>,
    roots: Arc<RootResolver>,
    maxcode_config: Arc<MaxCodeConfigReader>,
    cli_checker: Arc<dyn Fn(&str) -> bool + Send + Sync>, // padrão: verifica binário no PATH
}

impl ShortcutService {
    pub fn new(
        lookup: Arc<dyn ProjectLookup>,
        artisan: Arc<ArtisanService>,
        roots: Arc<RootResolver>,
        maxcode_config: Arc<MaxCodeConfigReader>,
    ) -> Self;

    pub fn with_cli_checker(
        self,
        checker: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    ) -> Self;

    pub async fn list(&self, cwd: &Path) -> MtResult<ShortcutList>;
    pub async fn resolve(&self, cwd: &Path, id: ShortcutId) -> MtResult<ResolvedShortcut>;
}
```

**Regras Exatas da Tabela de Resolução (`resolve.rs` — CA-26 a CA-30, CA-35, CA-40):**
1. **Ordem de Composição:** `Composer` → `Gerenciador JS` → `Cargo`, unidos por `" && "`.
2. **Regra de Invalidação Total em Conflito/Erro:** Se o projeto possui `package.json` e a parte JS é necessária para `/install`, `/install-force`, `/update` ou `/update-force`, mas há `conflict_reason`, `manifest_error` ou CLI ausente, o atalho inteiro fica desabilitado (`line: None`, `disabled_reason: Some(...)`) — nunca executa silenciosamente apenas `composer install` omitindo a parte JS conflitante (CA-40).
3. **Matriz de Capacidades para `install` / `install-force` / `update` / `update-force`:**
   - `Composer`:
     - `/install`: `composer install`
     - `/install-force`: `composer install --no-cache`
     - `/update`: `composer update`
     - `/update-force`: `composer update --no-cache`
   - `Npm`:
     - `/install`: `npm install`
     - `/install-force`: `npm install --force`
     - `/update`: `npm update`
     - `/update-force`: `npm update --force`
   - `Pnpm`:
     - `/install`: `pnpm install`
     - `/install-force`: `pnpm install --force`
     - `/update`: `pnpm update`
     - `/update-force`: desabilitado com motivo `"O pnpm não possui flag --force validada para update"` (CA-35).
   - `Yarn`:
     - `YarnVariant::Classic`:
       - `/install`: `yarn install`
       - `/install-force`: `yarn install --force`
       - `/update`: `yarn upgrade`
       - `/update-force`: desabilitado com motivo `"O Yarn Classic não possui flag --force segura para upgrade"` (CA-35).
     - `YarnVariant::Modern`:
       - `/install`: `yarn install`
       - `/install-force`: desabilitado com motivo `"O Yarn Modern não suporta install --force"` (CA-35).
       - `/update`: `yarn up -R '*'`
       - `/update-force`: desabilitado com motivo `"O Yarn Modern não suporta update --force"` (CA-35).
     - `YarnVariant::Unknown`: `/install` resolve `yarn install`; demais dependentes da variante ficam desabilitados com motivo `"Variante do Yarn (Classic vs Modern) ambígua"`.
   - `Bun`:
     - `/install`: `bun install`
     - `/install-force`: `bun install --force`
     - `/update`: `bun update`
     - `/update-force`: desabilitado com motivo `"O Bun não possui flag --force validada para update"` (CA-35).
   - `Cargo` (`$M` = `["--manifest-path", "src-tauri/Cargo.toml"]` quando em `src-tauri/`, vazio quando na raiz):
     - `/install` e `/install-force`: `cargo build` (+ `$M`).
     - `/update` e `/update-force`: `cargo update` (+ `$M`).
4. **Resolução de `/build`:**
   - 1º: `comandoBuild` não-vazio do MaxCode (`PartSource::MaxCodeConfig`, exclusivo).
   - 2º: script `"build"` não-vazio em `package.json` → `<pm> run build` (`PartSource::PackageScript`).
   - 3º: script `"build"` não-vazio em `composer.json` → `composer run-script build` (`PartSource::ComposerScript`).
   - 4º: `Cargo` (somente se não houver build JS) → `cargo build --release` (+ `$M`) (`PartSource::Table`).
   - Senão: desabilitado com `disabled_reason: Some("Nenhum comando de build encontrado".into())`.
5. **Resolução de `/deploy` (CA-35):**
   - 1º: `comandoDeploy` não-vazio do MaxCode (`PartSource::MaxCodeConfig`, exclusivo).
   - 2º: script `"deploy"` não-vazio em `package.json` → `<pm> run deploy` (`PartSource::PackageScript`).
   - 3º: script `"deploy"` não-vazio em `composer.json` → `composer run-script deploy` (`PartSource::ComposerScript`).
   - Senão: desabilitado com `disabled_reason: Some("Nenhum comando de deploy encontrado".into())` (nunca infere `cargo publish`, SSH ou migrações).
6. **Resolução de `/test` (composição — CA-26, CA-27, CA-28):**
   - Parte PHP (primeira aplicável):
     - Se for raiz Laravel e o catálogo Artisan possuir o comando `"test"` (ou `vendor/laravel/framework` + `artisan` com suporte a test): `<invoker_root> test` (`php artisan test`, `PartSource::Artisan`).
     - Senão, se `composer.json` tiver script `"test"` não-vazio: `composer run-script test` (`PartSource::ComposerScript`).
     - Senão, se `vendor/bin/pest` existir como arquivo na raiz: `./vendor/bin/pest` (`PartSource::VendorBin`).
     - Senão, se `vendor/bin/phpunit` existir como arquivo na raiz: `./vendor/bin/phpunit` (`PartSource::VendorBin`).
   - **E** Parte JS: se `package.json` tiver script `"test"` válido (não vazio e diferente do padrão `npm init`): `<pm> run test` (`PartSource::PackageScript`).
   - **E** Parte Cargo: se houver `Cargo.toml` (raiz ou `src-tauri/Cargo.toml`): `cargo test` (+ `$M`) (`PartSource::Table`).
   - Se nenhuma das 3 partes existir: desabilitado com `disabled_reason: Some("Nenhum comando de teste encontrado".into())` (CA-28).
7. **Renderização quando `cwd != root` (CA-40):**
   - Quando `canonical_cwd == root`: `line` é a junção das partes por `" && "` (ex.: `"composer install && npm install"`).
   - Quando `canonical_cwd != root` (ex.: usuário está em subpasta `packages/web` ou `app/Models`, mas a raiz resolvida é `root`): envolve a cadeia em `sh -c <posix_quote("cd -- <posix_quote(root)> && <joined_parts>")>` para não alterar o `cwd` do shell interativo da aba e proteger caminhos com espaços ou apóstrofos em bash/zsh/fish.

### 3.5. Comandos Tauri IPC (`src-tauri/src/commands/quick_commands.rs`)

Cada domínio registra o próprio estado via `app.manage(...)` (D-09): `QuickCommandsState` (Fase 14) é gerenciado no `setup` e a Fase 15 registra, separadamente, `AppCommandGroupState`. Os dois estados não se referenciam e nenhum deles é campo de `AppState`.

```rust
pub struct QuickCommandsState {
    pub roots: Arc<RootResolver>,
    pub artisan: Arc<ArtisanService>,
    pub generator: Arc<CommandGenerator>,
    pub shortcuts: Arc<ShortcutService>,
    pub runners: Arc<RunnerSelector>,
}

#[tauri::command]
#[specta::specta]
pub async fn artisan_resolve_root(
    state: tauri::State<'_, QuickCommandsState>,
    cwd: String,
    project_id: Option<ProjectId>,
) -> Result<Option<ArtisanRoot>, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn artisan_catalog_get(
    state: tauri::State<'_, QuickCommandsState>,
    root: String,
) -> Result<ArtisanCatalog, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn artisan_catalog_refresh(
    state: tauri::State<'_, QuickCommandsState>,
    root: String,
) -> Result<ArtisanCatalog, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn artisan_form_schema(
    state: tauri::State<'_, QuickCommandsState>,
) -> Result<serde_json::Value, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn artisan_validate_form(
    state: tauri::State<'_, QuickCommandsState>,
    root: String,
    form: ArtisanCommandForm,
) -> Result<ValidationReport, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn artisan_preview_command(
    state: tauri::State<'_, QuickCommandsState>,
    root: String,
    form: ArtisanCommandForm,
) -> Result<ArtisanPreview, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn artisan_create_command(
    state: tauri::State<'_, QuickCommandsState>,
    root: String,
    form: ArtisanCommandForm,
    expected_revision: String,
) -> Result<ArtisanCreateResult, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn artisan_format_maxcode_prompt(
    state: tauri::State<'_, QuickCommandsState>,
    root: String,
    form: ArtisanCommandForm,
) -> Result<String, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn shortcut_list(
    state: tauri::State<'_, QuickCommandsState>,
    cwd: String,
) -> Result<ShortcutList, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn shortcut_resolve(
    state: tauri::State<'_, QuickCommandsState>,
    cwd: String,
    id: ShortcutId,
) -> Result<ResolvedShortcut, MtError>;
```

### 3.6. Cliente IPC Frontend (`src/ipc/quickCommands.ts`)

```ts
import {
    commands,
    type ArtisanCatalog,
    type ArtisanCommandForm,
    type ArtisanCreateResult,
    type ArtisanPreview,
    type ArtisanRoot,
    type CatalogUpdatedPayload,
    type CommandCreatedPayload,
    type DiscoveryFailedPayload,
    type ProjectId,
    type ResolvedShortcut,
    type ShortcutId,
    type ShortcutList,
    type ValidationReport
} from '@/types/bindings';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from './core';

export const quickCommandsIpc = {
    resolveRoot: (cwd: string, projectId: ProjectId | null): Promise<ArtisanRoot | null> =>
        unwrap(commands.artisanResolveRoot(cwd, projectId), 'artisan_resolve_root'),
    catalogGet: (root: string): Promise<ArtisanCatalog> =>
        unwrap(commands.artisanCatalogGet(root), 'artisan_catalog_get'),
    catalogRefresh: (root: string): Promise<ArtisanCatalog> =>
        unwrap(commands.artisanCatalogRefresh(root), 'artisan_catalog_refresh'),
    formSchema: (): Promise<Record<string, unknown>> =>
        unwrap(commands.artisanFormSchema(), 'artisan_form_schema') as Promise<Record<string, unknown>>,
    validateForm: (root: string, form: ArtisanCommandForm): Promise<ValidationReport> =>
        unwrap(commands.artisanValidateForm(root, form), 'artisan_validate_form'),
    preview: (root: string, form: ArtisanCommandForm): Promise<ArtisanPreview> =>
        unwrap(commands.artisanPreviewCommand(root, form), 'artisan_preview_command'),
    create: (root: string, form: ArtisanCommandForm, expectedRevision: string): Promise<ArtisanCreateResult> =>
        unwrap(commands.artisanCreateCommand(root, form, expectedRevision), 'artisan_create_command'),
    maxcodePrompt: (root: string, form: ArtisanCommandForm): Promise<string> =>
        unwrap(commands.artisanFormatMaxcodePrompt(root, form), 'artisan_format_maxcode_prompt'),
    shortcutList: (cwd: string): Promise<ShortcutList> =>
        unwrap(commands.shortcutList(cwd), 'shortcut_list'),
    shortcutResolve: (cwd: string, id: ShortcutId): Promise<ResolvedShortcut> =>
        unwrap(commands.shortcutResolve(cwd, id), 'shortcut_resolve')
};

export const onCatalogUpdated = (cb: (payload: CatalogUpdatedPayload) => void): Promise<UnlistenFn> =>
    listen<CatalogUpdatedPayload>('artisan://catalog-updated', (event) => cb(event.payload));

export const onDiscoveryFailed = (cb: (payload: DiscoveryFailedPayload) => void): Promise<UnlistenFn> =>
    listen<DiscoveryFailedPayload>('artisan://discovery-failed', (event) => cb(event.payload));

export const onCommandCreated = (cb: (payload: CommandCreatedPayload) => void): Promise<UnlistenFn> =>
    listen<CommandCreatedPayload>('artisan://command-created', (event) => cb(event.payload));
```

---

## 4. Tratamento de Erros e Casos de Borda

1. **Caminhos com espaços e apóstrofos (`'/home/user/Meu Projeto d'Ávila'`):** O quoting POSIX duplo em `sh -c` substitui cada `'` por `'\''` em cada camada antes de envolver com aspas simples (CA-40).
2. **`yarn up -R '*'` no Yarn Modern:** O argumento `'*'` é renderizado com aspas simples literais para evitar globbing pelo shell (`bash`/`zsh`/`fish`).
3. **Múltiplos arquivos `config-trabalho.json` divergentes (Code vs VSCodium):** Se mais de um arquivo candidato existir e declarar valores diferentes para a mesma raiz de projeto sem uma origem primária definida pela Fase 11, registra `ambiguous_warning` em `ShortcutList.warnings` e ignora o override ambíguo sem escolher arbitrariamente.
