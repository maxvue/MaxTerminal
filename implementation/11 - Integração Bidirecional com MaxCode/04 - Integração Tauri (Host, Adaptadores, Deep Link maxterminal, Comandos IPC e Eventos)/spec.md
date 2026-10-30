# Implementação 04 — Integração Tauri (Host, Adaptadores, Deep Link `maxterminal://`, Comandos IPC e Eventos) (`spec.md`)

> Escopo da Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§2.3, §6.1, §11, §12, §14).
> Depende de: **Implementação 01**, **Implementação 02** e **Implementação 03**.

---

## 1. Contexto, Objetivo e Escopo Exato

Esta é a **quarta unidade de implementação da Fase 11**. Ela conecta o crate puro `mt-bridge` ao binário Tauri v2 (`src-tauri/`), implementando:
1. Camada de adaptadores (`src-tauri/src/bridge/adapters.rs`) para isolar o consumo dos contratos das Fases 01 (`AppPaths`, `MtError`), 03 (`mt_db::sanitizer::sanitize`), 05 (`ShellService::error_context`), 06 (`ProjectRepo` / listagem e busca por caminho), 07 (`WorkspaceManager::open_tab` e `TemplateRepo::list` com seleção estrita de template neutro `Shell` para URIs) e 09 (`SingleInstance` com feature `deep-link`, `QuakeController::show_and_focus`, `ShutdownHooks`).
2. Implementação concreta `TauriBridgeHost` (`src-tauri/src/bridge/host.rs`) da trait `BridgeHost`, emitindo eventos tipados `bridge://*` para a janela principal e coordenando o snapshot `BridgeStatus` com revisão monotônica e confirmação de foco da UI.
3. `BridgeActivationHandler` para deep links `maxterminal://` (`src-tauri/src/bridge/deep_link.rs`), registrado no único `ActivationRouter` da Fase 09. Consumir sua fila de 32 envelopes/TTL 30 s; coordenar readiness de projetos/workspace/UI (timeout UI 10 s), deduplicar por `requestId` (TTL 5 min) para `tab/open` e coalescer foco por 2 s. Implementar também `BridgeEditorOpener` registrado via `EditorLauncher::set_bridge_opener`.
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
| Criar | `src-tauri/src/bridge/deep_link.rs` | `BridgeActivationHandler`, registro no roteador da 09, barreira de prontidão conjunta e dedup de negócio; nenhum listener/plugin/fila de transporte adicional. |
| Criar | `src-tauri/src/bridge/editor.rs` | `BridgeEditorOpener`, composição com `EditorLauncher::set_bridge_opener`, consumo de `MaxCodeClient::open_file`. |
| Modificar | `src-tauri/crates/mt-bridge/src/{protocol,client}.rs` | Extensão proposta `editor_open_v1` / `maxcode.open_file`, payload e método `open_file`, condicionados ao adaptador validado e ao handshake. |
| Criar | `src-tauri/tests/bridge_os_adapters.rs` | Contratos `ActivationHandler`/`EditorOpener`, dedup, prontidão, camelCase, fallback e registro idempotente. |
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

pub const PROJECTS_READY_TIMEOUT_SECS: u64 = 10;
pub const TAB_OPEN_DEDUP_TTL_SECS: u64 = 300;
pub const PROJECT_OPEN_COALESCE_MS: u64 = 2000;

pub struct BridgeActivationHandler {
    // Host, bridge, barreira de prontidão e cache de dedup de negócio; transporte/fila pertencem à 09.
}

impl BridgeActivationHandler {
    /// Sinaliza consumidor de UI pronto; só a composição conjunta libera ActivationRouter::set_ready().
    pub fn mark_ready(&self);
}

#[async_trait::async_trait]
impl mt_os::activation::ActivationHandler for BridgeActivationHandler {
    // Aceita o scheme maxterminal para produzir InvalidUri também em rotas de negócio desconhecidas.
    fn accepts(&self, uri: &url::Url) -> bool;
    async fn handle(&self, envelope: &mt_os::activation::ActivationEnvelope) -> mt_os::error::OsResult<()>;
}

/// Registra handler antes da prontidão. A 09 possui o único on_open_url/get_current e o plugin single-instance.
pub async fn init(
    activation: &mt_os::activation::ActivationRouter,
    bridge: BridgeHandle,
    host: Arc<TauriBridgeHost>,
) -> MtResult<Arc<BridgeActivationHandler>>;

/// Processa envelope individual após a barreira de prontidão, usando MaxTerminalUri::parse_parts(&envelope.uri, &envelope.params).
/// - `MaxTerminalUri::ProjectOpen { path, worktree, correlation_id }`: canonicaliza e resolve worktree (`resolve_worktree_root_cached`),
///   valida worktree opcional, faz match_project, aplica coalescência de 2 s e SyncContext de origem Maxterminal,
///   chama `host.focus_project` e `host.show_main_window()`.
/// - `MaxTerminalUri::TabOpen { project, cwd, request_id, worktree, correlation_id }`: verifica dedup de request_id (TTL 5 min),
///   resolve projeto, valida `cwd` com `validate_cwd_within_project`, chama `host.open_tab` com `template: None`
///   (forçando template Shell neutro sem comando inicial), foca o projeto e exibe a janela.
pub async fn route_single_uri(
    host: &TauriBridgeHost,
    bridge: &BridgeHandle,
    envelope: &mt_os::activation::ActivationEnvelope,
) -> Result<(), BridgeError>;
```

`handle` converte `BridgeError` em `MtError` preservando código (`OsResult` é alias de `MtResult`). A estrutura inválida já é rejeitada pela 09 como `os.activation_invalid`. `correlationId` não muda origem/autorização e nunca substitui `requestId`; se ausente, gerar UUID v7. Não repetir percent-decoding. `register_all()` só no perfil explícito de teste da 09, sem sobrescrever associação MIME em todo boot debug.

### 3.3.1. `src-tauri/src/bridge/editor.rs` — contrato T-13

```rust
pub struct BridgeEditorOpener {
    // Referência ao estado gerenciado da ponte para obter o cliente atual após restart.
}

#[async_trait::async_trait]
impl mt_os::editor::EditorOpener for BridgeEditorOpener {
    fn handles(&self, kind: mt_os::settings::EditorKind) -> bool;
    async fn open(&self, link: &mt_os::links::ResolvedLink) -> mt_os::error::OsResult<()>;
}
```

- `handles` retorna verdadeiro somente para `EditorKind::MaxCode`; no setup, registrar `Arc<BridgeEditorOpener>` via `EditorLauncher::set_bridge_opener`. O setter substitui o slot e evita duplicações ao reiniciar.
- Estender aditivamente `protocol.rs` com `Capability::EditorOpenV1` (`editor_open_v1`), `MessageType::MaxcodeOpenFile` (`maxcode.open_file`), `Message::MaxcodeOpenFile(OpenFilePayload)` e `OpenFilePayload { path: PathBuf, line: u32, column: u32 }`. São **propostas** a confirmar com a extensão, não API comprovada. Direção MT → MC; envelope/dedup e limites de 11/01–02 preservados.
- Adicionar `MaxCodeClient::open_file(&self, path: &Path, line: u32, column: u32) -> Result<DeliveryReport, BridgeError>` em `client.rs`. Exigir adaptador validado e peer autenticado com `editor_open_v1`; validar arquivo absoluto regular, ≤4096 bytes, sem controles, linha/coluna `1..=10_000_000`. Linha/coluna ausentes do `ResolvedLink` viram 1. A 09 já revalida contexto/cwd antes da chamada.
- Selecionar peer por workspace do arquivo, ou único peer de editor compatível quando o arquivo está fora de projeto; ambiguidade retorna indisponibilidade, nunca broadcast. Enviar `maxcode.open_file`, confirmar apenas ACK do mesmo peer/`reply_to` com `handled:true`. Arquivos externos de compiladores são permitidos pela 09, sem exigir cadastro de projeto.
- Sem suporte, desconectado ou em erro: retornar `BridgeError` convertido para `MtError`; `EditorLauncher` executa seu fallback explícito `vscode://file/...`. Não usar `open_plan` para abrir arquivos nem atribuir sucesso do fallback à ponte. Sanitizar logs, não enviar conteúdo do arquivo.
- Testar sucesso autenticado, capability ausente (nenhum frame), `handled:false`, erro/timeout com fallback, linha/coluna/Unicode preservados e registro idempotente.

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
    - O plugin `single-instance` da 09 permanece único e anterior ao deep-link; ele encaminha a URI ao listener único da 09, sem reenfileiramento manual. `BridgeActivationHandler` é registrado antes de liberar o transporte.
    - A primeira chamada `bridge_status` por `useMaxCodeBridgeStore.init()` sinaliza `BridgeActivationHandler::mark_ready()` após a UI instalar listeners. A composição aguarda também projetos/workspace restaurados e chama `ActivationRouter::set_ready().await` uma vez; nenhuma camada libera fila prematuramente.
   - O encerramento real do aplicativo ("Sair" na bandeja do sistema, Fase 09) invoca `bridge.shutdown().await` dentro de `ShutdownHooks`, fechando todas as sessões com código `1001` e removendo `bridge.json`. Minimizar para a bandeja mantém a ponte ativa.
