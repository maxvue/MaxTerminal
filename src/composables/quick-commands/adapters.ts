import { computed, ref } from 'vue';
import { Toast } from '@maxvue/max-components-ui';
import { useAiGenerate } from '@/composables/ai/useAiGenerate';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
import { useProjectsStore } from '@/stores/projects';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { triggerRegistry } from '@/terminal/triggers/triggerRegistry';
import type {
    ArtisanCommandForm,
    ArtisanParam,
    MtError,
    PaneId,
    ProjectId,
    ScheduleFrequency,
    ScheduleOption,
    ScheduleSpec
} from '@/types/bindings';
import type {
    CompletionResult,
    Disposable,
    TriggerDefinition
} from '@/types/terminal';

export interface AiGenerateRunOptions {
    signal?: AbortSignal | undefined;
    paneId?: PaneId | null | undefined;
    projectId?: ProjectId | null | undefined;
    system?: string | null | undefined;
}

export const TRIGGER_PASSTHROUGH: CompletionResult = Object.freeze({
    items: [],
    passthrough: true
});

export interface AiStructuredGenerator<T> {
    loading: Readonly<{ value: boolean }>;
    error: Readonly<{ value: MtError | null }>;
    /** `null` = sem resultado utilizável ou cancelado (contrato da Fase 12). */
    run: (prompt: string, options?: AiGenerateRunOptions) => Promise<T | null>;
    cancel: () => Promise<void>;
}

export interface QuickCommandsAdapters {
    registerTrigger: (def: TriggerDefinition) => Disposable;
    projectStackHint: (projectId: ProjectId | null) => string | null;
    openMaxCodePlan: (prompt: string, projectPath: string) => Promise<void>;
    createAiGenerator: <T>(
        schemaId: string,
        schema: Record<string, unknown>,
        decode: (value: unknown) => T
    ) => AiStructuredGenerator<T>;
    notifyToast: (opts: {
        title: string;
        description?: string;
        severity: 'success' | 'info' | 'warning' | 'warn' | 'error';
    }) => void;
    focusPaneInput: (paneId: PaneId | null) => void;
}

/** Schema registrado na Fase 12 para a Geração Assistida do modal (D-05). */
export const ARTISAN_COMMAND_SCHEMA_ID = 'artisan.command.v1';

const ALLOWED_FORM_KEYS = new Set([
    'className',
    'signatureName',
    'description',
    'params',
    'isolatable',
    'promptsForMissingInput',
    'schedule',
    'objective'
]);

const ALLOWED_PARAM_KEYS = new Set([
    'kind',
    'name',
    'description',
    'optional',
    'isArray',
    'default',
    'shortcut',
    'acceptsValue'
]);

const ALLOWED_SCHEDULE_KEYS = new Set([
    'frequency',
    'arguments',
    'options',
    'withoutOverlapping',
    'onOneServer',
    'runInBackground'
]);

const ALLOWED_SCHEDULE_OPT_KEYS = new Set(['name', 'values', 'enabled']);

function isPlainObject(v: unknown): v is Record<string, unknown> {
    return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function assertKnownKeys(obj: Record<string, unknown>, allowed: Set<string>, label: string): void {
    for (const key of Object.keys(obj)) {
        if (!allowed.has(key)) {
            throw new Error(`Campo desconhecido "${key}" em ${label}.`);
        }
    }
}

function decodeScheduleFrequency(raw: unknown): ScheduleFrequency {
    if (!isPlainObject(raw) || typeof raw.kind !== 'string') {
        throw new Error('Frequência de agendamento inválida.');
    }
    switch (raw.kind) {
        case 'everyMinute':
        case 'everyFiveMinutes':
        case 'everyFifteenMinutes':
        case 'everyThirtyMinutes':
        case 'hourly':
        case 'daily':
        case 'weekly':
        case 'monthly':
            return { kind: raw.kind };
        case 'hourlyAt':
            if (typeof raw.minute !== 'number') {
                throw new Error('Minuto inválido em hourlyAt.');
            }
            return { kind: 'hourlyAt', minute: raw.minute };
        case 'dailyAt':
            if (typeof raw.time !== 'string') {
                throw new Error('Horário inválido em dailyAt.');
            }
            return { kind: 'dailyAt', time: raw.time };
        case 'twiceDaily':
            if (typeof raw.first !== 'number' || typeof raw.second !== 'number') {
                throw new Error('Horas inválidas em twiceDaily.');
            }
            return { kind: 'twiceDaily', first: raw.first, second: raw.second };
        case 'weeklyOn':
            if (typeof raw.day !== 'number' || typeof raw.time !== 'string') {
                throw new Error('Parâmetros inválidos em weeklyOn.');
            }
            return { kind: 'weeklyOn', day: raw.day, time: raw.time };
        case 'monthlyOn':
            if (typeof raw.day !== 'number' || typeof raw.time !== 'string') {
                throw new Error('Parâmetros inválidos em monthlyOn.');
            }
            return { kind: 'monthlyOn', day: raw.day, time: raw.time };
        case 'cron':
            if (typeof raw.expression !== 'string') {
                throw new Error('Expressão cron inválida.');
            }
            return { kind: 'cron', expression: raw.expression };
        default:
            throw new Error(`Frequência de agendamento desconhecida: "${raw.kind}".`);
    }
}

function decodeScheduleOption(raw: unknown): ScheduleOption {
    if (!isPlainObject(raw)) {
        throw new Error('Opção de agendamento inválida.');
    }
    assertKnownKeys(raw, ALLOWED_SCHEDULE_OPT_KEYS, 'ScheduleOption');
    if (typeof raw.name !== 'string') {
        throw new Error('Nome da opção de agendamento inválido.');
    }
    const values = Array.isArray(raw.values) ? raw.values.map(String) : [];
    const enabled = typeof raw.enabled === 'boolean' ? raw.enabled : true;
    return {
        name: raw.name,
        values,
        enabled
    };
}

function decodeScheduleSpec(raw: unknown): ScheduleSpec {
    if (!isPlainObject(raw)) {
        throw new Error('Especificação de agendamento inválida.');
    }
    assertKnownKeys(raw, ALLOWED_SCHEDULE_KEYS, 'ScheduleSpec');
    const frequency = decodeScheduleFrequency(raw.frequency);
    const argumentsList = Array.isArray(raw.arguments) ? raw.arguments.map(String) : [];
    const options = Array.isArray(raw.options) ? raw.options.map(decodeScheduleOption) : [];
    return {
        frequency,
        arguments: argumentsList,
        options,
        withoutOverlapping: Boolean(raw.withoutOverlapping),
        onOneServer: Boolean(raw.onOneServer),
        runInBackground: Boolean(raw.runInBackground)
    };
}

function decodeArtisanParam(raw: unknown): ArtisanParam {
    if (!isPlainObject(raw)) {
        throw new Error('Parâmetro Artisan inválido.');
    }
    assertKnownKeys(raw, ALLOWED_PARAM_KEYS, 'ArtisanParam');
    if (raw.kind !== 'argument' && raw.kind !== 'option') {
        throw new Error('Tipo de parâmetro Artisan inválido.');
    }
    if (typeof raw.name !== 'string') {
        throw new Error('Nome de parâmetro Artisan inválido.');
    }
    return {
        kind: raw.kind,
        name: raw.name,
        description: typeof raw.description === 'string' ? raw.description : '',
        optional: Boolean(raw.optional),
        isArray: Boolean(raw.isArray),
        default: typeof raw.default === 'string' ? raw.default : null,
        shortcut: typeof raw.shortcut === 'string' ? raw.shortcut : null,
        acceptsValue: Boolean(raw.acceptsValue)
    };
}

/**
 * Decodifica e valida estritamente um objeto `ArtisanCommandForm` vindo da IA,
 * rejeitando campos desconhecidos e tipos inválidos.
 */
export function decodeArtisanCommandForm(value: unknown): ArtisanCommandForm {
    if (!isPlainObject(value)) {
        throw new Error('Formato de resposta inválido para ArtisanCommandForm.');
    }
    assertKnownKeys(value, ALLOWED_FORM_KEYS, 'ArtisanCommandForm');
    if (
        typeof value.className !== 'string' ||
        typeof value.signatureName !== 'string' ||
        typeof value.description !== 'string'
    ) {
        throw new Error('Campos obrigatórios ausentes ou inválidos em ArtisanCommandForm.');
    }
    const params = Array.isArray(value.params) ? value.params.map(decodeArtisanParam) : [];
    const schedule =
        value.schedule !== null && value.schedule !== undefined
            ? decodeScheduleSpec(value.schedule)
            : null;

    return {
        className: value.className,
        signatureName: value.signatureName,
        description: value.description,
        params,
        isolatable: Boolean(value.isolatable),
        promptsForMissingInput: Boolean(value.promptsForMissingInput),
        schedule,
        objective: typeof value.objective === 'string' ? value.objective : ''
    };
}

function createDefaultAdapters(): QuickCommandsAdapters {
    return {
        registerTrigger: (def: TriggerDefinition): Disposable => triggerRegistry.register(def),
        projectStackHint: (projectId: ProjectId | null): string | null => {
            if (!projectId) {
                return null;
            }
            const projectsStore = useProjectsStore();
            return projectsStore.projectById(projectId)?.stack ?? null;
        },
        openMaxCodePlan: async (prompt: string, projectPath: string): Promise<void> => {
            const bridgeStore = useMaxCodeBridgeStore();
            await bridgeStore.openPlan(prompt, projectPath, 'artisan_create');
        },
        createAiGenerator: <T>(
            schemaId: string,
            schema: Record<string, unknown>,
            decode: (value: unknown) => T
        ): AiStructuredGenerator<T> => {
            const gen = useAiGenerate<T>({ schemaId, schema, decode });
            const mtErr = computed<MtError | null>(() =>
                gen.error.value
                    ? {
                        code: 'ai.structured_failed',
                        message: gen.error.value,
                        details: null
                    }
                    : null
            );
            const cancelled = ref(false);

            return {
                loading: gen.loading,
                error: mtErr,
                run: async (prompt: string, options?: AiGenerateRunOptions): Promise<T | null> => {
                    cancelled.value = false;
                    if (options?.signal?.aborted) {
                        return null;
                    }
                    const onAbort = (): void => {
                        cancelled.value = true;
                        void gen.cancel();
                    };
                    options?.signal?.addEventListener('abort', onAbort, { once: true });
                    try {
                        const result = await gen.generate(prompt, {
                            paneId: options?.paneId ?? null,
                            projectId: options?.projectId ?? null,
                            system: options?.system ?? null
                        });
                        if (cancelled.value || options?.signal?.aborted) {
                            return null;
                        }
                        return result ?? null;
                    } catch {
                        return null;
                    } finally {
                        options?.signal?.removeEventListener('abort', onAbort);
                    }
                },
                cancel: async (): Promise<void> => {
                    cancelled.value = true;
                    await gen.cancel();
                }
            };
        },
        notifyToast: (opts): void => {
            try {
                Toast.show({
                    title: opts.title,
                    ...(opts.description !== undefined ? { message: opts.description } : {}),
                    severity: opts.severity === 'warn' ? 'warning' : opts.severity
                });
            } catch {
                // Ambiente de teste sem container de Toast montado
            }
        },
        focusPaneInput: (paneId: PaneId | null): void => {
            if (!paneId) {
                return;
            }
            const inst = terminalInstanceRegistry.get(paneId);
            inst?.focus();
        }
    };
}

let testOverrides: Partial<QuickCommandsAdapters> | null = null;

export function getQuickCommandsAdapters(): QuickCommandsAdapters {
    const base = createDefaultAdapters();
    if (!testOverrides) {
        return base;
    }
    return {
        ...base,
        ...testOverrides
    };
}

export function setQuickCommandsAdaptersForTest(
    overrides: Partial<QuickCommandsAdapters> | null
): void {
    testOverrides = overrides;
}
