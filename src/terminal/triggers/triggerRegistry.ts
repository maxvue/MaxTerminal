import {
    MtTerminalError,
    TriggerConflictError,
    type Disposable,
    type TriggerDefinition
} from '@/types/terminal';

/** Resultado léxico de correspondência de um gatilho na posição 0 da linha. */
export interface TriggerMatch {
    def: TriggerDefinition;
    query: string;
}

const INVALID_PREFIX_RE = /\s|[\x00-\x1f\x7f]/;

/**
 * Registro extensível de gatilhos de entrada rica (`>`, `>>`, `@`, `?`, `/`).
 * Garante precedência estrita pelo maior prefixo na posição 0 e suporte a escape literal (`\>`).
 */
export class TriggerRegistry {
    public static readonly MAX_PREFIX_LENGTH = 32;

    private readonly definitions: TriggerDefinition[] = [];
    private readonly changeListeners = new Set<() => void>();

    /** Registra uma definição de gatilho validando prefixo e unicidade. */
    public register(def: TriggerDefinition): Disposable {
        if (
            typeof def.prefix !== 'string' ||
            def.prefix.length < 1 ||
            def.prefix.length > TriggerRegistry.MAX_PREFIX_LENGTH ||
            INVALID_PREFIX_RE.test(def.prefix)
        ) {
            throw new MtTerminalError(
                'trigger.invalid_prefix',
                `Prefixo de gatilho inválido: "${def.prefix}". Deve ter entre 1 e 32 caracteres sem espaços ou controles.`
            );
        }

        const conflict = this.definitions.find(
            (existing) => existing.prefix === def.prefix || existing.id === def.id
        );
        if (conflict) {
            throw new TriggerConflictError(
                conflict.prefix === def.prefix ? def.prefix : def.id
            );
        }

        this.definitions.push(def);
        this.definitions.sort((a, b) => b.prefix.length - a.prefix.length);
        this.emitChange();

        return {
            dispose: () => {
                const idx = this.definitions.indexOf(def);
                if (idx !== -1) {
                    this.definitions.splice(idx, 1);
                    this.emitChange();
                }
            }
        };
    }

    /** Lista todas as definições registradas ordenadas por comprimento de prefixo decrescente. */
    public list(): readonly TriggerDefinition[] {
        return [...this.definitions];
    }

    public getById(id: string): TriggerDefinition | undefined {
        return this.definitions.find((d) => d.id === id);
    }

    /**
     * Verifica se o texto começa com uma barra invertida escapando um prefixo registrado (ex.: `\>` ou `\?`).
     */
    public isEscapedTriggerLiteral(input: string): boolean {
        if (!input.startsWith('\\') || input.length < 2) {
            return false;
        }
        const afterSlash = input.slice(1);
        return this.definitions.some((def) => afterSlash.startsWith(def.prefix));
    }

    /**
     * Avalia se `input` começa (na posição 0 exata) com algum prefixo registrado.
     * Seleciona sempre o maior prefixo registrado (`>>` vence `>`) sem recaimento.
     */
    public match(input: string): TriggerMatch | null {
        if (input.length === 0 || this.isEscapedTriggerLiteral(input)) {
            return null;
        }

        for (const def of this.definitions) {
            if (input.startsWith(def.prefix)) {
                return {
                    def,
                    query: input.slice(def.prefix.length)
                };
            }
        }

        return null;
    }

    public onChange(cb: () => void): Disposable {
        this.changeListeners.add(cb);
        return {
            dispose: () => {
                this.changeListeners.delete(cb);
            }
        };
    }

    public clearForTests(): void {
        this.definitions.length = 0;
        this.emitChange();
    }

    private emitChange(): void {
        for (const cb of this.changeListeners) {
            cb();
        }
    }
}

/** Cria uma nova instância isolada de `TriggerRegistry` (útil em testes). */
export function createTriggerRegistry(): TriggerRegistry {
    return new TriggerRegistry();
}

/** Instância singleton do registro de gatilhos. */
export const triggerRegistry: TriggerRegistry = createTriggerRegistry();
