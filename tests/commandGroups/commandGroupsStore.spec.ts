import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { useCommandGroupEditor } from '@/composables/commandGroups/useCommandGroupEditor';
import { useCommandGroupRun } from '@/composables/commandGroups/useCommandGroupRun';
import { useGroupRunOutput } from '@/composables/commandGroups/useGroupRunOutput';
import { commandGroupsIpc } from '@/ipc/commandGroups';
import { MtIpcError } from '@/ipc/core';
import { useCommandGroupsStore } from '@/stores/commandGroups';
import { useTemplatesStore } from '@/stores/templates';
import type {
    CommandGroup,
    CommandGroupCatalog,
    GroupOutputFrame,
    GroupRun,
    GroupRunCounts,
    GroupRunEvent,
    GroupRunItem
} from '@/types/commandGroups';

vi.mock('@/ipc/commandGroups', () => ({
    commandGroupsIpc: {
        list: vi.fn(),
        get: vi.fn(),
        create: vi.fn(),
        update: vi.fn(),
        remove: vi.fn(),
        reorder: vi.fn(),
        run: vi.fn(),
        cancelRun: vi.fn(),
        listRuns: vi.fn(),
        getRun: vi.fn(),
        attachItemOutput: vi.fn(),
        detachItemOutput: vi.fn(),
        suggest: vi.fn(),
        recurringCommands: vi.fn(),
        onGroupChanged: vi.fn(async () => () => {}),
        onRunStarted: vi.fn(async () => () => {}),
        onRunItemChanged: vi.fn(async () => () => {}),
        onRunFinished: vi.fn(async () => () => {})
    }
}));

function makeSampleGroup(overrides: Partial<CommandGroup> = {}): CommandGroup {
    return {
        id: overrides.id ?? 'grp-1',
        scope: overrides.scope ?? 'local',
        projectId: overrides.scope === 'global' ? null : (overrides.projectId ?? 'proj-a'),
        name: overrides.name ?? 'Subir Ambiente',
        description: overrides.description ?? 'Inicia Vite e Horizon',
        labels: overrides.labels ?? ['dev', 'backend'],
        mode: overrides.mode ?? 'sequential',
        separator: overrides.separator ?? 'and',
        cwd: overrides.cwd ?? null,
        env: overrides.env ?? {},
        sortOrder: overrides.sortOrder ?? 0,
        revision: overrides.revision ?? 1,
        createdAt: overrides.createdAt ?? 1700000000000,
        updatedAt: overrides.updatedAt ?? 1700000000000,
        items: overrides.items ?? [
            {
                id: 'item-1',
                groupId: overrides.id ?? 'grp-1',
                position: 0,
                title: 'Servidor Vite',
                command: 'npm run dev',
                cwd: null,
                env: {},
                continuous: false
            }
        ]
    };
}

function makeCounts(partial: Partial<GroupRunCounts> = {}): GroupRunCounts {
    return {
        total: partial.total ?? 2,
        pending: partial.pending ?? 1,
        starting: partial.starting ?? 0,
        running: partial.running ?? 1,
        cancelling: partial.cancelling ?? 0,
        succeeded: partial.succeeded ?? 0,
        failed: partial.failed ?? 0,
        skipped: partial.skipped ?? 0,
        cancelled: partial.cancelled ?? 0,
        interrupted: partial.interrupted ?? 0
    };
}

function makeRunItem(pos: number, overrides: Partial<GroupRunItem> = {}): GroupRunItem {
    return {
        id: overrides.id ?? `run-item-${pos}`,
        runId: overrides.runId ?? 'run-1',
        position: pos,
        snapshot: {
            sourceItemId: `src-${pos}`,
            position: pos,
            title: `Passo ${pos + 1}`,
            command: `echo ${pos}`,
            definedCwd: null,
            resolvedCwd: '/srv/app',
            env: {},
            continuous: false,
            templateId: null,
            shellPath: '/bin/zsh',
            shellDialect: 'zsh'
        },
        status: overrides.status ?? (pos === 0 ? 'running' : 'pending'),
        version: overrides.version ?? 1,
        ptyId: overrides.ptyId ?? 'pty-1',
        tabId: overrides.tabId ?? 'tab-1',
        paneId: overrides.paneId ?? 'pane-1',
        sessionId: overrides.sessionId ?? 'sess-1',
        commandBlockId: overrides.commandBlockId ?? null,
        exitCode: overrides.exitCode ?? null,
        signal: overrides.signal ?? null,
        errorCode: overrides.errorCode ?? null,
        errorMessage: overrides.errorMessage ?? null,
        startedAt: overrides.startedAt ?? 1700000000100,
        finishedAt: overrides.finishedAt ?? null,
        outputTruncated: overrides.outputTruncated ?? false
    };
}

function makeSampleRun(overrides: Partial<GroupRun> = {}): GroupRun {
    return {
        id: overrides.id ?? 'run-1',
        requestId: overrides.requestId ?? 'req-1',
        groupId: overrides.groupId ?? 'grp-1',
        projectId: overrides.projectId ?? 'proj-a',
        originSessionId: overrides.originSessionId ?? 'sess-1',
        snapshot: {
            groupId: 'grp-1',
            groupRevision: 1,
            scope: 'local',
            name: 'Subir Ambiente',
            description: '',
            labels: ['dev'],
            definedMode: 'sequential',
            effectiveMode: 'sequential',
            separator: 'and',
            cwd: null,
            env: {},
            projectRoot: '/srv/app',
            shellPath: '/bin/zsh',
            shellDialect: 'zsh',
            originTabId: 'tab-1',
            originPaneId: 'pane-1',
            originPtyId: 'pty-1',
            originSessionId: 'sess-1',
            originGeneration: 1
        },
        mode: overrides.mode ?? 'sequential',
        status: overrides.status ?? 'running',
        counts: overrides.counts ?? makeCounts(),
        version: overrides.version ?? 1,
        lastExitCode: overrides.lastExitCode ?? null,
        createdAt: overrides.createdAt ?? 1700000000000,
        startedAt: overrides.startedAt ?? 1700000000100,
        cancelRequestedAt: overrides.cancelRequestedAt ?? null,
        finishedAt: overrides.finishedAt ?? null,
        items: overrides.items ?? [makeRunItem(0), makeRunItem(1)]
    };
}

describe('useCommandGroupsStore (Fase 15 — CA-11, CA-14, CA-26)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('deduplica chamadas single-flight concorrentes para a mesma chave de projeto e aplica tombstones de exclusão (CA-14)', async () => {
        const store = useCommandGroupsStore();
        const localA = makeSampleGroup({ id: 'grp-local-a', projectId: 'proj-a', revision: 2 });
        const global1 = makeSampleGroup({
            id: 'grp-global-1',
            scope: 'global',
            projectId: null,
            revision: 3
        });

        let resolveList!: (val: CommandGroupCatalog) => void;
        vi.mocked(commandGroupsIpc.list).mockImplementation(
            () =>
                new Promise<CommandGroupCatalog>((res) => {
                    resolveList = res;
                })
        );

        const p1 = store.load('proj-a');
        const p2 = store.load('proj-a');
        expect(commandGroupsIpc.list).toHaveBeenCalledTimes(1);

        resolveList({
            projectId: 'proj-a',
            localGroups: [localA],
            globalGroups: [global1]
        });

        const [cat1, cat2] = await Promise.all([p1, p2]);
        expect(cat1.localGroups).toHaveLength(1);
        expect(cat2.globalGroups).toHaveLength(1);

        // Remove localA com revisão 4 -> registra tombstone
        vi.mocked(commandGroupsIpc.remove).mockResolvedValueOnce({
            id: 'grp-local-a',
            revision: 4
        });
        await store.remove('grp-local-a', 2);
        expect(store.localGroupsFor('proj-a')).toHaveLength(0);

        // Uma leitura atrasada retornando revisão 2 não deve ressuscitar o grupo excluído com revisão 4
        vi.mocked(commandGroupsIpc.list).mockResolvedValueOnce({
            projectId: 'proj-a',
            localGroups: [localA],
            globalGroups: [global1]
        });
        await store.load('proj-a', true);
        expect(store.localGroupsFor('proj-a')).toHaveLength(0);
    });

    it('invalida seletivamente apenas o projeto afetado em changed local e apenas globais em changed global (CA-14)', async () => {
        const store = useCommandGroupsStore();
        const grpA = makeSampleGroup({ id: 'grp-a', projectId: 'proj-a' });
        const grpB = makeSampleGroup({ id: 'grp-b', projectId: 'proj-b' });
        const grpGlobal = makeSampleGroup({
            id: 'grp-g',
            scope: 'global',
            projectId: null,
            name: 'Global Inicial'
        });

        vi.mocked(commandGroupsIpc.list)
            .mockResolvedValueOnce({
                projectId: 'proj-a',
                localGroups: [grpA],
                globalGroups: [grpGlobal]
            })
            .mockResolvedValueOnce({
                projectId: 'proj-b',
                localGroups: [grpB],
                globalGroups: [grpGlobal]
            });

        await store.load('proj-a');
        await store.load('proj-b');
        expect(store.localLoadedByProject['proj-a']).toBe(true);
        expect(store.localLoadedByProject['proj-b']).toBe(true);
        expect(commandGroupsIpc.list).toHaveBeenCalledTimes(2);

        // Evento changed local para proj-a -> recarrega apenas proj-a, mantendo proj-b intacto
        const grpAUpdated = makeSampleGroup({
            id: 'grp-a',
            projectId: 'proj-a',
            name: 'Subir Ambiente v2',
            revision: 2
        });
        vi.mocked(commandGroupsIpc.list).mockResolvedValueOnce({
            projectId: 'proj-a',
            localGroups: [grpAUpdated],
            globalGroups: [grpGlobal]
        });

        store.handleGroupChanged({
            groupId: 'grp-a',
            scope: 'local',
            projectId: 'proj-a',
            revision: 2,
            operation: 'updated'
        });

        await vi.waitFor(() => {
            expect(store.localGroupsFor('proj-a')[0]?.name).toBe('Subir Ambiente v2');
        });
        expect(store.localLoadedByProject['proj-b']).toBe(true);
        expect(commandGroupsIpc.list).toHaveBeenLastCalledWith('proj-a');

        // Evento changed global -> recarrega apenas list(null), mantendo localLoadedByProject['proj-a'] e ['proj-b'] true
        const grpGlobalUpdated = makeSampleGroup({
            id: 'grp-g',
            scope: 'global',
            projectId: null,
            name: 'Global v2',
            revision: 2
        });
        vi.mocked(commandGroupsIpc.list).mockResolvedValueOnce({
            projectId: null,
            localGroups: [],
            globalGroups: [grpGlobalUpdated]
        });

        store.handleGroupChanged({
            groupId: 'grp-g',
            scope: 'global',
            projectId: null,
            revision: 2,
            operation: 'updated'
        });

        await vi.waitFor(() => {
            expect(store.globalGroups[0]?.name).toBe('Global v2');
        });
        expect(store.localLoadedByProject['proj-a']).toBe(true);
        expect(store.localLoadedByProject['proj-b']).toBe(true);
        expect(commandGroupsIpc.list).toHaveBeenLastCalledWith(null);
    });

    it('descarta eventos com seq <= version, aplica seq === version + 1 e reconcilia salto de seq via getRun (CA-14, CA-26)', async () => {
        const store = useCommandGroupsStore();
        const initialRun = makeSampleRun({ id: 'run-1', version: 2 });
        store.applyRunSnapshot(initialRun);

        // 1. Evento antigo (seq = 2 <= version 2): descartado silenciosamente
        const staleEvent: GroupRunEvent = {
            runId: 'run-1',
            projectId: 'proj-a',
            seq: 2,
            status: 'failed',
            counts: makeCounts({ running: 0, failed: 2 }),
            lastExitCode: 99,
            startedAt: 1700000000100,
            cancelRequestedAt: null,
            finishedAt: 1700000000900,
            item: null
        };
        store.handleRunEvent(staleEvent);
        expect(store.runsById['run-1']?.version).toBe(2);
        expect(store.runsById['run-1']?.status).toBe('running');
        expect(commandGroupsIpc.getRun).not.toHaveBeenCalled();

        // 2. Evento contíguo (seq = 3 === version + 1): aplica delta do item 0
        const nextEvent: GroupRunEvent = {
            runId: 'run-1',
            projectId: 'proj-a',
            seq: 3,
            status: 'running',
            counts: makeCounts({ pending: 0, running: 1, succeeded: 1 }),
            lastExitCode: 0,
            startedAt: 1700000000100,
            cancelRequestedAt: null,
            finishedAt: null,
            item: {
                id: 'run-item-0',
                position: 0,
                status: 'succeeded',
                version: 2,
                ptyId: 'pty-1',
                tabId: 'tab-1',
                paneId: 'pane-1',
                sessionId: 'sess-1',
                commandBlockId: 'blk-1',
                exitCode: 0,
                signal: null,
                errorCode: null,
                errorMessage: null,
                startedAt: 1700000000100,
                finishedAt: 1700000000500,
                outputTruncated: false
            }
        };
        store.handleRunEvent(nextEvent);
        expect(store.runsById['run-1']?.version).toBe(3);
        expect(store.runsById['run-1']?.items[0]?.status).toBe('succeeded');
        expect(store.runsById['run-1']?.items[0]?.exitCode).toBe(0);

        // 3. Salto de sequência (seq = 5 > version 3 + 1): dispara getRun('run-1') para reconciliar
        const reconciledRun = makeSampleRun({
            id: 'run-1',
            version: 5,
            status: 'succeeded',
            counts: makeCounts({ pending: 0, running: 0, succeeded: 2 }),
            lastExitCode: 0,
            finishedAt: 1700000001000,
            items: [
                makeRunItem(0, { status: 'succeeded', version: 2, exitCode: 0 }),
                makeRunItem(1, { status: 'succeeded', version: 3, exitCode: 0 })
            ]
        });
        vi.mocked(commandGroupsIpc.getRun).mockResolvedValueOnce(reconciledRun);

        store.handleRunEvent({
            runId: 'run-1',
            projectId: 'proj-a',
            seq: 5,
            status: 'succeeded',
            counts: makeCounts({ pending: 0, running: 0, succeeded: 2 }),
            lastExitCode: 0,
            startedAt: 1700000000100,
            cancelRequestedAt: null,
            finishedAt: 1700000001000,
            item: null
        });

        await vi.waitFor(() => {
            expect(commandGroupsIpc.getRun).toHaveBeenCalledWith('run-1');
            expect(store.runsById['run-1']?.version).toBe(5);
            expect(store.runsById['run-1']?.status).toBe('succeeded');
        });
    });

    it('bufferiza eventos que chegam antes do snapshot de startRun e drena em ordem após o retorno (CA-14)', async () => {
        const store = useCommandGroupsStore();
        let resolveStart!: (run: GroupRun) => void;
        vi.mocked(commandGroupsIpc.run).mockImplementation(
            () =>
                new Promise<GroupRun>((res) => {
                    resolveStart = res;
                })
        );
        vi.mocked(commandGroupsIpc.getRun).mockImplementation(() => new Promise(() => {}));

        const startPromise = store.startRun({
            requestId: 'req-race',
            groupId: 'grp-1',
            expectedRevision: 1,
            projectId: 'proj-a',
            originPaneId: 'pane-1',
            modeOverride: null,
            templateByItem: {}
        });

        // Evento seq 2 chega antes do retorno de startRun (que vem em version 1)
        store.handleRunEvent({
            runId: 'run-race',
            projectId: 'proj-a',
            seq: 2,
            status: 'succeeded',
            counts: makeCounts({ total: 1, pending: 0, running: 0, succeeded: 1 }),
            lastExitCode: 0,
            startedAt: 1700000000100,
            cancelRequestedAt: null,
            finishedAt: 1700000000500,
            item: {
                id: 'run-item-0',
                position: 0,
                status: 'succeeded',
                version: 2,
                ptyId: 'pty-1',
                tabId: 'tab-1',
                paneId: 'pane-1',
                sessionId: 'sess-1',
                commandBlockId: 'blk-1',
                exitCode: 0,
                signal: null,
                errorCode: null,
                errorMessage: null,
                startedAt: 1700000000100,
                finishedAt: 1700000000500,
                outputTruncated: false
            }
        });

        resolveStart(
            makeSampleRun({
                id: 'run-race',
                version: 1,
                status: 'running',
                items: [makeRunItem(0, { runId: 'run-race', version: 1 })]
            })
        );

        const finalRun = await startPromise;
        expect(finalRun.version).toBe(2);
        expect(finalRun.status).toBe('succeeded');
        expect(finalRun.items[0]?.status).toBe('succeeded');
    });

    it('exercita useCommandGroupEditor, useCommandGroupRun e useGroupRunOutput (CA-16, CA-18, CA-20, CA-26)', async () => {
        const group = makeSampleGroup({ id: 'grp-edit', revision: 3 });
        const editor = useCommandGroupEditor({
            initialGroup: group
        });

        expect(editor.isEditMode.value).toBe(true);
        expect(editor.dirty.value).toBe(false);

        editor.addItem({ title: 'Watcher Contínuo', command: 'npm run watch', continuous: true });
        editor.moveItemUp(1);
        expect(editor.reorderAnnouncement.value).toBe('Item movido para a posição 1 de 2');
        expect(editor.hasMidSequenceContinuousWarning.value).toBe(true);

        // Proteção contra resposta tardia de IA com draftRevision obsoleto (CA-20)
        const revBefore = editor.draftRevision.value;
        editor.addLabel('ci');
        const appliedStale = editor.applySuggestion(
            {
                scope: 'local',
                projectId: 'proj-a',
                name: 'Sugestão IA Obsoleta',
                description: 'Gerado',
                labels: ['ia'],
                mode: 'parallel',
                separator: 'and',
                cwd: null,
                env: {},
                items: [
                    {
                        id: null,
                        title: 'Teste',
                        command: 'npm test',
                        cwd: null,
                        env: {},
                        continuous: false
                    }
                ]
            },
            revBefore
        );
        expect(appliedStale).toBe(false);
        expect(editor.draft.value.name).toBe('Subir Ambiente');

        // Aplicar sugestão com draftRevision atual e depois desfazer
        const appliedFresh = editor.applySuggestion(
            {
                scope: 'local',
                projectId: 'proj-a',
                name: 'Sugestão IA Válida',
                description: 'Gerado',
                labels: ['ia'],
                mode: 'parallel',
                separator: 'and',
                cwd: null,
                env: {},
                items: [
                    {
                        id: null,
                        title: 'Teste',
                        command: 'npm test',
                        cwd: null,
                        env: {},
                        continuous: false
                    }
                ]
            },
            editor.draftRevision.value
        );
        expect(appliedFresh).toBe(true);
        expect(editor.draft.value.name).toBe('Sugestão IA Válida');
        expect(editor.hasUndoSnapshot.value).toBe(true);

        editor.undoSuggestion();
        expect(editor.draft.value.name).toBe('Subir Ambiente');

        // Conflito de revisão não apaga as edições do usuário (CA-26)
        vi.mocked(commandGroupsIpc.update).mockRejectedValueOnce(
            new MtIpcError(
                {
                    code: 'commands.group.revision_conflict',
                    message: 'Conflito de revisão'
                },
                'command_group_update'
            )
        );
        vi.mocked(commandGroupsIpc.get).mockResolvedValueOnce(
            makeSampleGroup({ id: 'grp-edit', name: 'Versão Remota', revision: 4 })
        );

        const savedOnConflict = await editor.save();
        expect(savedOnConflict).toBeNull();
        expect(editor.revisionConflictGroup.value?.revision).toBe(4);
        expect(editor.draft.value.name).toBe('Subir Ambiente');

        // useCommandGroupRun seleciona templates compatíveis em multi_tabs e reutiliza requestId em retry
        const templatesStore = useTemplatesStore();
        templatesStore.byProject['proj-a'] = [
            {
                id: 'tpl-default',
                projectId: 'proj-a',
                config: {
                    name: 'Padrão',
                    icon: 'lucide:terminal',
                    cwd: null,
                    initialCommand: null,
                    addToWorkspace: false,
                    continuous: false,
                    environment: { kind: 'shell', agentId: null },
                    envVars: {}
                },
                isDefault: true,
                position: 0,
                revision: 1,
                createdAt: 1,
                updatedAt: 1
            },
            {
                id: 'tpl-cont',
                projectId: 'proj-a',
                config: {
                    name: 'Contínuo',
                    icon: 'lucide:activity',
                    cwd: null,
                    initialCommand: null,
                    addToWorkspace: false,
                    continuous: true,
                    environment: { kind: 'shell', agentId: null },
                    envVars: {}
                },
                isDefault: false,
                position: 1,
                revision: 1,
                createdAt: 1,
                updatedAt: 1
            }
        ];

        const runner = useCommandGroupRun();
        const map = runner.resolveCompatibleTemplatesForMultiTab(
            makeSampleGroup({
                mode: 'multi_tabs',
                items: [
                    {
                        id: 'i-1',
                        groupId: 'grp-1',
                        position: 0,
                        title: 'Build',
                        command: 'npm run build',
                        cwd: null,
                        env: {},
                        continuous: false
                    },
                    {
                        id: 'i-2',
                        groupId: 'grp-1',
                        position: 1,
                        title: 'Dev',
                        command: 'npm run dev',
                        cwd: null,
                        env: {},
                        continuous: true
                    }
                ]
            }),
            'proj-a'
        );
        expect(map).toEqual({ 'i-1': 'tpl-default', 'i-2': 'tpl-cont' });

        // useGroupRunOutput anexa e desanexa sem cancelar a execução
        let pushedFrameHandler!: (frame: GroupOutputFrame) => void;
        vi.mocked(commandGroupsIpc.attachItemOutput).mockImplementationOnce(
            async (_runId, _itemId, _afterSeq, onFrame) => {
                pushedFrameHandler = onFrame;
                return {
                    subscriptionId: 'sub-1',
                    replayStartSeq: 0,
                    replayEndSeq: 12,
                    truncated: false,
                    ended: false
                };
            }
        );
        vi.mocked(commandGroupsIpc.detachItemOutput).mockResolvedValueOnce(null);

        const out = useGroupRunOutput();
        await out.attach('run-1', 'run-item-0');
        expect(out.attached.value).toBe(true);

        pushedFrameHandler({
            subscriptionId: 'sub-1',
            itemId: 'run-item-0',
            seq: 12,
            kind: 'replay',
            text: 'Iniciando...\n',
            droppedBytes: null
        });
        pushedFrameHandler({
            subscriptionId: 'sub-1',
            itemId: 'run-item-0',
            seq: 20,
            kind: 'gap',
            text: null,
            droppedBytes: 512
        });
        expect(out.outputText.value).toContain('Iniciando...');
        expect(out.isTruncated.value).toBe(true);
        expect(out.droppedBytesTotal.value).toBe(512);

        await out.detach();
        expect(commandGroupsIpc.detachItemOutput).toHaveBeenCalledWith('sub-1');
        expect(commandGroupsIpc.cancelRun).not.toHaveBeenCalled();
    });
});
