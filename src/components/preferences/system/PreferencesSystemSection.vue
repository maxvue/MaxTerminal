<template>
    <div class="prefs-system">
        <div v-if="osSettingsStore.error" class="prefs-system__error" role="alert">
            {{ osSettingsStore.error }}
        </div>
        <section v-if="settings" class="prefs-system__group">
            <h3 class="prefs-system__heading">Janela, Bandeja e Inicialização</h3>
            <CloseBehaviorSelector :model-value="settings.closeBehavior" :tray-available="windowStore.trayAvailable" :disabled="osSettingsStore.saving" @update:model-value="onCloseBehaviorChange" />
            <div class="prefs-system__toggles">
                <label class="prefs-system__checkbox">
                    <input type="checkbox" :checked="autostartEnabled" :disabled="osSettingsStore.saving" @change="onAutostartToggle">
                    <span>Iniciar automaticamente o MaxTerminal ao entrar na sessão do CachyOS</span>
                </label>
                <label class="prefs-system__checkbox">
                    <input type="checkbox" :checked="settings.startMinimized" :disabled="osSettingsStore.saving" @change="onStartMinimizedToggle">
                    <span>Iniciar oculto na bandeja do sistema quando iniciado automaticamente</span>
                </label>
            </div>
        </section>

        <section v-if="settings" class="prefs-system__group">
            <h3 class="prefs-system__heading">Atalho Global e Modo Quake</h3>
            <div class="prefs-system__toggles">
                <label class="prefs-system__checkbox">
                    <input type="checkbox" :checked="settings.quake.enabled" :disabled="osSettingsStore.saving" @change="onQuakeEnabledToggle">
                    <span>Habilitar atalho rápido global para exibir/ocultar o MaxTerminal</span>
                </label>
            </div>
            <ShortcutRecorder :model-value="settings.quake.accelerator" :disabled="osSettingsStore.saving || !settings.quake.enabled" @update:model-value="onAcceleratorChange" />
            <WaylandShortcutHelp v-if="showWaylandHelp" :shortcut-status="windowStore.shortcutStatus" :portal-available="portalAvailable" :busy="osSettingsStore.saving" @configure-portal="onConfigurePortal" />
            <div class="prefs-system__grid">
                <div class="prefs-system__field">
                    <label class="prefs-system__label" for="mt-quake-mode">Estilo de abertura</label>
                    <select id="mt-quake-mode" class="prefs-system__select" :value="settings.quake.mode" :disabled="osSettingsStore.saving" @change="onQuakeModeChange">
                        <option value="drop_down">Drop-down no topo do monitor (Quake)</option>
                        <option value="normal">Janela normal</option>
                    </select>
                </div>
                <div class="prefs-system__field">
                    <label class="prefs-system__label" for="mt-quake-height">Altura no modo Quake ({{ settings.quake.dropDownHeightPct }}%)</label>
                    <input id="mt-quake-height" type="number" min="20" max="100" class="prefs-system__input" :value="settings.quake.dropDownHeightPct" :disabled="osSettingsStore.saving" @change="onQuakeHeightChange">
                </div>
            </div>
            <div class="prefs-system__toggles">
                <label class="prefs-system__checkbox">
                    <input type="checkbox" :checked="settings.quake.alwaysOnTop" :disabled="osSettingsStore.saving" @change="onAlwaysOnTopToggle">
                    <span>Manter janela sempre no topo quando em modo Quake</span>
                </label>
                <label class="prefs-system__checkbox">
                    <input type="checkbox" :checked="settings.quake.hideOnBlur" :disabled="osSettingsStore.saving" @change="onHideOnBlurToggle">
                    <span>Ocultar automaticamente ao clicar fora da janela</span>
                </label>
            </div>
        </section>

        <section v-if="settings" class="prefs-system__group">
            <h3 class="prefs-system__heading">Links Clicáveis para o Editor</h3>
            <EditorSelector :model-value="settings.editor" :editors="availableEditors" :disabled="osSettingsStore.saving" @update:model-value="onEditorChange" />
        </section>

        <section v-if="settings" class="prefs-system__group">
            <h3 class="prefs-system__heading">Notificações Nativas</h3>
            <div class="prefs-system__toggles">
                <label class="prefs-system__checkbox">
                    <input type="checkbox" :checked="settings.notifications.enabled" :disabled="osSettingsStore.saving" @change="onNotificationsEnabledToggle">
                    <span>Habilitar notificações nativas do sistema operacional</span>
                </label>
                <label class="prefs-system__checkbox">
                    <input type="checkbox" :checked="settings.notifications.longCommandEnabled" :disabled="osSettingsStore.saving || !settings.notifications.enabled" @change="onLongCommandToggle">
                    <span>Notificar quando comandos longos terminarem</span>
                </label>
                <label class="prefs-system__checkbox">
                    <input type="checkbox" :checked="settings.notifications.onlyWhenUnfocused" :disabled="osSettingsStore.saving || !settings.notifications.enabled" @change="onOnlyWhenUnfocusedToggle">
                    <span>Notificar comandos longos apenas quando a janela não estiver em foco</span>
                </label>
            </div>
            <div class="prefs-system__field prefs-system__field--inline">
                <label class="prefs-system__label" for="mt-long-cmd-secs">Tempo mínimo para comando longo (segundos)</label>
                <input id="mt-long-cmd-secs" type="number" min="3" max="3600" class="prefs-system__input prefs-system__input--short" :value="settings.notifications.longCommandThresholdSecs" :disabled="osSettingsStore.saving || !settings.notifications.enabled" @change="onThresholdSecsChange">
            </div>
        </section>

        <section v-if="settings" class="prefs-system__group" data-testid="prefs-backup-section">
            <div class="prefs-system__row-head">
                <h3 class="prefs-system__heading">Backups do Banco de Dados (SQLite) e Telemetria</h3>
                <div class="prefs-system__actions">
                    <button type="button" class="prefs-system__btn" :disabled="backupBusy" data-testid="prefs-backup-refresh-btn" @click="onRefreshBackups">Atualizar Lista</button>
                    <button type="button" class="prefs-system__btn prefs-system__btn--primary" :disabled="backupBusy" data-testid="prefs-backup-create-btn" @click="onCreateBackup">{{ backupBusy ? 'Processando...' : 'Criar Backup Manual' }}</button>
                    <button type="button" class="prefs-system__btn" data-testid="prefs-perf-toggle-btn" @click="perfVisible = !perfVisible">{{ perfVisible ? 'Ocultar Telemetria Dev' : 'Exibir Telemetria Dev' }}</button>
                </div>
            </div>
            <p v-if="backupFeedback" class="prefs-system__feedback" role="status" data-testid="prefs-backup-feedback">{{ backupFeedback }}</p>
            <ul v-if="backups.length > 0" class="prefs-system__backup-list" data-testid="prefs-backup-list">
                <li v-for="entry in backups" :key="entry.fileName" class="prefs-system__backup-item">
                    <div class="prefs-system__backup-meta">
                        <code>{{ entry.fileName }}</code>
                        <span>{{ formatBytes(entry.sizeBytes) }}</span>
                    </div>
                    <button type="button" class="prefs-system__btn" :disabled="backupBusy" :data-testid="`prefs-backup-restore-${entry.fileName}`" @click="onScheduleRestore(entry.fileName)">Agendar Restauração</button>
                </li>
            </ul>
        </section>
    </div>
</template>

<script setup lang="ts">
    import { computed, onMounted, ref } from 'vue';
    import CloseBehaviorSelector from '@/components/preferences/system/CloseBehaviorSelector.vue';
    import EditorSelector from '@/components/preferences/system/EditorSelector.vue';
    import ShortcutRecorder from '@/components/preferences/system/ShortcutRecorder.vue';
    import WaylandShortcutHelp from '@/components/preferences/system/WaylandShortcutHelp.vue';
    import { usePerfTelemetry } from '@/composables/dev/usePerfTelemetry';
    import { createDbBackup, listDbBackups, restoreDbBackup } from '@/ipc/backup';
    import { useOsSettingsStore } from '@/stores/osSettings';
    import { useWindowStore } from '@/stores/window';
    import type {
        BackupEntryDto,
        CloseBehavior,
        EditorSettings_Serialize,
        QuakeMode
    } from '@/types/bindings';

    const osSettingsStore = useOsSettingsStore();
    const windowStore = useWindowStore();
    const { visible: perfVisible } = usePerfTelemetry();
    const backups = ref<BackupEntryDto[]>([]);
    const backupBusy = ref<boolean>(false);
    const backupFeedback = ref<string | null>(null);

    const settings = computed(() => osSettingsStore.settings);
    const autostartEnabled = computed<boolean>(
        () => osSettingsStore.autostart?.enabled ?? settings.value?.autostart ?? false
    );
    const portalAvailable = computed<boolean>(
        () => windowStore.environment?.portalGlobalShortcuts ?? false
    );
    const availableEditors = computed(() => windowStore.environment?.editors ?? []);
    const showWaylandHelp = computed<boolean>(
        () =>
            windowStore.isWayland ||
            windowStore.shortcutStatus?.backend === 'portal' ||
            windowStore.shortcutStatus?.backend === 'cli_only'
    );

    onMounted(() => {
        if (!osSettingsStore.settings) {
            void osSettingsStore.load().catch(() => {
                // Erro já tratado no estado da store.
            });
        }
    });

    function onCloseBehaviorChange(closeBehavior: CloseBehavior): void {
        void osSettingsStore.update({ closeBehavior }).catch(() => {});
    }

    function onAutostartToggle(event: Event): void {
        const checked = (event.target as HTMLInputElement).checked;
        void osSettingsStore.setAutostart(checked).catch(() => {});
    }

    function onStartMinimizedToggle(event: Event): void {
        const checked = (event.target as HTMLInputElement).checked;
        void osSettingsStore.update({ startMinimized: checked }).catch(() => {});
    }

    function onQuakeEnabledToggle(event: Event): void {
        const checked = (event.target as HTMLInputElement).checked;
        void osSettingsStore.update({ quake: { enabled: checked } }).catch(() => {});
    }

    function onAcceleratorChange(accelerator: string): void {
        void osSettingsStore.setAccelerator(accelerator).catch(() => {});
    }

    function onConfigurePortal(): void {
        void osSettingsStore.configurePortal().catch(() => {});
    }

    function onQuakeModeChange(event: Event): void {
        const mode = (event.target as HTMLSelectElement).value as QuakeMode;
        void osSettingsStore.update({ quake: { mode } }).catch(() => {});
    }

    function onQuakeHeightChange(event: Event): void {
        const raw = Number.parseInt((event.target as HTMLInputElement).value, 10);
        if (Number.isFinite(raw)) {
            const dropDownHeightPct = Math.min(100, Math.max(20, raw));
            void osSettingsStore.update({ quake: { dropDownHeightPct } }).catch(() => {});
        }
    }

    function onAlwaysOnTopToggle(event: Event): void {
        const checked = (event.target as HTMLInputElement).checked;
        void osSettingsStore.update({ quake: { alwaysOnTop: checked } }).catch(() => {});
    }

    function onHideOnBlurToggle(event: Event): void {
        const checked = (event.target as HTMLInputElement).checked;
        void osSettingsStore.update({ quake: { hideOnBlur: checked } }).catch(() => {});
    }

    function onEditorChange(editor: EditorSettings_Serialize): void {
        void osSettingsStore.update({ editor }).catch(() => {});
    }

    function onNotificationsEnabledToggle(event: Event): void {
        const checked = (event.target as HTMLInputElement).checked;
        void osSettingsStore.update({ notifications: { enabled: checked } }).catch(() => {});
    }

    function onLongCommandToggle(event: Event): void {
        const checked = (event.target as HTMLInputElement).checked;
        void osSettingsStore
            .update({ notifications: { longCommandEnabled: checked } })
            .catch(() => {});
    }

    function onOnlyWhenUnfocusedToggle(event: Event): void {
        const checked = (event.target as HTMLInputElement).checked;
        void osSettingsStore
            .update({ notifications: { onlyWhenUnfocused: checked } })
            .catch(() => {});
    }

    function onThresholdSecsChange(event: Event): void {
        const raw = Number.parseInt((event.target as HTMLInputElement).value, 10);
        if (Number.isFinite(raw)) {
            const longCommandThresholdSecs = Math.min(3600, Math.max(3, raw));
            void osSettingsStore
                .update({ notifications: { longCommandThresholdSecs } })
                .catch(() => {});
        }
    }

    function formatBytes(bytes: number): string {
        if (bytes < 1024) {
            return `${bytes} B`;
        }
        return `${(bytes / 1024).toFixed(1)} KiB`;
    }

    async function onRefreshBackups(): Promise<void> {
        backupBusy.value = true;
        backupFeedback.value = null;
        try {
            backups.value = await listDbBackups();
        } catch {
            backupFeedback.value = 'Não foi possível listar os backups do banco de dados.';
        } finally {
            backupBusy.value = false;
        }
    }

    async function onCreateBackup(): Promise<void> {
        backupBusy.value = true;
        backupFeedback.value = null;
        try {
            const created = await createDbBackup();
            backups.value = [created, ...backups.value.filter((b) => b.fileName !== created.fileName)];
            backupFeedback.value = `Backup manual criado com sucesso: ${created.fileName}`;
        } catch {
            backupFeedback.value = 'Falha ao criar backup manual do banco de dados.';
        } finally {
            backupBusy.value = false;
        }
    }

    async function onScheduleRestore(fileName: string): Promise<void> {
        backupBusy.value = true;
        backupFeedback.value = null;
        try {
            const scheduled = await restoreDbBackup(fileName);
            backupFeedback.value = `Restauração agendada para o próximo início (${scheduled.backupFileName}).`;
        } catch {
            backupFeedback.value = 'Falha ao agendar restauração do backup.';
        } finally {
            backupBusy.value = false;
        }
    }
</script>

<style lang="scss" scoped>
    .prefs-system {
        display: flex;
        flex-direction: column;
        gap: 1.25rem;
    }

    .prefs-system__error {
        padding: 0.625rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-danger-500);
        background: color-mix(in srgb, var(--max-danger-500) 14%, transparent);
        color: var(--max-danger-400);
        font-size: 0.75rem;
    }

    .prefs-system__group {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        padding: 0.875rem;
        border-radius: 0.5rem;
        border: 1px solid var(--background-800);
        background: var(--background-900);
    }

    .prefs-system__heading {
        margin: 0;
        font-size: 0.875rem;
        font-weight: 600;
        color: var(--background-50);
    }

    .prefs-system__toggles {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .prefs-system__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.8125rem;
        color: var(--background-200);
        cursor: pointer;

        input {
            accent-color: var(--max-primary-500);
        }
    }

    .prefs-system__grid {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 0.625rem;
    }

    .prefs-system__field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;

        &--inline {
            flex-direction: row;
            align-items: center;
            justify-content: space-between;
            gap: 0.75rem;
        }
    }

    .prefs-system__label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--background-300);
    }

    .prefs-system__select,
    .prefs-system__input {
        height: 2rem;
        padding: 0 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-950);
        color: var(--background-100);
        font-size: 0.8125rem;

        &:disabled {
            opacity: 0.45;
            cursor: not-allowed;
        }
    }

    .prefs-system__input--short {
        width: 6.5rem;
    }

    .prefs-system__row-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        flex-wrap: wrap;
    }

    .prefs-system__actions {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .prefs-system__btn {
        padding: 0.3125rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-800);
        color: var(--background-100);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.45;
            cursor: not-allowed;
        }
    }

    .prefs-system__btn--primary {
        border-color: var(--max-primary-500);
        background: color-mix(in srgb, var(--max-primary-500) 18%, transparent);
        color: var(--max-primary-300);
    }

    .prefs-system__feedback {
        margin: 0;
        font-size: 0.75rem;
        color: var(--max-info-400);
    }

    .prefs-system__backup-list {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .prefs-system__backup-item {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.375rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-800);
        background: var(--background-950);
        font-size: 0.75rem;
    }

    .prefs-system__backup-meta {
        display: flex;
        align-items: center;
        gap: 0.625rem;
        color: var(--background-300);
    }
</style>
