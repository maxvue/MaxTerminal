import { beforeEach, describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import CommandBlock from '@/components/terminal/CommandBlock.vue';
import type { BlockAction, CommandBlockModel } from '@/types/terminal';
import type { VueWebTerminalProjectedEntry } from '@/terminal/blocks/vueWebTerminalAdapter';

describe('CommandBlock.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('renderiza status semântico, duração em pt-BR, código de saída, botões de ação e slot actions', async () => {
        const block: CommandBlockModel = {
            id: 'blk-ui',
            paneId: 'pane-1',
            command: 'composer test',
            cwd: '~/GitHub/MaxTerminal',
            startedAt: 1000,
            finishedAt: 3400,
            exitCode: 1,
            status: 'error',
            promptLine: 5,
            outputStartLine: 6,
            outputEndLine: 20,
            meta: Object.freeze({})
        };

        const entry: VueWebTerminalProjectedEntry = {
            key: 'pane-1:blk-ui',
            blockId: 'blk-ui',
            type: 'normal',
            class: 'error',
            tag: 'ERRO 1',
            content: 'composer test',
            command: 'composer test',
            cwd: '~/GitHub/MaxTerminal',
            status: 'error',
            exitCode: 1,
            durationMs: 2400
        };

        const actions: BlockAction[] = [
            {
                id: 'copy-command',
                label: 'Copiar comando',
                icon: 'mdi:content-copy',
                order: 10,
                when: () => true,
                run: () => {}
            }
        ];

        const wrapper = mount(CommandBlock, {
            props: {
                block,
                entry,
                actions
            },
            slots: {
                actions: '<template #actions="{ block: b }"><button class="slot-inspect-btn">Inspecionar {{ b.id }}</button></template>'
            }
        });

        expect(wrapper.find('.status-text').text()).toBe('Com erro');
        expect(wrapper.find('.block-command').text()).toBe('composer test');
        expect(wrapper.find('.block-duration').text()).toBe('2,4 s');
        expect(wrapper.find('.block-exit-code').text()).toContain('exit 1');
        expect(wrapper.find('.slot-inspect-btn').text()).toBe('Inspecionar blk-ui');

        await wrapper.find('.block-action-btn').trigger('click');
        expect(wrapper.emitted('action')?.[0]).toEqual(['copy-command']);
    });
});
