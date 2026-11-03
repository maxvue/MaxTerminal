import { describe, expect, it } from 'vitest';
import { MtTerminalError, TriggerConflictError, type TriggerDefinition } from '@/types/terminal';
import { createTriggerRegistry } from '@/terminal/triggers/triggerRegistry';

function makeDummyTrigger(id: string, prefix: string): TriggerDefinition {
    return {
        id,
        prefix,
        label: id,
        isAvailable: () => true,
        provide: () => ({ items: [] }),
        onSelect: () => ({ kind: 'close' })
    };
}

describe('TriggerRegistry', () => {
    it('garante precedência de >> sobre > independente da ordem de registro e exige posição 0', () => {
        const registry = createTriggerRegistry();
        registry.register(makeDummyTrigger('artisan', '>'));
        registry.register(makeDummyTrigger('groups', '>>'));

        expect(registry.match('>migrate')?.def.id).toBe('artisan');
        expect(registry.match('>migrate')?.query).toBe('migrate');

        expect(registry.match('>>dev')?.def.id).toBe('groups');
        expect(registry.match('>>dev')?.query).toBe('dev');

        // Espaço antes do prefixo desativa o match (exige posição 0).
        expect(registry.match(' >migrate')).toBeNull();
        expect(registry.match('echo > out.txt')).toBeNull();
    });

    it('suporta escape literal (\\> e \\?), detecta conflitos (TriggerConflictError) e valida prefixo', () => {
        const registry = createTriggerRegistry();
        const sub = registry.register(makeDummyTrigger('artisan', '>'));
        registry.register(makeDummyTrigger('ai-quick', '?'));

        expect(registry.isEscapedTriggerLiteral('\\>arquivo.txt')).toBe(true);
        expect(registry.match('\\>arquivo.txt')).toBeNull();
        expect(registry.match('\\?teste')).toBeNull();

        // Conflito de prefixo ou id lança TriggerConflictError.
        expect(() => registry.register(makeDummyTrigger('other', '>'))).toThrowError(
            TriggerConflictError
        );
        expect(() => registry.register(makeDummyTrigger('artisan', '/'))).toThrowError(
            TriggerConflictError
        );

        // Prefixo inválido com espaço ou vazio lança MtTerminalError.
        expect(() => registry.register(makeDummyTrigger('bad', ''))).toThrowError(MtTerminalError);
        expect(() => registry.register(makeDummyTrigger('bad2', '> a'))).toThrowError(
            MtTerminalError
        );

        sub.dispose();
        expect(registry.match('>migrate')).toBeNull();
    });
});
