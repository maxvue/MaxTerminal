# Implementação 03 — Ponte Tauri, Adaptador `AiContextSource`, Comandos IPC, Eventos e Store Pinia Frontend (`spec.md`)

> **Fase:** 12 — Integração MaxAiManager (`@` / `?`)
> **Unidade:** `03 - Ponte Tauri, Adaptador AiContextSource, Comandos IPC, Eventos e Store Pinia Frontend`
> **Pré-requisitos:** Implementações 01 e 02 da Fase 12 (`mt-ai` completo) + Contratos das Fases 04, 05, 06 e 07
> **Cobertura de Escopo da Fase 12:** `I-02` (IPC `Channel`), `I-03` (eventos de fechamento de aba/painel), `I-05` (`ai_context.rs`), `I-12` (composable `useAiGenerate<T>()`), `I-14` (`src/ipc/ai.ts`, `useAiStore`, `adapters.ts`), `I-15` (`ai_health` + `ai://status-changed`)
> **Cobertura de Critérios de Aceite:** `CA-02`, `CA-04`, `CA-28`, `CA-33` (ponta a ponta IPC + Store), `CA-36` (`useAiGenerate<T>` com decoder de runtime e revisão de formulário)

---

## 1. Contexto, Objetivo e Escopo

Esta terceira unidade conecta o crate `mt-ai` ao binário Tauri (`maxterminal`) e constrói toda a camada de estado reativo e IPC no frontend TypeScript/Vue:
1. **Adaptador de Contexto (`src-tauri/src/ai_context.rs`):** Implementa a trait `mt_ai::context::AiContextSource` no crate binário `maxterminal` lendo o estado autoritativo das Fases 05 (`ShellService`, `CommandBlocksRepo`), 06 (`ProjectStack`, nome do projeto) e 07 (`WorkspaceManager`, `TabView`/`PaneView`, `WorkspaceLifecycleObserver`), além de cancelar automaticamente requisições pendentes quando abas/painéis são fechados, PTYs são substituídos ou sessões são arquivadas (`CA-33`).
2. **Comandos Tauri IPC e Eventos (`src-tauri/src/commands/ai.rs`, `ai_state.rs`, `specta.rs`):** Expõe os 17 comandos IPC da Fase 12, o stream de tokens via `tauri::ipc::Channel<AiStreamEvent>` com fila limitada (`256 KiB`) e os 3 eventos globais (`ai://status-changed`, `ai://catalog-updated`, `ai://settings-changed`), exportando todos os bindings TypeScript em `src/types/bindings.ts` (`CA-02`, `CA-04`).
3. **Camada IPC Tipada (`src/ipc/ai.ts`) e Adaptador Reconciliado (`src/composables/ai/adapters.ts`):** Encapsula os comandos gerados em `bindings.ts` e isola a ponte com as APIs publicadas das Fases 04, 05 e 07 (`triggerRegistry`, `useRichInput`, `TerminalInstanceHandle.write`, `shell://command-finished`).
4. **Store Pinia (`src/stores/ai.ts` — `useAiStore`):** Gerencia o estado reativo de saúde do gateway, configurações, catálogo, sugestões em streaming por painel, diagnósticos por bloco e buscas RAG, aplicando revisões monotônicas, tombstones de cancelamento antes do aceite IPC, reclassificação de risco com debounce de `150 ms` ao editar comandos e limite de 100 cards por painel.
5. **Composable de Geração Assistida (`src/composables/ai/useAiGenerate.ts`):** Fornece o contrato genérico `useAiGenerate<T>(schemaId, schema, decode)` consumido pelas Fases 14 e 15, exigindo decoder de runtime e guarda de revisão do formulário consumidor (`CA-28`, `CA-36`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

### 2.1. Arquivos a Modificar
- `src-tauri/Cargo.toml` — adição de `mt-ai = { path = "crates/mt-ai" }` nas dependências do binário `maxterminal`.
- `src-tauri/src/lib.rs` (ou `src-tauri/src/commands/mod.rs`) — declaração de `pub mod ai_context;` e `pub mod ai;` em `commands`.
- `src-tauri/src/lib.rs` (`setup`) — construção de `TauriAiContextSource` e `AiService`, `app.manage(AppAiState::new(...))` (estado próprio do domínio; o `AppState` da Fase 01 **não** agrega `AiService`, decisão D-09), registro do observer de workspace e do emissor de `ai://status-changed`.
- `src-tauri/src/specta.rs` — registro dos 17 comandos `ai_*` e dos 3 eventos `ai://*` no builder `tauri-specta`.
- `src/types/bindings.ts` — atualizado automaticamente pela geração do `tauri-specta`.

### 2.2. Arquivos a Criar
- `src-tauri/src/ai_context.rs`
- `src-tauri/src/ai_state.rs` — `AppAiState { service: Arc<mt_ai::AiService> }`, registrado por `app.manage(...)`
- `src-tauri/src/commands/ai.rs`
- `src/ipc/ai.ts`
- `src/composables/ai/adapters.ts`
- `src/stores/ai.ts`
- `src/composables/ai/useAiGenerate.ts`
- `tests/stores/ai.spec.ts`
- `tests/composables/ai/useAiGenerate.spec.ts`

---

## 3. Especificação Técnica Exaustiva

### 3.1. Implementação de `AiContextSource` no Binário Tauri (`src-tauri/src/ai_context.rs`)

```rust
use mt_ai::config::ContextSettings;
use mt_ai::context::{
    abbreviate_home, detect_linux_os_info, strip_ansi_and_sanitize, AiContext, AiContextSource,
    BlockFailure, OsInfo, PaneBindingSnapshot, RecentCommand,
};
use mt_ai::error::{ai_error, codes};
use mt_core::{CommandBlockId, MtResult, PaneId, ProjectId};
use std::sync::Arc;

pub struct TauriAiContextSource {
    os_info: OsInfo,
    home_dir: Option<String>,
    // Handles `Arc` dos serviços das Fases 05, 06 e 07, resolvidos no `setup` a partir do estado gerenciado de
    // cada domínio (`app.state::<Arc<T>>()`; D-09). Nenhum deles é lido via `AppState` da Fase 01.
    // workspace: Arc<WorkspaceManager>,
    // shell: Arc<ShellService>,
    // projects: Arc<ProjectService>,
}

impl TauriAiContextSource {
    pub fn new(
        /* serviços Fases 05, 06, 07 */
    ) -> Arc<Self> {
        let home_dir = std::env::var("MAXCODE_REAL_HOME")
            .ok()
            .or_else(|| std::env::var("HOME").ok());
        Arc::new(Self {
            os_info: detect_linux_os_info(),
            home_dir,
        })
    }
}

#[async_trait::async_trait]
impl AiContextSource for TauriAiContextSource {
    async fn for_pane(&self, pane: PaneId, opts: &ContextSettings) -> MtResult<AiContext> {
        // 1. Obtém PaneView/TabView ativo da Fase 07 para o `pane`; se não existir, retorna Err(ai.context_stale).
        // 2. Obtém shell (ex.: "zsh") e cwd atual do ShellService (Fase 05), com fallback para o cwd do PaneView (Fase 07).
        // 3. Obtém nome e stack do projeto associado à aba na Fase 06.
        // 4. Lista os últimos `opts.recent_commands` blocos finalizados da sessão atual via CommandBlocksRepo (Fase 05),
        //    ordenados do mais antigo ao mais recente.
        // 5. Se `opts.include_output_tail` for true e houver último bloco, lê a cauda sanitizada via ShellService::error_context.
        // 6. Aplica `abbreviate_home`, `strip_ansi_and_sanitize` e retorna `ctx.sanitize_and_bound(opts.recent_commands, opts.output_tail_bytes)`.
        self.build_pane_context(pane, opts).await // helper privado; comportamento em "Comportamento esperado" abaixo
    }

    async fn for_block(&self, block: CommandBlockId, tail_bytes: u32) -> MtResult<(AiContext, BlockFailure)> {
        // 1. Consulta CommandBlocksRepo::get(block) e ShellService::error_context(block, tail_bytes) (Fase 05).
        // 2. Verifica se o painel e a sessão do bloco ainda correspondem à sessão ativa do painel na Fase 07;
        //    se a sessão foi arquivada ou o PTY substituído, retorna Err(ai.context_stale).
        // 3. Determina `is_interactive` verificando se o bloco rodou em alternate screen (TUI) ou se o template
        //    da aba (Fase 07) é de um Agente CLI.
        // 4. Monta `(AiContext, BlockFailure)` totalmente sanitizados.
        self.build_block_failure(block, tail_bytes).await // helper privado; ver "Comportamento esperado"
    }

    fn pane_snapshot(&self, pane: PaneId) -> Option<PaneBindingSnapshot> {
        self.read_pane_binding(pane) // helper privado síncrono; ver "Comportamento esperado"
    }

    fn project_of_pane(&self, pane: PaneId) -> Option<ProjectId> {
        self.pane_snapshot(pane).and_then(|s| s.project_id)
    }
}
```
- **Comportamento esperado dos helpers privados (corpos descritos por comportamento, sem placeholders):**
   - `read_pane_binding(pane) -> Option<PaneBindingSnapshot>`: leitura **síncrona e sem I/O** de uma projeção local de vínculos, mantida pelo adaptador a partir de `WorkspaceLifecycleSignal::PaneSpawned { owner, pty_id, .. }`. Copia `(project_id, tab_id, pane_id, session_id)` de `ExecutionOwner` e o PTY real; `SessionId` é do painel, não `TabSessionId` da aba. Guarda também a geração do owner para invalidar eventos obsoletos. Retorna `None` se removido, encerrado ou ainda sem spawn; não acessa campos privados nem bloqueia aguardando métodos async do `WorkspaceManager`.
  - `build_pane_context(pane, opts)`: executa os 6 passos listados em `for_pane` na ordem. Qualquer ausência de identidade (painel, aba, sessão) falha com `ai.context_stale` e **nunca** devolve contexto parcial inventado. Fallbacks permitidos: cwd do `PaneView` quando o `ShellService` não conhece o cwd observado; stack do projeto ausente vira `None`. Leituras de `CommandBlocksRepo`/`ShellService` são somente leitura; erros de repositório propagam o `MtError` da Fase 03/05 (`db.*`) sem fallback silencioso.
  - `build_block_failure(block, tail_bytes)`: lê `CommandBlocksRepo::get(block)`; bloco inexistente → `core.not_found`; compara `(pane_id, session_id, pty_id)` do bloco com `read_pane_binding(pane)` e, se divergirem (sessão arquivada, PTY substituído, painel fechado), falha com `ai.context_stale`. Cauda via `ShellService::error_context(block, tail_bytes)` (já sanitizada pela Fase 05, truncada mantendo o **final**); `exit_code` permanece `Option` e `null` nunca é tratado como erro confirmado (decisão de pular é da `DiagnosisService`).
  - **Sanitização (invariante):** todo texto que sai do adaptador passa por `strip_ansi_and_sanitize` (Fase 03) + `abbreviate_home`; a `AiService` reaplica `sanitize()` antes de montar o prompt (defesa em profundidade, `CA-18`). O adaptador **nunca** escreve no PTY e não mantém cache próprio de saída.
- **Integração com `WorkspaceLifecycleObserver` (Fase 07):**
   - No `setup` (após `app.manage(AppAiState)` e antes da restauração/abertura de abas), registra o adaptador via `WorkspaceManager::register_observer`. `on_signal` recebe `PaneSpawned`, `PaneExited`, `TabLayoutChanged`, `SessionArchived` e `TabClosed`; atualiza/invalida a projeção e invoca `ai_service.requests.cancel_pane(pane_id)` ao remover/reiniciar/arquivar (`CA-33`). Comparar owner/geração para que saída antiga não invalide o novo vínculo; não inventar eventos de backend `pane://pty-replaced` nem métodos `on_tab_closed`.

---

### 3.2. Comandos Tauri IPC (`src-tauri/src/commands/ai.rs`)

```rust
use crate::ai_state::AppAiState;
use mt_ai::catalog::HarnessCatalog;
use mt_ai::client::http::GatewayHealth;
use mt_ai::config::AiSettings;
use mt_ai::context::AiContext;
use mt_ai::diagnose::{DiagnoseRequest, DiagnosisOutcome};
use mt_ai::ids::AiRequestId;
use mt_ai::rag::{RagSearchRequest, RagSearchResponse, RagSlug};
use mt_ai::repo::{AiHistoryFilter, AiHistoryPage, CacheClearScope};
use mt_ai::risk::RiskAssessment;
use mt_ai::structured::{StructuredRequest, StructuredResponse};
use mt_ai::suggest::{AiStreamEvent, SuggestRequest};
use mt_core::{CommandBlockId, MtError, PaneId, ProjectId};
use tauri::{ ipc::Channel, AppHandle, Emitter, State };

#[tauri::command]
#[specta::specta]
pub async fn ai_health(state: State<'_, AppAiState>) -> Result<GatewayHealth, MtError> {
    Ok(state.service.health().await)
}

#[tauri::command]
#[specta::specta]
pub async fn ai_settings_get(state: State<'_, AppAiState>) -> Result<AiSettings, MtError> {
    Ok(state.service.get_settings())
}

#[tauri::command]
#[specta::specta]
pub async fn ai_settings_update(
    app: AppHandle,
    state: State<'_, AppAiState>,
    settings: AiSettings,
) -> Result<AiSettings, MtError> {
    let updated = state.service.update_settings(settings).await?;
    let _ = app.emit("ai://settings-changed", &updated);
    Ok(updated)
}

#[tauri::command]
#[specta::specta]
pub async fn ai_catalog_get(state: State<'_, AppAiState>) -> Result<HarnessCatalog, MtError> {
    Ok(state.service.catalog_get().await)
}

#[tauri::command]
#[specta::specta]
pub async fn ai_catalog_refresh(
    app: AppHandle,
    state: State<'_, AppAiState>,
) -> Result<HarnessCatalog, MtError> {
    let catalog = state.service.catalog_refresh().await?;
    let _ = app.emit("ai://catalog-updated", &catalog);
    Ok(catalog)
}

#[tauri::command]
#[specta::specta]
pub async fn ai_context_preview(
    state: State<'_, AppAiState>,
    pane_id: PaneId,
) -> Result<AiContext, MtError> {
    state.service.context_preview(pane_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_suggest_command(
    state: State<'_, AppAiState>,
    request: SuggestRequest,
    on_event: Channel<AiStreamEvent>,
) -> Result<AiRequestId, MtError> {
    let req_id = request.request_id;
    let ai = state.service.clone();
    // Fase síncrona (contrato em 02/spec.md §3.7): valida `enabled`, seleção válida/default e registra o `request_id`
    // no `AiRequestRegistry` ANTES do spawn; falhas voltam como `Err` imediato ao invoke.
    let ticket = ai.prepare_suggest(&request)?;
    tauri::async_runtime::spawn(async move {
        // Fase assíncrona: o `bool` do callback indica se o envio ao Channel funcionou (`false` cancela a requisição).
        let _ = ai
            .run_suggest_stream(request, ticket, move |evt| on_event.send(evt).is_ok())
            .await;
    });
    Ok(req_id)
}

#[tauri::command]
#[specta::specta]
pub async fn ai_cancel(state: State<'_, AppAiState>, request_id: AiRequestId) -> Result<bool, MtError> {
    Ok(state.service.cancel(request_id))
}

#[tauri::command]
#[specta::specta]
pub async fn ai_classify_risk(
    state: State<'_, AppAiState>,
    command: String,
) -> Result<RiskAssessment, MtError> {
    Ok(state.service.classify_risk(&command))
}

#[tauri::command]
#[specta::specta]
pub async fn ai_diagnose_block(
    state: State<'_, AppAiState>,
    request: DiagnoseRequest,
) -> Result<DiagnosisOutcome, MtError> {
    state.service.diagnose_block(request).await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_diagnosis_dismiss(
    state: State<'_, AppAiState>,
    block_id: CommandBlockId,
) -> Result<(), MtError> {
    state.service.dismiss_diagnosis(block_id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn ai_rag_list_slugs(state: State<'_, AppAiState>) -> Result<Vec<RagSlug>, MtError> {
    state.service.rag_list_slugs().await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_rag_search(
    state: State<'_, AppAiState>,
    request: RagSearchRequest,
) -> Result<RagSearchResponse, MtError> {
    state.service.rag_search(request).await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_generate_structured(
    state: State<'_, AppAiState>,
    request: StructuredRequest,
) -> Result<StructuredResponse, MtError> {
    state.service.generate_structured(request).await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_history_list(
    state: State<'_, AppAiState>,
    filter: AiHistoryFilter,
) -> Result<AiHistoryPage, MtError> {
    state.service.history_list(filter).await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_history_clear(
    state: State<'_, AppAiState>,
    project_id: Option<ProjectId>,
) -> Result<u32, MtError> {
    state.service.history_clear(project_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn ai_cache_clear(
    state: State<'_, AppAiState>,
    scope: CacheClearScope,
) -> Result<u32, MtError> {
    state.service.cache_clear(scope).await
}
```
- **Nomes exportados em `src/types/bindings.ts` (`CA-04`):**
  `aiHealth`, `aiSettingsGet`, `aiSettingsUpdate`, `aiCatalogGet`, `aiCatalogRefresh`, `aiContextPreview`, `aiSuggestCommand`, `aiCancel`, `aiClassifyRisk`, `aiDiagnoseBlock`, `aiDiagnosisDismiss`, `aiRagListSlugs`, `aiRagSearch`, `aiGenerateStructured`, `aiHistoryList`, `aiHistoryClear`, `aiCacheClear`.

---

### 3.3. Wrapper IPC Frontend (`src/ipc/ai.ts`)

```ts
import { Channel } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type AiContext,
    type AiHistoryFilter,
    type AiHistoryPage,
    type AiRequestId,
    type AiSettings,
    type AiStreamEvent,
    type CacheClearScope,
    type CommandBlockId,
    type DiagnoseRequest,
    type DiagnosisOutcome,
    type GatewayHealth,
    type HarnessCatalog,
    type PaneId,
    type ProjectId,
    type RagSearchRequest,
    type RagSearchResponse,
    type RagSlug,
    type RiskAssessment,
    type StructuredRequest,
    type StructuredResponse,
    type SuggestRequest
} from '@/types/bindings';

export function getHealth(): Promise<GatewayHealth> {
    return unwrap(commands.aiHealth(), 'ai_health');
}

export function getSettings(): Promise<AiSettings> {
    return unwrap(commands.aiSettingsGet(), 'ai_settings_get');
}

export function updateSettings(settings: AiSettings): Promise<AiSettings> {
    return unwrap(commands.aiSettingsUpdate(settings), 'ai_settings_update');
}

export function getCatalog(): Promise<HarnessCatalog> {
    return unwrap(commands.aiCatalogGet(), 'ai_catalog_get');
}

export function refreshCatalog(): Promise<HarnessCatalog> {
    return unwrap(commands.aiCatalogRefresh(), 'ai_catalog_refresh');
}

export function previewContext(paneId: PaneId): Promise<AiContext> {
    return unwrap(commands.aiContextPreview(paneId), 'ai_context_preview');
}

export function suggestCommand(
    request: SuggestRequest,
    onEvent: (event: AiStreamEvent) => void
): Promise<AiRequestId> {
    const channel = new Channel<AiStreamEvent>();
    channel.onmessage = onEvent;
    return unwrap(commands.aiSuggestCommand(request, channel), 'ai_suggest_command');
}

export function cancelRequest(requestId: AiRequestId): Promise<boolean> {
    return unwrap(commands.aiCancel(requestId), 'ai_cancel');
}

export function classifyRisk(command: string): Promise<RiskAssessment> {
    return unwrap(commands.aiClassifyRisk(command), 'ai_classify_risk');
}

export function diagnoseBlock(request: DiagnoseRequest): Promise<DiagnosisOutcome> {
    return unwrap(commands.aiDiagnoseBlock(request), 'ai_diagnose_block');
}

export function dismissDiagnosis(blockId: CommandBlockId): Promise<void> {
    return unwrap(commands.aiDiagnosisDismiss(blockId), 'ai_diagnosis_dismiss');
}

export function listRagSlugs(): Promise<RagSlug[]> {
    return unwrap(commands.aiRagListSlugs(), 'ai_rag_list_slugs');
}

export function ragSearch(request: RagSearchRequest): Promise<RagSearchResponse> {
    return unwrap(commands.aiRagSearch(request), 'ai_rag_search');
}

export function generateStructured(request: StructuredRequest): Promise<StructuredResponse> {
    return unwrap(commands.aiGenerateStructured(request), 'ai_generate_structured');
}

export function listHistory(filter: AiHistoryFilter): Promise<AiHistoryPage> {
    return unwrap(commands.aiHistoryList(filter), 'ai_history_list');
}

export function clearHistory(projectId: ProjectId | null): Promise<number> {
    return unwrap(commands.aiHistoryClear(projectId), 'ai_history_clear');
}

export function clearCache(scope: CacheClearScope): Promise<number> {
    return unwrap(commands.aiCacheClear(scope), 'ai_cache_clear');
}

export function onAiStatusChanged(cb: (health: GatewayHealth) => void): Promise<UnlistenFn> {
    return listen<GatewayHealth>('ai://status-changed', (e) => cb(e.payload));
}

export function onAiCatalogUpdated(cb: (catalog: HarnessCatalog) => void): Promise<UnlistenFn> {
    return listen<HarnessCatalog>('ai://catalog-updated', (e) => cb(e.payload));
}

export function onAiSettingsChanged(cb: (settings: AiSettings) => void): Promise<UnlistenFn> {
    return listen<AiSettings>('ai://settings-changed', (e) => cb(e.payload));
}
```

---

### 3.4. Adaptador Reconciliado (`src/composables/ai/adapters.ts`)

```ts
import type {
    AiRequestId,
    CommandBlockId,
    PaneId,
    ProjectId,
    PtyId,
    SessionId,
    TabId
} from '@/types/bindings';

export interface AiPaneContext {
    projectId: ProjectId | null;
    tabId: TabId;
    paneId: PaneId;
    sessionId: SessionId;
    ptyId: PtyId;
    shell: string;
    cwd: string;
    projectStack: string | null;
    isInteractiveOrAgent: boolean;
}

export type AiCardEntry =
    | { kind: 'ai-suggestion'; requestId: AiRequestId; paneId: PaneId; sessionId: SessionId; createdAt: number }
    | { kind: 'ai-rag'; requestId: AiRequestId; paneId: PaneId; sessionId: SessionId; createdAt: number };

export interface CommandFinishedObserved {
    blockId: CommandBlockId;
    paneId: PaneId;
    tabId: TabId;
    sessionId: SessionId;
    projectId: ProjectId | null;
    exitCode: number | null;
    isInteractiveOrAgent: boolean;
    hasReliableMetadata: boolean;
}

/**
 * Obtém o snapshot autoritativo do painel combinando stores das Fases 04, 05, 06 e 07.
 */
export function getPaneContext(paneId: PaneId): AiPaneContext | null;

/**
 * Verifica se o painel está em modo de shell rico (não está em alternate screen TUI nem é aba exclusiva de Agente CLI).
 */
export function isRichShellPane(paneId: PaneId): boolean;

/**
 * Insere texto na barra de input rico já montada do painel (`useRichInput().text = command`)
 * SEM enviar `\r` e SEM acionar `submit()` (CA-15).
 */
export function insertIntoInput(paneId: PaneId, command: string): boolean;

/**
 * Escreve um comando explicitamente no PTY do painel após revalidar que `(sessionId, ptyId)`
 * continuam idênticos ao snapshot em que a sugestão foi gerada e que o shell está no prompt ocioso (CA-15, CA-33).
 */
export function writeToPty(
    expected: { paneId: PaneId; sessionId: SessionId; ptyId: PtyId },
    commandWithNewline: string
): boolean;

/**
 * Registra um card rico (`ai-suggestion` ou `ai-rag`) na coleção do painel (`AiPaneResults`),
 * limitado a 100 cards por painel.
 */
export function pushRichBlock(paneId: PaneId, card: AiCardEntry): void;

/**
 * Remove um card rico específico da coleção do painel.
 */
export function removeRichBlock(paneId: PaneId, requestId: AiRequestId): void;

/**
 * Limpa todos os cards de um painel/sessão quando a sessão é arquivada ou o painel fechado.
 */
export function clearPaneBlocks(paneId: PaneId, sessionId?: SessionId): void;

/**
 * Retorna a lista reativa de cards ativos para `(paneId, sessionId)`.
 */
export function usePaneRichBlocks(paneId: PaneId, sessionId: SessionId): Readonly<AiCardEntry[]>;
```

---

### 3.5. Store Pinia `useAiStore` (`src/stores/ai.ts`)

- Implementa todas as interfaces `SuggestionState`, `RagRequestState`, `DiagnosisBlockState` e `AiState`, getters (`isAvailable`, `hasDefaultSelection`, `harnesses`, `modelsOf`, `reasoningOf`, `selectionLabel`, `activeSuggestion`, `diagnosisEnabledFor`) e actions (`init`, `dispose`, `updateSettings`, `refreshCatalog`, `startSuggestion`, `editCommand`, `cancel`, `discard`, `requestDiagnosis`, `dismissDiagnosis`, `loadRagSlugs`, `ragSearch`) especificados em `spec.md` §6.2 da Fase 12.
- **Regras críticas de concorrência na `useAiStore` (`CA-24`, `CA-33`):**
  1. `startSuggestion` aloca um `requestId` (UUID v7), captura `(projectId, tabId, paneId, sessionId, ptyId, configRevision)` via `getPaneContext(paneId)` e cria imediatamente o `SuggestionState` com `status: 'pending'` **antes** de chamar `ipc.suggestCommand`.
  2. Se `cancel(requestId)` for chamado enquanto `ipc.suggestCommand` ainda não resolveu o aceite IPC, marca o estado local como `'cancelled'`, registra intenção de cancelamento local, ignora qualquer evento subsequente da `Channel` e dispara `ipc.cancelRequest(requestId)` assim que o invoke retornar (`CA-33`).
  3. `editCommand(requestId, command)` incrementa `editRevision`, define `editedRisk = null` (desabilitando temporariamente o botão "Executar" enquanto a classificação está pendente — §11.4 da spec) e agenda `ipc.classifyRisk(command)` com debounce de `150 ms`; o retorno de `classifyRisk` só é gravado em `editedRisk` se `editRevision` ainda coincidir.
  4. Quando `updateSettings` desativa `diagnosis.enabled` (`false`), cancela todos os diagnósticos pendentes e limpa/oculta imediatamente os diagnósticos existentes (`CA-24`).

---

### 3.6. Composable `useAiGenerate<T>` (`src/composables/ai/useAiGenerate.ts`)

```ts
import { ref, shallowRef, type Ref, type ShallowRef } from 'vue';
import * as aiIpc from '@/ipc/ai';
import type { AiRequestId, AiSelection, MtError, PaneId, ProjectId } from '@/types/bindings';

/** Opções por chamada de `run()` (D-05). Todos os campos são opcionais. */
export interface AiGenerateRunOptions {
    system?: string;
    selection?: AiSelection;
    paneId?: PaneId;
    projectId?: ProjectId;
    /** Revisão atual do formulário do consumidor; se mudar antes da resposta, o resultado é descartado. */
    getFormRevision?: () => number;
    /** Cancelamento externo: ao abortar, equivale a `cancel()` (envia `ai_cancel` e descarta qualquer resposta tardia). */
    signal?: AbortSignal;
}

export interface UseAiGenerateReturn<T> {
    loading: Ref<boolean>;
    error: ShallowRef<MtError | null>;
    result: ShallowRef<T | null>;
    repaired: Ref<boolean>;
    /**
     * Retorna `T` somente quando a resposta foi validada pelo Rust (JSON Schema), aprovada pelo `decode` de runtime
     * e a revisão do formulário não mudou. Retorna `null` (sem lançar) quando: (a) `cancel()` ou `signal` abortou;
     * (b) a revisão do formulário mudou (resposta obsoleta); (c) o `decode` lançou exceção (`error` recebe
     * `ai.schema_validation`); (d) o backend falhou (`error` recebe o `MtError`, ex. `ai.gateway_unavailable`);
     * (e) uma nova chamada a `run()` substituiu esta. Em todos os casos `result` não é alterado por respostas descartadas.
     */
    run: (prompt: string, opts?: AiGenerateRunOptions) => Promise<T | null>;
    cancel: () => Promise<void>;
}

/**
 * Ponto de extensão público consumido pelas Fases 14 (Artisan, `artisan.command.v1`) e 15 (Grupos de Comandos,
 * `command-group.suggestion.v1`) (CA-28, CA-36). `decode` é OBRIGATÓRIO (sem cast cego de `unknown`) e
 * a revisão do formulário é verificada antes de publicar `result`. A chamada vai exclusivamente por
 * `ai_generate_structured` -> `AiService::generate_structured`.
 */
export function useAiGenerate<T>(
    schemaId: string,
    schema: Record<string, unknown>,
    decode: (value: unknown) => T
): UseAiGenerateReturn<T>;
```

Regras de `signal`: se já abortado, `run` retorna `null` sem invoke; registrar listener antes da chamada,
capturando seu `requestId` (abortar uma chamada antiga não cancela a nova). Ao abortar, marcar intenção local,
enviar `ai_cancel` com o mesmo ID e rejeitar respostas tardias, inclusive antes do aceite conforme tombstones.
Remover listener em `finally`/descarte do composable; somente a chamada atual altera `loading`, `error` e
`repaired`. `decode` e a guarda de revisão são verificados antes de publicar `result` e de devolver `T`.
