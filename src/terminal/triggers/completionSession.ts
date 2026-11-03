import type { PaneId } from '@/types/bindings';
import type {
    CompletionItem,
    CompletionResult,
    CompletionSection,
    Disposable,
    TerminalContext,
    TriggerDefinition,
    TriggerQuery,
    TriggerSelectOutcome
} from '@/types/terminal';
import { triggerRegistry, type TriggerRegistry } from './triggerRegistry';

/** Estados da máquina de autocompletar. */
export type CompletionSessionStatus = 'idle' | 'querying' | 'showing' | 'error';

/** Fotografia imutável do estado da sessão de autocompletar. */
export interface CompletionSessionSnapshot {
    status: CompletionSessionStatus;
    open: boolean;
    loading: boolean;
    trigger: TriggerDefinition | null;
    stage: number;
    stageLabel: string | undefined;
    items: CompletionItem[];
    sections: CompletionSection[];
    activeIndex: number;
    error: string | null;
    emptyMessage: string;
    selecting: boolean;
}

/** Opções de construção da `CompletionSession`. */
export interface CompletionSessionOptions {
    paneId: PaneId;
    registry?: TriggerRegistry | undefined;
    getContext: () => TerminalContext;
    providerTimeoutMs?: number | undefined;
    onStateChange?: ((snapshot: CompletionSessionSnapshot) => void) | undefined;
}

function orderCompletionItems(
    items: readonly CompletionItem[],
    sections?: readonly CompletionSection[]
): { orderedItems: CompletionItem[]; orderedSections: CompletionSection[] } {
    const regular = items.filter((item) => item.pinned !== 'bottom');
    const pinned = items.filter((item) => item.pinned === 'bottom');
    const orderedSections = sections ? [...sections].sort((a, b) => a.order - b.order) : [];

    if (orderedSections.length === 0) {
        return {
            orderedItems: [...regular, ...pinned],
            orderedSections: []
        };
    }

    const sectionBuckets = new Map<string, CompletionItem[]>();
    for (const sec of orderedSections) {
        sectionBuckets.set(sec.id, []);
    }
    const unsectioned: CompletionItem[] = [];

    for (const item of regular) {
        if (item.section && sectionBuckets.has(item.section)) {
            sectionBuckets.get(item.section)!.push(item);
        } else {
            unsectioned.push(item);
        }
    }

    const groupedRegular: CompletionItem[] = [];
    for (const sec of orderedSections) {
        const bucket = sectionBuckets.get(sec.id) ?? [];
        groupedRegular.push(...bucket);
    }
    groupedRegular.push(...unsectioned);

    return {
        orderedItems: [...groupedRegular, ...pinned],
        orderedSections
    };
}

function findFirstSelectableIndex(items: readonly CompletionItem[]): number {
    for (let i = 0; i < items.length; i++) {
        if (!items[i]!.disabled) {
            return i;
        }
    }
    return -1;
}

function findLastSelectableIndex(items: readonly CompletionItem[]): number {
    for (let i = items.length - 1; i >= 0; i--) {
        if (!items[i]!.disabled) {
            return i;
        }
    }
    return -1;
}

/**
 * Máquina de estados determinística de autocompletar para um painel.
 * Suporta debounce, AbortSignal, timeout de 5s, etapas multi-stage, itens `pinned: 'bottom'`,
 * `passthrough`, `refreshAfter` e supressão pós-`Escape`.
 */
export class CompletionSession implements Disposable {
    public static readonly DEFAULT_DEBOUNCE_MS = 80;
    public static readonly DEFAULT_TIMEOUT_MS = 5000;

    private readonly registry: TriggerRegistry;
    private readonly getContext: () => TerminalContext;
    private readonly providerTimeoutMs: number;
    private readonly onStateChange: ((snapshot: CompletionSessionSnapshot) => void) | undefined;
    private readonly registrySub: Disposable;

    private status: CompletionSessionStatus = 'idle';
    private activeTrigger: TriggerDefinition | null = null;
    private currentInput = '';
    private currentCursor = 0;
    private currentQuery = '';
    private stage = 0;
    private stageState: unknown = undefined;
    private stageLabel: string | undefined = undefined;
    private readonly stageHistory: Array<{ stageState: unknown; stageLabel: string | undefined }> = [];

    private items: CompletionItem[] = [];
    private sections: CompletionSection[] = [];
    private activeIndex = -1;
    private error: string | null = null;
    private emptyMessage = 'Nenhum resultado encontrado';
    private selecting = false;
    private suppressedPrefix: string | null = null;

    private generation = 0;
    private debounceTimer: ReturnType<typeof setTimeout> | null = null;
    private abortController: AbortController | null = null;
    private disposed = false;

    public constructor(opts: CompletionSessionOptions) {
        this.registry = opts.registry ?? triggerRegistry;
        this.getContext = opts.getContext;
        this.providerTimeoutMs = opts.providerTimeoutMs ?? CompletionSession.DEFAULT_TIMEOUT_MS;
        this.onStateChange = opts.onStateChange;

        this.registrySub = this.registry.onChange(() => {
            if (!this.disposed && this.currentInput.length > 0) {
                this.updateInput(this.currentInput, this.currentCursor);
            }
        });
    }

    /** Atualiza o texto e cursor do input, disparando ou encerrando a sessão de autocompletar. */
    public updateInput(input: string, cursor: number): void {
        if (this.disposed) {
            return;
        }

        this.currentInput = input;
        this.currentCursor = cursor;

        if (this.suppressedPrefix !== null) {
            if (input.length === 0 || !input.startsWith(this.suppressedPrefix)) {
                this.suppressedPrefix = null;
            } else {
                this.transitionToIdle(false);
                return;
            }
        }

        const matched = this.registry.match(input);
        if (!matched) {
            this.transitionToIdle(true);
            return;
        }

        // Se o gatilho mudou, reseta a pilha de etapas.
        if (this.activeTrigger?.id !== matched.def.id) {
            this.activeTrigger = matched.def;
            this.stage = 0;
            this.stageState = undefined;
            this.stageLabel = undefined;
            this.stageHistory.length = 0;
        }

        this.currentQuery = matched.query;
        this.scheduleQuery(matched.def, false);
    }

    /** Navega entre os itens habilitados com wrap-around. */
    public moveSelection(delta: number): void {
        if (this.disposed || this.items.length === 0 || delta === 0) {
            return;
        }
        const selectableIndices: number[] = [];
        for (let i = 0; i < this.items.length; i++) {
            if (!this.items[i]!.disabled) {
                selectableIndices.push(i);
            }
        }
        if (selectableIndices.length === 0) {
            this.activeIndex = -1;
            this.emitState();
            return;
        }

        const currentPos = selectableIndices.indexOf(this.activeIndex);
        let nextPos: number;
        if (currentPos === -1) {
            nextPos = delta > 0 ? 0 : selectableIndices.length - 1;
        } else {
            const len = selectableIndices.length;
            nextPos = (((currentPos + delta) % len) + len) % len;
        }

        this.activeIndex = selectableIndices[nextPos]!;
        this.emitState();
    }

    public setSelectionIndex(index: number): void {
        if (this.disposed || index < 0 || index >= this.items.length) {
            return;
        }
        if (this.items[index]?.disabled) {
            return;
        }
        this.activeIndex = index;
        this.emitState();
    }

    public selectFirst(): void {
        if (this.disposed) {
            return;
        }
        this.activeIndex = findFirstSelectableIndex(this.items);
        this.emitState();
    }

    public selectLast(): void {
        if (this.disposed) {
            return;
        }
        this.activeIndex = findLastSelectableIndex(this.items);
        this.emitState();
    }

    /**
     * Desempilha a etapa atual (`pop-stage`) quando `stage > 0`, retornando à etapa anterior.
     */
    public popStage(): boolean {
        if (this.disposed || !this.activeTrigger || this.stage <= 0) {
            return false;
        }
        const prev = this.stageHistory.pop();
        this.stage = Math.max(0, this.stage - 1);
        this.stageState = prev?.stageState;
        this.stageLabel = prev?.stageLabel;
        this.scheduleQuery(this.activeTrigger, true);
        return true;
    }

    /**
     * Se `stage > 0` e a query da etapa atual estiver vazia, desempilha a etapa anterior (`pop-stage`).
     */
    public handleBackspaceOnEmptyQuery(): boolean {
        if (this.currentQuery.length > 0) {
            return false;
        }
        return this.popStage();
    }

    /** Confirma o item atualmente selecionado (`Enter`, `Tab` ou `→`), protegendo contra chamadas concorrentes. */
    public async confirmSelection(
        mode: 'primary' | 'secondary' = 'primary'
    ): Promise<TriggerSelectOutcome | null> {
        if (this.disposed || this.selecting || !this.activeTrigger) {
            return null;
        }
        const item = this.items[this.activeIndex];
        if (!item || item.disabled) {
            return null;
        }

        const trigger = this.activeTrigger;
        const q = this.buildTriggerQuery(trigger);
        const ctx = this.getContext();
        const genAtStart = this.generation;

        this.selecting = true;
        this.emitState();

        try {
            const handler =
                mode === 'secondary' && trigger.onSecondarySelect
                    ? trigger.onSecondarySelect.bind(trigger)
                    : trigger.onSelect.bind(trigger);
            const outcome = await handler(item, q, ctx);
            if (this.disposed || genAtStart !== this.generation) {
                return null;
            }
            this.selecting = false;
            this.applyOutcomeInternal(trigger, outcome);
            return outcome;
        } catch (err: unknown) {
            if (!this.disposed && genAtStart === this.generation) {
                this.selecting = false;
                this.status = 'error';
                this.error =
                    err instanceof Error ? err.message : 'Falha ao executar ação do gatilho.';
                this.emitState();
            }
            return null;
        }
    }

    /** Invoca `onSubmitWithoutSelection` quando o usuário pressiona `Enter` sem item selecionado. */
    public async submitWithoutSelection(): Promise<TriggerSelectOutcome | null> {
        const trigger = this.activeTrigger;
        const onSubmit = trigger?.onSubmitWithoutSelection;
        if (this.disposed || this.selecting || !trigger || !onSubmit) {
            return null;
        }

        const q = this.buildTriggerQuery(trigger);
        const ctx = this.getContext();
        const genAtStart = this.generation;

        this.selecting = true;
        this.emitState();

        try {
            const outcome = await onSubmit(q, ctx);
            if (this.disposed || genAtStart !== this.generation) {
                return null;
            }
            this.selecting = false;
            this.applyOutcomeInternal(trigger, outcome);
            return outcome;
        } catch {
            if (!this.disposed && genAtStart === this.generation) {
                this.selecting = false;
                this.emitState();
            }
            return null;
        }
    }

    /** Suprime o gatilho atual após `Escape` até que a linha fique vazia ou o prefixo mude. */
    public suppressCurrentTrigger(): void {
        if (this.disposed) {
            return;
        }
        if (this.activeTrigger) {
            this.suppressedPrefix = this.activeTrigger.prefix;
        }
        this.transitionToIdle(false);
    }

    /** Cancela imediatamente qualquer consulta/popup e reseta para `idle`. */
    public cancel(): void {
        if (this.disposed) {
            return;
        }
        this.suppressedPrefix = null;
        this.transitionToIdle(true);
    }

    public getSnapshot(): CompletionSessionSnapshot {
        const isOpen =
            this.status === 'querying' || this.status === 'showing' || this.status === 'error';
        return {
            status: this.status,
            open: isOpen,
            loading: this.status === 'querying',
            trigger: this.activeTrigger,
            stage: this.stage,
            stageLabel: this.stageLabel,
            items: [...this.items],
            sections: [...this.sections],
            activeIndex: this.activeIndex,
            error: this.error,
            emptyMessage: this.emptyMessage,
            selecting: this.selecting
        };
    }

    public dispose(): void {
        if (this.disposed) {
            return;
        }
        this.disposed = true;
        this.registrySub.dispose();
        this.invalidateInFlight();
    }

    private applyOutcomeInternal(
        trigger: TriggerDefinition,
        outcome: TriggerSelectOutcome
    ): void {
        if (outcome.kind === 'push-stage') {
            this.stageHistory.push({
                stageState: this.stageState,
                stageLabel: this.stageLabel
            });
            this.stage += 1;
            this.stageState = outcome.stageState;
            this.currentQuery = outcome.initialQuery ?? '';
            this.currentInput = trigger.prefix + this.currentQuery;
            this.currentCursor = this.currentInput.length;
            this.scheduleQuery(trigger, true);
            return;
        }

        if (outcome.kind === 'replace-input') {
            this.currentInput = outcome.text;
            this.currentCursor = outcome.cursor ?? outcome.text.length;
            if (outcome.keepOpen) {
                this.updateInput(this.currentInput, this.currentCursor);
            } else {
                this.transitionToIdle(true);
            }
            return;
        }

        this.transitionToIdle(true);
    }

    private scheduleQuery(trigger: TriggerDefinition, immediate: boolean): void {
        this.invalidateInFlight();
        const gen = this.generation;
        const debounceMs = immediate
            ? 0
            : (trigger.debounceMs ?? CompletionSession.DEFAULT_DEBOUNCE_MS);

        this.status = 'querying';
        this.error = null;
        this.emitState();

        const run = (): void => {
            this.debounceTimer = null;
            void this.executeQuery(trigger, gen);
        };

        if (debounceMs <= 0) {
            run();
        } else {
            this.debounceTimer = setTimeout(run, debounceMs);
        }
    }

    private async executeQuery(trigger: TriggerDefinition, gen: number): Promise<void> {
        if (this.disposed || gen !== this.generation) {
            return;
        }

        const ctx = this.getContext();
        try {
            const available = await trigger.isAvailable(ctx);
            if (this.disposed || gen !== this.generation) {
                return;
            }
            if (!available) {
                // Proibição de recaimento: se >> está indisponível, volta a idle sem consultar >.
                this.transitionToIdle(false);
                return;
            }
        } catch {
            if (!this.disposed && gen === this.generation) {
                this.transitionToIdle(false);
            }
            return;
        }

        const controller = new AbortController();
        this.abortController = controller;
        const queryObj = this.buildTriggerQuery(trigger);

        let timeoutId: ReturnType<typeof setTimeout> | null = null;
        const timeoutPromise = new Promise<never>((_resolve, reject) => {
            timeoutId = setTimeout(() => {
                controller.abort();
                reject(new Error('Tempo esgotado ao carregar sugestões'));
            }, this.providerTimeoutMs);
        });

        try {
            const result = await Promise.race([
                Promise.resolve(trigger.provide(queryObj, ctx, controller.signal)),
                timeoutPromise
            ]);

            if (timeoutId !== null) {
                clearTimeout(timeoutId);
            }
            if (this.disposed || gen !== this.generation) {
                return;
            }

            this.applyCompletionResult(trigger, result, gen);
        } catch (err: unknown) {
            if (timeoutId !== null) {
                clearTimeout(timeoutId);
            }
            if (this.disposed || gen !== this.generation) {
                return;
            }
            this.status = 'error';
            this.items = [];
            this.sections = [];
            this.activeIndex = -1;
            this.error =
                err instanceof Error ? err.message : 'Falha ao carregar sugestões do gatilho.';
            this.emitState();
        }
    }

    private applyCompletionResult(
        trigger: TriggerDefinition,
        result: CompletionResult,
        gen: number
    ): void {
        if (result.passthrough === true) {
            this.transitionToIdle(false);
            return;
        }

        const { orderedItems, orderedSections } = orderCompletionItems(
            result.items,
            result.sections
        );

        this.status = 'showing';
        this.items = orderedItems;
        this.sections = orderedSections;
        this.stageLabel = result.stageLabel;
        this.emptyMessage = result.emptyMessage ?? 'Nenhum resultado encontrado';
        this.error = null;
        this.activeIndex = findFirstSelectableIndex(orderedItems);
        this.emitState();

        if (result.refreshAfter) {
            const inputAtRefresh = this.currentInput;
            void result.refreshAfter
                .then(async () => {
                    if (
                        this.disposed ||
                        gen !== this.generation ||
                        this.currentInput !== inputAtRefresh
                    ) {
                        return;
                    }
                    const refreshController = new AbortController();
                    this.abortController = refreshController;
                    const refreshed = await trigger.provide(
                        this.buildTriggerQuery(trigger),
                        this.getContext(),
                        refreshController.signal
                    );
                    if (this.disposed || gen !== this.generation) {
                        return;
                    }
                    const nextOrdered = orderCompletionItems(
                        refreshed.items,
                        refreshed.sections
                    );
                    this.items = nextOrdered.orderedItems;
                    this.sections = nextOrdered.orderedSections;
                    if (refreshed.stageLabel !== undefined) {
                        this.stageLabel = refreshed.stageLabel;
                    }
                    this.activeIndex = findFirstSelectableIndex(this.items);
                    this.emitState();
                })
                .catch(() => {
                    // Erros em refreshAfter silencioso preservam a lista já exibida.
                });
        }
    }

    private buildTriggerQuery(trigger: TriggerDefinition): TriggerQuery {
        return {
            prefix: trigger.prefix,
            query: this.currentQuery,
            input: this.currentInput,
            cursor: this.currentCursor,
            stage: this.stage,
            stageState: this.stageState
        };
    }

    private transitionToIdle(clearTriggerAndStages: boolean): void {
        this.invalidateInFlight();
        this.status = 'idle';
        this.items = [];
        this.sections = [];
        this.activeIndex = -1;
        this.error = null;
        this.selecting = false;
        if (clearTriggerAndStages) {
            this.activeTrigger = null;
            this.stage = 0;
            this.stageState = undefined;
            this.stageLabel = undefined;
            this.stageHistory.length = 0;
        }
        this.emitState();
    }

    private invalidateInFlight(): void {
        this.generation += 1;
        if (this.debounceTimer !== null) {
            clearTimeout(this.debounceTimer);
            this.debounceTimer = null;
        }
        if (this.abortController) {
            this.abortController.abort();
            this.abortController = null;
        }
    }

    private emitState(): void {
        this.onStateChange?.(this.getSnapshot());
    }
}
