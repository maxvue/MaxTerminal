import { Channel } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type AiContext_Serialize,
    type AiHistoryFilter_Deserialize,
    type AiHistoryPage_Serialize,
    type AiRequestId,
    type AiSettings_Deserialize,
    type AiSettings_Serialize,
    type AiStreamEvent_Deserialize,
    type AiStreamEvent_Serialize,
    type CacheClearScope,
    type CommandBlockId,
    type DiagnoseRequest,
    type DiagnosisOutcome_Serialize,
    type GatewayHealth_Serialize,
    type HarnessCatalog_Serialize,
    type PaneId,
    type ProjectId,
    type RagSearchRequest_Deserialize,
    type RagSearchResponse_Serialize,
    type RagSlug_Serialize,
    type RiskAssessment_Serialize,
    type StructuredRequest_Deserialize,
    type StructuredResponse,
    type SuggestRequest_Deserialize
} from '@/types/bindings';

/** Executa sonda imediata de conectividade com o gateway `MaxAiManager` (`ai_health`). */
export function aiHealth(): Promise<GatewayHealth_Serialize> {
    return unwrap(commands.aiHealth(), 'ai_health');
}

/** Retorna as configurações de IA (`ai_settings_get`). */
export function aiSettingsGet(): Promise<AiSettings_Serialize> {
    return unwrap(commands.aiSettingsGet(), 'ai_settings_get');
}

/** Atualiza configurações de IA e emite `ai://settings-changed` (`ai_settings_update`). */
export function aiSettingsUpdate(settings: AiSettings_Deserialize): Promise<AiSettings_Serialize> {
    return unwrap(commands.aiSettingsUpdate(settings), 'ai_settings_update');
}

/** Obtém o catálogo de harnesses, modelos e níveis de raciocínio (`ai_catalog_get`). */
export function aiCatalogGet(): Promise<HarnessCatalog_Serialize> {
    return unwrap(commands.aiCatalogGet(), 'ai_catalog_get');
}

/** Força a reconsulta do catálogo ao gateway e emite `ai://catalog-updated` (`ai_catalog_refresh`). */
export function aiCatalogRefresh(): Promise<HarnessCatalog_Serialize> {
    return unwrap(commands.aiCatalogRefresh(), 'ai_catalog_refresh');
}

/** Retorna o preview exato e sanitizado do contexto que será enviado ao gateway (`ai_context_preview`). */
export function aiContextPreview(paneId: PaneId): Promise<AiContext_Serialize> {
    return unwrap(commands.aiContextPreview(paneId), 'ai_context_preview');
}

/**
 * Inicia a tradução NL → Shell com streaming incremental via `Channel<AiStreamEvent>` (`ai_suggest_command`).
 * Retorna `AiRequestId` imediatamente após validação síncrona de pré-voo (`CA-09`).
 */
export async function aiSuggestCommand(
    req: SuggestRequest_Deserialize,
    onEvent: (event: AiStreamEvent_Serialize) => void
): Promise<AiRequestId> {
    const channel = new Channel<AiStreamEvent_Deserialize>();
    channel.onmessage = (ev) => {
        onEvent(ev as unknown as AiStreamEvent_Serialize);
    };
    return unwrap(commands.aiSuggestCommand(req, channel), 'ai_suggest_command');
}

/** Cancela imediatamente uma requisição de IA em andamento (`ai_cancel`). */
export function aiCancel(requestId: AiRequestId): Promise<boolean> {
    return unwrap(commands.aiCancel(requestId), 'ai_cancel');
}

/** Reavalia deterministicamente o risco de um comando shell (`ai_classify_risk`). */
export function aiClassifyRisk(command: string): Promise<RiskAssessment_Serialize> {
    return unwrap(commands.aiClassifyRisk(command), 'ai_classify_risk');
}

/** Gera ou recupera do cache o diagnóstico rápido inline para um bloco falho (`ai_diagnose_block`). */
export function aiDiagnoseBlock(req: DiagnoseRequest): Promise<DiagnosisOutcome_Serialize> {
    return unwrap(commands.aiDiagnoseBlock(req), 'ai_diagnose_block');
}

/** Marca o diagnóstico rápido inline de um bloco como dispensado (`ai_diagnosis_dismiss`). */
export async function aiDiagnosisDismiss(blockId: CommandBlockId): Promise<void> {
    await unwrap(commands.aiDiagnosisDismiss(blockId), 'ai_diagnosis_dismiss');
}

/** Lista os slugs RAG disponíveis no `MaxAiManager` (`ai_rag_list_slugs`). */
export function aiRagListSlugs(): Promise<RagSlug_Serialize[]> {
    return unwrap(commands.aiRagListSlugs(), 'ai_rag_list_slugs');
}

/** Executa consulta RAG em `/v1/rags/{slug}/search` (`ai_rag_search`). */
export function aiRagSearch(req: RagSearchRequest_Deserialize): Promise<RagSearchResponse_Serialize> {
    return unwrap(commands.aiRagSearch(req), 'ai_rag_search');
}

/** Executa geração estruturada JSON validada por schema (`ai_generate_structured`). */
export function aiGenerateStructured(req: StructuredRequest_Deserialize): Promise<StructuredResponse> {
    return unwrap(commands.aiGenerateStructured(req), 'ai_generate_structured');
}

/** Lista paginadamente o histórico de interações de IA (`ai_history_list`). */
export function aiHistoryList(filter: AiHistoryFilter_Deserialize): Promise<AiHistoryPage_Serialize> {
    return unwrap(commands.aiHistoryList(filter), 'ai_history_list');
}

/** Remove registros do histórico de IA e retorna a quantidade removida (`ai_history_clear`). */
export function aiHistoryClear(projectId: ProjectId | null = null): Promise<number> {
    return unwrap(commands.aiHistoryClear(projectId), 'ai_history_clear');
}

/** Limpa os caches de IA conforme o escopo informado e retorna a quantidade removida (`ai_cache_clear`). */
export function aiCacheClear(scope: CacheClearScope): Promise<number> {
    return unwrap(commands.aiCacheClear(scope), 'ai_cache_clear');
}

/** Escuta o evento global `ai://status-changed`. */
export function onAiStatusChanged(
    cb: (health: GatewayHealth_Serialize) => void
): Promise<UnlistenFn> {
    return listen<GatewayHealth_Serialize>('ai://status-changed', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `ai://catalog-updated`. */
export function onAiCatalogUpdated(
    cb: (catalog: HarnessCatalog_Serialize) => void
): Promise<UnlistenFn> {
    return listen<HarnessCatalog_Serialize>('ai://catalog-updated', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `ai://settings-changed`. */
export function onAiSettingsChanged(
    cb: (settings: AiSettings_Serialize) => void
): Promise<UnlistenFn> {
    return listen<AiSettings_Serialize>('ai://settings-changed', (ev) => {
        cb(ev.payload);
    });
}

export const aiIpc = {
    health: aiHealth,
    settingsGet: aiSettingsGet,
    settingsUpdate: aiSettingsUpdate,
    catalogGet: aiCatalogGet,
    catalogRefresh: aiCatalogRefresh,
    contextPreview: aiContextPreview,
    suggestCommand: aiSuggestCommand,
    cancel: aiCancel,
    classifyRisk: aiClassifyRisk,
    diagnoseBlock: aiDiagnoseBlock,
    diagnosisDismiss: aiDiagnosisDismiss,
    ragListSlugs: aiRagListSlugs,
    ragSearch: aiRagSearch,
    generateStructured: aiGenerateStructured,
    historyList: aiHistoryList,
    historyClear: aiHistoryClear,
    cacheClear: aiCacheClear,
    onStatusChanged: onAiStatusChanged,
    onCatalogUpdated: onAiCatalogUpdated,
    onSettingsChanged: onAiSettingsChanged
};
