import { beforeEach, describe, expect, it, vi } from 'vitest';
import { LinkResolutionCache } from '@/composables/editorLinks/linkCache';
import { useEditorLinks } from '@/composables/useEditorLinks';
import { osIpc } from '@/ipc';
import type {
    LinkCandidate_Deserialize,
    LinkContext,
    OsEvent,
    PaneId,
    PtyId,
    ResolvedLink_Serialize,
    TabId
} from '@/types/bindings';

vi.mock('@/ipc', () => ({
    osIpc: {
        resolveLinks: vi.fn(),
        openLink: vi.fn(),
        onLinkContextChanged: vi.fn()
    }
}));

const BASE_CTX: LinkContext = {
    tabId: '01950000-0000-7000-8000-000000000001' as TabId,
    paneId: '01950000-0000-7000-8000-000000000002' as PaneId,
    ptyId: '01950000-0000-7000-8000-000000000003' as PtyId,
    cwdEpoch: 1
};

describe('useEditorLinks & LinkResolutionCache (Fase 09)', () => {
    let contextChangedHandler: ((ev: OsEvent<LinkContext>) => void) | null = null;

    beforeEach(() => {
        vi.clearAllMocks();
        contextChangedHandler = null;
        vi.mocked(osIpc.onLinkContextChanged).mockImplementation(async (cb) => {
            contextChangedHandler = cb;
            return () => {
                contextChangedHandler = null;
            };
        });
    });

    it('resolve candidatos em lote, armazena em cache LRU e retorna apenas arquivos existentes', async () => {
        vi.mocked(osIpc.resolveLinks).mockImplementationOnce(async (_ctx, candidates) => {
            const firstCand = candidates[0];
            if (!firstCand) {
                return [];
            }
            const resolved: ResolvedLink_Serialize = {
                key: firstCand.key,
                absolutePath: '/repo/src/main.ts',
                line: 10,
                column: 2
            };
            return [resolved];
        });

        const links = useEditorLinks(null, {
            getContext: () => BASE_CTX
        });

        const first = await links.resolveLine('Erro em src/main.ts:10:2 e src/missing.ts:1');
        expect(first).toHaveLength(1);
        expect(first[0]?.resolved.absolutePath).toBe('/repo/src/main.ts');
        expect(osIpc.resolveLinks).toHaveBeenCalledTimes(1);

        // Segunda chamada usa o cache LRU (tanto para hit positivo quanto negativo) sem chamar IPC novamente
        const second = await links.resolveLine('Erro em src/main.ts:10:2 e src/missing.ts:1');
        expect(second).toHaveLength(1);
        expect(osIpc.resolveLinks).toHaveBeenCalledTimes(1);

        links.dispose();
    });

    it('invalida entradas do cache quando os://link-context-changed é emitido para o painel', async () => {
        const cache = new LinkResolutionCache(16, 5_000);
        vi.mocked(osIpc.resolveLinks).mockImplementation(async (_ctx, candidates) => {
            const firstCand = candidates[0];
            if (!firstCand) {
                return [];
            }
            return [
                {
                    key: firstCand.key,
                    absolutePath: '/repo/src/lib.rs',
                    line: 42,
                    column: null
                }
            ];
        });

        const links = useEditorLinks(null, {
            getContext: () => BASE_CTX,
            cache
        });

        await links.resolveLine('em src/lib.rs:42');
        expect(cache.size).toBe(1);

        contextChangedHandler?.({
            revision: 2,
            payload: { ...BASE_CTX, cwdEpoch: 2 }
        });
        expect(cache.size).toBe(0);

        links.dispose();
    });

    it('exige Ctrl+Clique ou Meta+Clique para acionar os_link_open', async () => {
        vi.mocked(osIpc.openLink).mockResolvedValue(null);
        const links = useEditorLinks(null, {
            getContext: () => BASE_CTX
        });

        const candidate: LinkCandidate_Deserialize = {
            key: '1:13:src/lib.rs',
            path: 'src/lib.rs',
            line: 42,
            column: null
        };

        const plainClick = new MouseEvent('click', { ctrlKey: false, metaKey: false });
        expect(await links.openCandidate(candidate, plainClick)).toBe(false);
        expect(osIpc.openLink).not.toHaveBeenCalled();

        const ctrlClick = new MouseEvent('click', { ctrlKey: true });
        expect(await links.openCandidate(candidate, ctrlClick)).toBe(true);
        expect(osIpc.openLink).toHaveBeenCalledWith(BASE_CTX, candidate);

        links.dispose();
    });
});
