import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import BugCycleConfirmModal from '@/components/github/BugCycleConfirmModal.vue';
import BugCycleProgress from '@/components/github/BugCycleProgress.vue';
import IssueActionsMenu from '@/components/github/IssueActionsMenu.vue';
import IssueOpenTabModal from '@/components/github/IssueOpenTabModal.vue';
import type { BugCyclePreview, BugCycleRunState, IssueSummary } from '@/types/bindings';

const sampleSummary: IssueSummary = {
    repoKey: 'github.com/johnattas/maxterminal',
    number: 77,
    title: 'Falha ao sincronizar worktree',
    state: 'open',
    stateReason: null,
    authorLogin: 'johnattas',
    authorAvatarUrl: null,
    assignees: [],
    milestoneTitle: null,
    labels: [
        {
            name: 'Integracao',
            nameLower: 'integracao',
            colorHex: 'bfdadc',
            description: 'Categoria',
            canonicalGroup: 'category'
        }
    ],
    activeStateLabel: null,
    activeClosureLabel: null,
    categoryLabels: ['Integracao'],
    commentsCount: 0,
    hasPlan: false,
    htmlUrl: 'https://github.com/johnattas/MaxTerminal/issues/77',
    createdAtMs: 1700000000000,
    updatedAtMs: 1700000005000,
    closedAtMs: null
};

describe('Fase 16 — Ações 1-Clique MaxCode, Abertura de Aba em Worktree e Ciclo em Lote', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('dispara ações Abrir, Analisar (BUG PLAN), Corrigir (BUG FIX) e Abrir Aba no IssueActionsMenu', async () => {
        const wrapper = mount(IssueActionsMenu, {
            global: { stubs: { MaxIcon: true } }
        });

        const buttons = wrapper.findAll('button');
        await buttons[0]!.trigger('click');
        await buttons[1]!.trigger('click');
        await buttons[2]!.trigger('click');
        await buttons[3]!.trigger('click');

        expect(wrapper.emitted('send-maxcode')).toEqual([['open'], ['bug_plan'], ['bug_fix']]);
        expect(wrapper.emitted('open-tab-modal')).toHaveLength(1);
    });

    it('IssueOpenTabModal permite selecionar Terminal ou Agente CLI e emite OpenIssueTabRequest', async () => {
        const wrapper = mount(IssueOpenTabModal, {
            props: {
                open: true,
                projectId: 'proj-1',
                issue: sampleSummary,
                resolution: {
                    status: 'existing_worktree',
                    path: '/repo/.max-code-worktrees/wt-issue-77',
                    branch: 'fix/issue-77'
                }
            },
            global: { stubs: { MaxIcon: true } }
        });

        expect(wrapper.text()).toContain('wt-issue-77');
        const radios = wrapper.findAll('input[type="radio"]');
        await radios[3]!.setValue(); // agy

        const confirmBtn = wrapper.find('.issue-open-tab-modal__btn--primary');
        await confirmBtn.trigger('click');

        expect(wrapper.emitted('confirm')?.[0]?.[0]).toEqual({
            projectId: 'proj-1',
            issueNumber: 77,
            target: {
                kind: 'agent',
                agent: 'agy',
                inject_issue_context: true
            },
            directory: {
                strategy: 'use_or_create_issue_worktree',
                branch_name: null,
                base_ref: null
            }
        });
    });

    it('BugCycleConfirmModal e BugCycleProgress exibem prévia, iniciam e controlam o ciclo em lote', async () => {
        const preview: BugCyclePreview = {
            projectId: 'proj-1',
            repoKey: 'github.com/johnattas/maxterminal',
            candidates: [
                {
                    issueNumber: 77,
                    title: 'Bug A',
                    activeStateLabel: 'Waiting Plan',
                    hasPlan: false,
                    plannedStage: 'plan'
                }
            ],
            skipped: [
                {
                    issueNumber: 78,
                    title: 'Bug B',
                    reason: 'Aguardando aprovação humana'
                }
            ],
            activeCycle: null
        };

        const modal = mount(BugCycleConfirmModal, {
            props: {
                open: true,
                projectId: 'proj-1',
                preview
            },
            global: { stubs: { MaxIcon: true } }
        });

        expect(modal.text()).toContain('#77');
        expect(modal.text()).toContain('Aguardando aprovação humana');
        await modal.find('.bug-cycle-confirm-modal__btn--primary').trigger('click');
        expect(modal.emitted('start')?.[0]?.[0]).toEqual({
            projectId: 'proj-1',
            issueNumbers: [77],
            includeReplanAndRejected: true
        });

        const cycle: BugCycleRunState = {
            cycleId: 'cycle-1',
            projectId: 'proj-1',
            repoKey: 'github.com/johnattas/maxterminal',
            status: 'running',
            totalIssues: 1,
            completedIssues: 0,
            failedIssues: 0,
            currentIssue: 77,
            currentStage: 'plan',
            issueNumbers: [77],
            skipped: [],
            errorMessage: null,
            startedAtMs: 1700000000000,
            updatedAtMs: 1700000001000,
            finishedAtMs: null
        };

        const progress = mount(BugCycleProgress, {
            props: { cycle },
            global: { stubs: { MaxIcon: true } }
        });
        expect(progress.text()).toContain('Processando #77');
        await progress.find('button').trigger('click');
        expect(progress.emitted('control')?.[0]).toEqual(['pause']);
    });
});
