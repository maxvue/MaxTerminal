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
import { useSearchStore } from '@/stores/search';

interface A11yViolation {
    id: string;
    impact: 'minor' | 'moderate' | 'serious' | 'critical';
    description: string;
}

/**
 * Executa auditoria WCAG AA / ARIA (compatível com regras serious/critical do axe-core)
 * sobre o container DOM renderizado.
 */
function auditSeriousAndCriticalA11y(root: HTMLElement): A11yViolation[] {
    const violations: A11yViolation[] = [];

    // 1. aria-dialog-name: todo role="dialog" deve ter aria-label ou aria-labelledby válido
    const dialogs = root.querySelectorAll<HTMLElement>('[role="dialog"]');
    for (const dialog of dialogs) {
        const label = dialog.getAttribute('aria-label')?.trim();
        const labelledBy = dialog.getAttribute('aria-labelledby')?.trim();
        const titleEl = labelledBy ? root.querySelector(`#${CSS.escape(labelledBy)}`) : null;
        if (!label && (!titleEl || !titleEl.textContent?.trim())) {
            violations.push({
                id: 'aria-dialog-name',
                impact: 'serious',
                description: 'Elemento role="dialog" sem nome acessível.'
            });
        }
    }

    // 2. combobox + listbox + aria-activedescendant
    const comboboxes = root.querySelectorAll<HTMLElement>('[role="combobox"]');
    for (const cb of comboboxes) {
        if (!cb.getAttribute('aria-expanded')) {
            violations.push({
                id: 'aria-required-attr',
                impact: 'critical',
                description: 'role="combobox" exige aria-expanded.'
            });
        }
        const isExpanded = cb.getAttribute('aria-expanded') === 'true';
        const controlsId = cb.getAttribute('aria-controls');
        if (isExpanded && (!controlsId || !root.querySelector(`#${CSS.escape(controlsId)}`))) {
            violations.push({
                id: 'aria-valid-attr-value',
                impact: 'critical',
                description: 'aria-controls deve referenciar um listbox existente quando o combobox estiver aberto.'
            });
        } else if (controlsId && !root.querySelector(`#${CSS.escape(controlsId)}`)) {
            violations.push({
                id: 'aria-valid-attr-value',
                impact: 'critical',
                description: 'aria-controls deve referenciar um listbox existente.'
            });
        }
        const activeDesc = cb.getAttribute('aria-activedescendant');
        if (activeDesc && !root.querySelector(`#${CSS.escape(activeDesc)}`)) {
            violations.push({
                id: 'aria-valid-attr-value',
                impact: 'serious',
                description: `aria-activedescendant="${activeDesc}" aponta para ID inexistente.`
            });
        }
    }

    // 3. button-name: todo botão deve ter texto visível ou aria-label
    const buttons = root.querySelectorAll<HTMLButtonElement>('button');
    for (const btn of buttons) {
        const text = btn.textContent?.trim() ?? '';
        const ariaLabel = btn.getAttribute('aria-label')?.trim() ?? '';
        if (!text && !ariaLabel) {
            violations.push({
                id: 'button-name',
                impact: 'critical',
                description: 'Botão sem texto ou aria-label acessível.'
            });
        }
    }

    // 4. select-name / input label
    const selects = root.querySelectorAll<HTMLSelectElement>('select');
    for (const sel of selects) {
        const ariaLabel = sel.getAttribute('aria-label')?.trim() ?? '';
        const hasWrappingLabel = Boolean(sel.closest('label'));
        if (!ariaLabel && !hasWrappingLabel) {
            violations.push({
                id: 'select-name',
                impact: 'critical',
                description: 'Elemento <select> sem rótulo acessível.'
            });
        }
    }

    return violations;
}

describe('Acessibilidade da GlobalSearchPalette (CA-29)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('não possui violações serious ou critical nos estados vazio, com resultados e com filtros ativos', async () => {
        const store = useSearchStore();
        store.show();

        const wrapper = mount(GlobalSearchPalette, {
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        // Estado 1: vazio / idle
        expect(auditSeriousAndCriticalA11y(wrapper.element as HTMLElement)).toEqual([]);

        // Estado 2: com resultados e item selecionado
        store.status = 'ready';
        store.sections = [
            {
                source: { kind: 'commands' },
                items: [
                    {
                        key: 'commands:10',
                        source: { kind: 'commands' },
                        target: {
                            kind: 'command',
                            projectId: 'proj-1',
                            tabId: 'tab-1',
                            sessionId: 'sess-1',
                            paneId: 'pane-1',
                            blockId: 'blk-10'
                        },
                        title: [{ text: 'npm run check', matched: true }],
                        excerpt: [{ text: 'Todos os checks aprovados', matched: false }],
                        occurredAtMs: 1_700_000_000_000,
                        sourceTruncated: false,
                        fetchedAtMs: null
                    }
                ],
                nextCursor: 'cursor-next',
                truncated: false
            }
        ];
        store.selectedKey = 'commands:10';
        await wrapper.vm.$nextTick();

        expect(auditSeriousAndCriticalA11y(wrapper.element as HTMLElement)).toEqual([]);

        // Estado 3: com filtro de saída ativo
        store.filters = {
            ...store.filters,
            exit: { kind: 'nonzero' }
        };
        await wrapper.vm.$nextTick();

        expect(auditSeriousAndCriticalA11y(wrapper.element as HTMLElement)).toEqual([]);
    });
});
