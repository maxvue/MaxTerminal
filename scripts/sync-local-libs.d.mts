/**
 * Declaração de tipos para o módulo scripts/sync-local-libs.mjs
 */

export interface SyncOptions {
    workspaceRoot?: string;
    libsRoot?: string;
    checkOnly?: boolean;
    logger?: (line: string) => void;
}

export interface SyncResultItem {
    pkg: string;
    target?: string;
    error?: string;
}

export interface SyncResult {
    ok: boolean;
    results: SyncResultItem[];
}

export function syncLocalLibs(options?: SyncOptions): SyncResult;
