import { describe, expect, it } from 'vitest';
import type { ArtisanCommand } from '@/types/bindings';

interface MaxCodeArtisanOption {
    name: string;
    description?: string;
}

interface MaxCodeArtisanCommand {
    name: string;
    description?: string;
    options?: MaxCodeArtisanOption[];
}

/**
 * Port verbatim de `lerCacheArtisan` de `MaxCode/src/utils/artisanDiscovery.ts` (CA-06).
 */
function lerCacheArtisan(rawJson: string): MaxCodeArtisanCommand[] | null {
    try {
        const parsed = JSON.parse(rawJson) as unknown;
        if (!Array.isArray(parsed)) {
            return null;
        }
        return parsed
            .filter(
                (item): item is Record<string, unknown> =>
                    Boolean(item) &&
                    typeof item === 'object' &&
                    typeof (item as { name?: unknown }).name === 'string' &&
                    ((item as { name: string }).name.trim().length > 0)
            )
            .map((item) => {
                const optionsRaw = Array.isArray(item.options) ? item.options : [];
                const options: MaxCodeArtisanOption[] = optionsRaw
                    .filter(
                        (opt): opt is Record<string, unknown> =>
                            Boolean(opt) &&
                            typeof opt === 'object' &&
                            typeof (opt as { name?: unknown }).name === 'string'
                    )
                    .map((opt) => ({
                        name: String(opt.name),
                        ...(typeof opt.description === 'string'
                            ? { description: opt.description }
                            : {})
                    }));
                return {
                    name: String(item.name),
                    ...(typeof item.description === 'string'
                        ? { description: item.description }
                        : {}),
                    ...(options.length > 0 ? { options } : {})
                };
            });
    } catch {
        return null;
    }
}

const LEGACY_MAXCODE_CACHE_JSON = JSON.stringify([
    {
        name: 'migrate',
        description: 'Run the database migrations',
        options: [
            { name: '--database', description: 'The database connection to use' },
            { name: '--force', description: 'Force the operation to run when in production' }
        ]
    },
    {
        name: 'optimize:clear',
        description: 'Remove the cached bootstrap files'
    }
]);

describe('Compatibilidade bidirecional de .max-code/commands/artisan.json com MaxCode (CA-06)', () => {
    it('lê com sucesso o cache legado do MaxCode e o formato estendido gerado pelo MaxTerminal', () => {
        const legacyParsed = lerCacheArtisan(LEGACY_MAXCODE_CACHE_JSON);
        expect(legacyParsed).not.toBeNull();
        expect(legacyParsed?.map((c) => c.name)).toEqual(['migrate', 'optimize:clear']);

        // Catálogo estendido produzido pelo MaxTerminal (com arguments, aliases, acceptValue, etc.)
        const extended: ArtisanCommand[] = [
            {
                name: 'migrate:fresh',
                description: 'Drop all tables and re-run all migrations',
                options: [
                    {
                        name: '--database',
                        description: 'The database connection to use',
                        shortcut: '-d',
                        acceptValue: true,
                        isValueRequired: false,
                        isMultiple: false,
                        isGlobal: false
                    },
                    {
                        name: '--seed',
                        description: 'Indicates if the seed task should be re-run',
                        acceptValue: false
                    }
                ],
                arguments: [
                    {
                        name: 'connection',
                        description: 'Optional connection',
                        required: false,
                        isArray: false
                    }
                ],
                aliases: ['mf']
            }
        ];

        const serialized = `${JSON.stringify(extended, null, 2)}\n`;
        const readByMaxCode = lerCacheArtisan(serialized);
        expect(readByMaxCode).toEqual([
            {
                name: 'migrate:fresh',
                description: 'Drop all tables and re-run all migrations',
                options: [
                    {
                        name: '--database',
                        description: 'The database connection to use'
                    },
                    {
                        name: '--seed',
                        description: 'Indicates if the seed task should be re-run'
                    }
                ]
            }
        ]);
    });
});
