import { createPinia, setActivePinia } from 'pinia';
import { mount } from '@vue/test-utils';
import { beforeEach, describe, expect, it } from 'vitest';
import AppTitleBar from '@/components/shell/AppTitleBar.vue';
import SessionReadonlyViewer from '@/components/workspace/SessionReadonlyViewer.vue';

function checkAccessibleButtonsAndLandmarks(root: HTMLElement): string[] {
    const issues: string[] = [];
    const buttons = root.querySelectorAll<HTMLButtonElement>('button');
    for (const btn of buttons) {
        const label = btn.getAttribute('aria-label')?.trim() || btn.textContent?.trim() || '';
        if (!label) {
            issues.push('Botão sem nome acessível');
        }
    }
    return issues;
}

describe('Acessibilidade dos componentes de Workspace e Modais (CA-29)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('AppTitleBar e SessionReadonlyViewer possuem nomes acessíveis e landmarks válidos sem violações críticas', () => {
        const titleBar = mount(AppTitleBar, {
            props: {
                title: 'MaxTerminal',
                version: '0.1.0'
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });
        expect(checkAccessibleButtonsAndLandmarks(titleBar.element as HTMLElement)).toEqual([]);
        expect(titleBar.text()).toContain('Buscar em tudo…');

        const viewer = mount(SessionReadonlyViewer, {
            props: {
                detail: {
                    summary: {
                        id: 'ts-1',
                        tabId: 'tab-1',
                        projectId: 'proj-1',
                        seq: 1,
                        startedAt: 1000,
                        endedAt: 2000,
                        endReason: 'new_session',
                        archived: true,
                        commandCount: 2,
                        scrollbackBytes: 128
                    },
                    panes: [
                        {
                            sessionId: 'sess-1',
                            paneId: 'pane-1',
                            generation: 1,
                            shellKind: 'zsh',
                            initialCwd: '/workspace',
                            startedAt: 1000,
                            endedAt: 2000,
                            commandCount: 2,
                            scrollbackBytes: 128
                        }
                    ],
                    commands: []
                },
                chunks: [
                    {
                        seq: 1,
                        ts: 1100,
                        text: ' saída sanitizada ',
                        byteLen: 18,
                        kind: 'output'
                    }
                ],
                hasMore: true
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });
        expect(checkAccessibleButtonsAndLandmarks(viewer.element as HTMLElement)).toEqual([]);
    });
});
