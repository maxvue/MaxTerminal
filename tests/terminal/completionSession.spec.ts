import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TerminalContext, TriggerDefinition } from '@/types/terminal';
import { CompletionSession } from '@/terminal/triggers/completionSession';
import { createTriggerRegistry } from '@/terminal/triggers/triggerRegistry';

const dummyCtx: TerminalContext = {
    paneId: 'pane-1',
    ptyId: 'pty-1',
    projectId: null,
    cwd: '/tmp',
    extra: Object.freeze({})
};

describe('CompletionSession', () => {
    beforeEach(() => {
        vi.useFakeTimers();
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('suporta fluxo multi-stage (push-stage e pop-stage com Backspace), seções e itens pinned: bottom', async () => {
        const registry = createTriggerRegistry();
        const trigger: TriggerDefinition = {
            id: 'ai',
            prefix: '@',
            label: 'Selecionar Agente',
            debounceMs: 0,
            isAvailable: () => true,
            provide: (q) => {
                if (q.stage === 0) {
                    return {
                        stageLabel: 'Harness',
                        sections: [
                            { id: 'sec-b', label: 'Secundários', order: 2 },
                            { id: 'sec-a', label: 'Principais', order: 1 }
                        ],
                        items: [
                            { id: 'create', label: 'Configurar Agente', kind: 'action', pinned: 'bottom' },
                            { id: 'codex', label: 'Codex', section: 'sec-b', kind: 'value' },
                            { id: 'claude', label: 'Claude Code', section: 'sec-a', kind: 'value' }
                        ]
                    };
                }
                return {
                    stageLabel: 'Harness › Modelo',
                    items: [{ id: 'sonnet', label: 'Claude 3.7 Sonnet', kind: 'value' }]
                };
            },
            onSelect: (item, q) => {
                if (q.stage === 0 && item.id === 'claude') {
                    return { kind: 'push-stage', stageState: { harness: 'claude' } };
                }
                return { kind: 'submit', text: `echo ${item.id}` };
            }
        };

        registry.register(trigger);

        const session = new CompletionSession({
            paneId: 'pane-1',
            registry,
            getContext: () => dummyCtx
        });

        session.updateInput('@', 1);
        await vi. advanceTimersByTimeAsync(1);

        const snap0 = session.getSnapshot();
        expect(snap0.open).toBe(true);
        expect(snap0.stage).toBe(0);
        expect(snap0.stageLabel).toBe('Harness');
        // Ordenado por section.order (sec-a antes de sec-b) e pinned: 'bottom' por último.
        expect(snap0.items.map((i) => i.id)).toEqual(['claude', 'codex', 'create']);

        // Confirma 'claude' -> avança para stage 1 (push-stage).
        const outcome = await session.confirmSelection();
        expect(outcome?.kind).toBe('push-stage');
        await vi.advanceTimersByTimeAsync(1);

        const snap1 = session.getSnapshot();
        expect(snap1.stage).toBe(1);
        expect(snap1.stageLabel).toBe('Harness › Modelo');
        expect(snap1.items.map((i) => i.id)).toEqual(['sonnet']);

        // Backspace com query vazia faz pop-stage de volta para stage 0.
        expect(session.handleBackspaceOnEmptyQuery()).toBe(true);
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().stage).toBe(0);

        session.dispose();
    });

    it('suporta passthrough, refreshAfter, supressão pós-Escape, não-recaimento de >> em > e timeout de 5s', async () => {
        const registry = createTriggerRegistry();
        let resolveRefresh: (() => void) | null = null;
        let refreshedReady = false;

        registry.register({
            id: 'artisan',
            prefix: '>',
            label: 'Artisan',
            debounceMs: 0,
            isAvailable: () => true,
            provide: () => ({
                items: [{ id: 'migrate', label: 'migrate', kind: 'value' }]
            }),
            onSelect: () => ({ kind: 'close' })
        });

        // >> indisponível: NÃO pode cair em >.
        registry.register({
            id: 'groups',
            prefix: '>>',
            label: 'Grupos',
            debounceMs: 0,
            isAvailable: () => false,
            provide: () => ({ items: [] }),
            onSelect: () => ({ kind: 'close' })
        });

        registry.register({
            id: 'slash',
            prefix: '/',
            label: 'Atalhos',
            debounceMs: 0,
            isAvailable: () => true,
            provide: (q) => {
                if (q.input.startsWith('/usr/')) {
                    return { items: [], passthrough: true };
                }
                if (!refreshedReady) {
                    return {
                        items: [{ id: 'loading', label: 'Carregando...', kind: 'value' }],
                        refreshAfter: new Promise<void>((r) => {
                            resolveRefresh = r;
                        })
                    };
                }
                return {
                    items: [{ id: 'install', label: '/install', kind: 'value' }]
                };
            },
            onSelect: () => ({ kind: 'close' })
        });

        const session = new CompletionSession({
            paneId: 'pane-1',
            registry,
            getContext: () => dummyCtx,
            providerTimeoutMs: 100
        });

        // >> indisponível permanece idle (sem cair em >).
        session.updateInput('>>foo', 5);
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().open).toBe(false);

        // passthrough === true para /usr/bin.
        session.updateInput('/usr/bin', 8);
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().open).toBe(false);

        // refreshAfter atualiza automaticamente quando a Promise resolve.
        session.updateInput('/in', 3);
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().items.map((i) => i.id)).toEqual(['loading']);

        refreshedReady = true;
        resolveRefresh!();
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().items.map((i) => i.id)).toEqual(['install']);

        // Supressão pós-Escape: mantém suprimido até esvaziar a linha.
        session.updateInput('>mig', 4);
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().open).toBe(true);

        session.suppressCurrentTrigger();
        expect(session.getSnapshot().open).toBe(false);

        session.updateInput('>migrate', 8);
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().open).toBe(false);

        // Limpa a linha -> reabilita o gatilho.
        session.updateInput('', 0);
        session.updateInput('>m', 2);
        await vi.advanceTimersByTimeAsync(1);
        expect(session.getSnapshot().open).toBe(true);

        // Testa navegação moveSelection, selectFirst, selectLast e cancel.
        session.moveSelection(1);
        session.selectFirst();
        session.selectLast();
        session.setSelectionIndex(0);
        session.cancel();
        expect(session.getSnapshot().open).toBe(false);

        // Testa replace-input (keepOpen: false), submitWithoutSelection e timeout (100ms).
        registry.register({
            id: 'slow',
            prefix: '?',
            label: 'Lento',
            debounceMs: 10,
            isAvailable: () => true,
            provide: () => new Promise(() => {}),
            onSelect: () => ({ kind: 'replace-input', text: 'ls -la', keepOpen: false }),
            onSubmitWithoutSelection: () => ({ kind: 'submit', text: 'echo fallback' })
        });
        session.updateInput('?consulta', 9);
        await vi.advanceTimersByTimeAsync(15);
        const subNoSel = await session.submitWithoutSelection();
        expect(subNoSel).toEqual({ kind: 'submit', text: 'echo fallback' });

        session.updateInput('?timeout', 8);
        await vi.advanceTimersByTimeAsync(200);
        expect(session.getSnapshot().status).toBe('error');

        session.dispose();
    });
});
