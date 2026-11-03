import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { memoryIpc } from '@/ipc/memory';
import { skillsIpc } from '@/ipc/skills';
import { useSkillsStore } from '@/stores/skills';
import type { MemoryEntry_Serialize, SyncPlan, SyncResult } from '@/types/bindings';

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn(() => Promise.resolve(() => {}))
}));

describe('useSkillsStore (Fase 13)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('executa scan, planSync (dry-run) e applySync limpando plano consumido', async () => {
        vi.spyOn(skillsIpc, 'scan').mockResolvedValue({
            sourceDir: '/home/user/GitHub/MaxSkills',
            fromIndex: true,
            scannedAtMs: 1_700_000_000_000,
            skills: [
                {
                    name: 'vue-components',
                    description: 'Componentes Vue 3',
                    sourceDir: '/home/user/GitHub/MaxSkills/vue-components',
                    tags: ['vue'],
                    stacks: ['vue'],
                    hash: 'a'.repeat(64),
                    relevance: 'recommended'
                }
            ]
        });

        const fakePlan: SyncPlan = {
            planId: 'plan-1',
            targetId: 7,
            destDir: '/proj/.claude/skills',
            actions: [
                {
                    action: 'conflict',
                    skill: 'vue-components',
                    dest: '/proj/.claude/skills/vue-components',
                    conflict: { kind: 'unmanaged' }
                }
            ],
            warnings: [],
            fingerprint: 'b'.repeat(64)
        };
        vi.spyOn(skillsIpc, 'plan').mockResolvedValue(fakePlan);

        const fakeResult: SyncResult = {
            targetId: 7,
            applied: 1,
            skipped: 0,
            backups: ['/data/skills-backup/7/vue-components'],
            errors: []
        };
        const applySpy = vi.spyOn(skillsIpc, 'apply').mockResolvedValue(fakeResult);

        const store = useSkillsStore();
        const catalog = await store.scan('proj-1');
        expect(catalog.skills).toHaveLength(1);
        expect(store.catalogFor('proj-1')?.sourceDir).toBe('/home/user/GitHub/MaxSkills');

        const plan = await store.planSync(7);
        expect(plan.planId).toBe('plan-1');
        expect(store.plansByTarget[7]).toBeDefined();

        const result = await store.applySync('plan-1', { 'vue-components': 'overwrite' }, 7);
        expect(applySpy).toHaveBeenCalledWith('plan-1', { 'vue-components': 'overwrite' });
        expect(result.applied).toBe(1);
        expect(store.plansByTarget[7]).toBeUndefined();
        expect(store.syncResults[7]?.backups).toHaveLength(1);
    });

    it('gerencia memória contextual (loadMemory, pin, hide, forget e rebuild)', async () => {
        const entry: MemoryEntry_Serialize = {
            projectId: 'proj-1',
            command: 'cargo test --workspace',
            useCount: 5,
            successCount: 5,
            failureCount: 0,
            lastUsedAtMs: 1_700_000_000_000,
            lastCwd: '/proj',
            lastExitCode: 0,
            pinned: false,
            hidden: false,
            score: 2.5
        };

        vi.spyOn(memoryIpc, 'list').mockResolvedValue([entry]);
        vi.spyOn(memoryIpc, 'pin').mockResolvedValue({ ...entry, pinned: true, score: 1002.5 });
        vi.spyOn(memoryIpc, 'hide').mockResolvedValue({ ...entry, pinned: true, hidden: true });
        vi.spyOn(memoryIpc, 'forget').mockResolvedValue(true);

        const store = useSkillsStore();
        await store.loadMemory('proj-1');
        expect(store.memoryByProject['proj-1']).toHaveLength(1);

        await store.pin('proj-1', 'cargo test --workspace', true);
        expect(store.memoryByProject['proj-1']?.[0]?.pinned).toBe(true);

        store.suggestionsByPane['pane-1'] = [entry];
        await store.hide('proj-1', 'cargo test --workspace', true);
        expect(store.suggestionsByPane['pane-1']).toHaveLength(0);

        await store.forget('proj-1', 'cargo test --workspace');
        expect(store.memoryByProject['proj-1']).toHaveLength(0);
    });
});
