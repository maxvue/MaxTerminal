import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import type { CompletionItem } from '@/types/terminal';
import CompletionPopup from '@/components/terminal/CompletionPopup.vue';

describe('CompletionPopup.vue', () => {
    it('renderiza cabeçalho, seções, itens fixos no rodapé (pinned: bottom) e emite hover/select', async () => {
        const items: CompletionItem[] = [
            { id: 'g1', label: 'Subir Ambiente', section: 'local', kind: 'value' },
            { id: 'g2', label: 'Limpar Caches', section: 'global', kind: 'value' },
            { id: 'create-local', label: 'Criar grupo de comandos local', kind: 'action', pinned: 'bottom' }
        ];

        const wrapper = mount(CompletionPopup, {
            props: {
                title: 'Grupos de Comandos',
                stageLabel: 'Etapa 1',
                items,
                sections: [
                    { id: 'local', label: 'Projeto Atual', order: 1 },
                    { id: 'global', label: 'Globais', order: 2 }
                ],
                activeIndex: 0,
                loading: false,
                error: null
            }
        });

        expect(wrapper.find('.popup-title').text()).toBe('Grupos de Comandos');
        expect(wrapper.find('.popup-stage').text()).toBe('Etapa 1');
        expect(wrapper.findAll('.popup-section-header').map((el) => el.text())).toEqual([
            'Projeto Atual',
            'Globais'
        ]);
        expect(wrapper.find('.popup-pinned-footer').text()).toContain(
            'Criar grupo de comandos local'
        );

        const options = wrapper.findAll('[role="option"]');
        await options[1]!.trigger('mouseenter');
        expect(wrapper.emitted('hover')?.[0]).toEqual([1]);

        await options[2]!.trigger('mousedown');
        expect(wrapper.emitted('select')?.[0]?.[0]).toMatchObject({ id: 'create-local' });
    });

    it('ativa virtualização de lista quando há mais de 200 itens', () => {
        const manyItems: CompletionItem[] = Array.from({ length: 350 }, (_, idx) => ({
            id: `cmd-${idx}`,
            label: `artisan:cmd-${idx}`,
            kind: 'value'
        }));

        const wrapper = mount(CompletionPopup, {
            props: {
                title: 'Comandos Artisan',
                items: manyItems,
                activeIndex: 0,
                loading: false,
                error: null
            }
        });

        expect(wrapper.find('.virtual-spacer').exists()).toBe(true);
        // Com virtualização ativa, renderiza apenas a janela visível (<< 350 nós DOM).
        expect(wrapper.findAll('[role="option"]').length).toBeLessThan(40);
    });
});
