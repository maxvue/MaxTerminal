import { describe, expect, it } from 'vitest';
import { fuzzyFilter, fuzzyScore } from '@/terminal/triggers/fuzzy';

describe('fuzzy', () => {
    it('calcula pontuação por subsequência priorizando prefixo exato e início de palavra (-, :, /, _)', () => {
        const exact = fuzzyScore('migrate:fresh', 'migrate');
        const subseq = fuzzyScore('make:migrate', 'migrate');
        expect(exact).not.toBeNull();
        expect(subseq).not.toBeNull();
        expect(exact!.score).toBeGreaterThan(subseq!.score);

        const boundary = fuzzyScore('route:list', 'rl');
        const middle = fuzzyScore('parallel', 'rl');
        expect(boundary!.score).toBeGreaterThan(middle!.score);
    });

    it('filtra e ordena mantendo ordem original quando query está em branco ou empatada', () => {
        const items = [
            { id: '1', label: 'cache:clear' },
            { id: '2', label: 'migrate:fresh' },
            { id: '3', label: 'migrate:status' },
            { id: '4', label: 'route:list' }
        ];

        expect(fuzzyFilter(items, '', 'label')).toEqual(items);

        const filtered = fuzzyFilter(items, 'mf', (i) => i.label);
        expect(filtered.map((i) => i.label)).toEqual(['migrate:fresh']);
    });
});
