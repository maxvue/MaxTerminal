import { describe, expect, it } from 'vitest';
import {
    fuzzyMatch,
    fuzzyScore,
    rankCommands,
    rankOptions
} from '@/composables/quick-commands/fuzzy';
import type { ArtisanCommand, ArtisanOption } from '@/types/bindings';

describe('fuzzy (Fase 14 — CA-12)', () => {
    it('retorna score 0 e índices vazios para query vazia', () => {
        expect(fuzzyMatch('', 'migrate:fresh')).toEqual({
            score: 0,
            indices: []
        });
        expect(fuzzyScore('', 'migrate:fresh')).toBe(0);
    });

    it('prioriza match exato sobre prefixo e subsequência', () => {
        const exact = fuzzyScore('migrate', 'migrate')!;
        const prefix = fuzzyScore('migrate', 'migrate:fresh')!;
        const subseq = fuzzyScore('mig', 'make:migration')!;
        expect(exact).toBe(10_000);
        expect(exact).toBeGreaterThan(prefix);
        expect(prefix).toBeGreaterThan(subseq);
    });

    it('posiciona migrate antes de migrate:fresh ao buscar "mig" (CA-12)', () => {
        const commands: ArtisanCommand[] = [
            { name: 'migrate:fresh', description: 'Drop all tables' },
            { name: 'migrate', description: 'Run migrations' },
            { name: 'make:migration', description: 'Create migration' }
        ];
        const ranked = rankCommands('mig', commands);
        expect(ranked.map((c) => c.name)).toEqual([
            'migrate',
            'migrate:fresh',
            'make:migration'
        ]);
    });

    it('posiciona migrate:fresh no topo ao buscar "mfr" por fronteira de namespace (CA-12)', () => {
        const commands: ArtisanCommand[] = [
            { name: 'make:form-request', description: 'Create form request' },
            { name: 'migrate:fresh', description: 'Drop all tables' },
            { name: 'model:filter-rule', description: 'Filter rule' }
        ];
        const ranked = rankCommands('mfr', commands);
        expect(ranked[0]?.name).toBe('migrate:fresh');
    });

    it('considera aliases com penalidade e preserva ordem alfabética na query vazia', () => {
        const commands: ArtisanCommand[] = [
            { name: 'route:list', description: 'List routes' },
            { name: 'about', description: 'About app', aliases: ['info'] }
        ];
        expect(rankCommands('', commands).map((c) => c.name)).toEqual([
            'about',
            'route:list'
        ]);
        expect(rankCommands('info', commands).map((c) => c.name)).toEqual(['about']);
    });

    it('ordena opções Artisan por nome e atalho', () => {
        const options: ArtisanOption[] = [
            { name: '--database', shortcut: '-d' },
            { name: '--force', shortcut: '-f' },
            { name: '--seed' }
        ];
        expect(rankOptions('--for', options)[0]?.name).toBe('--force');
        expect(rankOptions('d', options)[0]?.name).toBe('--database');
    });
});
