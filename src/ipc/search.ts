import type { UnlistenFn } from '@tauri-apps/api/event';
import { listen } from '@tauri-apps/api/event';
import { call } from '@/ipc/core';
import { commands } from '@/types/bindings';
import type {
    CancelOutcome,
    RebuildJobHandle,
    SearchIndexProgressEvent,
    SearchIndexStat_Serialize,
    SearchInvalidatedEvent,
    SearchRequest_Deserialize,
    SearchResponse_Serialize,
    SearchSourceKind
} from '@/types/bindings';

/** Executa uma busca global multi-fonte FTS5 ou pagina uma seção sobre um snapshot existente. */
export async function searchGlobal(request: SearchRequest_Deserialize): Promise<SearchResponse_Serialize> {
    return call('search_query', () => commands.searchQuery(request));
}

/** Cancela cooperativamente uma consulta de busca em andamento para a janela principal. */
export async function cancelSearch(requestId: string): Promise<CancelOutcome> {
    return call('search_cancel', () => commands.searchCancel(requestId));
}

/** Libera explicitamente um snapshot materializado associado à janela principal. */
export async function releaseSearch(snapshotId: string): Promise<void> {
    await call('search_release', () => commands.searchRelease(snapshotId));
}

/** Retorna a contagem de documentos indexados e estado de prontidão de cada uma das 9 fontes. */
export async function getSearchIndexStats(): Promise<SearchIndexStat_Serialize[]> {
    return call('search_index_stats', () => commands.searchIndexStats());
}

/** Dispara a reconstrução dos índices FTS5 das fontes informadas (ou todas quando vazio). */
export async function rebuildSearchIndex(sources: SearchSourceKind[] = []): Promise<RebuildJobHandle> {
    return call('search_index_rebuild', () =>
        commands.searchIndexRebuild(sources.length > 0 ? sources : null)
    );
}

/** Registra listener para o evento `search://index-progress`. */
export async function onSearchIndexProgress(
    handler: (payload: SearchIndexProgressEvent) => void
): Promise<UnlistenFn> {
    return listen<SearchIndexProgressEvent>('search://index-progress', (event) => {
        handler(event.payload);
    });
}

/** Registra listener para o evento `search://invalidated`. */
export async function onSearchInvalidated(
    handler: (payload: SearchInvalidatedEvent) => void
): Promise<UnlistenFn> {
    return listen<SearchInvalidatedEvent>('search://invalidated', (event) => {
        handler(event.payload);
    });
}
