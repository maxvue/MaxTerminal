import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useAiGenerate } from '@/composables/ai/useAiGenerate';
import { aiIpc } from '@/ipc/ai';

describe('useAiGenerate (Fase 12 — C-05 / C-06)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('gera JSON estruturado validado por schema, aplica decode opcional e expõe repaired/latencyMs', async () => {
        vi.spyOn(aiIpc, 'generateStructured').mockResolvedValueOnce({
            requestId: 'req-struct-1',
            value: {
                className: 'SyncUsersCommand',
                signature: 'users:sync {--force}',
                description: 'Sincroniza usuários com o ERP'
            },
            repaired: true,
            latencyMs: 410
        });

        const gen = useAiGenerate<{
            className: string;
            signature: string;
            description: string;
        }>({
            schemaId: 'artisan.command.v1',
            schema: {
                type: 'object',
                required: ['className', 'signature', 'description']
            },
            decode: (raw) => raw as { className: string; signature: string; description: string }
        });

        const result = await gen.generate('Criar comando para sincronizar usuários', {
            projectId: 'proj-1'
        });

        expect(result.className).toBe('SyncUsersCommand');
        expect(gen.data.value?.signature).toBe('users:sync {--force}');
        expect(gen.repaired.value).toBe(true);
        expect(gen.latencyMs.value).toBe(410);
        expect(gen.loading.value).toBe(false);
        expect(gen.error.value).toBeNull();

        gen.reset();
        expect(gen.data.value).toBeNull();
        expect(gen.repaired.value).toBe(false);
    });

    it('rejeita prompt vazio e permite cancelar requisição em andamento', async () => {
        const gen = useAiGenerate({
            schemaId: 'command-group.suggestion.v1',
            schema: { type: 'object' }
        });

        await expect(gen.generate('   ')).rejects.toThrow(
            'Informe uma descrição ou objetivo para gerar com IA.'
        );
        expect(gen.error.value).toContain('Informe uma descrição');

        const cancelSpy = vi.spyOn(aiIpc, 'cancel').mockResolvedValue(true);
        vi.spyOn(aiIpc, 'generateStructured').mockImplementationOnce(
            () =>
                new Promise((resolve) => {
                    setTimeout(() => {
                        resolve({
                            requestId: 'req-slow',
                            value: { name: 'Dev' },
                            repaired: false,
                            latencyMs: 100
                        });
                    }, 50);
                })
        );

        const pending = gen.generate('Subir ambiente completo');
        const cancelled = await gen.cancel();
        expect(cancelled).toBe(true);
        expect(cancelSpy).toHaveBeenCalledTimes(1);

        await expect(pending).rejects.toThrow('Geração substituída');
    });
});
