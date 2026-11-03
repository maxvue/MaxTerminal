import { createPinia, setActivePinia } from 'pinia';
import { mount } from '@vue/test-utils';
import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/ipc/search', () => ({
    searchGlobal: vi.fn(),
    cancelSearch: vi.fn(async () => ({ cancelled: true })),
    releaseSearch: vi.fn(async () => {}),
    getSearchIndexStats: vi.fn(async () => []),
    rebuildSearchIndex: vi.fn(async () => ({ jobId: 'job-1' })),
    onSearchIndexProgress: vi.fn(async () => () => {}),
    onSearchInvalidated: vi.fn(async () => () => {})
}));

import GlobalSearchPalette from '@/components/search/GlobalSearchPalette.vue';
import SearchResultSection from '@/components/search/SearchResultSection.vue';
import { SEARCH_SECTION_LABELS, useSearchStore } from '@/stores/search';

describe('Localização pt-BR na Busca Global (CA-15)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('exibe todos os rótulos obrigatórios de seção em pt-BR', () => {
        expect(SEARCH_SECTION_LABELS.commands).toBe('Blocos de comando');
        expect(SEARCH_SECTION_LABELS.output).toBe('Histórico de sessões');
        expect(SEARCH_SECTION_LABELS.projects).toBe('Projetos');
        expect(SEARCH_SECTION_LABELS.templates).toBe('Templates');
        expect(SEARCH_SECTION_LABELS.groups).toBe('Grupos de comandos');
        expect(SEARCH_SECTION_LABELS.issues).toBe('Issues');
    });

    it('exibe "Buscar em tudo…", "Nenhum resultado para “x”", "Carregar mais" e formata datas em pt-BR', async () => {
        const store = useSearchStore();
        store.show();
        store.query = 'inexistente';
        store.status = 'empty';

        const palette = mount(GlobalSearchPalette, {
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        expect(palette.html()).toContain('Buscar em tudo…');
        expect(palette.text()).toContain('Nenhum resultado para “inexistente”');

        const timestamp = Date.UTC(2026, 3, 17, 15, 30, 0);
        const expectedDate = new Intl.DateTimeFormat('pt-BR', {
            day: '2-digit',
            month: '2-digit',
            year: 'numeric',
            hour: '2-digit',
            minute: '2-digit'
        }).format(new Date(timestamp));

        const sectionWrapper = mount(SearchResultSection, {
            props: {
                section: {
                    source: { kind: 'commands' },
                    items: [
                        {
                            key: 'commands:1',
                            source: { kind: 'commands' },
                            target: {
                                kind: 'command',
                                projectId: 'proj-1',
                                tabId: 'tab-1',
                                sessionId: 'sess-1',
                                paneId: 'pane-1',
                                blockId: 'blk-1'
                            },
                            title: [{ text: 'php artisan test', matched: true }],
                            excerpt: [{ text: 'PASS', matched: false }],
                            occurredAtMs: timestamp,
                            sourceTruncated: false,
                            fetchedAtMs: null
                        }
                    ],
                    nextCursor: 'next-tok',
                    truncated: false
                },
                selectedKey: 'commands:1',
                loadingMore: false,
                optionIdFor: (k: string) => `opt-${k}`
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        expect(sectionWrapper.text()).toContain('Blocos de comando');
        expect(sectionWrapper.text()).toContain('Carregar mais');
        expect(sectionWrapper.text()).toContain(expectedDate);
    });
});
