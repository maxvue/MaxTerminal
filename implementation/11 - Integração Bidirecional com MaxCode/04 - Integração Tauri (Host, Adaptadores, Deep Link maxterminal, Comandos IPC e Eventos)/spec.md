# Implementação 04 — Integração Tauri (Host, Adaptadores, Deep Link `maxterminal://`, Comandos IPC e Eventos) (`spec.md`)

> Escopo da Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§2.3, §6.1, §11, §12, §14).
> Depende de: **Implementação 01**, **Implementação 02** e **Implementação 03**.

---

## 1. Contexto, Objetivo e Escopo Exato

Esta é a **quarta unidade de implementação da Fase 11**. Ela conecta o crate puro `mt-bridge` ao binário Tauri v2 (`src-tauri/`), implementando:
1. Camada de adaptadores (`src-tauri/src/bridge/adapters.rs`) para isolar o consumo dos contratos das Fases 01 (`AppPaths`, `MtError`), 03 (`mt_db::sanitizer::sanitize`), 05 (`ShellService::error_context`), 06 (`ProjectRepo` / listagem e busca por caminho), 07 (`WorkspaceManager::open_tab` e `TemplateRepo::list` com seleção estrita de template neutro `Shell` para URIs) e 09 (`SingleInstance` com feature `deep-link`, `QuakeController::show_and_focus`, `ShutdownHooks`).
2. Implementação concreta `TauriBridgeHost` (`src-tauri/src/bridge/host.rs`) da trait `BridgeHost`, emitindo eventos tipados `bridge://*` para a janela principal e coordenando o snapshot `BridgeStatus` com revisão monotônica e confirmação de foco da UI.
3. Roteador e fila de deep links `maxterminal://` (`src-tauri/src/bridge/deep_link.rs`) com barreira de readiness (`projects_ready` + consumidor de UI pronto com timeout de 10 s), fila limitada a 32 itens, deduplicação por `requestId` (TTL 5 min) para `tab/open` e coalescência de 2 s para `project/open`, integrando startup (`get_current()`), `on_open_url` e `single-instance`.
4. Os **9 comandos IPC Tauri** anotados com `#[tauri::command]` e `#[specta::specta]` em `src-tauri/src/commands/bridge.rs` e os **6 eventos `bridge://*`** em `src-tauri/src/events/bridge.rs`.
5. Configuração do `tauri.conf.json` (`plugins.deep-link.desktop.schemes = ["maxterminal"]`), revisão de menor privilégio em `src-tauri/capabilities/default.json`, criação de `packaging/linux/maxterminal.desktop` e regeneração de `src/types/bindings.ts`.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho Exato | Descrição |
|------|---------------|-----------|
| Modificar | `src-tauri/Cargo.toml` | Adicionar dependência `mt-bridge = { path = "crates/mt-bridge" }`, `tauri-plugin-deep-link = "2"` e feature `"deep-link"` em `tauri-plugin-single-instance`. |
| Criar | `src-tauri/src/bridge/mod.rs` | `BridgeStateManaged`, `init(app)` no `setup()` e registro do hook de encerramento em `ShutdownHooks` da Fase 09. |
| Criar | `src-tauri/src/bridge/adapters.rs` | Adaptadores desacoplados para Fases 03, 05, 06, 07 e 09 (`InspectBlockContext`, resolução de template neutro `Shell`, foco de janela e sanitização). |
| Criar | `src-tauri/src/bridge/host.rs` | `TauriBridgeHost` implementando `mt_bridge::handler::BridgeHost`. |
| Criar | `src-tauri/src/bridge/deep_link.rs` | Inicialização do `tauri-plugin-deep-link`, fila de 32 URIs, barreira `projects_ready`, dedup `requestId`/2s e roteamento `maxterminal://`. |
| Criar | `src-tauri/src/events/bridge.rs` | Constantes e emissão tipada dos 6 eventos `bridge://*`. |
| Criar | `src-tauri/src/commands/bridge.rs` | Implementação dos 9 comandos IPC (`bridge_status`, `bridge_get_config`, `bridge_set_config`, `bridge_restart`, `bridge_regenerate_token`, `maxcode_open_plan`, `maxcode_open_project`, `maxcode_inspect_block`, `bridge_notify_active_project`). |
| Modificar | `src-tauri/src/lib.rs` (ou `main.rs` / registro specta) | Registrar `bridge::init`, os 9 comandos IPC e os eventos no builder `tauri_specta`. |
| Modificar | `src-tauri/tauri.conf.json` | Adicionar `"plugins": { "deep-link": { "desktop": { "schemes": ["maxterminal"] } } }`. |
| Modificar | `src-tauri/capabilities/default.json` | Garantir menor privilégio (não expor permissões de registro XDG dinâmico ao webview). |
| Criar | `packaging/linux/maxterminal.desktop` | Desktop Entry oficial com `MimeType=x-scheme-handler/maxterminal;` e `Exec=maxterminal %u`. |
| Modificar | `src/types/bindings.ts` | Arquivo gerado via `npm run bindings` exportando todos os 9 comandos e tipos da ponte. |

---

## 3. Assinaturas Exatas e Completas (Rust e Config)

### 3.1. `src-tauri/src/bridge/adapters.rs`

```rust
use mt_bridge::error::BridgeError;
use mt_bridge::handler::{OpenTabRequest, ProjectRef};
use mt_core::{CommandBlockId, ProjectId, TabId};
use std::path::Path;
use tauri::AppHandle;

#[derive(Debug, Clone)]
pub struct BlockInspectionData {
    pub block_id: CommandBlockId,
    pub command: String,
    pub tail: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
    pub project_path: Option< std::path::PathBuf>,
}

/// Sanitiza texto delegando para `mt_db::sanitizer::sanitize(text).into_owned()` (Fase 03).
pub fn sanitize_secret_text(text: &str) -> String;

/// Lista todos os projetos cadastrados na Fase 06 convertidos para `Vec<ProjectRef>`.
pub async fn list_registered_projects(app: &AppHandle) -> Result<Vec<ProjectRef>, BridgeError>;

/// Busca um projeto pelo caminho canônico exato na Fase 06.
pub async fn find_registered_project_by_path(
    app: &AppHandle,
    canonical: &Path,
) -> Option<ProjectRef>;

/// Busca um projeto pelo `ProjectId` na Fase 06.
pub async fn find_registered_project_by_id(
    app: &AppHandle,
    project_id: ProjectId,
) -> Result<ProjectRef, BridgeError>;

/// Abre uma aba na Fase 07 (`WorkspaceManager::open_tab`):
/// - Quando `req.template` é `Some(name)`: localiza o template com esse nome em `TemplateRepo::list(req.project.id)`
///   e abre a aba com `cwd: req.cwd` e `focus: true`. Se o template não existir, retorna `Err(BridgeError::InvalidMessage)`.
/// - Quando `req.template` é `None` (chamada oriunda de URI não autenticada `maxterminal://tab/open`):
///   procura nos templates do projeto um template neutro cujo `environment.kind == Shell` e `env_vars.is_empty()`,
///   e chama `WorkspaceManager::open_tab` com `overrides: Some(TabOpenOverrides { cwd: req.cwd, initial_command: Some(String::new()) })`.
///   Se não existir template `Shell` neutro sem variáveis de ambiente, retorna `Err(BridgeError::UnsupportedCapability)`
///   (nunca usa um template de Agente CLI nem executa comando inicial a partir de URI).
pub async fn open_workspace_tab(
    app: &AppHandle,
    req: OpenTabRequest,
) -> Result<TabId, BridgeError>;

/// Obtém o `ErrorContext` das últimas 80 linhas na Fase 05 (`ShellService::error_context(block_id, 80)`)
/// e resolve o `project_path` associado à sessão/aba do bloco (mesmo que a aba/sessão já tenha sido arquivada).
pub async fn load_block_inspection_data(
    app: &AppHandle,
    block_id: CommandBlockId,
) -> Result<BlockInspectionData, mt_core::MtError>;

/// Exibe e foca a janela principal delegando ao `QuakeController::show_and_focus` da Fase 09.
pub async fn show_and_focus_main_window(app: &AppHandle);
```

### 3.2. `src-tauri/src/bridge/host.rs`

```rust
use mt_bridge::error::BridgeError;
use mt_bridge::handler::{
    BridgeEvent, BridgeHost, FocusRequest, OpenTabRequest, ProjectRef,
};
use mt_core::TabId;
use std::path::Path;
use tauri::AppHandle;

pub struct TauriBridgeHost {
    app: AppHandle,
    // Canal/ estado para rastrear revisão monotônica e confirmação de foco da UI
}

impl TauriBridgeHost {
    pub fn new(app: AppHandle) -> Self;
}

#[async_trait::async_trait]
impl BridgeHost for TauriBridgeHost {
    async fn list_projects(&self) -> Result<Vec<ProjectRef>, BridgeError>;
    async fn find_project_by_path(&self, canonical: &Path) -> Option<ProjectRef>;
    async fn focus_project(&self, req: FocusRequest) -> Result<(), BridgeError>;
    async fn open_tab(&self, req: OpenTabRequest) -> Result<TabId, BridgeError>;
    async fn show_main_window(&self);
    fn sanitize(&self, text: &str) -> String;
    fn on_event(&self, event: BridgeEvent);
}
```

### 3.3. `src-tauri/src/bridge/deep_link.rs`

```rust
use crate::bridge::host::TauriBridgeHost;
use mt_bridge::BridgeHandle;
use mt_bridge::error::BridgeError;
use mt_core::MtResult;
use std::sync::Arc;
use tauri::AppHandle;

pub const DEEP_LINK_QUEUE_CAPACITY: usize = 32;
pub const PROJECTS_READY_TIMEOUT_SECS: u64 = 10;
pub const TAB_OPEN_DEDUP_TTL_SECS: u64 = 300;
pub const PROJECT_OPEN_COALESCE_MS: u64 = 2000;

pub struct DeepLinkRouter {
    // Fila mpsc(32), watch::Receiver<bool> de readiness, cache de requestId (5 min) e último project/open (2 s)
}

impl DeepLinkRouter {
    /// Enfileira URIs brutas vindas de `get_current()`, `on_open_url` ou do callback `single-instance` da Fase 09.
    /// Nunca loga a URI crua (apenas rota e resultado).
    pub fn enqueue(&self, raw_uris: Vec<String>);

    /// Sinaliza que os repositórios de projetos (Fase 06) e a store do frontend estão prontos para processar URIs.
    pub fn mark_ready(&self);
}

/// Inicializa o roteador de deep links:
/// 1) Em `cfg!(debug_assertions)`, executa `app.deep_link().register_all()` (ignorando falha apenas com log WARN em ambientes headless sem XDG).
/// 2) Registra `app.deep_link().on_open_url(...)` ANTES de ler `app.deep_link().get_current()`.
/// 3) Enfileira URLs iniciais de `get_current()`.
pub fn init(
    app: &AppHandle,
    bridge: BridgeHandle,
    host: Arc<TauriBridgeHost>,
) -> MtResult<Arc<DeepLinkRouter>>;

/// Processa uma URI individual após a barreira `projects_ready`:
/// - `MaxTerminalUri::ProjectOpen { path }`: canonicaliza e resolve worktree (`resolve_worktree_root_cached`),
///   faz `match_project`, aplica coalescência de 2 s, gera `SyncContext { origin: PeerApp::Maxterminal, correlation_id: Uuid::now_v7() }`,
///   chama `host.focus_project` e `host.show_main_window()`.
/// - `MaxTerminalUri::TabOpen { project, cwd, request_id }`: verifica dedup de `request_id` (TTL 5 min),
///   resolve projeto, valida `cwd` com `validate_cwd_within_project`, chama `host.open_tab` com `template: None`
///   (forçando template Shell neutro sem comando inicial), foca o projeto e exibe a janela.
pub async fn route_single_uri(
    host: &TauriBridgeHost,
    bridge: &BridgeHandle,
    raw: &str,
) -> Result<(), BridgeError>;
```

### 3.4. `src-tauri/src/commands/bridge.rs`

```rust
use crate::bridge::BridgeManagerState;
use mt_bridge::client::DeliveryReport;
use mt_bridge::config::BridgeConfig;
use mt_bridge::handler::{BridgeStatus, SyncContext};
use mt_bridge::protocol::ConversationSource;
use mt_core::{CommandBlockId, MtError, ProjectId};
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn bridge_status(state: State<'_, BridgeManagerState>) -> Result<BridgeStatus, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn bridge_get_config(state: State<'_, BridgeManagerState>) -> Result<BridgeConfig, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn bridge_set_config(
    state: State<'_, BridgeManagerState>,
    config: BridgeConfig,
) -> Result<BridgeStatus, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn bridge_restart(state: State<'_, BridgeManagerState>) -> Result<BridgeStatus, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn bridge_regenerate_token(
    state: State<'_, BridgeManagerState>,
) -> Result<BridgeStatus, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn maxcode_open_plan(
    state: State<'_, BridgeManagerState>,
    prompt: String,
    project_path: Option<String>,
    source: Option<ConversationSource>,
) -> Result<DeliveryReport, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn maxcode_open_project(
    state: State<'_, BridgeManagerState>,
    project_path: String,
) -> Result<DeliveryReport, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn maxcode_inspect_block(
    app: tauri::AppHandle,
    state: State<'_, BridgeManagerState>,
    block_id: CommandBlockId,
) -> Result<DeliveryReport, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn bridge_notify_active_project(
    app: tauri::AppHandle,
    state: State<'_, BridgeManagerState>,
    project_id: ProjectId,
    context: SyncContext,
) -> Result<(), MtError>;
```

### 3.5. `src-tauri/src/events/bridge.rs`

```rust
pub const EVENT_BRIDGE_STATUS_CHANGED: &str = "bridge://status-changed";
pub const EVENT_BRIDGE_PEER_CONNECTED: &str = "bridge://peer-connected";
pub const EVENT_BRIDGE_PEER_DISCONNECTED: &str = "bridge://peer-disconnected";
pub const EVENT_BRIDGE_PROJECT_FOCUS_REQUESTED: &str = "bridge://project-focus-requested";
pub const EVENT_BRIDGE_TAB_OPENED: &str = "bridge://tab-opened";
pub const EVENT_BRIDGE_DELIVERY_FAILED: &str = "bridge://delivery-failed";
```

### 3.6. `packaging/linux/maxterminal.desktop`

```ini
[Desktop Entry]
Type=Application
Name=MaxTerminal
Comment=Terminal acelerado por GPU e orquestrador de agentes de IA
Exec=maxterminal %u
Icon=maxterminal
Terminal=false
Categories=System;TerminalEmulator;Development;
MimeType=x-scheme-handler/maxterminal;
StartupWMClass=maxterminal
```

---

## 4. Regras de Negócio, Segurança e Casos de Borda

1. **Inspeção Segura de Bloco (`maxcode_inspect_block`, `CA-28`..`CA-31`)**:
   - O frontend envia apenas o `CommandBlockId`.
   - O comando Rust consulta `load_block_inspection_data(app, block_id)`. Se o bloco não existir, retorna o erro `shell.block_not_found` da Fase 05.
   - Se `exit_code` for `None` ou `Some(0)`, retorna imediatamente `BridgeError::InvalidMessage("O bloco de comando não terminou com erro".into())`.
   - Constrói o prompt via `mt_bridge::prompt::build_inspect_prompt(&data.command, &data.tail, exit_code, &InspectLimits::default(), &sanitize_secret_text)`.
   - Propaga `pre_truncated = data.truncated || inspect_prompt.truncated` para `client.open_plan(..., ConversationSource::Inspect, pre_truncated)` de modo que `DeliveryReport.truncated` reflita qualquer truncamento ocorrido na Fase 05 ou na Fase 11.
2. **Aplicação de `bridge_set_config` a Quente vs Restart (`spec.md` §11)**:
   - Sempre executa `config.validate()` e `config.save(&paths)` primeiro.
   - Se apenas `sync_active_project` mudou (e `enabled`, `preferred_port`, `fallback_range`, `allow_ephemeral_fallback`, `extra_allowed_origins` e `ack_timeout_ms` permaneceram iguais), aplica `bridge.sync().set_enabled(config.sync_active_project)` a quente sem derrubar as conexões WS ativas.
   - Se parâmetros de rede/segurança ou `enabled` mudaram, invoca `bridge.restart(config).await`.
3. **Coordenação Single-Instance + Deep Link + Shutdown (`CA-14`, `CA-17`, `CA-18`, `CA-35`)**:
   - O plugin `single-instance` (Fase 09) permanece único e registrado antes de `tauri_plugin_deep_link`. Qualquer URL `maxterminal://...` recebida via `argv` na segunda instância é encaminhada para `DeepLinkRouter::enqueue` (que deduplica se o plugin `deep-link` também notificar `on_open_url`).
   - Quando `bridge_status` é chamado pela primeira vez pelo `useMaxCodeBridgeStore.init()`, o backend marca a barreira `DeepLinkRouter::mark_ready()`, garantindo que a UI já instalou seus listeners antes de disparar o foco inicial de uma URI de startup.
   - O encerramento real do aplicativo ("Sair" na bandeja do sistema, Fase 09) invoca `bridge.shutdown().await` dentro de `ShutdownHooks`, fechando todas as sessões com código `1001` e removendo `bridge.json`. Minimizar para a bandeja mantém a ponte ativa.
