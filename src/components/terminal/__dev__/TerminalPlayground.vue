<template>
    <div class="terminal-playground">
        <header class="playground-toolbar">
            <div class="toolbar-group">
                <label class="toolbar-label" for="pg-shell-select">Shell:</label>
                <MaxInputSelect id="pg-shell-select" v-model="selectedShell" :options="shellOptions" option-label="label" option-value="value" option-name="label" class="toolbar-select" />
                <MaxButton label="Iniciar (Spawn)" size="small" class="pg-btn pg-btn-primary" @click="spawnActiveSlot" />
                <MaxButton label="Reiniciar" severity="secondary" size="small" class="pg-btn" :disabled="!activeSlot.ptyId" @click="restartActiveSlot" />
                <MaxButton label="Encerrar (Kill)" severity="danger" size="small" class="pg-btn pg-btn-danger" :disabled="!activeSlot.ptyId" @click="killActiveSlot" />
            </div>
            <div class="toolbar-group">
                <button type="button" class="pg-btn pg-tab-btn" :class="{ 'is-active': activeSlotIndex === 0 }" @click="switchSlot(0)">
                    <span>Aba 1 (pane-1)</span>
                    <TabStatusIndicator :status="slotTabStatus(0)" size="sm" :show-label="true" />
                </button>
                <button type="button" class="pg-btn pg-tab-btn" :class="{ 'is-active': activeSlotIndex === 1 }" @click="switchSlot(1)">
                    <span>Aba 2 (pane-2)</span>
                    <TabStatusIndicator :status="slotTabStatus(1)" size="sm" :show-label="true" />
                </button>
                <MaxButton label="Perder contexto WebGL" severity="secondary" size="small" class="pg-btn" @click="triggerWebglLoss" />
                <MaxButton :label="demoTriggersEnabled ? 'Gatilhos Demo: ON' : 'Gatilhos Demo: OFF'" severity="secondary" size="small" :class="demoTriggersEnabled ? 'pg-btn is-active' : 'pg-btn'" @click="toggleDemoTriggers" />
            </div>
            <div class="toolbar-group">
                <MaxButton label="+ Bloco OK (0)" severity="secondary" size="small" class="pg-btn" @click="simulateBlock('success')" />
                <MaxButton label="+ Bloco Erro (1)" severity="secondary" size="small" class="pg-btn" @click="simulateBlock('error')" />
                <MaxButton label="+ Bloco Abort (null)" severity="secondary" size="small" class="pg-btn" @click="simulateBlock('aborted')" />
            </div>
        </header>
        <MaxAlert v-if="cleanupAlertMessage" severity="info" role="alert" :message="cleanupAlertMessage" class="playground-alert" />
        <section class="playground-telemetry" aria-label="Telemetria em tempo real">
            <span class="telemetry-chip">Spawn: {{ activeSlot.spawnLatencyMs }} ms</span>
            <span class="telemetry-chip">Mount: {{ activeSlot.mountLatencyMs }} ms</span>
            <span class="telemetry-chip">Renderer: {{ activeState?.renderer ?? 'n/a' }}</span>
            <span class="telemetry-chip">WebGL ativos: {{ terminalStore.webglCount }}</span>
            <span class="telemetry-chip">Grade: {{ activeState?.cols ?? 0 }}×{{ activeState?.rows ?? 0 }}</span>
            <span class="telemetry-chip">Input: {{ activeState?.inputMode ?? 'n/a' }}</span>
            <span class="telemetry-chip">altScreen: {{ Boolean(activeState?.modes.altScreen) }}</span>
            <span class="telemetry-chip">mouse: {{ activeState?.modes.mouseTracking ?? 'none' }}</span>
            <span class="telemetry-chip">bracketedPaste: {{ Boolean(activeState?.modes.bracketedPaste) }}</span>
            <span class="telemetry-chip">pendingBytes: {{ activeState?.flow.pendingBytes ?? 0 }}</span>
            <span class="telemetry-chip">paused: {{ Boolean(activeState?.flow.paused) }}</span>
            <span class="telemetry-chip">droppedBytes: {{ activeState?.flow.droppedBytes ?? '0' }}</span>
            <span class="telemetry-chip">longtasks: {{ longtaskCount }}</span>
            <span class="telemetry-chip">Status Aba: {{ activeTabStatus }}</span>
            <span class="telemetry-chip">Integração: {{ activeIntegrationMode }}</span>
            <span class="telemetry-chip">CWD: {{ activeCwd ?? 'n/a' }}</span>
        </section>
        <main class="playground-stage">
            <HybridTerminal v-if="slots[0].ptyId" :pane-id="slots[0].paneId" :pty-id="slots[0].ptyId" :visible="activeSlotIndex === 0" @restart-requested="restartSlot(0)" />
            <HybridTerminal v-if="slots[1].ptyId" :pane-id="slots[1].paneId" :pty-id="slots[1].ptyId" :visible="activeSlotIndex === 1" @restart-requested="restartSlot(1)" />
            <div v-if="!activeSlot.ptyId" class="playground-placeholder">
                Clique em "Iniciar (Spawn)" para abrir um PTY real no painel {{ activeSlot.paneId }}.
            </div>
        </main>
    </div>
</template>

<script setup lang="ts">
    import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue';
    import { MaxAlert, MaxButton, MaxInputSelect } from '@maxvue/max-components-ui';
    import type { UnlistenFn } from '@tauri-apps/api/event';
    import type { TabStatus } from '@/types/bindings';
    import type { Disposable } from '@/types/terminal';
    import TabStatusIndicator from '@/components/shell/TabStatusIndicator.vue';
    import HybridTerminal from '@/components/terminal/HybridTerminal.vue';
    import { useShellCommandBlocks } from '@/composables/useShellCommandBlocks';
    import { useTabSeenTracker } from '@/composables/useTabSeenTracker';
    import { useCommandBlocksStore } from '@/stores/commandBlocks';
    import { useShellHistoryStore } from '@/stores/shellHistory';
    import { useTabStatusStore } from '@/stores/tabStatus';
    import { useTerminalStore } from '@/stores/terminal';
    import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
    import { installShellIntegrationExtension } from '@/terminal/extensions/shellIntegration';
    import { ptyClient } from '@/terminal/stream/ptyClient';
    import { ptyStreamHub } from '@/terminal/stream/ptyStreamHub';
    import { fuzzyFilter } from '@/terminal/triggers/fuzzy';
    import { triggerRegistry } from '@/terminal/triggers/triggerRegistry';

    interface PlaygroundSlot {
        tabId: string;
        paneId: string;
        ptyId: string | null;
        spawnLatencyMs: number;
        mountLatencyMs: number;
    }

    const terminalStore = useTerminalStore();
    const commandBlocksStore = useCommandBlocksStore();
    const tabStatusStore = useTabStatusStore();
    const shellHistoryStore = useShellHistoryStore();

    const selectedShell = ref<string>('');
    const shellOptions = [
        { label: 'Padrão ($SHELL)', value: '' },
        { label: 'zsh (/bin/zsh)', value: '/bin/zsh' },
        { label: 'bash (/bin/bash)', value: '/bin/bash' },
        { label: 'fish (/usr/bin/fish)', value: '/usr/bin/fish' },
        { label: 'sh (/bin/sh)', value: '/bin/sh' }
    ];
    const activeSlotIndex = ref<0 | 1>(0);
    const demoTriggersEnabled = ref<boolean>(false);
    const cleanupAlertMessage = ref<string | null>(null);
    const longtaskCount = ref<number>(0);

    const slots = reactive<[PlaygroundSlot, PlaygroundSlot]>([
        {
            tabId: '01960000-0000-7000-8000-000000000101',
            paneId: '01960000-0000-7000-8000-000000000201',
            ptyId: null,
            spawnLatencyMs: 0,
            mountLatencyMs: 0
        },
        {
            tabId: '01960000-0000-7000-8000-000000000102',
            paneId: '01960000-0000-7000-8000-000000000202',
            ptyId: null,
            spawnLatencyMs: 0,
            mountLatencyMs: 0
        }
    ]);

    const activeSlot = computed<PlaygroundSlot>(() => slots[activeSlotIndex.value]);
    const activeState = computed(() => terminalStore.instance(activeSlot.value.paneId));
    const activeTabId = computed<string>(() => activeSlot.value.tabId);
    const activePaneId = computed<string>(() => activeSlot.value.paneId);
    const activeTabStatus = computed<TabStatus>(() => tabStatusStore.statusOf(activeSlot.value.tabId));
    const activeIntegrationMode = computed<string>(() => {
        const ptyId = activeSlot.value.ptyId;
        if (!ptyId) {
            return 'n/a';
        }
        return (
            shellHistoryStore.integrationOf(ptyId)?.mode ??
            tabStatusStore.paneOf(activeSlot.value.tabId, activeSlot.value.paneId)?.integrationMode ??
            'pending'
        );
    });
    const activeCwd = computed<string | null>(() => {
        const ptyId = activeSlot.value.ptyId;
        if (!ptyId) {
            return null;
        }
        return (
            shellHistoryStore.cwdOf(ptyId) ??
            tabStatusStore.paneOf(activeSlot.value.tabId, activeSlot.value.paneId)?.cwd ??
            null
        );
    });

    const shellBlocksFeed = useShellCommandBlocks();
    useTabSeenTracker({
        activeTabId,
        activePaneId
    });

    const demoTriggerDisposables: Disposable[] = [];
    let shellExtensionDisposable: Disposable | null = null;
    let unlistenCleanupFailed: UnlistenFn | null = null;
    let perfObserver: PerformanceObserver | null = null;
    let simBlockCounter = 1;

    function slotTabStatus(slotIdx: 0 | 1): TabStatus {
        return tabStatusStore.statusOf(slots[slotIdx].tabId);
    }

    async function spawnSlot(slotIdx: 0 | 1): Promise<void> {
        const slot = slots[slotIdx];
        if (slot.ptyId) {
            return;
        }
        const t0 = performance.now();
        const summary = await ptyStreamHub.spawn(
            {
                shell: selectedShell.value || null,
                args: [],
                login: true,
                cwd: null,
                env: {},
                envRemove: [],
                size: {
                    rows: 24,
                    cols: 80,
                    pixelWidth: 0,
                    pixelHeight: 0
                },
                owner: {
                    tabId: slot.tabId,
                    paneId: slot.paneId
                }
            },
            {
                enabled: true,
                continuousPolicy: 'auto',
                projectId: null,
                tabSessionId: null
            }
        );
        const t1 = performance.now();
        slot.spawnLatencyMs = Math.round(t1 - t0);
        slot.ptyId = summary.id;
        slot.mountLatencyMs = Math.round(performance.now() - t1);
    }

    async function killSlot(slotIdx: 0 | 1): Promise<void> {
        const slot = slots[slotIdx];
        if (!slot.ptyId) {
            return;
        }
        await ptyClient.kill(slot.ptyId, 'graceful');
    }

    async function restartSlot(slotIdx: 0 | 1): Promise<void> {
        const slot = slots[slotIdx];
        const oldPtyId = slot.ptyId;
        if (oldPtyId) {
            try {
                await ptyClient.kill(oldPtyId, 'graceful');
            } catch {
                // Ignora caso já tenha encerrado.
            }
            terminalInstanceRegistry.dispose(slot.paneId);
            ptyStreamHub.release(oldPtyId);
            commandBlocksStore.feed.clear(slot.paneId);
            terminalStore.unregister(slot.paneId);
            slot.ptyId = null;
        }
        await spawnSlot(slotIdx);
    }

    async function spawnActiveSlot(): Promise<void> {
        await spawnSlot(activeSlotIndex.value);
    }

    async function restartActiveSlot(): Promise<void> {
        await restartSlot(activeSlotIndex.value);
    }

    async function killActiveSlot(): Promise<void> {
        await killSlot(activeSlotIndex.value);
    }

    function switchSlot(idx: 0 | 1): void {
        activeSlotIndex.value = idx;
        terminalStore.setActive(slots[idx].paneId);
    }

    function triggerWebglLoss(): void {
        const inst = terminalInstanceRegistry.get(activeSlot.value.paneId);
        inst?.forceWebglContextLossForDebug();
    }

    function simulateBlock(kind: 'success' | 'error' | 'aborted'): void {
        const paneId = activeSlot.value.paneId;
        const id = `sim-block-${simBlockCounter++}`;
        const now = Date.now();
        let cmd = 'tail -f storage/logs/laravel.log';
        if (kind === 'success') {
            cmd = 'npm run check';
        } else if (kind === 'error') {
            cmd = 'php artisan migrate --invalid';
        }

        commandBlocksStore.feed.begin(paneId, {
            id,
            command: cmd,
            cwd: '~/GitHub/MaxTerminal',
            startedAt: now - 420
        });
        commandBlocksStore.feed.markOutputStart(paneId, id);
        commandBlocksStore.feed.end(paneId, id, {
            exitCode: kind === 'success' ? 0 : kind === 'error' ? 1 : null,
            finishedAt: now
        });
    }

    function registerDemoTriggers(): void {
        clearDemoTriggers();

        const artisanCmds = [
            { id: 'migrate', label: 'migrate', description: 'Executa migrações do banco de dados', kind: 'value' as const },
            { id: 'migrate:fresh', label: 'migrate:fresh', description: 'Recria todas as tabelas', kind: 'value' as const },
            { id: 'make:model', label: 'make:model', description: 'Cria um novo Model Eloquent', kind: 'value' as const },
            { id: 'route:list', label: 'route:list', description: 'Lista todas as rotas registradas', kind: 'value' as const },
            { id: 'cache:clear', label: 'cache:clear', description: 'Limpa o cache da aplicação', kind: 'value' as const }
        ];

        demoTriggerDisposables.push(
            triggerRegistry.register({
                id: 'dev.artisan',
                prefix: '>',
                label: 'Comandos Artisan (Simulado)',
                isAvailable: () => true,
                provide: (q) => {
                    const filtered = fuzzyFilter(artisanCmds, q.query, 'label');
                    return {
                        items: [
                            ...filtered,
                            {
                                id: 'artisan.create',
                                label: 'Criar comando',
                                description: 'Abre assistente de criação de comando Artisan',
                                kind: 'action',
                                pinned: 'bottom'
                            }
                        ]
                    };
                },
                onSelect: (item) => {
                    if (item.id === 'artisan.create') {
                        return { kind: 'handled' };
                    }
                    return { kind: 'replace-input', text: `php artisan ${item.label} ` };
                }
            })
        );

        demoTriggerDisposables.push(
            triggerRegistry.register({
                id: 'dev.groups',
                prefix: '>>',
                label: 'Grupos de Comandos (Simulado)',
                isAvailable: () => true,
                provide: (q) => {
                    const base = [
                        { id: 'grp.dev', label: 'Subir Ambiente Dev', section: 'local', kind: 'value' as const },
                        { id: 'grp.check', label: 'Pipeline Completo', section: 'global', kind: 'value' as const }
                    ];
                    const filtered = fuzzyFilter(base, q.query, 'label');
                    return {
                        sections: [
                            { id: 'local', label: 'Projeto Atual', order: 1 },
                            { id: 'global', label: 'Globais', order: 2 }
                        ],
                        items: [
                            ...filtered,
                            { id: 'grp.create.local', label: 'Criar grupo de comandos local', kind: 'action', pinned: 'bottom' },
                            { id: 'grp.create.global', label: 'Criar grupo de comandos global', kind: 'action', pinned: 'bottom' }
                        ]
                    };
                },
                onSelect: (item) => {
                    if (item.kind === 'action') {
                        return { kind: 'handled' };
                    }
                    return { kind: 'submit', text: `echo "Executando grupo: ${item.label}"` };
                }
            })
        );

        demoTriggerDisposables.push(
            triggerRegistry.register({
                id: 'dev.ai.select',
                prefix: '@',
                label: 'Selecionar Agente / Modelo (Simulado)',
                isAvailable: () => true,
                provide: (q) => {
                    if (q.stage === 0) {
                        return {
                            stageLabel: 'Harness',
                            items: [
                                { id: 'claude', label: 'Claude Code', kind: 'value' },
                                { id: 'codex', label: 'Codex', kind: 'value' },
                                { id: 'agy', label: 'Antigravity (agy)', kind: 'value' },
                                { id: 'opencode', label: 'OpenCode', kind: 'value' }
                            ]
                        };
                    }
                    if (q.stage === 1) {
                        return {
                            stageLabel: 'Harness › Modelo',
                            items: [
                                { id: 'model-pro', label: 'Modelo Principal (Pro/Sonnet)', kind: 'value' },
                                { id: 'model-fast', label: 'Modelo Rápido (Flash/Haiku)', kind: 'value' }
                            ]
                        };
                    }
                    return {
                        stageLabel: 'Harness › Modelo › Nível de Raciocínio',
                        items: [
                            { id: 'low', label: 'Baixo', kind: 'value' },
                            { id: 'medium', label: 'Médio', kind: 'value' },
                            { id: 'high', label: 'Alto', kind: 'value' }
                        ]
                    };
                },
                onSelect: (item, q) => {
                    if (q.stage < 2) {
                        return { kind: 'push-stage', stageState: item.id };
                    }
                    return { kind: 'replace-input', text: '? ' };
                }
            })
        );

        demoTriggerDisposables.push(
            triggerRegistry.register({
                id: 'dev.ai.quick',
                prefix: '?',
                label: 'Comando por Linguagem Natural (Simulado)',
                isAvailable: () => true,
                provide: (q) => ({
                    items: [
                        {
                            id: 'ai-cmd',
                            label: `find . -name "*${q.query.trim() || 'ts'}*"`,
                            description: 'Sugestão gerada para a solicitação',
                            kind: 'value'
                        }
                    ]
                }),
                onSelect: (item) => ({ kind: 'replace-input', text: item.label })
            })
        );

        demoTriggerDisposables.push(
            triggerRegistry.register({
                id: 'dev.shortcuts',
                prefix: '/',
                label: 'Atalhos Universais (Simulado)',
                isAvailable: () => true,
                provide: (q) => {
                    if (/^\/(usr|etc|home|tmp|var|opt)\//.test(q.input)) {
                        return { items: [], passthrough: true };
                    }
                    const shortcuts = [
                        { id: 'install', label: '/install', detail: 'npm install', kind: 'value' as const },
                        { id: 'build', label: '/build', detail: 'npm run build', kind: 'value' as const },
                        { id: 'test', label: '/test', detail: 'npm test', kind: 'value' as const }
                    ];
                    return {
                        items: fuzzyFilter(shortcuts, q.query, 'label')
                    };
                },
                onSelect: (item) => ({ kind: 'submit', text: item.detail ?? item.label })
            })
        );
    }

    function clearDemoTriggers(): void {
        while (demoTriggerDisposables.length > 0) {
            demoTriggerDisposables.pop()?.dispose();
        }
    }

    function toggleDemoTriggers(): void {
        demoTriggersEnabled.value = !demoTriggersEnabled.value;
        if (demoTriggersEnabled.value) {
            registerDemoTriggers();
        } else {
            clearDemoTriggers();
        }
    }

    onMounted(async () => {
        shellExtensionDisposable = installShellIntegrationExtension();
        await Promise.all([
            tabStatusStore.init(),
            shellHistoryStore.init(),
            shellBlocksFeed.start()
        ]);
        unlistenCleanupFailed = await ptyClient.onCleanupFailed((ev) => {
            cleanupAlertMessage.value = `Falha na limpeza do PTY ${ev.ptyId}: ${ev.survivors.length} processos sobreviventes.`;
        });

        if (typeof PerformanceObserver !== 'undefined') {
            try {
                perfObserver = new PerformanceObserver((list) => {
                    longtaskCount.value += list.getEntries().length;
                });
                perfObserver.observe({ entryTypes: ['longtask'] });
            } catch {
                // Ambiente sem suporte a longtask observer.
            }
        }
    });

    onBeforeUnmount(() => {
        clearDemoTriggers();
        shellBlocksFeed.stop();
        shellExtensionDisposable?.dispose();
        shellExtensionDisposable = null;
        unlistenCleanupFailed?.();
        perfObserver?.disconnect();
    });
</script>

<style lang="scss" scoped>
    .terminal-playground {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .playground-toolbar {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        justify-content: space-between;
        gap: 8px;
        padding: 8px 12px;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .toolbar-group {
        display: flex;
        align-items: center;
        gap: 6px;
    }

    .toolbar-label {
        font-size: 12px;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .toolbar-select,
    .pg-btn {
        padding: 4px 8px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 4px;
        color: var(--mt-ui-text, var(--background-800));
        font-size: 12px;
        cursor: pointer;

        &:disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }
    }

    .pg-btn-primary,
    .pg-btn.is-active {
        background: var(--max-primary-600);
        border-color: var(--max-primary-500);
        color: var(--max-primary-content, #fff);
    }

    .pg-btn-danger {
        background: var(--max-danger-600);
        border-color: var(--max-danger-500);
        color: var(--max-danger-content, #fff);
    }

    .pg-tab-btn {
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }

    .playground-alert {
        padding: 6px 12px;
        background: color-mix(in srgb, var(--max-danger-500) 20%, transparent);
        border-bottom: 1px solid var(--max-danger-500);
        color: var(--max-danger-300);
        font-size: 12px;
    }

    .playground-telemetry {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
        padding: 6px 12px;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
        font-family: var(--mt-font-mono);
        font-size: 11px;
    }

    .telemetry-chip {
        padding: 2px 6px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 4px;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .playground-stage {
        position: relative;
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: column;
    }

    .playground-placeholder {
        display: flex;
        align-items: center;
        justify-content: center;
        flex: 1;
        color: var(--background-400);
        font-size: 13px;
    }
</style>
