import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import CommandBlockStatus from '@/components/shell/CommandBlockStatus.vue';
import TabStatusIndicator from '@/components/shell/TabStatusIndicator.vue';

describe('TabStatusIndicator & CommandBlockStatus', () => {
    it('renderiza os 5 estados canônicos em pt-BR e projeta conclusões vistas para Parado', () => {
        const running = mount(TabStatusIndicator, {
            props: { status: 'running' }
        });
        expect(running.text()).toContain('Executando');
        expect(running.attributes('data-bucket')).toBe('running');

        const continuous = mount(TabStatusIndicator, {
            props: { status: 'continuous' }
        });
        expect(continuous.text()).toContain('Execução Contínua');

        const completedUnseen = mount(TabStatusIndicator, {
            props: { status: 'completed_unseen', seen: false }
        });
        expect(completedUnseen.text()).toContain('Finalizado');
        expect(completedUnseen.attributes('data-bucket')).toBe('completed_unseen');

        const completedSeen = mount(TabStatusIndicator, {
            props: { status: 'completed_unseen', seen: true }
        });
        expect(completedSeen.text()).toContain('Parado');
        expect(completedSeen.attributes('data-bucket')).toBe('idle');

        const errorUnseen = mount(TabStatusIndicator, {
            props: { status: 'error_unseen', unknownOutcome: true }
        });
        expect(errorUnseen.text()).toContain('Com Erro');
        expect(errorUnseen.text()).toContain('Resultado desconhecido');
    });

    it('exibe código de saída, resultado desconhecido, falha de persistência e saída parcial em CommandBlockStatus', () => {
        const failedBlock = mount(CommandBlockStatus, {
            props: {
                status: 'error',
                backendStatus: 'failed',
                exitCode: 42,
                persistence: 'failed',
                outputTruncated: true
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });
        expect(failedBlock.text()).toContain('Com erro');
        expect(failedBlock.text()).toContain('Código de saída 42');
        expect(failedBlock.text()).toContain('Histórico não salvo');
        expect(failedBlock.text()).toContain('Saída parcial');

        const unknownBlock = mount(CommandBlockStatus, {
            props: {
                status: 'aborted',
                backendStatus: 'pty_exited',
                exitCode: null
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });
        expect(unknownBlock.text()).toContain('Resultado desconhecido');
    });
});
