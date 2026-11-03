import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as templatesIpc from '@/ipc/templates';
import { useTemplatesStore } from '@/stores/templates';
import type { TabTemplate_Serialize } from '@/types/bindings';

vi.mock('@/ipc/templates', () => ({
    templateList: vi.fn(),
    templateGet: vi.fn(),
    templateCreate: vi.fn(),
    templateUpdate: vi.fn(),
    templateDelete: vi.fn(),
    templateSetDefault: vi.fn(),
    onTemplateChanged: vi.fn()
}));

function makeTemplate(overrides: Partial<TabTemplate_Serialize> = {}): TabTemplate_Serialize {
    return {
        id: '01950000-0000-7000-8000-000000000001',
        projectId: '01950000-0000-7000-8000-000000000010',
        config: {
            name: 'Terminal',
            icon: 'lucide:terminal',
            cwd: null,
            initialCommand: null,
            addToWorkspace: false,
            continuous: false,
            environment: { kind: 'shell', agentId: null },
            envVars: {},
            connection: { kind: 'local' }
        },
        isDefault: true,
        position: 0,
        revision: 1,
        createdAt: 1000,
        updatedAt: 1000,
        ...overrides
    };
}

describe('useTemplatesStore (Fase 07)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('carrega templates do projeto e identifica o template padrão', async () => {
        const store = useTemplatesStore();
        const t1 = makeTemplate({ id: 'tpl-1', isDefault: true, position: 0 });
        const t2 = makeTemplate({
            id: 'tpl-2',
            isDefault: false,
            position: 1,
            config: {
                ...makeTemplate().config,
                name: 'Servidor Dev',
                continuous: true
            }
        });

        vi.mocked(templatesIpc.templateList).mockResolvedValueOnce([t2, t1]);

        const list = await store.loadForProject('proj-1');
        expect(list.map((t) => t.id)).toEqual(['tpl-1', 'tpl-2']);
        expect(store.defaultTemplateFor('proj-1')?.id).toBe('tpl-1');
    });

    it('cria, atualiza, define padrão e remove templates recarregando a lista', async () => {
        const store = useTemplatesStore();
        const t1 = makeTemplate({ id: 'tpl-1', projectId: 'proj-1', isDefault: true });
        const t2 = makeTemplate({
            id: 'tpl-2',
            projectId: 'proj-1',
            isDefault: false,
            position: 1
        });

        vi.mocked(templatesIpc.templateCreate).mockResolvedValueOnce(t2);
        vi.mocked(templatesIpc.templateList).mockResolvedValueOnce([t1, t2]);

        const created = await store.createTemplate('proj-1', {
            name: 'Vite',
            icon: 'lucide:zap',
            cwd: null,
            initialCommand: 'npm run dev',
            addToWorkspace: true,
            continuous: true,
            environment: { kind: 'shell', agentId: null },
            envVars: {},
            connection: { kind: 'local' },
            makeDefault: false
        });
        expect(created.id).toBe('tpl-2');
        expect(store.templatesFor('proj-1')).toHaveLength(2);

        const promoted = [
            { ...t1, isDefault: false, revision: 2 },
            { ...t2, isDefault: true, revision: 2 }
        ];
        vi.mocked(templatesIpc.templateSetDefault).mockResolvedValueOnce(promoted);
        await store.setDefaultTemplate('proj-1', 'tpl-2');
        expect(store.defaultTemplateFor('proj-1')?.id).toBe('tpl-2');
        expect(store.templateById('tpl-2')?.id).toBe('tpl-2');
        expect(store.templateById('inexistente')).toBeNull();

        // updateTemplate e deleteTemplate
        vi.mocked(templatesIpc.templateUpdate).mockResolvedValueOnce({ ...t2, revision: 3 });
        vi.mocked(templatesIpc.templateList).mockResolvedValueOnce([t1, { ...t2, revision: 3 }]);
        const updated = await store.updateTemplate('tpl-2', 2, {
            name: 'Vite Atualizado',
            icon: 'lucide:zap',
            cwd: null,
            initialCommand: 'npm run dev',
            addToWorkspace: true,
            continuous: true,
            environment: { kind: 'shell', agentId: null },
            envVars: {},
            makeDefault: true
        });
        expect(updated.revision).toBe(3);

        vi.mocked(templatesIpc.templateDelete).mockResolvedValueOnce(null);
        vi.mocked(templatesIpc.templateList).mockResolvedValueOnce([t1]);
        await store.deleteTemplate('proj-1', 'tpl-2', 3);
        expect(store.templatesFor('proj-1')).toHaveLength(1);

        vi.mocked(templatesIpc.templateList).mockResolvedValueOnce([t1]);
        store.handleTemplateChangedEvent({
            project_id: 'proj-1',
            template_ids: ['tpl-1']
        });
        store.clearProject('proj-1');
        expect(store.templatesFor('proj-1')).toHaveLength(0);
        store.resetForTests();
    });
});
