import { effectScope, nextTick } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useCommandGroupEditor } from '@/composables/commandGroups/useCommandGroupEditor';
import { useCommandGroupRun } from '@/composables/commandGroups/useCommandGroupRun';
import {
    decodeArtisanCommandForm,
    getQuickCommandsAdapters,
    setQuickCommandsAdaptersForTest
} from '@/composables/quick-commands/adapters';
import { useArtisanForm } from '@/composables/quick-commands/useArtisanForm';
import { useAgentAttention } from '@/composables/useAgentAttention';
import { useCommandMemorySuggestions } from '@/composables/useCommandMemorySuggestions';
import { useShellMarkers } from '@/composables/useShellMarkers';
import { useTabIndicatorsSync } from '@/composables/useTabIndicatorsSync';
import { agentsIpc } from '@/ipc/agents';
import { bridgeIpc } from '@/ipc/bridge';
import { commandGroupsIpc } from '@/ipc/commandGroups';
import { MtIpcError } from '@/ipc/core';
import * as gitIpc from '@/ipc/git';
import { memoryIpc } from '@/ipc/memory';
import * as portsIpc from '@/ipc/ports';
import * as projectsIpc from '@/ipc/projects';
import {
    onCatalogUpdated,
    onCommandCreated,
    onDiscoveryFailed,
    quickCommandsIpc
} from '@/ipc/quickCommands';
import * as sessionsIpc from '@/ipc/sessions';
import * as shellIpc from '@/ipc/shell';
import { skillsIpc } from '@/ipc/skills';
import * as tabsIpc from '@/ipc/tabs';
import * as tabStateIpc from '@/ipc/tabState';
import * as templatesIpc from '@/ipc/templates';
import * as themesIpc from '@/ipc/themes';
import * as workspaceIpc from '@/ipc/workspace';
import { useAgentsStore } from '@/stores/agents';
import { isTerminalRunStatus, useCommandGroupsStore } from '@/stores/commandGroups';
import { useQuickCommandsStore } from '@/stores/quickCommands';
import { useSkillsStore } from '@/stores/skills';
import {
    commands,
    type CommandGroup_Serialize,
    type GroupRun_Serialize
} from '@/types/bindings';

const mockListenHandlers = new Map<string, (ev: { payload: unknown }) => void>();

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn(async (eventName: string, cb: (ev: { payload: unknown }) => void) => {
        mockListenHandlers.set(eventName, cb);
        return () => {
            mockListenHandlers.delete(eventName);
        };
    })
}));

vi.mock('@tauri-apps/api/core', () => ({
    invoke: vi.fn(async () => '/srv/selected-folder'),
    Channel: class MockChannel {
        public onmessage: ((frame: unknown) => void) | null = null;
    }
}));

function makeGroup(overrides: Partial<CommandGroup_Serialize> = {}): CommandGroup_Serialize {
    return {
        id: 'grp-100',
        scope: 'local',
        projectId: 'proj-1',
        name: 'Grupo Teste',
        description: 'Desc',
        labels: ['api'],
        mode: 'sequential',
        separator: 'and',
        cwd: null,
        env: {},
        items: [
            {
                id: 'it-1',
                groupId: 'grp-100',
                position: 0,
                title: 'Passo 1',
                command: 'npm test',
                cwd: null,
                env: {},
                continuous: false
            }
        ],
        sortOrder: 0,
        revision: 1,
        createdAt: 1000,
        updatedAt: 1000,
        ...overrides
    };
}

function makeRun(overrides: Partial<GroupRun_Serialize> = {}): GroupRun_Serialize {
    return {
        id: 'run-100',
        requestId: 'req-100',
        groupId: 'grp-100',
        projectId: 'proj-1',
        originSessionId: null,
        snapshot: {
            groupId: 'grp-100',
            groupRevision: 1,
            scope: 'local',
            name: 'Grupo Teste',
            description: '',
            labels: [],
            definedMode: 'sequential',
            effectiveMode: 'sequential',
            separator: 'and',
            cwd: null,
            env: {},
            projectRoot: '/srv/proj',
            shellPath: '/bin/zsh',
            shellDialect: 'zsh',
            originTabId: null,
            originPaneId: null,
            originPtyId: null,
            originSessionId: null,
            originGeneration: null
        },
        mode: 'sequential',
        status: 'running',
        counts: {
            total: 1,
            pending: 0,
            starting: 0,
            running: 1,
            cancelling: 0,
            succeeded: 0,
            failed: 0,
            skipped: 0,
            cancelled: 0,
            interrupted: 0
        },
        version: 1,
        lastExitCode: null,
        createdAt: 1000,
        startedAt: 1000,
        cancelRequestedAt: null,
        finishedAt: null,
        items: [
            {
                id: 'ri-1',
                runId: 'run-100',
                position: 0,
                snapshot: {
                    sourceItemId: 'it-1',
                    position: 0,
                    title: 'Passo 1',
                    command: 'npm test',
                    definedCwd: null,
                    resolvedCwd: '/srv/proj',
                    env: {},
                    continuous: false,
                    templateId: null,
                    shellPath: '/bin/zsh',
                    shellDialect: 'zsh'
                },
                status: 'running',
                version: 1,
                ptyId: null,
                tabId: null,
                paneId: null,
                sessionId: null,
                commandBlockId: null,
                exitCode: null,
                signal: null,
                errorCode: null,
                errorMessage: null,
                startedAt: 1000,
                finishedAt: null,
                outputTruncated: false
            }
        ],
        ...overrides
    };
}

describe('IPC Wrappers & Composables Coverage Suite', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
        mockListenHandlers.clear();
        setQuickCommandsAdaptersForTest(null);
    });

    it('exercita todos os métodos de commandGroupsIpc, agentsIpc, skillsIpc, memoryIpc e quickCommandsIpc', async () => {
        const grp = makeGroup();
        const run = makeRun();

        vi.spyOn(commands, 'commandGroupList').mockResolvedValue({
            status: 'ok',
            data: { projectId: 'proj-1', localGroups: [grp], globalGroups: [] }
        });
        vi.spyOn(commands, 'commandGroupGet').mockResolvedValue({ status: 'ok', data: grp });
        vi.spyOn(commands, 'commandGroupCreate').mockResolvedValue({ status: 'ok', data: grp });
        vi.spyOn(commands, 'commandGroupUpdate').mockResolvedValue({ status: 'ok', data: grp });
        vi.spyOn(commands, 'commandGroupDelete').mockResolvedValue({
            status: 'ok',
            data: { id: grp.id, revision: 2 }
        });
        vi.spyOn(commands, 'commandGroupReorder').mockResolvedValue({
            status: 'ok',
            data: { projectId: 'proj-1', localGroups: [grp], globalGroups: [] }
        });
        vi.spyOn(commands, 'commandGroupRun').mockResolvedValue({ status: 'ok', data: run });
        vi.spyOn(commands, 'commandGroupRunCancel').mockResolvedValue({ status: 'ok', data: run });
        vi.spyOn(commands, 'commandGroupRunList').mockResolvedValue({
            status: 'ok',
            data: { items: [], nextCursor: null }
        });
        vi.spyOn(commands, 'commandGroupRunGet').mockResolvedValue({ status: 'ok', data: run });
        vi.spyOn(commands, 'commandGroupRunItemAttach').mockResolvedValue({
            status: 'ok',
            data: {
                subscriptionId: 'sub-1',
                replayStartSeq: 0,
                replayEndSeq: 1,
                truncated: false
            }
        });
        vi.spyOn(commands, 'commandGroupRunItemDetach').mockResolvedValue({
            status: 'ok',
            data: null
        });
        vi.spyOn(commands, 'commandGroupSuggest').mockResolvedValue({
            status: 'ok',
            data: {
                requestId: 'req-1',
                draftRevision: 1,
                suggestion: {
                    scope: 'local',
                    projectId: 'proj-1',
                    name: 'Sugestão',
                    description: '',
                    labels: [],
                    mode: 'sequential',
                    separator: 'and',
                    cwd: null,
                    env: {},
                    items: []
                }
            }
        });
        vi.spyOn(commands, 'commandGroupRecurringCommands').mockResolvedValue({
            status: 'ok',
            data: { available: true, items: [] }
        });

        expect((await commandGroupsIpc.list('proj-1')).localGroups).toHaveLength(1);
        expect((await commandGroupsIpc.get(grp.id)).id).toBe(grp.id);
        expect(
            (
                await commandGroupsIpc.create({
                    scope: 'local',
                    projectId: 'proj-1',
                    name: 'N',
                    description: '',
                    labels: [],
                    mode: 'sequential',
                    separator: 'and',
                    cwd: null,
                    env: {},
                    items: []
                })
            ).id
        ).toBe(grp.id);
        expect(
            (
                await commandGroupsIpc.update(grp.id, 1, {
                    scope: 'local',
                    projectId: 'proj-1',
                    name: 'N',
                    description: '',
                    labels: [],
                    mode: 'sequential',
                    separator: 'and',
                    cwd: null,
                    env: {},
                    items: []
                })
            ).id
        ).toBe(grp.id);
        expect((await commandGroupsIpc.remove(grp.id, 1)).revision).toBe(2);
        expect((await commandGroupsIpc.reorder('local', 'proj-1', [])).localGroups).toHaveLength(1);
        expect(
            (
                await commandGroupsIpc.run({
                    requestId: 'r-1',
                    groupId: grp.id,
                    expectedRevision: 1,
                    projectId: 'proj-1',
                    originPaneId: null,
                    modeOverride: null,
                    templateByItem: {}
                })
            ).id
        ).toBe(run.id);
        expect((await commandGroupsIpc.cancelRun(run.id, null)).id).toBe(run.id);
        expect((await commandGroupsIpc.listRuns('proj-1')).items).toEqual([]);
        expect((await commandGroupsIpc.getRun(run.id)).id).toBe(run.id);
        expect(
            (await commandGroupsIpc.attachItemOutput(run.id, 'ri-1', null, () => {})).subscriptionId
        ).toBe('sub-1');
        expect(await commandGroupsIpc.detachItemOutput('sub-1')).toBeNull();
        expect(
            (
                await commandGroupsIpc.suggest({
                    requestId: 'r-1',
                    projectId: 'proj-1',
                    paneId: null,
                    scope: 'local',
                    goal: 'Test',
                    draft: null,
                    draftRevision: 1
                })
            ).requestId
        ).toBe('req-1');
        expect((await commandGroupsIpc.recurringCommands('proj-1')).available).toBe(true);

        const u1 = await commandGroupsIpc.onGroupChanged(() => {});
        const u2 = await commandGroupsIpc.onRunStarted(() => {});
        const u3 = await commandGroupsIpc.onRunItemChanged(() => {});
        const u4 = await commandGroupsIpc.onRunFinished(() => {});
        mockListenHandlers.get('command-group://changed')?.({ payload: {} });
        mockListenHandlers.get('command-group://run-started')?.({ payload: {} });
        mockListenHandlers.get('command-group://run-item-changed')?.({ payload: {} });
        mockListenHandlers.get('command-group://run-finished')?.({ payload: {} });
        u1();
        u2();
        u3();
        u4();

        // agentsIpc, skillsIpc, memoryIpc, quickCommandsIpc
        vi.spyOn(commands, 'agentList').mockResolvedValue({ status: 'ok', data: [] });
        vi.spyOn(commands, 'agentDetectRefresh').mockResolvedValue({ status: 'ok', data: [] });
        vi.spyOn(commands, 'agentDescriptorsReload').mockResolvedValue({ status: 'ok', data: [] });
        vi.spyOn(commands, 'agentTemplatesEnsure').mockResolvedValue({ status: 'ok', data: 4 });
        vi.spyOn(commands, 'agentWorktreePendingList').mockResolvedValue({ status: 'ok', data: [] });
        vi.spyOn(commands, 'agentTelemetryList').mockResolvedValue({ status: 'ok', data: [] });
        vi.spyOn(commands, 'agentTelemetryGet').mockResolvedValue({ status: 'ok', data: null });
        vi.spyOn(commands, 'agentAttentionGet').mockResolvedValue({ status: 'ok', data: null });
        vi.spyOn(commands, 'agentTabLinkGet').mockResolvedValue({ status: 'ok', data: null });
        vi.spyOn(commands, 'agentTabCloseCheck').mockResolvedValue({ status: 'ok', data: null });

        await agentsIpc.list();
        await agentsIpc.detectRefresh();
        await agentsIpc.descriptorsReload();
        await agentsIpc.templatesEnsure('proj-1');
        await agentsIpc.worktreePendingList('proj-1');
        await agentsIpc.telemetryList();
        await agentsIpc.telemetryGet('tab-1');
        await agentsIpc.attentionGet('tab-1');
        await agentsIpc.tabLinkGet('tab-1');
        await agentsIpc.tabCloseCheck('tab-1');

        vi.spyOn(commands, 'skillsTargetsList').mockResolvedValue({ status: 'ok', data: [] });
        vi.spyOn(commands, 'skillsTargetRemove').mockResolvedValue({ status: 'ok', data: 1 });
        await skillsIpc.targetsList('proj-1');
        await skillsIpc.targetRemove(1, true);
        const us1 = await skillsIpc.onChanged(() => {});
        const us2 = await skillsIpc.onSyncCompleted(() => {});
        mockListenHandlers.get('skills://changed')?.({ payload: {} });
        mockListenHandlers.get('skills://sync-completed')?.({ payload: {} });
        us1();
        us2();

        vi.spyOn(commands, 'memorySuggestions').mockResolvedValue({ status: 'ok', data: [] });
        vi.spyOn(commands, 'memoryList').mockResolvedValue({ status: 'ok', data: [] });
        vi.spyOn(commands, 'memoryForget').mockResolvedValue({ status: 'ok', data: true });
        vi.spyOn(commands, 'memoryRebuild').mockResolvedValue({ status: 'ok', data: 5 });
        await memoryIpc.suggestions({
            projectId: 'proj-1',
            prefix: 'git',
            cwd: null,
            limit: 5,
            includeHidden: false
        });
        await memoryIpc.list('proj-1', false, 10);
        await memoryIpc.forget('proj-1', 'git status');
        await memoryIpc.rebuild('proj-1');
        const um1 = await memoryIpc.onUpdated(() => {});
        mockListenHandlers.get('memory://updated')?.({ payload: { projectId: 'proj-1' } });
        um1();

        vi.spyOn(commands, 'artisanResolveRoot').mockResolvedValue({ status: 'ok', data: null });
        vi.spyOn(commands, 'artisanFormSchema').mockResolvedValue({ status: 'ok', data: {} });
        vi.spyOn(commands, 'artisanFormatMaxcodePrompt').mockResolvedValue({
            status: 'ok',
            data: 'prompt'
        });
        await quickCommandsIpc.resolveRoot('/srv', 'proj-1');
        await quickCommandsIpc.formSchema();
        await quickCommandsIpc.maxcodePrompt('/srv', {
            className: 'SyncCmd',
            signatureName: 'app:sync',
            description: 'Desc',
            params: [],
            isolatable: false,
            promptsForMissingInput: false,
            schedule: null,
            objective: ''
        });
        const uq1 = await onCatalogUpdated(() => {});
        const uq2 = await onDiscoveryFailed(() => {});
        const uq3 = await onCommandCreated(() => {});
        mockListenHandlers.get('artisan://catalog-updated')?.({ payload: {} });
        mockListenHandlers.get('artisan://discovery-failed')?.({ payload: {} });
        mockListenHandlers.get('artisan://command-created')?.({ payload: {} });
        uq1();
        uq2();
        uq3();
    });

    it('exercita useCommandGroupEditor, useCommandGroupRun e useCommandGroupsStore em todos os ramos', async () => {
        const grp = makeGroup();
        const run = makeRun();

        vi.spyOn(commandGroupsIpc, 'list').mockResolvedValue({
            projectId: 'proj-1',
            localGroups: [grp],
            globalGroups: []
        });
        vi.spyOn(commandGroupsIpc, 'create').mockResolvedValue(grp);
        vi.spyOn(commandGroupsIpc, 'update').mockResolvedValue({ ...grp, revision: 2 });
        vi.spyOn(commandGroupsIpc, 'remove').mockResolvedValue({ id: grp.id, revision: 3 });
        vi.spyOn(commandGroupsIpc, 'reorder').mockResolvedValue({
            projectId: 'proj-1',
            localGroups: [grp],
            globalGroups: []
        });
        vi.spyOn(commandGroupsIpc, 'run').mockResolvedValue(run);
        vi.spyOn(commandGroupsIpc, 'cancelRun').mockResolvedValue({
            ...run,
            status: 'cancelled'
        });
        vi.spyOn(commandGroupsIpc, 'listRuns').mockResolvedValue({
            items: [],
            nextCursor: null
        });
        vi.spyOn(commandGroupsIpc, 'recurringCommands').mockResolvedValue({
            available: true,
            items: [
                {
                    command: 'php artisan migrate',
                    cwd: 'backend',
                    frequency: 5,
                    lastUsedAt: 1000
                }
            ]
        });

        const store = useCommandGroupsStore();
        expect(isTerminalRunStatus('succeeded')).toBe(true);
        expect(isTerminalRunStatus('running')).toBe(false);

        await store.initListeners();
        await store.load('proj-1');
        expect(store.localGroupsFor('proj-1')).toHaveLength(1);
        expect(store.localGroupsFor(null)).toEqual([]);

        store.openCreateModal('local', 'proj-1', 'Novo');
        expect(store.editorModalState.open).toBe(true);
        store.openEditModal(grp);
        expect(store.editorModalState.group?.id).toBe(grp.id);
        store.closeEditorModal();
        expect(store.editorModalState.open).toBe(false);

        await store.reorder('local', 'proj-1', [{ id: grp.id, revision: 1 }]);
        await store.loadHistory('proj-1', null, 10, false);
        await store.loadRecurring('proj-1', 10);

        const editor = useCommandGroupEditor({
            initialGroup: grp,
            initialScope: 'local',
            initialProjectId: 'proj-1'
        });

        editor.setMode('parallel');
        editor.setSeparator('semicolon');
        editor.addLabel('  backend  ');
        editor.removeLabel(0);
        editor.addGroupEnvPair();
        editor.draft.value.envPairs[0] = { key: 'API_URL', value: 'https://example.com' };
        editor.removeGroupEnvPair(0);

        editor.addItem({ title: 'Passo 2', command: 'npm run lint', continuous: true });
        editor.duplicateItem(0);
        editor.moveItemDown(0);
        editor.moveItemUp(1);
        editor.removeItem(2);

        editor.appendRecurringCommand({
            command: 'php artisan horizon',
            cwd: 'api',
            frequency: 3,
            lastUsedAt: 2000
        });

        editor.copyToScope('global', null);
        expect(editor.isEditMode.value).toBe(false);
        editor.setScope('local', 'proj-1');
        await editor.save();

        const editExisting = useCommandGroupEditor({
            initialGroup: grp
        });
        await editExisting.deleteCurrentGroup();

        const runner = useCommandGroupRun();
        const started = await runner.launchGroup({
            group: grp,
            projectId: 'proj-1'
        });
        expect(started.id).toBe(run.id);
        expect(store.activeRunsForProject('proj-1')).toHaveLength(1);

        await runner.retryLastLaunch();
        await runner.cancelRun(run.id, null);
        await runner.runInNewTab(grp, 'proj-1');

        vi.spyOn(commandGroupsIpc, 'run').mockRejectedValueOnce(
            new MtIpcError(
                {
                    code: 'commands.group.pane_busy',
                    message: 'Painel ocupado',
                    details: null
                },
                'command_group_run'
            )
        );
        await expect(
            runner.launchGroup({
                group: grp,
                projectId: 'proj-1',
                originPaneId: 'pane-busy-1'
            })
        ).rejects.toThrow();
        expect(store.busyPaneFallback?.groupId).toBe(grp.id);

        store.disposeListeners();
    });

    it('exercita decodeArtisanCommandForm, useArtisanForm, useTabIndicatorsSync, useAgentAttention, useAgentsStore e useSkillsStore', async () => {
        const decoded = decodeArtisanCommandForm({
            className: 'SyncUsersCommand',
            signatureName: 'users:sync',
            description: 'Sincroniza usuários',
            params: [
                {
                    kind: 'argument',
                    name: 'tenant',
                    description: 'Tenant ID',
                    optional: false,
                    isArray: false,
                    default: null,
                    shortcut: null,
                    acceptsValue: false
                },
                {
                    kind: 'option',
                    name: 'force',
                    description: 'Forçar execução',
                    optional: true,
                    isArray: false,
                    default: null,
                    shortcut: 'f',
                    acceptsValue: false
                }
            ],
            isolatable: true,
            promptsForMissingInput: true,
            schedule: {
                frequency: { kind: 'dailyAt', time: '03:00' },
                arguments: ['default-tenant'],
                options: [{ name: 'force', values: [], enabled: true }],
                withoutOverlapping: true,
                onOneServer: true,
                runInBackground: false
            },
            objective: 'Sincronizar base'
        });
        expect(decoded.className).toBe('SyncUsersCommand');
        expect(decoded.params).toHaveLength(2);
        expect(decoded.schedule?.frequency.kind).toBe('dailyAt');

        for (const freq of [
            { kind: 'everyMinute' },
            { kind: 'hourlyAt', minute: 15 },
            { kind: 'twiceDaily', first: 1, second: 13 },
            { kind: 'weeklyOn', day: 1, time: '08:00' },
            { kind: 'monthlyOn', day: 15, time: '09:30' },
            { kind: 'cron', expression: '0 * * * *' }
        ]) {
            const f = decodeArtisanCommandForm({
                className: 'Cmd',
                signatureName: 'app:cmd',
                description: 'D',
                params: [],
                isolatable: false,
                promptsForMissingInput: false,
                schedule: {
                    frequency: freq,
                    arguments: [],
                    options: [],
                    withoutOverlapping: false,
                    onOneServer: false,
                    runInBackground: false
                },
                objective: ''
            });
            expect(f.schedule?.frequency.kind).toBe(freq.kind);
        }

        const adapters = getQuickCommandsAdapters();
        expect(adapters.projectStackHint(null)).toBeNull();
        adapters.focusPaneInput(null);
        adapters.notifyToast({ title: 'Teste', severity: 'info' });

        // useArtisanForm + useQuickCommandsStore
        const qcStore = useQuickCommandsStore();
        vi.spyOn(quickCommandsIpc, 'preview').mockResolvedValue({
            path: 'app/Console/Commands/SyncCmd.php',
            signature: 'app:sync',
            php: '<?php',
            report: {
                valid: false,
                issues: [{ path: 'signatureName', code: 'invalid', message: 'Erro sig', severity: 'error' }]
            },
            consolePatch: null,
            manualSteps: [],
            revision: 'rev-1'
        });

        const scope = effectScope();
        await scope.run(async () => {
            const formHelper = useArtisanForm();
            formHelper.setSignatureName('billing:run');
            expect(formHelper.form.value.className).toBe('BillingRun');
            formHelper.setClassName('CustomBillingCommand');
            formHelper.setDescription('Descrição');
            formHelper.setIsolatable(true);
            formHelper.setPromptsForMissingInput(true);
            formHelper.setObjective('Objetivo');
            formHelper.setParams([]);
            formHelper.setSchedule(null);

            qcStore.modal.report = {
                valid: false,
                issues: [
                    {
                        path: 'signatureName',
                        code: 'err',
                        message: 'Assinatura inválida',
                        severity: 'error'
                    }
                ]
            };
            await nextTick();
            expect(formHelper.firstErrorMessageFor('signatureName')).toBe('Assinatura inválida');

            const div = document.createElement('div');
            const input = document.createElement('input');
            input.setAttribute('data-field-path', 'signatureName');
            div.appendChild(input);
            formHelper.focusFirstInvalidField(div);

            const syncInd = useTabIndicatorsSync();
            syncInd.stop();

            const attention = useAgentAttention('tab-1');
            expect(attention.isAwaiting.value).toBe(false);
            await attention.focusAgentTab();
            await attention.dismissNotice();
            attention.stop();

            vi.spyOn(memoryIpc, 'suggestions').mockResolvedValue([
                {
                    projectId: 'proj-1',
                    command: 'npm run dev',
                    lastCwd: '/srv',
                    useCount: 5,
                    successCount: 5,
                    failureCount: 0,
                    lastExitCode: 0,
                    lastUsedAtMs: 1000,
                    pinned: true,
                    hidden: false,
                    score: 10
                }
            ]);
            const memSug = useCommandMemorySuggestions('pane-1', {
                projectId: 'proj-1',
                cwd: '/srv'
            });
            const entries = await memSug.queryNow('npm');
            expect(entries).toHaveLength(1);
            memSug.applySuggestion(entries[0]!);
            memSug.cancelPending();
        });
        scope.stop();

        // useAgentsStore & useSkillsStore methods
        const agentsStore = useAgentsStore();
        vi.spyOn(agentsIpc, 'list').mockResolvedValue([]);
        vi.spyOn(agentsIpc, 'detectRefresh').mockResolvedValue([]);
        vi.spyOn(agentsIpc, 'descriptorsReload').mockResolvedValue([]);
        vi.spyOn(agentsIpc, 'settingsGet').mockResolvedValue({
            shortcutsEnabled: true,
            shortcuts: {},
            isolation: {
                defaultLocation: 'project',
                branchPrefix: 'agent/',
                openIsolatedByDefault: false
            },
            attention: {
                enabled: true,
                onTurnComplete: 'notify',
                silenceMs: 5000,
                notify: true
            },
            telemetry: {
                enabled: true,
                staleAfterS: 60,
                readSessionFiles: true
            },
            inject: {},
            enabled: {}
        });
        vi.spyOn(agentsIpc, 'telemetryList').mockResolvedValue([]);
        vi.spyOn(agentsIpc, 'worktreePendingList').mockResolvedValue([]);
        vi.spyOn(agentsIpc, 'templatesEnsure').mockResolvedValue(4);
        vi.spyOn(agentsIpc, 'telemetryGet').mockResolvedValue(null);
        vi.spyOn(agentsIpc, 'attentionGet').mockResolvedValue(null);
        vi.spyOn(agentsIpc, 'tabLinkGet').mockResolvedValue(null);
        vi.spyOn(agentsIpc, 'tabCloseCheck').mockResolvedValue(null);

        await agentsStore.init();
        await agentsStore.refreshInstallations();
        await agentsStore.reloadDescriptors();
        await agentsStore.ensureTemplates('proj-1');
        await agentsStore.loadPendingWorktrees('proj-1');
        await agentsStore.fetchTabTelemetry('tab-1');
        await agentsStore.fetchTabAttention('tab-1');
        await agentsStore.fetchTabLink('tab-1');
        await agentsStore.inspectClose('tab-1');

        agentsStore.openAgentDialog('proj-1', 'claude-code', true);
        expect(agentsStore.openDialogState.open).toBe(true);
        agentsStore.closeAgentDialog();
        expect(agentsStore.openDialogState.open).toBe(false);
        agentsStore.markTabClosed('tab-closed-1');
        expect(agentsStore.isTabClosed('tab-closed-1')).toBe(true);
        agentsStore.dispose();

        // useQuickCommandsStore extra methods
        qcStore.openCreateModal('/srv/app', { signatureName: 'users:clean', paneId: 'pane-1' });
        expect(qcStore.modal.open).toBe(true);
        const confirmPromise = qcStore.requestConfirm('Confirmar', 'rm -rf', 'pane-1');
        qcStore.notifyActivePaneChanged('pane-2');
        expect(await confirmPromise).toBe(false);
        qcStore.closeModal({ preserveDraft: false });

        const skillsStore = useSkillsStore();
        vi.spyOn(skillsIpc, 'scan').mockResolvedValue({
            sourceDir: '/skills',
            fromIndex: true,
            skills: [],
            scannedAtMs: 1000
        });
        vi.spyOn(skillsIpc, 'targetsList').mockResolvedValue([]);
        vi.spyOn(memoryIpc, 'list').mockResolvedValue([]);
        vi.spyOn(memoryIpc, 'pin').mockResolvedValue({
            projectId: 'proj-1',
            command: 'npm test',
            lastCwd: null,
            useCount: 5,
            successCount: 5,
            failureCount: 0,
            lastExitCode: 0,
            lastUsedAtMs: 1000,
            pinned: true,
            hidden: false,
            score: 10
        });
        vi.spyOn(memoryIpc, 'hide').mockResolvedValue({
            projectId: 'proj-1',
            command: 'npm test',
            lastCwd: null,
            useCount: 5,
            successCount: 5,
            failureCount: 0,
            lastExitCode: 0,
            lastUsedAtMs: 1000,
            pinned: false,
            hidden: true,
            score: 10
        });
        vi.spyOn(memoryIpc, 'forget').mockResolvedValue(true);
        vi.spyOn(memoryIpc, 'rebuild').mockResolvedValue(3);

        await skillsStore.init();
        await skillsStore.scan('proj-1');
        expect(skillsStore.catalogFor('proj-1')?.fromIndex).toBe(true);
        await skillsStore.loadTargets('proj-1');
        await skillsStore.loadMemory('proj-1');
        await skillsStore.pin('proj-1', 'npm test', true);
        await skillsStore.hide('proj-1', 'npm test', true);
        await skillsStore.forget('proj-1', 'npm test');
        await skillsStore.rebuild('proj-1');
        skillsStore.dispose();
    });

    it('exercita wrappers IPC de projects, bridge, themes, tabs, sessions, templates, workspace e ramos restantes das stores', async () => {
        const cmdRecord = commands as unknown as Record<string, (...args: unknown[]) => unknown>;
        for (const fnName of [
            'projectSnapshot',
            'projectList',
            'projectAdd',
            'projectUpdate',
            'projectRemove',
            'projectReorder',
            'projectRedetect',
            'projectPreviewIdentity',
            'projectRefreshMissing',
            'projectRootList',
            'projectRootAdd',
            'projectRootUpdate',
            'projectRootRemove',
            'projectRootRescan',
            'projectDiscoveryList',
            'projectDiscoveryAccept',
            'projectDiscoveryDismiss',
            'bridgeStatus',
            'bridgeGetSettings',
            'bridgeUpdateSettings',
            'bridgeInspectCommandError',
            'bridgeNotifyProjectSelected',
            'bridgeOpenFileInMaxcode',
            'bridgeReconnectNow',
            'bridgeIssueProtocolSupport',
            'bridgeSendIssueCommand',
            'bridgeBugsCyclePreview',
            'bridgeBugsCycleStart',
            'themeList',
            'themeGet',
            'themeAppearanceGet',
            'themeAppearanceUpdate',
            'themeImportFile',
            'themeImportDir',
            'themeSetActive',
            'themeDelete',
            'themeExport',
            'tabList',
            'tabOpen',
            'tabClose',
            'tabReorder',
            'tabFocus',
            'paneSplit',
            'paneClose',
            'paneRestart',
            'paneLayoutUpdate',
            'sessionNew',
            'sessionList',
            'sessionGet',
            'sessionScrollbackGet',
            'sessionSearch',
            'templateList',
            'templateGet',
            'templateCreate',
            'templateUpdate',
            'templateDelete',
            'templateSetDefault',
            'workspaceSnapshot',
            'workspaceInitialCommandRun',
            'agentSettingsGet',
            'agentSettingsUpdate',
            'agentTemplateOptionsGet',
            'agentTemplateOptionsSet',
            'agentWorktreePropose',
            'agentTabOpen',
            'agentWorktreeResolve',
            'agentAttentionAck',
            'skillsScan',
            'skillsTargetUpsert',
            'skillsPlan',
            'skillsApply',
            'gitStatusForPath',
            'gitStatusRefresh',
            'gitWorktreeList',
            'gitWorktreeCreate',
            'gitWorktreeRemove',
            'portsListForTab',
            'portsOpenBrowser',
            'portsKill',
            'portsSetPollMode',
            'shellIntegrationInfo',
            'commandBlockList',
            'commandBlockGet',
            'commandBlockErrorContext',
            'commandBlockSearch',
            'tabStateList',
            'tabStateGet',
            'tabMarkSeen',
            'tabSetFocus',
            'tabSetContinuous',
            'artisanCatalogGet',
            'artisanCatalogRefresh',
            'artisanValidateForm',
            'artisanPreviewCommand',
            'artisanCreateCommand',
            'shortcutList',
            'shortcutResolve',
            'memoryPin',
            'memoryHide'
        ]) {
            cmdRecord[fnName] = vi.fn().mockResolvedValue({
                status: 'ok',
                data: {}
            });
        }

        // gitIpc, portsIpc, shellIpc, tabStateIpc, quickCommandsIpc, memoryIpc
        await gitIpc.gitStatusForPath('/srv/a');
        await gitIpc.gitStatusRefresh('/srv/a');
        await gitIpc.gitWorktreeList('/srv/a');
        await gitIpc.gitWorktreeCreate({
            projectRoot: '/srv/a',
            name: 'wt-1',
            branch: 'feat/wt-1',
            baseRef: 'HEAD',
            location: 'max_terminal'
        });
        await gitIpc.gitWorktreeRemove('/srv/a/.worktrees/wt-1', true);
        const ug = await gitIpc.onGitStatusChanged(() => {});
        mockListenHandlers.get('git://status-changed')?.({ payload: {} });
        ug();

        await portsIpc.portsListForTab('tab-1');
        await portsIpc.portsOpenBrowser('pane-1', 5173);
        await portsIpc.portsKill('pane-1', 5173, 1234);
        await portsIpc.portsSetPollMode('active');
        const upo1 = await portsIpc.onPortOpened(() => {});
        const upo2 = await portsIpc.onPortUpdated(() => {});
        const upo3 = await portsIpc.onPortClosed(() => {});
        mockListenHandlers.get('ports://opened')?.({ payload: {} });
        mockListenHandlers.get('ports://updated')?.({ payload: {} });
        mockListenHandlers.get('ports://closed')?.({ payload: {} });
        upo1();
        upo2();
        upo3();

        await shellIpc.shellIntegrationInfo('pty-1');
        await shellIpc.commandBlockList({});
        await shellIpc.commandBlockGet('blk-1');
        await shellIpc.commandBlockErrorContext('blk-1');
        await shellIpc.commandBlockSearch({ query: 'git' });
        const ush1 = await shellIpc.onCommandStarted(() => {});
        const ush2 = await shellIpc.onCommandFinished(() => {});
        const ush3 = await shellIpc.onShellCwdChanged(() => {});
        const ush4 = await shellIpc.onShellIntegrationMode(() => {});
        mockListenHandlers.get('shell://command-started')?.({ payload: {} });
        mockListenHandlers.get('shell://command-finished')?.({ payload: {} });
        mockListenHandlers.get('shell://cwd-changed')?.({ payload: {} });
        mockListenHandlers.get('shell://integration-mode')?.({ payload: {} });
        ush1();
        ush2();
        ush3();
        ush4();

        await tabStateIpc.tabStateList();
        await tabStateIpc.tabStateGet('tab-1');
        await tabStateIpc.tabMarkSeen({ tabId: 'tab-1' });
        await tabStateIpc.tabSetFocus({
            windowFocused: true,
            activeTabId: 'tab-1',
            activePaneId: 'pane-1'
        });
        await tabStateIpc.tabSetContinuous('tab-1', 'pane-1', 'auto');
        const uts1 = await tabStateIpc.onTabStateChanged(() => {});
        const uts2 = await tabStateIpc.onTabStateRemoved(() => {});
        mockListenHandlers.get('tab://state-changed')?.({ payload: {} });
        mockListenHandlers.get('tab://state-removed')?.({ payload: {} });
        uts1();
        uts2();

        const sampleForm = {
            className: 'SyncCmd',
            signatureName: 'app:sync',
            description: 'Desc',
            params: [],
            isolatable: false,
            promptsForMissingInput: false,
            schedule: null,
            objective: 'Obj'
        };
        await quickCommandsIpc.catalogGet('/srv/a');
        await quickCommandsIpc.catalogRefresh('/srv/a');
        await quickCommandsIpc.validateForm('/srv/a', sampleForm);
        await quickCommandsIpc.preview('/srv/a', sampleForm);
        await quickCommandsIpc.create('/srv/a', sampleForm, 'rev-1');
        await quickCommandsIpc.shortcutList('/srv/a');
        await quickCommandsIpc.shortcutResolve('/srv/a', 'test');
        await memoryIpc.pin('proj-1', 'npm test', true);
        await memoryIpc.hide('proj-1', 'npm test', true);

        const shellMarkers = useShellMarkers();
        expect(shellMarkers.resolveAnchor('pane-missing').positionQuality).toBe('unavailable');
        let flushedAnchor: unknown = null;
        const cancelFlush = shellMarkers.runAfterFlush('pane-missing', (a) => {
            flushedAnchor = a;
        });
        cancelFlush();
        expect(flushedAnchor).toBeTruthy();
        shellMarkers.markPaneMarkersUnavailable('pane-missing');
        expect(shellMarkers.getPositionQuality('pane-missing', 'blk-1')).toBe('unavailable');

        await projectsIpc.projectSnapshot();
        await projectsIpc.projectList();
        await projectsIpc.projectAdd({ path: '/srv/a' });
        await projectsIpc.projectUpdate('proj-1', { title: 'Novo' });
        await projectsIpc.projectRemove('proj-1');
        await projectsIpc.projectReorder(['proj-1']);
        await projectsIpc.projectRedetect('proj-1');
        await projectsIpc.projectPreviewIdentity('/srv/a');
        await projectsIpc.projectRefreshMissing();
        await projectsIpc.projectRootList();
        await projectsIpc.projectRootAdd({
            path: '/srv',
            discoveryMode: 'auto_import',
            importExisting: true,
            watchEnabled: true
        });
        await projectsIpc.projectRootUpdate('root-1', { watchEnabled: false });
        await projectsIpc.projectRootRemove('root-1');
        await projectsIpc.projectRootRescan('root-1');
        await projectsIpc.projectDiscoveryList();
        await projectsIpc.projectDiscoveryAccept(['/srv/a']);
        await projectsIpc.projectDiscoveryDismiss(['/srv/a']);
        expect(await projectsIpc.pickFolder('Selecionar')).toBe('/srv/selected-folder');

        const up1 = await projectsIpc.onProjectDiscovered(() => {});
        const up2 = await projectsIpc.onProjectRemoved(() => {});
        const up3 = await projectsIpc.onProjectChanged(() => {});
        const up4 = await projectsIpc.onProjectRootChanged(() => {});
        mockListenHandlers.get('project://discovered')?.({ payload: {} });
        mockListenHandlers.get('project://removed')?.({ payload: {} });
        mockListenHandlers.get('project://changed')?.({ payload: {} });
        mockListenHandlers.get('project://root-changed')?.({ payload: {} });
        up1();
        up2();
        up3();
        up4();

        // bridgeIpc
        await bridgeIpc.status();
        await bridgeIpc.getSettings();
        await bridgeIpc.updateSettings({});
        await bridgeIpc.inspectCommandError('blk-1');
        await bridgeIpc.notifyProjectSelected('/srv/a', 'Proj', 'main');
        await bridgeIpc.openFileInMaxCode({
            filePath: '/srv/a/index.ts',
            line: 10,
            column: 1,
            focusEditor: true
        });
        await bridgeIpc.reconnectNow();
        await bridgeIpc.issueProtocolSupport();
        await bridgeIpc.sendIssueCommand({
            kind: 'open',
            projectPath: '/srv/a',
            repoFullName: 'max/repo',
            issueNumber: 1,
            issueTitle: 'Bug',
            labels: [],
            stage: 'inspect',
            focusEditor: true
        });
        await bridgeIpc.bugsCyclePreview({
            cycleId: 'c-1',
            projectPath: '/srv/a',
            repoFullName: 'max/repo',
            issueNumbers: [1],
            filters: {
                includeLabels: [],
                excludeLabels: [],
                explicitIssueNumbers: []
            },
            dryRun: true
        });
        await bridgeIpc.bugsCycleStart({
            cycleId: 'c-1',
            projectPath: '/srv/a',
            repoFullName: 'max/repo',
            issueNumbers: [1],
            filters: {
                includeLabels: [],
                excludeLabels: [],
                explicitIssueNumbers: []
            },
            dryRun: false
        });
        const ub1 = await bridgeIpc.onStatusChanged(() => {});
        const ub2 = await bridgeIpc.onProjectSynced(() => {});
        const ub3 = await bridgeIpc.onTerminalOpened(() => {});
        const ub4 = await bridgeIpc.onBugCycleProgress(() => {});
        const ub5 = await bridgeIpc.onError(() => {});
        mockListenHandlers.get('bridge://status-changed')?.({ payload: {} });
        mockListenHandlers.get('bridge://project-synced')?.({ payload: {} });
        mockListenHandlers.get('bridge://terminal-opened')?.({ payload: {} });
        mockListenHandlers.get('bridge://bug-cycle-progress')?.({ payload: {} });
        mockListenHandlers.get('bridge://error')?.({ payload: {} });
        ub1();
        ub2();
        ub3();
        ub4();
        ub5();

        // themesIpc
        await themesIpc.themeList();
        await themesIpc.themeGet('max-navy');
        await themesIpc.themeAppearanceGet();
        await themesIpc.themeAppearanceUpdate({});
        await themesIpc.themeImportFile('/tmp/theme.yaml', { dryRun: true });
        await themesIpc.themeImportDir('/tmp/themes', { dryRun: true });
        await themesIpc.themeSetActive('max-navy', { kind: 'global' });
        await themesIpc.themeDelete('custom-1', null);
        await themesIpc.themeExport('max-navy', 'tabby_yaml');
        const uth1 = await themesIpc.onThemeChanged(() => {});
        const uth2 = await themesIpc.onThemeLibraryChanged(() => {});
        mockListenHandlers.get('theme://changed')?.({ payload: {} });
        mockListenHandlers.get('theme://library-changed')?.({ payload: {} });
        uth1();
        uth2();

        // tabsIpc, sessionsIpc, templatesIpc, workspaceIpc
        await tabsIpc.tabList('proj-1');
        await tabsIpc.tabOpen({ projectId: 'proj-1', requestId: 'r-1', focus: true });
        await tabsIpc.tabClose('tab-1', 'r-1');
        await tabsIpc.tabReorder('proj-1', ['tab-1']);
        await tabsIpc.tabFocus('tab-1', 'pane-1');
        await tabsIpc.paneSplit('pane-1', 'horizontal', 1, 'r-1');
        await tabsIpc.paneClose('pane-1', 'r-1');
        await tabsIpc.paneRestart('pane-1', true, 'r-1');
        await tabsIpc.paneLayoutUpdate('tab-1', 1, []);
        const ut1 = await tabsIpc.onTabOpened(() => {});
        const ut2 = await tabsIpc.onTabClosed(() => {});
        const ut3 = await tabsIpc.onTabReordered(() => {});
        const ut4 = await tabsIpc.onTabLayoutChanged(() => {});
        const ut5 = await tabsIpc.onPanePtyReplaced(() => {});
        const ut6 = await tabsIpc.onPaneExited(() => {});
        mockListenHandlers.get('tab://opened')?.({ payload: {} });
        mockListenHandlers.get('tab://closed')?.({ payload: {} });
        mockListenHandlers.get('tab://reordered')?.({ payload: {} });
        mockListenHandlers.get('tab://layout-changed')?.({ payload: {} });
        mockListenHandlers.get('pane://pty-replaced')?.({ payload: {} });
        mockListenHandlers.get('pane://exited')?.({ payload: {} });
        ut1();
        ut2();
        ut3();
        ut4();
        ut5();
        ut6();

        await sessionsIpc.sessionNew('tab-1', 'r-1');
        await sessionsIpc.sessionList('tab-1', null, 10);
        await sessionsIpc.sessionGet('ts-1');
        await sessionsIpc.sessionScrollbackGet('ts-1', 's-1', null, 10);
        await sessionsIpc.sessionSearch('tab-1', 'git', 10);
        const uss1 = await sessionsIpc.onSessionStarted(() => {});
        const uss2 = await sessionsIpc.onSessionArchived(() => {});
        mockListenHandlers.get('session://started')?.({ payload: {} });
        mockListenHandlers.get('session://archived')?.({ payload: {} });
        uss1();
        uss2();

        await templatesIpc.templateList('proj-1');
        await templatesIpc.templateGet('tpl-1');
        await templatesIpc.templateCreate('proj-1', {
            name: 'Padrao',
            icon: 'mdi:console',
            environment: { kind: 'shell' },
            continuous: false,
            addToWorkspace: false,
            envVars: {},
            makeDefault: false
        });
        await templatesIpc.templateUpdate('tpl-1', 1, {
            name: 'Padrao',
            icon: 'mdi:console',
            environment: { kind: 'shell' },
            continuous: false,
            addToWorkspace: false,
            envVars: {},
            makeDefault: false
        });
        await templatesIpc.templateDelete('tpl-1', 1);
        await templatesIpc.templateSetDefault('tpl-1');
        const utpl = await templatesIpc.onTemplateChanged(() => {});
        mockListenHandlers.get('template://changed')?.({ payload: {} });
        utpl();

        await workspaceIpc.workspaceSnapshot();
        await workspaceIpc.workspaceInitialCommandRun('tab-1', 'pane-1', 1, 'r-1');
        const uws = await workspaceIpc.onWorkspaceRestored(() => {});
        mockListenHandlers.get('workspace://restored')?.({ payload: {} });
        uws();

        // Restantes de agentsIpc e skillsIpc
        await agentsIpc.settingsGet();
        await agentsIpc.settingsUpdate({});
        await agentsIpc.templateOptionsGet('tpl-1');
        await agentsIpc.templateOptionsSet('tpl-1', {
            model: null,
            extraArgs: [],
            isolationDefault: false,
            location: 'project'
        });
        await agentsIpc.worktreePropose('proj-1', 'claude-code', 'feat');
        await agentsIpc.tabOpen({
            projectId: 'proj-1',
            agent: 'claude-code',
            isolation: { mode: 'none' },
            focus: true
        });
        await agentsIpc.worktreeResolve(1, 'keep', 'tok-1');
        await agentsIpc.attentionAck('tab-1', 'run-1', 1, 'dismissed');

        await skillsIpc.scan('proj-1');
        await skillsIpc.targetUpsert({
            id: null,
            scope: 'global',
            projectId: null,
            agent: 'claude-code',
            mode: 'symlink',
            autoSync: false,
            enabled: true,
            selection: { kind: 'all' }
        });
        await skillsIpc.plan(1);
        await skillsIpc.apply('plan-1', { 'skill-a': 'overwrite' });

        // useAgentsStore extra branches
        const agentsStore = useAgentsStore();
        await agentsStore.applyTelemetry({
            tabId: 'tab-1',
            paneId: 'pane-1',
            sessionId: 'sess-1',
            agent: 'claude-code',
            runId: 'run-1',
            revision: 1,
            sessionRef: null,
            model: null,
            context: null,
            quota5h: null,
            quotaWeekly: null,
            costUsd: null,
            availability: {},
            agentActive: true,
            updatedAtMs: 1000
        });
        await agentsStore.applyTelemetry({
            tabId: 'tab-1',
            paneId: 'pane-1',
            sessionId: 'sess-1',
            agent: 'claude-code',
            runId: 'run-1',
            revision: 2,
            sessionRef: null,
            model: null,
            context: null,
            quota5h: null,
            quotaWeekly: null,
            costUsd: null,
            availability: {},
            agentActive: true,
            updatedAtMs: 2000
        });
        await agentsStore.applyAttention({
            tabId: 'tab-1',
            paneId: 'pane-1',
            projectId: 'proj-1',
            sessionId: 'sess-1',
            agent: 'claude-code',
            runId: 'run-1',
            revision: 1,
            acknowledged: false,
            state: {
                state: 'awaiting',
                kind: 'awaitingApproval',
                episode: 1,
                sinceMs: 1000,
                source: 'nativeHook',
                message: 'Aprovar'
            }
        });
        await agentsStore.applyAttention({
            tabId: 'tab-1',
            paneId: 'pane-1',
            projectId: 'proj-1',
            sessionId: 'sess-1',
            agent: 'claude-code',
            runId: 'run-1',
            revision: 2,
            acknowledged: true,
            state: { state: 'working' }
        });
        expect(agentsStore.telemetryForTab('tab-1')?.revision).toBe(2);
        expect(agentsStore.attentionForTab('tab-1')?.revision).toBe(2);

        // useSkillsStore extra branches
        const skillsStore = useSkillsStore();
        vi.spyOn(skillsIpc, 'targetUpsert').mockResolvedValue({
            id: 10,
            scope: 'global',
            projectId: null,
            agent: 'claude-code',
            mode: 'symlink',
            autoSync: false,
            enabled: true,
            selection: { kind: 'all' },
            projectRoot: null,
            destOverride: null,
            destDir: '/tmp/skills'
        });
        vi.spyOn(skillsIpc, 'targetRemove').mockResolvedValue(1);
        vi.spyOn(skillsIpc, 'plan').mockResolvedValue({
            planId: 'plan-10',
            targetId: 10,
            destDir: '/tmp/skills',
            actions: [],
            warnings: [],
            fingerprint: 'fp-1'
        });
        vi.spyOn(skillsIpc, 'apply').mockResolvedValue({
            targetId: 10,
            applied: 1,
            skipped: 0,
            backups: [],
            errors: []
        });
        vi.spyOn(memoryIpc, 'suggestions').mockResolvedValue([]);

        await skillsStore.upsertTarget({
            id: null,
            scope: 'global',
            projectId: null,
            agent: 'claude-code',
            mode: 'symlink',
            autoSync: false,
            enabled: true,
            selection: { kind: 'all' }
        });
        await skillsStore.planSync(10);
        await skillsStore.applySync('plan-10', {}, 10);
        await skillsStore.removeTarget(10, true);
        await skillsStore.suggest('pane-1', {
            projectId: 'proj-1',
            prefix: 'npm',
            cwd: null,
            limit: 5,
            includeHidden: false
        });

        // useQuickCommandsStore extra branches
        const qcStore = useQuickCommandsStore();
        vi.spyOn(quickCommandsIpc, 'catalogGet').mockResolvedValue({
            root: '/srv/app',
            generatedAtMs: 1000,
            source: 'fresh',
            stale: false,
            commands: [
                {
                    name: 'app:sync',
                    description: 'Sync',
                    aliases: ['sync'],
                    arguments: [],
                    options: []
                }
            ],
            lastError: null
        });
        vi.spyOn(quickCommandsIpc, 'catalogRefresh').mockResolvedValue({
            root: '/srv/app',
            generatedAtMs: 2000,
            source: 'fresh',
            stale: false,
            commands: [],
            lastError: null
        });
        vi.spyOn(quickCommandsIpc, 'shortcutList').mockResolvedValue({
            root: '/srv/app',
            managers: [],
            warnings: [],
            items: []
        });
        vi.spyOn(quickCommandsIpc, 'resolveRoot').mockResolvedValue({
            root: '/srv/app',
            projectId: 'proj-1',
            isWorktree: false,
            invoker: 'php artisan',
            invokerTokens: ['php', 'artisan'],
            runner: 'local',
            layout: 'modern',
            laravelVersion: '11.0'
        });
        vi.spyOn(quickCommandsIpc, 'preview').mockResolvedValue({
            path: 'app/Console/Commands/SyncCmd.php',
            signature: 'app:sync',
            php: '<?php class SyncCmd {}',
            report: { valid: true, issues: [] },
            consolePatch: null,
            manualSteps: [],
            revision: 'rev-10'
        });
        vi.spyOn(quickCommandsIpc, 'create').mockResolvedValue({
            path: 'app/Console/Commands/SyncCmd.php',
            consoleRoutesPatched: false,
            manualSteps: [],
            catalogRefreshPending: true
        });
        vi.spyOn(quickCommandsIpc, 'maxcodePrompt').mockResolvedValue('Criar comando artisan: ...');
        vi.spyOn(quickCommandsIpc, 'formSchema').mockResolvedValue({ type: 'object' });
        vi.spyOn(quickCommandsIpc, 'validateForm').mockResolvedValue({ valid: true, issues: [] });

        await qcStore.init();
        mockListenHandlers.get('artisan://catalog-updated')?.({
            payload: { root: '/srv/app', revision: 2 }
        });
        mockListenHandlers.get('artisan://discovery-failed')?.({
            payload: {
                root: '/srv/app-err',
                revision: 1,
                error: { code: 'artisan.failed', message: 'Erro', details: null }
            }
        });
        mockListenHandlers.get('artisan://command-created')?.({
            payload: { root: '/srv/app', revision: 3 }
        });

        await qcStore.resolveRoot('/srv/app/sub', 'proj-1');
        await qcStore.resolveRoot('/srv/app/sub', 'proj-1');
        expect(qcStore.isCatalogInBackoff('/srv/app-err')).toBe(true);
        await qcStore.ensureCatalog('/srv/app');
        expect(qcStore.commandByName('/srv/app', 'sync')?.name).toBe('app:sync');
        await qcStore.ensureCatalog('/srv/app', { force: true });
        await qcStore.loadShortcuts('/srv/app');
        await qcStore.loadShortcuts('/srv/app');

        qcStore.openCreateModal('/srv/app', { signatureName: 'app:sync', paneId: 'pane-1' });
        qcStore.updateForm(
            {
                description: 'Sincroniza',
                objective: 'Sincronizar registros'
            },
            { immediatePreview: true }
        );
        await qcStore.refreshPreviewNow();
        await qcStore.createWithMaxCode();
        await qcStore.fillWithAi();
        qcStore.modal.aiSnapshot = {
            className: 'OldCmd',
            signatureName: 'old:cmd',
            description: 'Old',
            params: [],
            isolatable: false,
            promptsForMissingInput: false,
            schedule: null,
            objective: ''
        };
        qcStore.undoAiSuggestion();
        await qcStore.refreshPreviewNow();
        await qcStore.create();

        const pConfirm = qcStore.requestConfirm('T', 'cmd', 'pane-1');
        qcStore.resolveConfirm(true, 'pane-1');
        expect(await pConfirm).toBe(true);
        qcStore.dispose();

        // useAgentsStore remaining methods
        vi.spyOn(agentsIpc, 'settingsUpdate').mockResolvedValue({
            shortcutsEnabled: true,
            shortcuts: {},
            isolation: {
                defaultLocation: 'project',
                branchPrefix: 'agent/',
                openIsolatedByDefault: false
            },
            attention: {
                enabled: true,
                onTurnComplete: 'notify',
                silenceMs: 5000,
                notify: true
            },
            telemetry: {
                enabled: true,
                staleAfterS: 60,
                readSessionFiles: true
            },
            inject: {},
            enabled: {}
        });
        vi.spyOn(agentsIpc, 'templateOptionsGet').mockResolvedValue({
            model: null,
            extraArgs: [],
            isolationDefault: false,
            location: 'project'
        });
        vi.spyOn(agentsIpc, 'templateOptionsSet').mockResolvedValue({
            model: 'sonnet',
            extraArgs: [],
            isolationDefault: true,
            location: 'project'
        });
        vi.spyOn(agentsIpc, 'worktreePropose').mockResolvedValue({
            repoRoot: '/srv/a',
            slug: 'wt-1',
            name: 'wt-1',
            branch: 'agent/claude-code/wt-1',
            path: '/srv/a/.worktrees/wt-1',
            location: 'project',
            rootIgnored: true,
            warnings: [],
            baseRef: '0000000000000000000000000000000000000000'
        });
        vi.spyOn(agentsIpc, 'tabOpen').mockResolvedValue({
            tab: {
                id: 'tab-ag-1',
                projectId: 'proj-1',
                templateId: null,
                title: 'Claude',
                icon: 'lucide:bot',
                position: 0,
                origin: 'manual',
                continuous: false,
                lifecycle: 'ready',
                revision: 1,
                layoutRevision: 1,
                layout: { kind: 'leaf', paneId: 'pane-ag-1' },
                panes: [],
                activePaneId: 'pane-ag-1',
                primaryPaneId: 'pane-ag-1',
                initialDispatch: 'none',
                tabSessionId: 'ts-ag-1'
            },
            link: {
                id: 1,
                tabId: 'tab-ag-1',
                projectId: 'proj-1',
                agent: 'claude-code',
                worktreePath: '/srv/a/.worktrees/wt-1',
                branch: 'agent/claude-code/wt-1',
                baseRef: 'main',
                createdByApp: true,
                openedAtMs: 1000,
                tabClosedAtMs: null,
                resolution: null,
                resolvedAtMs: null
            }
        });
        vi.spyOn(agentsIpc, 'attentionAck').mockResolvedValue(null);
        vi.spyOn(agentsIpc, 'worktreeResolve').mockResolvedValue({
            id: 1,
            tabId: 'tab-ag-1',
            projectId: 'proj-1',
            agent: 'claude-code',
            worktreePath: '/srv/a/.worktrees/wt-1',
            branch: 'agent/claude-code/wt-1',
            baseRef: 'main',
            createdByApp: true,
            openedAtMs: 1000,
            tabClosedAtMs: 2000,
            resolution: 'keep',
            resolvedAtMs: 2000
        });

        await agentsStore.updateSettings({});
        await agentsStore.getTemplateOptions('tpl-1');
        await agentsStore.setTemplateOptions('tpl-1', {
            model: 'sonnet',
            extraArgs: [],
            isolationDefault: true,
            location: 'project'
        });
        await agentsStore.proposeWorktree('proj-1', 'claude-code', 'wt-1');
        await agentsStore.openAgent({
            projectId: 'proj-1',
            agent: 'claude-code',
            isolation: { mode: 'none' },
            focus: true
        });
        expect(agentsStore.linkForTab('tab-ag-1')?.branch).toBe('agent/claude-code/wt-1');
        await agentsStore.ackAttention('tab-ag-1', 'run-1', 1, 'dismissed');
        await agentsStore.resolveWorktree(1, 'keep', 'tok-1');
    });
});
