import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { mount } from '@vue/test-utils';
import TerminalExitBanner from '@/components/terminal/TerminalExitBanner.vue';

describe('TerminalExitBanner.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('exibe mensagem por código de saída ou sinal e emite restart-requested ao clicar em Reiniciar', async () => {
        const wrapperCode = mount(TerminalExitBanner, {
            props: {
                exit: {
                    code: 127,
                    signal: null,
                    signalName: null,
                    reason: 'exited',
                    atMs: '1700000000000'
                }
            }
        });
        expect(wrapperCode.text()).toContain('Processo encerrado (código 127)');

        await wrapperCode.find('button.restart-btn').trigger('click');
        expect(wrapperCode.emitted('restart-requested')).toHaveLength(1);

        const wrapperSignal = mount(TerminalExitBanner, {
            props: {
                exit: {
                    code: null,
                    signal: 9,
                    signalName: 'SIGKILL',
                    reason: 'signaled',
                    atMs: '1700000000000'
                }
            }
        });
        expect(wrapperSignal.text()).toContain('Processo encerrado pelo sinal SIGKILL');
    });
});
