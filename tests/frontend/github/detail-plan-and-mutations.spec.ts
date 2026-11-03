import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import IssueDetail from '@/components/github/IssueDetail.vue';
import IssueLabelEditor from '@/components/github/IssueLabelEditor.vue';
import IssuePlanSection from '@/components/github/IssuePlanSection.vue';
import { renderSafeMarkdown } from '@/composables/useMarkdownRenderer';
import type { IssueDetail as IssueDetailDto } from '@/types/bindings';

function makeDetail(stateLabel = 'Planned'): IssueDetailDto {
    return {
        summary: {
            repoKey: 'github.com/johnattas/maxterminal',
            number: 42,
            title: 'Implementar Aba Fixa de Issues',
            state: 'open',
            stateReason: null,
            authorLogin: 'johnattas',
            authorAvatarUrl: null,
            assignees: ['johnattas'],
            milestoneTitle: null,
            labels: [
                {
                    name: stateLabel,
                    nameLower: stateLabel.toLowerCase(),
                    colorHex: '0e8a16',
                    description: 'Estado',
                    canonicalGroup: 'state'
                },
                {
                    name: 'Layout / UI',
                    nameLower: 'layout / ui',
                    colorHex: 'c2e0c6',
                    description: 'Categoria UI',
                    canonicalGroup: 'category'
                }
            ],
            activeStateLabel: stateLabel,
            activeClosureLabel: null,
            categoryLabels: ['Layout / UI'],
            commentsCount: 1,
            hasPlan: true,
            htmlUrl: 'https://github.com/johnattas/MaxTerminal/issues/42',
            createdAtMs: 1700000000000,
            updatedAtMs: 1700000005000,
            closedAtMs: null
        },
        bodyMarkdown: 'Descrição com <script>alert(1)</script> e **negrito**.',
        comments: [
            {
                commentId: 900,
                issueNumber: 42,
                authorLogin: 'maxbot',
                authorAvatarUrl: null,
                bodyMarkdown: '## PLANO DE IMPLEMENTAÇÃO\n- Passo 1\n- Passo 2',
                htmlUrl: 'https://github.com/johnattas/MaxTerminal/issues/42#issuecomment-900',
                createdAtMs: 1700000001000,
                updatedAtMs: 1700000001000,
                containsActivePlan: true
            }
        ],
        commentsComplete: true,
        plan: {
            source: 'comment',
            commentId: 900,
            authorLogin: 'maxbot',
            updatedAtMs: 1700000001000,
            markdown: '- Passo 1\n- Passo 2',
            provisional: false
        }
    };
}

describe('Fase 16 — Detalhe da Issue, Plano de Implementação, Markdown Seguro e Editor de Labels', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('sanitiza tags perigosas e URLs javascript: no renderSafeMarkdown', () => {
        const html = renderSafeMarkdown(
            'Olá <script>evil()</script> [link](javascript:alert(1)) e [seguro](https://github.com)'
        );
        expect(html).not.toContain('<script>');
        expect(html).not.toContain('evil()');
        expect(html).not.toContain('href="javascript:');
        expect(html).toContain('href="https://github.com"');
    });

    it('exibe IssuePlanSection e marca como Provisório quando estado é Waiting Plan / Replan / Rejected', () => {
        const plannedWrapper = mount(IssuePlanSection, {
            props: { detail: makeDetail('Planned') },
            global: { stubs: { MaxIcon: true } }
        });
        expect(plannedWrapper.find('.issue-plan-section').exists()).toBe(true);
        expect(plannedWrapper.find('.issue-plan-section__provisional').exists()).toBe(false);
        expect(plannedWrapper.text()).toContain('Passo 1');

        const provisionalWrapper = mount(IssuePlanSection, {
            props: { detail: makeDetail('Waiting Plan') },
            global: { stubs: { MaxIcon: true } }
        });
        expect(provisionalWrapper.find('.issue-plan-section__provisional').exists()).toBe(true);
    });

    it('IssueLabelEditor emite patch respeitando exclusividade de state e closure e categorias acumuláveis', async () => {
        const onUpdate = vi.fn();
        const wrapper = mount(IssueLabelEditor, {
            props: {
                projectId: 'proj-1',
                issueNumber: 42,
                labels: [
                    {
                        name: 'Waiting Plan',
                        nameLower: 'waiting plan',
                        colorHex: 'fbca04',
                        description: 'Estado',
                        canonicalGroup: 'state'
                    },
                    {
                        name: 'Seguranca',
                        nameLower: 'seguranca',
                        colorHex: 'ee0701',
                        description: 'Categoria',
                        canonicalGroup: 'category'
                    }
                ],
                onUpdateLabels: onUpdate
            }
        });

        const selects = wrapper.findAll('select');
        await selects[0]!.setValue('Planned');
        expect(onUpdate).toHaveBeenCalledWith(
            expect.objectContaining({
                projectId: 'proj-1',
                issueNumber: 42,
                stateChange: { op: 'set', value: 'Planned' }
            })
        );

        const detailWrapper = mount(IssueDetail, {
            props: {
                projectId: 'proj-1',
                detail: makeDetail('Planned')
            },
            global: { stubs: { MaxIcon: true } }
        });
        expect(detailWrapper.text()).toContain('#42');
        expect(detailWrapper.text()).toContain('Implementar Aba Fixa de Issues');
    });
});
