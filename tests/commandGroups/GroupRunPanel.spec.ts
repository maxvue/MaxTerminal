import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import GroupRunPanel from '@/components/commandGroups/GroupRunPanel.vue';
import type { GroupRun, GroupRunItem } from '@/types/commandGroups';

function makeItem(overrides: Partial<GroupRunItem> = {}): GroupRunItem {
    return {
        id: 'run-item-1',
        runId: 'run-1',
        position: 0,
        snapshot: {
            sourceItemId: 'src-1',
            position: 0,
            title: 'Comando 1',
            command: 'npm run build',
            definedCwd: null,
            resolvedCwd: '/home/user/proj',
            env: {},
            continuous: false,
            templateId: null,
            shellPath: '/bin/zsh',
            shellDialect: 'zsh'
        },
        status: 'running',
        version: 1,
        ptyId: null,
        tabId: null,
        paneId: null,
        sessionId: null,
        commandBlockId: null,
        exitCode: null,
        signal: null,
        errorCode: null,
        errorMessage: null,
        startedAt: 1700000000000,
        finishedAt: null,
        outputTruncated: false,
        ...overrides
    };
}

function makeRun(overrides: Partial<GroupRun> = {}): GroupRun {
    return {
        id: 'run-1',
        requestId: 'req-1',
        groupId: 'grp-1',
        projectId: 'proj-1',
        originSessionId: null,
        snapshot: {
            groupId: 'grp-1',
            groupRevision: 1,
            scope: 'local',
            name: 'Subir Ambiente',
            description: '',
            labels: [],
            definedMode: 'parallel',
            effectiveMode: 'parallel',
            separator: 'and',
            cwd: null,
            env: {},
            projectRoot: '/home/user/proj',
            shellPath: '/bin/zsh',
            shellDialect: 'zsh',
            originTabId: null,
            originPaneId: null,
            originPtyId: null,
            originSessionId: null,
            originGeneration: null
        },
        mode: 'parallel',
        status: 'running',
        counts: {
            total: 4,
            pending: 0,
            starting: 0,
            running: 2,
            cancelling: 0,
            succeeded: 1,
            failed: 1,
            skipped: 0,
            cancelled: 0,
            interrupted: 0
        },
        version: 4,
        lastExitCode: 1,
        createdAt: 1700000000000,
        startedAt: 1700000000000,
        cancelRequestedAt: null,
        finishedAt: null,
        items: [
            makeItem({
                id: 'item-running-temp',
                position: 0,
                status: 'running',
                snapshot: {
                    ...makeItem().snapshot,
                    title: 'Build Temporário',
                    continuous: false
                }
            }),
            makeItem({
                id: 'item-running-cont',
                position: 1,
                status: 'running',
                snapshot: {
                    ...makeItem().snapshot,
                    position: 1,
                    title: 'Servidor Contínuo',
                    continuous: true
                }
            }),
            makeItem({
                id: 'item-succeeded',
                position: 2,
                status: 'succeeded',
                exitCode: 0,
                snapshot: {
                    ...makeItem().snapshot,
                    position: 2,
                    title: 'Migração Concluída',
                    continuous: false
                }
            }),
            makeItem({
                id: 'item-failed',
                position: 3,
                status: 'failed',
                exitCode: 2,
                snapshot: {
                    ...makeItem().snapshot,
                    position: 3,
                    title: 'Lint com Falha',
                    continuous: false
                }
            })
        ],
        ...overrides
    };
}

describe('GroupRunPanel.vue', () => {
    it('panel_renders_semantic_badges_for_continuous_and_exit_codes', () => {
        const run = makeRun();
        const wrapper = mount(GroupRunPanel, {
            props: {
                run
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        const rowTemp = wrapper.get('[data-testid="group-run-item-item-running-temp"]');
        const badgeTemp = rowTemp.get('[data-testid="group-run-item-badge"]');
        expect(badgeTemp.classes()).toContain('badge-success');
        expect(badgeTemp.text()).toContain('Executando');

        const rowCont = wrapper.get('[data-testid="group-run-item-item-running-cont"]');
        const badgeCont = rowCont.get('[data-testid="group-run-item-badge"]');
        expect(badgeCont.classes()).toContain('badge-info');
        expect(badgeCont.text()).toContain('Execução Contínua');

        const rowSucc = wrapper.get('[data-testid="group-run-item-item-succeeded"]');
        const badgeSucc = rowSucc.get('[data-testid="group-run-item-badge"]');
        expect(badgeSucc.classes()).toContain('badge-warning');
        expect(badgeSucc.text()).toContain('Finalizado (0)');

        const rowFail = wrapper.get('[data-testid="group-run-item-item-failed"]');
        const badgeFail = rowFail.get('[data-testid="group-run-item-badge"]');
        expect(badgeFail.classes()).toContain('badge-danger');
        expect(badgeFail.text()).toContain('Falhou (exit 2)');
    });

    it('panel_displays_background_non_interactive_note_in_parallel_mode', () => {
        const wrapper = mount(GroupRunPanel, {
            props: {
                run: makeRun({ mode: 'parallel' })
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        const note = wrapper.get('[data-testid="group-run-parallel-note"]');
        expect(note.text()).toContain(
            'Modo em background sem entrada interativa — use Multi-Abas para comandos interativos'
        );
    });

    it('panel_offers_run_in_new_tab_when_origin_pane_is_busy', async () => {
        const wrapper = mount(GroupRunPanel, {
            props: {
                run: null,
                busyPaneFallback: {
                    groupId: 'grp-busy-1',
                    projectId: 'proj-1',
                    reason: 'O painel atual está ocupado executando outro comando.'
                }
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        expect(wrapper.get('[data-testid="group-run-busy-fallback"]').text()).toContain(
            'O painel atual está ocupado'
        );

        await wrapper.get('[data-testid="group-run-in-new-tab-btn"]').trigger('click');
        expect(wrapper.emitted('run-in-new-tab')).toEqual([
            [{ groupId: 'grp-busy-1', projectId: 'proj-1' }]
        ]);
    });
});
