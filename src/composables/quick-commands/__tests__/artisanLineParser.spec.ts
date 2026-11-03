import { describe, expect, it } from 'vitest';
import { parseArtisanLine } from '@/composables/quick-commands/artisanLineParser';
import type { ArtisanCommand, ArtisanRoot } from '@/types/bindings';

const baseRoot: ArtisanRoot = {
    root: '/srv/app',
    projectId: null,
    isWorktree: false,
    invoker: 'php artisan',
    invokerTokens: ['php', 'artisan'],
    runner: 'local',
    layout: 'modern',
    laravelVersion: '11.31.0'
};

const migrateCmd: ArtisanCommand = {
    name: 'migrate',
    description: 'Run the database migrations',
    options: [
        {
            name: '--database',
            shortcut: '-d',
            acceptValue: true,
            isValueRequired: true
        },
        {
            name: '--force',
            shortcut: '-f',
            acceptValue: false,
            isValueRequired: false
        },
        {
            name: '--path',
            acceptValue: true,
            isValueRequired: false,
            isMultiple: true
        }
    ],
    arguments: []
};

const makeModelCmd: ArtisanCommand = {
    name: 'make:model',
    description: 'Create a new Eloquent model class',
    options: [{ name: '--migration', shortcut: '-m', acceptValue: false }],
    arguments: [
        {
            name: 'name',
            description: 'The name of the model',
            required: true,
            isArray: false
        }
    ]
};

const resolveCmd = (name: string): ArtisanCommand | undefined => {
    if (name === 'migrate') {
        return migrateCmd;
    }
    if (name === 'make:model') {
        return makeModelCmd;
    }
    return undefined;
};

describe('parseArtisanLine (Fase 14 — CA-14, CA-15)', () => {
    it('identifica modo option quando o token sob o cursor inicia com "-"', () => {
        const line = 'php artisan migrate --for';
        const parsed = parseArtisanLine(line, line.length, baseRoot, resolveCmd);
        expect(parsed).not.toBeNull();
        expect(parsed?.command).toBe('migrate');
        expect(parsed?.mode).toBe('option');
        expect(parsed?.current.text).toBe('--for');
    });

    it('registra opções já usadas (--database=mysql, --database pgsql e -f) sem contá-las como posicionais', () => {
        const line = 'php artisan make:model --database=mysql -m ';
        const parsed = parseArtisanLine(line, line.length, baseRoot, resolveCmd);
        expect(parsed?.usedOptions.has('--database')).toBe(true);
        expect(parsed?.usedOptions.has('--migration')).toBe(true);
        expect(parsed?.positionalsBefore).toBe(0);
        expect(parsed?.mode).toBe('argument-hint');
    });

    it('consome o próximo token como valor de opção quando isValueRequired === true', () => {
        const line = 'php artisan migrate --database mysql -';
        const parsed = parseArtisanLine(line, line.length, baseRoot, resolveCmd);
        expect(parsed?.usedOptions.has('--database')).toBe(true);
        expect(parsed?.positionalsBefore).toBe(0);
        expect(parsed?.mode).toBe('option');
    });

    it('respeita o terminador "--" isolado tratando tudo após ele como argumento posicional', () => {
        const line = 'php artisan make:model -- -';
        const parsed = parseArtisanLine(line, line.length, baseRoot, resolveCmd);
        expect(parsed?.mode).toBe('none');
    });

    it('retorna null para operadores shell fora de aspas (|, &&, ;, >) ou comandos não-Artisan', () => {
        expect(
            parseArtisanLine('php artisan migrate && echo ok', 28, baseRoot, resolveCmd)
        ).toBeNull();
        expect(
            parseArtisanLine('php -v', 6, baseRoot, resolveCmd)
        ).toBeNull();
    });

    it('suporta invocadores relativos com caminhos quotados e equivalentes canônicos (./artisan, sail artisan)', () => {
        const quotedRoot: ArtisanRoot = {
            ...baseRoot,
            invoker: "php '../Meu Projeto/artisan'",
            invokerTokens: ['php', '../Meu Projeto/artisan']
        };
        const line = "php '../Meu Projeto/artisan' migrate --";
        const parsed = parseArtisanLine(line, line.length, quotedRoot, resolveCmd);
        expect(parsed?.command).toBe('migrate');
        expect(parsed?.mode).toBe('option');

        const dotArtisan = './artisan make:model ';
        const parsedDot = parseArtisanLine(dotArtisan, dotArtisan.length, baseRoot, resolveCmd);
        expect(parsedDot?.command).toBe('make:model');
        expect(parsedDot?.mode).toBe('argument-hint');
    });
});
