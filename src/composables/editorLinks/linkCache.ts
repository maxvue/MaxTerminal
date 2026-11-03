import type {
    LinkCandidate_Deserialize,
    LinkContext,
    PaneId,
    ResolvedLink_Serialize
} from '@/types/bindings';

interface CacheEntry {
    value: ResolvedLink_Serialize | null;
    expiresAt: number;
    paneId: PaneId;
}

/**
 * Cache LRU em memória com TTL para resultados de `os_link_resolve`.
 * Indexado por `(tabId, paneId, ptyId, cwdEpoch, path, line, column)`.
 * Armazena `ResolvedLink_Serialize` para arquivos existentes e `null` para arquivos inexistentes.
 */
export class LinkResolutionCache {
    private readonly maxEntries: number;
    private readonly ttlMs: number;
    private readonly map = new Map<string, CacheEntry>();

    public constructor(maxEntries = 512, ttlMs = 5_000) {
        this.maxEntries = maxEntries;
        this.ttlMs = ttlMs;
    }

    public static makeKey(ctx: LinkContext, candidate: LinkCandidate_Deserialize): string {
        return [
            ctx.tabId,
            ctx.paneId,
            ctx.ptyId,
            String(ctx.cwdEpoch),
            candidate.path,
            candidate.line ?? '',
            candidate.column ?? ''
        ].join('|');
    }

    public lookup(
        ctx: LinkContext,
        candidate: LinkCandidate_Deserialize,
        now = Date.now()
    ): { hit: true; value: ResolvedLink_Serialize | null } | { hit: false } {
        const key = LinkResolutionCache.makeKey(ctx, candidate);
        const entry = this.map.get(key);
        if (!entry) {
            return { hit: false };
        }
        if (now >= entry.expiresAt) {
            this.map.delete(key);
            return { hit: false };
        }
        // Promove para o fim da fila LRU
        this.map.delete(key);
        this.map.set(key, entry);
        return { hit: true, value: entry.value };
    }

    public set(
        ctx: LinkContext,
        candidate: LinkCandidate_Deserialize,
        value: ResolvedLink_Serialize | null,
        now = Date.now()
    ): void {
        const key = LinkResolutionCache.makeKey(ctx, candidate);
        if (this.map.has(key)) {
            this.map.delete(key);
        } else if (this.map.size >= this.maxEntries) {
            const oldestKey = this.map.keys().next().value;
            if (oldestKey !== undefined) {
                this.map.delete(oldestKey);
            }
        }
        this.map.set(key, {
            value,
            expiresAt: now + this.ttlMs,
            paneId: ctx.paneId
        });
    }

    public invalidatePane(paneId: PaneId): void {
        for (const [key, entry] of this.map.entries()) {
            if (entry.paneId === paneId) {
                this.map.delete(key);
            }
        }
    }

    public clear(): void {
        this.map.clear();
    }

    public get size(): number {
        return this.map.size;
    }
}
