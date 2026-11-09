import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import TemplateEditorModal from '@/components/workspace/TemplateEditorModal.vue';
import IssueCreateModal from '@/components/github/IssueCreateModal.vue';
import IssueOpenTabModal from '@/components/github/IssueOpenTabModal.vue';
import BugCycleConfirmModal from '@/components/github/BugCycleConfirmModal.vue';
import ProjectAddModal from '@/components/projects/ProjectAddModal.vue';
import ProjectRootModal from '@/components/projects/ProjectRootModal.vue';

vi.mock('@/ipc/projects', () => ({
    pickFolder: vi.fn(),
    addProject: vi.fn(),
    detectStack: vi.fn()
}));

vi.mock('@/ipc/templates', () => ({
    templateTestConnection: vi.fn()
}));

describe('MaxModalBusyGuards — Protocolo beforeClose assíncrono para modais ocupados', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    describe('TemplateEditorModal', () => {
        it('impede fechamento com preventDefault() quando saving = true', async () => {
            const wrapper = mount(TemplateEditorModal, {
                props: {
                    open: true,
                    projectId: 'p1',
                    template: null,
                    saving: true
                }
            });

            const modal = wrapper.findComponent({ name: 'MaxModal' });
            expect(modal.exists()).toBe(true);
            const beforeClose = modal.props('beforeClose') as ((ev: { preventDefault: () => void }) => Promise<boolean>);
            expect(typeof beforeClose).toBe('function');

            const mockEv = { preventDefault: vi.fn() };
            const allowed = await beforeClose(mockEv);
            expect(allowed).toBe(false);
            expect(mockEv.preventDefault).toHaveBeenCalledTimes(1);

            await wrapper.setProps({ saving: false });
            const mockEvFree = { preventDefault: vi.fn() };
            const allowedFree = await beforeClose(mockEvFree);
            expect(allowedFree).toBe(true);
            expect(mockEvFree.preventDefault).not.toHaveBeenCalled();
        });
    });

    describe('IssueCreateModal', () => {
        it('impede fechamento com preventDefault() quando submitting = true', async () => {
            const wrapper = mount(IssueCreateModal, {
                props: {
                    open: true,
                    projectId: 'p1',
                    submitting: true
                }
            });

            const modal = wrapper.findComponent({ name: 'MaxModal' });
            expect(modal.exists()).toBe(true);
            const beforeClose = modal.props('beforeClose') as ((ev: { preventDefault: () => void }) => Promise<boolean>);
            expect(typeof beforeClose).toBe('function');

            const mockEv = { preventDefault: vi.fn() };
            const allowed = await beforeClose(mockEv);
            expect(allowed).toBe(false);
            expect(mockEv.preventDefault).toHaveBeenCalledTimes(1);

            await wrapper.setProps({ submitting: false });
            const mockEvFree = { preventDefault: vi.fn() };
            const allowedFree = await beforeClose(mockEvFree);
            expect(allowedFree).toBe(true);
            expect(mockEvFree.preventDefault).not.toHaveBeenCalled();
        });
    });

    describe('IssueOpenTabModal', () => {
        it('impede fechamento com preventDefault() quando submitting = true', async () => {
            const wrapper = mount(IssueOpenTabModal, {
                props: {
                    open: true,
                    projectId: 'p1',
                    issue: {
                        number: 42,
                        title: 'Bug na tela',
                        body: '',
                        state: 'open',
                        authorLogin: 'john',
                        commentsCount: 0,
                        hasPlan: false,
                        labels: []
                    },
                    resolution: null,
                    submitting: true
                }
            });

            const modal = wrapper.findComponent({ name: 'MaxModal' });
            expect(modal.exists()).toBe(true);
            const beforeClose = modal.props('beforeClose') as ((ev: { preventDefault: () => void }) => Promise<boolean>);
            expect(typeof beforeClose).toBe('function');

            const mockEv = { preventDefault: vi.fn() };
            const allowed = await beforeClose(mockEv);
            expect(allowed).toBe(false);
            expect(mockEv.preventDefault).toHaveBeenCalledTimes(1);

            await wrapper.setProps({ submitting: false });
            const mockEvFree = { preventDefault: vi.fn() };
            const allowedFree = await beforeClose(mockEvFree);
            expect(allowedFree).toBe(true);
            expect(mockEvFree.preventDefault).not.toHaveBeenCalled();
        });
    });

    describe('BugCycleConfirmModal', () => {
        it('impede fechamento com preventDefault() quando submitting = true', async () => {
            const wrapper = mount(BugCycleConfirmModal, {
                props: {
                    open: true,
                    projectId: 'p1',
                    preview: {
                        candidates: [],
                        skipped: []
                    },
                    submitting: true
                }
            });

            const modal = wrapper.findComponent({ name: 'MaxModal' });
            expect(modal.exists()).toBe(true);
            const beforeClose = modal.props('beforeClose') as ((ev: { preventDefault: () => void }) => Promise<boolean>);
            expect(typeof beforeClose).toBe('function');

            const mockEv = { preventDefault: vi.fn() };
            const allowed = await beforeClose(mockEv);
            expect(allowed).toBe(false);
            expect(mockEv.preventDefault).toHaveBeenCalledTimes(1);

            await wrapper.setProps({ submitting: false });
            const mockEvFree = { preventDefault: vi.fn() };
            const allowedFree = await beforeClose(mockEvFree);
            expect(allowedFree).toBe(true);
            expect(mockEvFree.preventDefault).not.toHaveBeenCalled();
        });
    });

    describe('ProjectAddModal e ProjectRootModal', () => {
        it('ProjectAddModal possui beforeClose configurado', () => {
            const wrapper = mount(ProjectAddModal, {
                props: { visible: true }
            });

            const modal = wrapper.findComponent({ name: 'MaxModal' });
            expect(modal.exists()).toBe(true);
            const beforeClose = modal.props('beforeClose');
            expect(typeof beforeClose).toBe('function');
        });

        it('ProjectRootModal possui beforeClose configurado', () => {
            const wrapper = mount(ProjectRootModal, {
                props: { visible: true, root: null }
            });

            const modal = wrapper.findComponent({ name: 'MaxModal' });
            expect(modal.exists()).toBe(true);
            const beforeClose = modal.props('beforeClose');
            expect(typeof beforeClose).toBe('function');
        });
    });
});
